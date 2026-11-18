//! Bounded local record storage. A matching record never grants egress.

use super::{
    CheckedCa, HeldFile, ProbeAuthority, ProfileFiles, inode, open_regular, private_mode,
    read_bounded, wall_in,
};
use crate::{
    Error, Result,
    command_config::{
        peer_intent::{Relay, Trust},
        peer_origin, peer_trust,
    },
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::CStr,
    fs::{File, OpenOptions},
    io::{self, Write},
    ops::Range,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const RECORD: &CStr = c"peer-relay-qualification.json";
const STAGING: &CStr = c"peer-relay-qualification.new";
const MANAGED_ATTESTATION: &CStr = c"peer-relay-attestation.json";
const ATTESTATION_STAGING: &CStr = c"peer-relay-attestation.new";
const RECORD_LIMIT: usize = 8192;
const MAX_TIMESTAMP: u64 = 253_402_300_799;
const MAX_VALIDITY: u64 = 30 * 24 * 60 * 60;
const VERSIONS: Versions = Versions {
    suite: "private-wss-probe-v1",
    bounds: "private-wss-bounds-v1",
    contract: "private-wss-contract-v1",
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u8,
    profile_id: String,
    origin: String,
    trust: String,
    probe_suite: String,
    bounds_version: String,
    relay_contract: String,
    mdk_revision: String,
    target: String,
    probe_evidence: String,
    operator_evidence: String,
    qualified_at: u64,
    last_observed_at: u64,
    expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    build_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issuer_tag: Option<String>,
}

#[derive(Clone, Copy)]
struct Versions {
    suite: &'static str,
    bounds: &'static str,
    contract: &'static str,
}

struct Scope<'a> {
    profile_id: &'a str,
    relay: &'a Relay,
    versions: Versions,
}

#[derive(Debug, PartialEq, Eq)]
enum LocalStatus {
    Unqualified,
    Stale,
    Expired,
    ClockUntrusted,
    // Locally consistent metadata awaiting independent issuer evidence.
    CurrentMetadata,
    RequalifyRequired,
}

#[derive(Serialize)]
pub(crate) struct Report {
    origin: String,
    trust: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    qualified_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}

pub(super) fn trust_expression(trust: &Trust) -> String {
    let (prefix, pin) = match trust {
        Trust::SpkiSha256(pin) => ("spki-sha256", pin),
        Trust::CaSha256(pin) => ("ca-sha256", pin),
    };
    format!("{prefix}:{}", hex::encode(pin))
}

pub(super) fn wall() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|now| now.as_secs())
}

pub(super) fn show(profile: &ProfileFiles, relay: &Relay) -> Report {
    let record = read_record(profile).filter(|record| record.schema != 1);
    let status = record.as_ref().map_or(LocalStatus::Unqualified, |record| {
        record.readback(
            Scope {
                profile_id: profile.lease.profile_id(),
                relay,
                versions: VERSIONS,
            },
            wall(),
        )
    });
    let (record, label) = match status {
        LocalStatus::Unqualified => (None, "unqualified"),
        LocalStatus::Stale => (None, "stale"),
        LocalStatus::Expired => (record.as_ref(), "expired"),
        LocalStatus::ClockUntrusted => (None, "clock_untrusted"),
        LocalStatus::CurrentMetadata => (record.as_ref(), "current_metadata"),
        LocalStatus::RequalifyRequired => (record.as_ref(), "requalify_required"),
    };
    report(relay, record, label)
}

fn report(relay: &Relay, record: Option<&Record>, status: &'static str) -> Report {
    let full_trust = trust_expression(&relay.trust);
    let trust = full_trust[..full_trust.len() - 52].to_owned();
    Report {
        origin: relay.origin.to_string(),
        trust,
        status,
        qualified_at: record.map(|record| record.qualified_at),
        expires_at: record.map(|record| record.expires_at),
    }
}

