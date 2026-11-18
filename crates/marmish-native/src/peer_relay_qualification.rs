//! Bounded local record storage. A matching record never grants egress.

use super::{ProfileFiles, inode, open_regular, read_bounded};
use crate::{
    Error, Result,
    command_config::{peer_intent::Relay, peer_origin, peer_trust},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::CStr,
    fs::File,
    io::{self, Write},
    os::fd::AsRawFd,
};

const RECORD: &CStr = c"peer-relay-qualification.json";
const STAGING: &CStr = c"peer-relay-qualification.new";
const RECORD_LIMIT: usize = 8192;
const MAX_TIMESTAMP: u64 = 253_402_300_799;
const MAX_VALIDITY: u64 = 30 * 24 * 60 * 60;

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
        record.is_canonical().then_some(record)
    }

    fn is_canonical(&self) -> bool {
        let hex = crate::foundation::valid_hex;
        self.schema == 1
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
                    LocalStatus::CurrentMetadata
                }
            }
            _ => LocalStatus::ClockUntrusted,
        }
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

fn read(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    wall: Option<u64>,
) -> LocalStatus {
    let read_checked = || -> Result<Option<Record>> {
        profile.recheck()?;
        let file = profile.required(RECORD)?;
        let bytes = read_bounded(file, RECORD_LIMIT)?;
        profile.recheck()?;
        Ok(Record::decode(&bytes))
    };
    read_checked()
        .ok()
        .flatten()
        .map_or(LocalStatus::Unqualified, |record| {
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

// A future typed issuer must supply the completed evidence before it can
// call this private primitive under the owner lease.
fn replace(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    record: &Record,
) -> Result<()> {
    replace_using(profile, relay, versions, record, File::sync_all)
}

fn replace_using(
    profile: &ProfileFiles,
    relay: &Relay,
    versions: Versions,
    record: &Record,
    sync_parent: impl FnOnce(&File) -> io::Result<()>,
) -> Result<()> {
    let _turn = profile.record_writer.lock().map_err(|_| WRITE_REFUSED)?;
    let bytes = serde_json::to_vec(record).map_err(|_| WRITE_REFUSED)?;
    let scope = Scope {
        profile_id: profile.lease.profile_id(),
        relay,
        versions,
    };
    if Record::decode(&bytes).is_none()
        || !matches!(
            record.readback(scope, Some(record.last_observed_at)),
            LocalStatus::CurrentMetadata | LocalStatus::Expired
        )
    {
        return Err(WRITE_REFUSED);
    }
    profile.recheck()?;
    let old = checked_optional(&profile.dir, RECORD)?;
    if let Some(old) = &old {
        let old_bytes = read_bounded(old.try_clone()?, RECORD_LIMIT).map_err(|_| WRITE_REFUSED)?;
        let old_record = Record::decode(&old_bytes).ok_or(WRITE_REFUSED)?;
        if !permits_replacement(&old_record, record) {
            return Err(WRITE_REFUSED);
        }
    }
    if let Some(stale_stage) = checked_optional(&profile.dir, STAGING)? {
        unlink_owned_stage(profile, &stale_stage)?;
    }
    let staged = open_regular(
        &profile.dir,
        STAGING,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )
    .map_err(|_| WRITE_REFUSED)?;
    let stage_and_rename = || -> Result<()> {
        (&staged).write_all(&bytes).map_err(|_| WRITE_REFUSED)?;
        staged.sync_all().map_err(|_| WRITE_REFUSED)?;
        profile.recheck()?;
        let latest = checked_optional(&profile.dir, RECORD)?;
        let latest = file_id(latest.as_ref())?;
        if latest != file_id(old.as_ref())?
            || file_id(Some(&staged))? != file_id(Some(&profile.required(STAGING)?))?
        {
            return Err(WRITE_REFUSED);
        }
        // SAFETY: both static filenames and the held directory descriptor
        // are valid for this call, and the owner lease serializes writers.
        let status = unsafe {
            libc::renameat(
                profile.dir.as_raw_fd(),
                STAGING.as_ptr(),
                profile.dir.as_raw_fd(),
                RECORD.as_ptr(),
            )
        };
        if status != 0 {
            return Err(WRITE_REFUSED);
        }
        Ok(())
    };
    if let Err(error) = stage_and_rename() {
        unlink_owned_stage(profile, &staged)?;
        return Err(error);
    }
    sync_parent(&profile.dir).map_err(|_| WRITE_UNCERTAIN)?;
    profile.recheck().map_err(|_| WRITE_UNCERTAIN)?;
    let live = profile.required(RECORD).map_err(|_| WRITE_UNCERTAIN)?;
    if file_id(Some(&staged)).map_err(|_| WRITE_UNCERTAIN)?
        != file_id(Some(&live)).map_err(|_| WRITE_UNCERTAIN)?
    {
        return Err(WRITE_UNCERTAIN);
    }
    Ok(())
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

fn unlink_owned_stage(profile: &ProfileFiles, file: &File) -> Result<()> {
    profile.recheck()?;
    let stage = profile.required(STAGING).map_err(|_| WRITE_REFUSED)?;
    if file_id(Some(&stage))? != file_id(Some(file))? {
        return Err(WRITE_REFUSED);
    }
    // SAFETY: static filename and held directory descriptor remain live.
    let status = unsafe { libc::unlinkat(profile.dir.as_raw_fd(), STAGING.as_ptr(), 0) };
    if status != 0 {
        return Err(WRITE_REFUSED);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_relay_trust::tests::{private_write, test_lease};
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
        }
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
        let text = std::str::from_utf8(&bytes).unwrap();
        let cases = [
            text.replace(r#""schema":1"#, r#""schema":1,"schema":1"#),
            text.replace(r#""schema":1"#, r#""schema":1,"extra":1"#),
            text.replace(r#""schema":1"#, r#""schema":2"#),
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
            LocalStatus::CurrentMetadata
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
            LocalStatus::CurrentMetadata
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
            LocalStatus::CurrentMetadata
        );
        assert!(staging.is_symlink());
        fs::remove_file(&staging).unwrap();
        assert!(matches!(
            replace_using(&profile, &relay, VERSIONS, &advanced, |_| Err(
                io::Error::from(io::ErrorKind::Interrupted)
            )),
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
}