const ATTESTATION_REFUSED: Error = Error::Invalid("peer_qualification_attestation");

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Attestation {
    schema: u8,
    origin: String,
    trust: String,
    enforcement_source: String,
    valid_from: u64,
    valid_until: u64,
    control_limits: ControlLimits,
    enforcement: Enforcement,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ControlLimits {
    event_bytes: u32,
    stored_per_recipient: u32,
    per_timestamp_second: u32,
    accepted_per_600s: u32,
    stored_per_writer_recipient: u32,
    retention_max_seconds: u32,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Enforcement {
    control_registered_writer: bool,
    control_exact_recipient_auth: bool,
    control_expiration_no_early_eviction: bool,
    group_exact_route_acl: bool,
    group_authenticated_members: bool,
    group_reconnect_history: bool,
}

pub(super) struct CheckedAttestation {
    data: Attestation,
    file: (u64, u64),
    digest: [u8; 32],
}

impl CheckedAttestation {
    pub(super) fn read(path: &Path, relay: &Relay, now: u64) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| ATTESTATION_REFUSED)?;
        Self::read_file(file, relay, now, false)
    }

    fn read_file(file: File, relay: &Relay, now: u64, managed: bool) -> Result<Self> {
        let meta = file.metadata().map_err(|_| ATTESTATION_REFUSED)?;
        if !meta.is_file() || meta.nlink() != 1 || !private_mode(&meta, 0o600) {
            return Err(ATTESTATION_REFUSED);
        }
        let bytes = read_bounded(&file, RECORD_LIMIT).map_err(|_| ATTESTATION_REFUSED)?;
        Self::decode(&bytes, inode(&meta), relay, now, managed)
    }

    fn decode(
        bytes: &[u8],
        file: (u64, u64),
        relay: &Relay,
        now: u64,
        managed: bool,
    ) -> Result<Self> {
        if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
            return Err(ATTESTATION_REFUSED);
        }
        let data: Attestation = serde_json::from_slice(bytes).map_err(|_| ATTESTATION_REFUSED)?;
        if data.schema != 1
            || data.origin != relay.origin.as_str()
            || data.trust != trust_expression(&relay.trust)
            || !(1..=512).contains(&data.enforcement_source.len())
            || data.enforcement_source.trim() != data.enforcement_source
            || data.enforcement_source.chars().any(char::is_control)
            || data.valid_from == 0
            || !(data.valid_from..data.valid_until).contains(&now)
            || data.valid_until > MAX_TIMESTAMP
            || data.control_limits
                != (ControlLimits {
                    event_bytes: 65536,
                    stored_per_recipient: 256,
                    per_timestamp_second: 3,
                    accepted_per_600s: 32,
                    stored_per_writer_recipient: 32,
                    retention_max_seconds: 324000,
                })
            || data.enforcement
                != (Enforcement {
                    control_registered_writer: true,
                    control_exact_recipient_auth: true,
                    control_expiration_no_early_eviction: true,
                    group_exact_route_acl: true,
                    group_authenticated_members: true,
                    group_reconnect_history: true,
                })
        {
            return Err(ATTESTATION_REFUSED);
        }
        let canonical = serde_json::to_vec(&data).map_err(|_| ATTESTATION_REFUSED)?;
        if managed && bytes != canonical {
            return Err(ATTESTATION_REFUSED);
        }
        Ok(Self {
            data,
            file,
            digest: Sha256::digest(canonical).into(),
        })
    }

    pub(super) fn recheck(&self, path: &Path, relay: &Relay, now: u64) -> Result<()> {
        let current = Self::read(path, relay, now)?;
        if self.file != current.file || self.digest != current.digest {
            return Err(ATTESTATION_REFUSED);
        }
        Ok(())
    }

    pub(super) fn operator_view(&self) -> String {
        format!(
            "Enforcement source: {:?}\nAttestation validity: {} to {} (Unix seconds)\nConfirm the v1 Control retention, limits and access rules, and Group access and reconnect history.",
            self.data.enforcement_source, self.data.valid_from, self.data.valid_until
        )
    }
}

pub(super) fn check_build_scope() -> Result<()> {
    check_build_commit(env!("MARMISH_BUILD_COMMIT"))?;
    if !crate::foundation::valid_hex(env!("MARMISH_MDK_REVISION"), 40)
        || !valid_label(env!("MARMISH_BUILD_TARGET"), 128)
    {
        return Err(Error::Invalid("peer_qualification_build"));
    }
    Ok(())
}

fn check_build_commit(commit: &str) -> Result<()> {
    if !crate::foundation::valid_hex(commit, 40) || commit.bytes().all(|byte| byte == b'0') {
        return Err(Error::Invalid("peer_qualification_build"));
    }
    Ok(())
}

pub(super) fn issue(
    authority: &ProbeAuthority<'_>,
    completed: super::probe::Completed,
    issuer_key: &[u8; 32],
) -> Result<Report> {
    check_build_scope()?;
    let ProbeAuthority {
        profile,
        relay,
        attestation,
        ..
    } = *authority;
    let now = authority.check()?;
    let (started, finished, evidence) = completed.evidence(relay, now)?;
    let expires_at = started
        .saturating_add(MAX_VALIDITY)
        .min(attestation.data.valid_until);
    if finished < started || now >= expires_at {
        return Err(WRITE_REFUSED);
    }
    let mut record = Record {
        schema: 3,
        profile_id: profile.lease.profile_id().to_owned(),
        origin: relay.origin.to_string(),
        trust: trust_expression(&relay.trust),
        probe_suite: VERSIONS.suite.to_owned(),
        bounds_version: VERSIONS.bounds.to_owned(),
        relay_contract: VERSIONS.contract.to_owned(),
        mdk_revision: env!("MARMISH_MDK_REVISION").to_owned(),
        target: env!("MARMISH_BUILD_TARGET").to_owned(),
        probe_evidence: hex::encode(evidence),
        operator_evidence: hex::encode(attestation.digest),
        qualified_at: started,
        last_observed_at: finished,
        expires_at,
        build_commit: Some(env!("MARMISH_BUILD_COMMIT").to_owned()),
        issuer_tag: None,
    };
    record.issuer_tag = Some(hex::encode(record.mac(issuer_key)?.finalize().into_bytes()));
    let staged_check = || {
        let wall = authority.check().map_err(io::Error::other)?;
        if wall >= expires_at || !completed.valid_clock(wall) {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(())
    };
    let exact = replace_pair(
        profile,
        relay,
        &record,
        attestation,
        staged_check,
        File::sync_all,
    )?;
    let readback = read_issued(
        profile,
        relay,
        issuer_key,
        authority.check().map_err(|_| WRITE_UNCERTAIN)?,
    )
    .map_err(|_| WRITE_UNCERTAIN)?;
    if readback.files != exact || readback.record != record || staged_check().is_err() {
        return Err(WRITE_UNCERTAIN);
    }
    Ok(report(relay, Some(&readback.record), "qualified"))
}

impl Record {
    fn decode(bytes: &[u8]) -> Option<Self> {
        // Derived struct deserializers also accept JSON sequences.
        if bytes.len() > RECORD_LIMIT
            || bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{')
        {
            return None;
        }
        let record: Self = serde_json::from_slice(bytes).ok()?;
        (record.is_canonical()
            && (record.schema != 3 || serde_json::to_vec(&record).ok()?.as_slice() == bytes))
            .then_some(record)
    }

    fn is_canonical(&self) -> bool {
        let hex = crate::foundation::valid_hex;
        let schema = match self.schema {
            1 | 2 => self.build_commit.is_none() && self.issuer_tag.is_none(),
            3 => {
                self.build_commit
                    .as_deref()
                    .is_some_and(|build| check_build_commit(build).is_ok())
                    && self.issuer_tag.as_deref().is_some_and(|tag| hex(tag, 64))
            }
            _ => false,
        };
        schema
            && hex(&self.profile_id, 32)
            && peer_origin(self.origin.clone()).is_ok_and(|url| url.as_str() == self.origin)
            && peer_trust(&self.trust).is_ok()
            && [
                self.probe_suite.as_str(),
                &self.bounds_version,
                &self.relay_contract,
            ]
            .into_iter()
            .all(|value| valid_label(value, 64))
            && hex(&self.mdk_revision, 40)
            && valid_label(&self.target, 128)
            && valid_evidence(&self.probe_evidence)
            && valid_evidence(&self.operator_evidence)
            && self.probe_evidence != self.operator_evidence
            && (1..=MAX_TIMESTAMP).contains(&self.qualified_at)
            && (self.qualified_at..=MAX_TIMESTAMP).contains(&self.last_observed_at)
            && self.expires_at <= MAX_TIMESTAMP
            && self
                .expires_at
                .checked_sub(self.qualified_at)
                .is_some_and(|duration| (1..=MAX_VALIDITY).contains(&duration))
    }

    fn readback(&self, scope: Scope<'_>, wall: Option<u64>) -> LocalStatus {
        let revision = env!("MARMISH_MDK_REVISION");
        if revision.is_empty() {
            return LocalStatus::Unqualified;
        }
        if self.profile_id != scope.profile_id
            || self.origin != scope.relay.origin.as_str()
            || peer_trust(&self.trust).ok().as_ref() != Some(&scope.relay.trust)
            || self.probe_suite != scope.versions.suite
            || self.bounds_version != scope.versions.bounds
            || self.relay_contract != scope.versions.contract
            || self.mdk_revision != revision
            || self.target != env!("MARMISH_BUILD_TARGET")
            || (self.schema == 3
                && self.build_commit.as_deref() != Some(env!("MARMISH_BUILD_COMMIT")))
        {
            return LocalStatus::Stale;
        }
        if self.last_observed_at >= self.expires_at {
            return LocalStatus::Expired;
        }
        match wall {
            Some(now) if (self.last_observed_at..=MAX_TIMESTAMP).contains(&now) => {
                if now >= self.expires_at {
                    LocalStatus::Expired
                } else {
                    match self.schema {
                        2 => LocalStatus::RequalifyRequired,
                        3 => LocalStatus::CurrentMetadata,
                        _ => LocalStatus::Unqualified,
                    }
                }
            }
            _ => LocalStatus::ClockUntrusted,
        }
    }

    fn mac(&self, key: &[u8; 32]) -> Result<Hmac<Sha256>> {
        let mut unsigned = self.clone();
        unsigned.issuer_tag = None;
        let bytes = serde_json::to_vec(&unsigned).map_err(|_| WRITE_REFUSED)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| WRITE_REFUSED)?;
        mac.update(b"marmish/peer/qualification/record/v3\0");
        mac.update(&bytes);
        Ok(mac)
    }
}

fn valid_label(value: &str, cap: usize) -> bool {
    !value.is_empty()
        && value.len() <= cap
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn valid_evidence(digest: &str) -> bool {
    crate::foundation::valid_hex(digest, 64) && digest.bytes().any(|b| b != b'0')
}

fn read_record(profile: &ProfileFiles) -> Option<Record> {
    let read_checked = || -> Result<Option<Record>> {
        profile.recheck()?;
        let file = profile.required(RECORD)?;
        let bytes = read_bounded(&file, RECORD_LIMIT)?;
        profile.recheck()?;
        Ok(Record::decode(&bytes))
    };
    read_checked().ok().flatten()
}

const EVIDENCE_REFUSED: Error = Error::Invalid("peer_qualification_evidence");

/// Authenticated local evidence only. It grants no network or custody authority.
pub(super) struct IssuedQualification {
    record: Record,
    files: EvidenceFiles,
}

#[derive(PartialEq, Eq)]
struct EvidenceFiles {
    record: (u64, u64),
    attestation: (u64, u64),
}

pub(super) fn read_issued(
    profile: &ProfileFiles,
    relay: &Relay,
    key: &[u8; 32],
    now: u64,
) -> Result<IssuedQualification> {
    let held = read_issued_held(profile, key, now)?;
    if &held.relay != relay {
        return Err(EVIDENCE_REFUSED);
    }
    Ok(held.issued)
}

pub(super) struct HeldQualification {
    relay: Relay,
    config: HeldFile,
    ca: Option<CheckedCa>,
    record: HeldFile,
    attestation: HeldFile,
    issued: IssuedQualification,
    attestation_window: Range<u64>,
}

impl HeldQualification {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(super) fn matches_config(&self, config: &HeldFile) -> bool {
        self.config.witness == config.witness
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(super) fn verify_issuer(&self, key: &[u8; 32]) -> Result<()> {
        verify_issuer(&self.issued.record, key)
    }

    pub(super) fn end_exclusive(&self) -> Result<u64> {
        let end = self
            .issued
            .record
            .expires_at
            .min(self.attestation_window.end);
        self.ca.as_ref().map_or(Ok(end), |ca| {
            super::inclusive_end(&ca.window)
                .map(|anchor_end| end.min(anchor_end))
                .ok_or(super::CA_ERROR)
        })
    }

    pub(super) fn live_relay(&self) -> (&Relay, Option<&CheckedCa>) {
        (&self.relay, self.ca.as_ref())
    }

    #[cfg(test)]
    pub(super) fn test_descriptor(&self, name: &str) -> &File {
        match name {
            "config.json" => &self.config.file,
            "peer-relay-ca.der" => &self.ca.as_ref().unwrap().file.file,
            "peer-relay-qualification.json" => &self.record.file,
            "peer-relay-attestation.json" => &self.attestation.file,
            _ => panic!("unexpected route file"),
        }
    }

    pub(super) fn check_time(&self, now: u64) -> Result<()> {
        let record = &self.issued.record;
        if !(record.last_observed_at..record.expires_at).contains(&now)
            || !self.attestation_window.contains(&now)
        {
            return Err(EVIDENCE_REFUSED);
        }
        if self.ca.as_ref().is_some_and(|ca| !wall_in(&ca.window, now)) {
            return Err(super::CA_ERROR);
        }
        Ok(())
    }

    pub(super) fn recheck_files(&self, profile: &ProfileFiles) -> Result<()> {
        self.config
            .recheck(profile)
            .map_err(|_| super::PROFILE_CHANGED)?;
        if let Some(ca) = &self.ca {
            ca.file.recheck(profile).map_err(|_| super::CA_ERROR)?;
        }
        self.record.recheck(profile).map_err(|_| EVIDENCE_REFUSED)?;
        self.attestation
            .recheck(profile)
            .map_err(|_| EVIDENCE_REFUSED)
    }
}

pub(super) fn read_issued_held(
    profile: &ProfileFiles,
    key: &[u8; 32],
    now: u64,
) -> Result<HeldQualification> {
    check_build_scope()?;
    let (loaded, config) = profile.checked_command()?;
    let relay = loaded.bound_peer_relay(&loaded)?;
    let ca = match relay.trust {
        Trust::CaSha256(pin) => Some(profile.read_ca(&pin, now)?),
        Trust::SpkiSha256(_) => None,
    };
    let (record, bytes) =
        HeldFile::open(profile, RECORD, RECORD_LIMIT).map_err(|_| EVIDENCE_REFUSED)?;
    let parsed = authenticated_record(&bytes, profile, relay, key, now)?;
    let (attestation, bytes) = HeldFile::open(profile, MANAGED_ATTESTATION, RECORD_LIMIT)
        .map_err(|_| ATTESTATION_REFUSED)?;
    let checked =
        CheckedAttestation::decode(&bytes, attestation.witness.file_id, relay, now, true)?;
    if parsed.operator_evidence != hex::encode(checked.digest)
        || checked.data.valid_from > parsed.qualified_at
        || parsed.expires_at
            != parsed
                .qualified_at
                .saturating_add(MAX_VALIDITY)
                .min(checked.data.valid_until)
    {
        return Err(EVIDENCE_REFUSED);
    }
    let held = HeldQualification {
        relay: relay.clone(),
        config,
        ca,
        issued: IssuedQualification {
            record: parsed,
            files: EvidenceFiles {
                record: record.witness.file_id,
                attestation: attestation.witness.file_id,
            },
        },
        record,
        attestation,
        attestation_window: checked.data.valid_from..checked.data.valid_until,
    };
    profile.recheck()?;
    held.recheck_files(profile)?;
    profile.recheck()?;
    Ok(held)
}

fn authenticated_record(
    bytes: &[u8],
    profile: &ProfileFiles,
    relay: &Relay,
    key: &[u8; 32],
    now: u64,
) -> Result<Record> {
    let record = Record::decode(bytes).ok_or(EVIDENCE_REFUSED)?;
    let scope = Scope {
        profile_id: profile.lease.profile_id(),
        relay,
        versions: VERSIONS,
    };
    if record.readback(scope, Some(now)) != LocalStatus::CurrentMetadata {
        return Err(EVIDENCE_REFUSED);
    }
    verify_issuer(&record, key)?;
    Ok(record)
}

fn verify_issuer(record: &Record, key: &[u8; 32]) -> Result<()> {
    let tag = record
        .issuer_tag
        .as_deref()
        .and_then(|tag| hex::decode(tag).ok())
        .ok_or(EVIDENCE_REFUSED)?;
    record
        .mac(key)?
        .verify_slice(&tag)
        .map_err(|_| EVIDENCE_REFUSED)
}

#[cfg(test)]
fn read(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    wall: Option<u64>,
) -> LocalStatus {
    read_record(profile).map_or(LocalStatus::Unqualified, |record| {
        record.readback(
            Scope {
                profile_id: profile.lease.profile_id(),
                relay,
                versions,
            },
            wall,
        )
    })
}

const WRITE_REFUSED: Error = Error::Storage("peer_qualification_write");
const WRITE_UNCERTAIN: Error = Error::Storage("peer_qualification_write_uncertain");

#[derive(Clone, Copy)]
struct Slot {
    name: &'static CStr,
    stage: &'static CStr,
}

const RECORD_SLOT: Slot = Slot {
    name: RECORD,
    stage: STAGING,
};
const ATTESTATION_SLOT: Slot = Slot {
    name: MANAGED_ATTESTATION,
    stage: ATTESTATION_STAGING,
};

#[derive(PartialEq, Eq)]
struct Snapshot {
    id: (u64, u64),
    bytes: Vec<u8>,
}

impl Snapshot {
    fn read(profile: &ProfileFiles, name: &CStr) -> Result<Option<Self>> {
        checked_optional(&profile.dir, name)?
            .map(|file| {
                let id = inode(&file.metadata().map_err(|_| WRITE_REFUSED)?);
                let bytes = read_bounded(&file, RECORD_LIMIT).map_err(|_| WRITE_REFUSED)?;
                Ok(Self { id, bytes })
            })
            .transpose()
    }
}

fn prepare_record(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    record: &Record,
) -> Result<(Vec<u8>, Option<Snapshot>)> {
    let bytes = serde_json::to_vec(record).map_err(|_| WRITE_REFUSED)?;
    let scope = Scope {
        profile_id: profile.lease.profile_id(),
        relay,
        versions,
    };
    if Record::decode(&bytes).is_none()
        || matches!(
            record.readback(scope, Some(record.last_observed_at)),
            LocalStatus::Stale | LocalStatus::ClockUntrusted
        )
    {
        return Err(WRITE_REFUSED);
    }
    profile.recheck()?;
    let old = Snapshot::read(profile, RECORD)?;
    if let Some(old) = &old {
        let old_record = Record::decode(&old.bytes).ok_or(WRITE_REFUSED)?;
        if !permits_replacement(&old_record, record) {
            return Err(WRITE_REFUSED);
        }
    }
    Ok((bytes, old))
}

fn replace_pair(
    profile: &ProfileFiles,
    relay: &Relay,
    record: &Record,
    attestation: &CheckedAttestation,
    staged_check: impl Fn() -> io::Result<()>,
    sync_parent: impl Fn(&File) -> io::Result<()>,
) -> Result<EvidenceFiles> {
    let _turn = profile.record_writer.lock().map_err(|_| WRITE_REFUSED)?;
    let (record_bytes, previous_record) = prepare_record(profile, relay, VERSIONS, record)?;
    let attestation_bytes = serde_json::to_vec(&attestation.data).map_err(|_| WRITE_REFUSED)?;
    let previous_attestation = Snapshot::read(profile, MANAGED_ATTESTATION)?;
    checked_optional(&profile.dir, STAGING)?;
    let attestation_id = if let Some(existing) = previous_attestation
        .as_ref()
        .filter(|existing| existing.id == attestation.file)
    {
        if existing.bytes != attestation_bytes {
            return Err(WRITE_REFUSED);
        }
        existing.id
    } else {
        replace_file(
            profile,
            ATTESTATION_SLOT,
            &attestation_bytes,
            previous_attestation.as_ref(),
            &staged_check,
            &sync_parent,
        )?
    };
    let check_pair = || {
        staged_check()?;
        let live = Snapshot::read(profile, MANAGED_ATTESTATION).map_err(io::Error::other)?;
        if !live.is_some_and(|live| live.id == attestation_id && live.bytes == attestation_bytes) {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        Ok(())
    };
    let record_id = replace_file(
        profile,
        RECORD_SLOT,
        &record_bytes,
        previous_record.as_ref(),
        check_pair,
        sync_parent,
    )
    .map_err(|_| WRITE_UNCERTAIN)?;
    Ok(EvidenceFiles {
        record: record_id,
        attestation: attestation_id,
    })
}

// Isolated existing record-replacement regressions use pre-v3 metadata.
#[cfg(test)]
fn replace(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    record: &Record,
) -> Result<()> {
    replace_using(profile, relay, versions, record, || Ok(()), File::sync_all)
}

#[cfg(test)]
fn replace_using(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    record: &Record,
    staged_check: impl FnOnce() -> io::Result<()>,
    sync_parent: impl FnOnce(&File) -> io::Result<()>,
) -> Result<()> {
    let _turn = profile.record_writer.lock().map_err(|_| WRITE_REFUSED)?;
    let (bytes, old) = prepare_record(profile, relay, versions, record)?;
    replace_file(
        profile,
        RECORD_SLOT,
        &bytes,
        old.as_ref(),
        staged_check,
        sync_parent,
    )
    .map(|_| ())
}

fn replace_file(
    profile: &ProfileFiles,
    slot: Slot,
    bytes: &[u8],
    old: Option<&Snapshot>,
    staged_check: impl FnOnce() -> io::Result<()>,
    sync_parent: impl FnOnce(&File) -> io::Result<()>,
) -> Result<(u64, u64)> {
    if let Some(stale_stage) = checked_optional(&profile.dir, slot.stage)? {
        unlink_owned_stage(profile, slot.stage, &stale_stage)?;
    }
    let staged = open_regular(
        &profile.dir,
        slot.stage,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )
    .map_err(|_| WRITE_REFUSED)?;
    let stage_and_rename = || -> Result<(u64, u64)> {
        (&staged).write_all(bytes).map_err(|_| WRITE_REFUSED)?;
        staged.sync_all().map_err(|_| WRITE_REFUSED)?;
        staged_check().map_err(|_| WRITE_REFUSED)?;
        profile.recheck()?;
        let latest = Snapshot::read(profile, slot.name)?;
        let id = inode(&staged.metadata().map_err(|_| WRITE_REFUSED)?);
        if old != latest.as_ref()
            || !Snapshot::read(profile, slot.stage)?
                .is_some_and(|live| live.id == id && live.bytes == bytes)
        {
            return Err(WRITE_REFUSED);
        }
        // SAFETY: both static filenames and the held directory descriptor
        // are valid for this call, and the owner lease serializes writers.
        let status = unsafe {
            libc::renameat(
                profile.dir.as_raw_fd(),
                slot.stage.as_ptr(),
                profile.dir.as_raw_fd(),
                slot.name.as_ptr(),
            )
        };
        if status != 0 {
            return Err(WRITE_REFUSED);
        }
        Ok(id)
    };
    let id = match stage_and_rename() {
        Ok(id) => id,
        Err(error) => {
            unlink_owned_stage(profile, slot.stage, &staged)?;
            return Err(error);
        }
    };
    sync_parent(&profile.dir).map_err(|_| WRITE_UNCERTAIN)?;
    profile.recheck().map_err(|_| WRITE_UNCERTAIN)?;
    let live = Snapshot::read(profile, slot.name).map_err(|_| WRITE_UNCERTAIN)?;
    if !live.is_some_and(|live| live.id == id && live.bytes == bytes) {
        return Err(WRITE_UNCERTAIN);
    }
    Ok(id)
}

fn permits_replacement(old: &Record, new: &Record) -> bool {
    if new.last_observed_at < old.last_observed_at {
        return false;
    }
    let mut advanced = old.clone();
    advanced.last_observed_at = new.last_observed_at;
    &advanced == new
        || (new.qualified_at >= old.last_observed_at && new.probe_evidence != old.probe_evidence)
}

fn checked_optional(dir: &File, name: &CStr) -> Result<Option<File>> {
    match open_regular(dir, name, libc::O_RDONLY) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(WRITE_REFUSED),
    }
}

fn file_id(file: Option<&File>) -> Result<Option<(u64, u64)>> {
    file.map(|file| {
        file.metadata()
            .map(|meta| inode(&meta))
            .map_err(|_| WRITE_REFUSED)
    })
    .transpose()
}

fn unlink_owned_stage(profile: &ProfileFiles, name: &CStr, file: &File) -> Result<()> {
    profile.recheck()?;
    let stage = profile.required(name).map_err(|_| WRITE_REFUSED)?;
    if file_id(Some(&stage))? != file_id(Some(file))? {
        return Err(WRITE_REFUSED);
    }
    // SAFETY: static filename and held directory descriptor remain live.
    let status = unsafe { libc::unlinkat(profile.dir.as_raw_fd(), name.as_ptr(), 0) };
    if status != 0 {
        return Err(WRITE_REFUSED);
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(super) use tests::write_candidate_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_relay_trust::tests::{config_fixture, private_write, test_lease};
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    const VERSIONS: Versions = Versions {
        suite: "loopback-fixture-v1",
        bounds: "bounds-v1",
        contract: "private-wss-v1",
    };

    fn relay() -> Relay {
        Relay {
            origin: peer_origin("wss://relay.example:8443/".into()).unwrap(),
            trust: peer_trust(&format!("spki-sha256:{}", "12".repeat(32))).unwrap(),
        }
    }

    fn record(id: &str, relay: &Relay) -> Record {
        Record {
            schema: 1,
            profile_id: id.to_owned(),
            origin: relay.origin.to_string(),
            trust: format!("spki-sha256:{}", "12".repeat(32)),
            probe_suite: VERSIONS.suite.into(),
            bounds_version: VERSIONS.bounds.into(),
            relay_contract: VERSIONS.contract.into(),
            mdk_revision: env!("MARMISH_MDK_REVISION").into(),
            target: env!("MARMISH_BUILD_TARGET").into(),
            probe_evidence: "a1".repeat(32),
            operator_evidence: "b2".repeat(32),
            qualified_at: 100,
            last_observed_at: 104,
            expires_at: 120,
            build_commit: None,
            issuer_tag: None,
        }
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub(in crate::peer_relay_trust) fn write_candidate_fixture(
        scope: &ProfileFiles,
        key: &[u8; 32],
    ) {
        use crate::peer_relay_trust::tests::private_write;
        let (config, _) = scope.checked_command().unwrap();
        let relay = config.bound_peer_relay(&config).unwrap();
        let now = wall().unwrap();
        let attestation = serde_json::json!({
            "schema":1, "origin":relay.origin, "trust":trust_expression(&relay.trust),
            "enforcement_source":"local registration test", "valid_from":now-1, "valid_until":now+90,
            "control_limits":{
                "event_bytes":65536, "stored_per_recipient":256, "per_timestamp_second":3,
                "accepted_per_600s":32, "stored_per_writer_recipient":32, "retention_max_seconds":324000
            },
            "enforcement":{
                "control_registered_writer":true, "control_exact_recipient_auth":true,
                "control_expiration_no_early_eviction":true, "group_exact_route_acl":true,
                "group_authenticated_members":true, "group_reconnect_history":true
            }
        });
        let bytes = serde_json::to_vec(&attestation).unwrap();
        let checked = CheckedAttestation::decode(&bytes, (0, 0), relay, now, false).unwrap();
        let mut row = record(scope.lease.profile_id(), relay);
        row.schema = 3;
        row.trust = trust_expression(&relay.trust);
        row.operator_evidence = hex::encode(checked.digest);
        row.qualified_at = now;
        row.last_observed_at = now;
        row.expires_at = now + 90;
        row.build_commit = Some(env!("MARMISH_BUILD_COMMIT").to_owned());
        row.issuer_tag = Some(hex::encode(row.mac(key).unwrap().finalize().into_bytes()));
        private_write(
            &scope.lease.root().join(RECORD.to_str().unwrap()),
            &serde_json::to_vec(&row).unwrap(),
        );
        private_write(
            &scope
                .lease
                .root()
                .join(MANAGED_ATTESTATION.to_str().unwrap()),
            &bytes,
        );
    }

    #[test]
    fn peer_qualification_closed_codec_and_exact_clock_scope() {
        let relay = relay();
        let profile_id = "12".repeat(16);
        let scope = || Scope {
            profile_id: &profile_id,
            relay: &relay,
            versions: VERSIONS,
        };
        let baseline = record(&profile_id, &relay);
        let bytes = serde_json::to_vec(&baseline).unwrap();
        assert_eq!(Record::decode(&bytes), Some(baseline.clone()));
        let mut exact_limit = bytes.clone();
        exact_limit.resize(RECORD_LIMIT, b' ');
        assert_eq!(Record::decode(&exact_limit), Some(baseline.clone()));
        let text = std::str::from_utf8(&bytes).unwrap();
        let cases = [
            text.replace(r#""schema":1"#, r#""schema":1,"schema":1"#),
            text.replace(r#""schema":1"#, r#""schema":1,"extra":1"#),
            text.replace(r#""schema":1"#, r#""schema":3"#),
            text.replace(r#""schema":1"#, r#""schema":null"#),
            text.replace(r#""schema":1,"#, ""),
            text.replace("wss://", "WSS://"),
            text.replace("spki-sha256", "Spki-sha256"),
            "[".to_owned() + &"1,".repeat(14) + "1]",
            " ".repeat(RECORD_LIMIT) + text,
        ];
        for case in cases {
            assert!(Record::decode(case.as_bytes()).is_none());
        }
        assert_eq!(
            baseline.readback(scope(), None),
            LocalStatus::ClockUntrusted
        );
        for wall in [Some(0), Some(103), Some(MAX_TIMESTAMP + 1)] {
            assert_eq!(
                baseline.readback(scope(), wall),
                LocalStatus::ClockUntrusted
            );
        }
        assert_eq!(
            baseline.readback(scope(), Some(104)),
            LocalStatus::Unqualified
        );
        assert_eq!(baseline.readback(scope(), Some(120)), LocalStatus::Expired);
        let mut terminal = baseline.clone();
        terminal.last_observed_at = terminal.expires_at;
        assert_eq!(terminal.readback(scope(), Some(101)), LocalStatus::Expired);

        for update in [
            |record: &mut Record| record.profile_id = "21".repeat(16),
            |record: &mut Record| record.origin = "wss://relay.example/".into(),
            |record: &mut Record| record.trust = format!("ca-sha256:{}", "12".repeat(32)),
            |record: &mut Record| record.mdk_revision = "de".repeat(20),
            |record: &mut Record| record.target = "aarch64-other-linux".into(),
            |record: &mut Record| record.probe_suite = "other".into(),
            |record: &mut Record| record.bounds_version = "other".into(),
            |record: &mut Record| record.relay_contract = "other".into(),
        ] {
            let mut stale = baseline.clone();
            update(&mut stale);
            assert_eq!(stale.readback(scope(), Some(104)), LocalStatus::Stale);
        }
    }

    #[test]
    fn peer_qualification_pair_commit_cut_and_managed_reuse() {
        let (temp, lease) = test_lease();
        let profile = ProfileFiles::open(lease.clone()).unwrap();
        let relay = relay();
        config_fixture(
            &lease,
            relay.origin.as_str(),
            &trust_expression(&relay.trust),
            "auto",
        );
        let open = |source: &Path| CheckedAttestation::read(source, &relay, 100).unwrap();
        let attestation_path = temp.path().join("operator.json");
        let mut data = Attestation {
            schema: 1,
            origin: relay.origin.to_string(),
            trust: trust_expression(&relay.trust),
            enforcement_source: "fixture before cut".into(),
            valid_from: 90,
            valid_until: 120,
            control_limits: ControlLimits {
                event_bytes: 65536,
                stored_per_recipient: 256,
                per_timestamp_second: 3,
                accepted_per_600s: 32,
                stored_per_writer_recipient: 32,
                retention_max_seconds: 324000,
            },
            enforcement: Enforcement {
                control_registered_writer: true,
                control_exact_recipient_auth: true,
                control_expiration_no_early_eviction: true,
                group_exact_route_acl: true,
                group_authenticated_members: true,
                group_reconnect_history: true,
            },
        };
        private_write(&attestation_path, &serde_json::to_vec(&data).unwrap());
        let first = open(&attestation_path);
        let key = [31; 32];
        let sign = |candidate: &mut Record| {
            candidate.issuer_tag = Some(hex::encode(
                candidate.mac(&key).unwrap().finalize().into_bytes(),
            ));
        };
        let mut baseline = record(lease.profile_id(), &relay);
        baseline.schema = 3;
        baseline.probe_suite = super::VERSIONS.suite.into();
        baseline.bounds_version = super::VERSIONS.bounds.into();
        baseline.relay_contract = super::VERSIONS.contract.into();
        baseline.build_commit = Some(env!("MARMISH_BUILD_COMMIT").into());
        baseline.operator_evidence = hex::encode(first.digest);
        sign(&mut baseline);

        let managed = lease.root().join("peer-relay-attestation.json");
        let fixed = lease.root().join("peer-relay-qualification.json");
        let files = replace_pair(
            &profile,
            &relay,
            &baseline,
            &first,
            || Ok(()),
            File::sync_all,
        )
        .ok()
        .unwrap();
        assert!(read_issued(&profile, &relay, &key, 104).is_ok());
        let before = fs::read(&fixed).unwrap();
        data.enforcement_source = "fixture after cut".into();
        fs::write(&attestation_path, serde_json::to_vec(&data).unwrap()).unwrap();
        let second = open(&attestation_path);

        let mut next = baseline.clone();
        next.qualified_at = 105;
        next.last_observed_at = 108;
        next.probe_evidence = "c3".repeat(32);
        next.operator_evidence = hex::encode(second.digest);
        sign(&mut next);
        let staged = lease.root().join("peer-relay-qualification.new");
        symlink(&fixed, &staged).unwrap();
        assert!(matches!(
            replace_pair(&profile, &relay, &next, &second, || Ok(()), File::sync_all),
            Err(Error::Storage("peer_qualification_write"))
        ));
        assert_eq!(open(&managed).file, files.attestation);
        assert_eq!(fs::read(&fixed).unwrap(), before);
        fs::remove_file(staged).unwrap();

        assert!(matches!(
            replace_pair(
                &profile,
                &relay,
                &next,
                &second,
                || Ok(()),
                |_| { Err(io::Error::from(io::ErrorKind::Interrupted)) }
            ),
            Err(Error::Storage("peer_qualification_write_uncertain"))
        ));
        assert_eq!(fs::read(&fixed).unwrap(), before);
        let retained = open(&managed);
        assert_eq!(retained.digest, second.digest);
        assert_ne!(retained.file, files.attestation);
        assert!(read_issued(&profile, &relay, &key, 108).is_err());

        let recovered = replace_pair(
            &profile,
            &relay,
            &next,
            &retained,
            || Ok(()),
            File::sync_all,
        )
        .ok()
        .unwrap();
        assert_eq!(recovered.attestation, retained.file);
        assert_eq!(open(&managed).file, retained.file);
        assert!(read_issued(&profile, &relay, &key, 108).is_ok());
        assert!(check_build_commit(&"0".repeat(40)).is_err());
    }

    #[test]
    fn peer_qualification_atomic_replacement_and_crash_floor() {
        let (_temp, lease) = test_lease();
        let profile = ProfileFiles::open(lease.clone()).unwrap();
        let relay = relay();
        let baseline = record(lease.profile_id(), &relay);
        let fixed = lease.root().join("peer-relay-qualification.json");
        let staging = lease.root().join("peer-relay-qualification.new");
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(104)),
            LocalStatus::Unqualified
        );
        private_write(&staging, b"interrupted stage, not read");
        replace(&profile, &relay, VERSIONS, &baseline).unwrap();
        assert!(!staging.exists());
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(104)),
            LocalStatus::Unqualified
        );

        let mut advanced = baseline.clone();
        advanced.last_observed_at = baseline.expires_at;
        symlink(&fixed, &staging).unwrap();
        assert!(matches!(
            replace(&profile, &relay, VERSIONS, &advanced),
            Err(Error::Storage("peer_qualification_write"))
        ));
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(104)),
            LocalStatus::Unqualified
        );
        assert!(staging.is_symlink());
        fs::remove_file(&staging).unwrap();
        assert!(matches!(
            replace_using(
                &profile,
                &relay,
                VERSIONS,
                &advanced,
                || Ok(()),
                |_| Err(io::Error::from(io::ErrorKind::Interrupted))
            ),
            Err(Error::Storage("peer_qualification_write_uncertain"))
        ));
        // Rename took place, but caller never receives success after failed
        // directory sync. Seeing the higher floor on replay is safe.
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(104)),
            LocalStatus::Expired
        );
        assert!(matches!(
            replace(&profile, &relay, VERSIONS, &baseline),
            Err(Error::Storage("peer_qualification_write"))
        ));
        let mut fake_renewal = advanced.clone();
        fake_renewal.expires_at += 20;
        assert!(matches!(
            replace(&profile, &relay, VERSIONS, &fake_renewal),
            Err(Error::Storage("peer_qualification_write"))
        ));
        assert!(!staging.exists());

        let alias = lease.root().join("alias");
        fs::hard_link(&fixed, &alias).unwrap();
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(120)),
            LocalStatus::Unqualified
        );
        fs::remove_file(&alias).unwrap();
        fs::set_permissions(&fixed, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(120)),
            LocalStatus::Unqualified
        );
        fs::set_permissions(&fixed, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&fixed, &alias).unwrap();
        symlink(&alias, &fixed).unwrap();
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(120)),
            LocalStatus::Unqualified
        );
        fs::remove_file(&fixed).unwrap();
        private_write(&fixed, &vec![b' '; RECORD_LIMIT + 1]);
        assert_eq!(
            read(&profile, &relay, VERSIONS, Some(120)),
            LocalStatus::Unqualified
        );
    }
    #[test]
    fn peer_qualification_failed_staging_preserves_prior_and_foreign_inodes() {
        let (_temp, lease) = test_lease();
        let profile = ProfileFiles::open(lease.clone()).unwrap();
        let relay = relay();
        let baseline = record(lease.profile_id(), &relay);
        let fixed = lease.root().join("peer-relay-qualification.json");
        let staging = lease.root().join("peer-relay-qualification.new");
        let moved = lease.root().join("moved");
        replace(&profile, &relay, VERSIONS, &baseline).unwrap();

        let mut advanced = baseline.clone();
        advanced.last_observed_at += 1;
        assert!(matches!(
            replace_using(
                &profile,
                &relay,
                VERSIONS,
                &advanced,
                || {
                    assert!(staging.is_file());
                    Err(io::Error::from(io::ErrorKind::Interrupted))
                },
                |_| panic!("parent synced before rename")
            ),
            Err(Error::Storage("peer_qualification_write"))
        ));
        assert!(!staging.exists());
        assert_eq!(
            Record::decode(&fs::read(&fixed).unwrap()),
            Some(baseline.clone())
        );

        assert!(matches!(
            replace_using(
                &profile,
                &relay,
                VERSIONS,
                &advanced,
                || {
                    fs::rename(&staging, &moved)?;
                    private_write(&staging, b"external replacement");
                    Ok(())
                },
                |_| panic!("parent synced a foreign stage")
            ),
            Err(Error::Storage("peer_qualification_write"))
        ));
        assert_eq!(fs::read(&staging).unwrap(), b"external replacement");
        assert_eq!(Record::decode(&fs::read(&moved).unwrap()), Some(advanced));
        assert_eq!(Record::decode(&fs::read(&fixed).unwrap()), Some(baseline));
    }
}
