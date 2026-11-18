//! Private Peer profile custody and TLS policy. Neither implies qualification.

use crate::{
    Error, Result,
    command_config::{
        CommandConfig,
        peer_intent::{Relay, Trust},
    },
    identity::ProfileLease,
    transport::{
        ArtiDirs, NetworkStream, PeerRelayConnector, RelayPolicy, RouteMode, TransportLimits,
    },
};
use nostr::types::Url;
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, UnixTime},
};
use sha2::{Digest, Sha256};
use std::{
    ffi::CStr,
    fs::{File, Metadata, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};
use tokio_util::sync::CancellationToken;
use x509_parser::{certificate::X509Certificate, parse_x509_certificate, public_key::PublicKey};

#[path = "peer_relay_probe.rs"]
mod probe;

#[path = "peer_relay_qualification.rs"]
mod qualification;

pub(crate) use qualification::Report as QualificationReport;

#[cfg(test)]
#[path = "peer_relay_trust_tests.rs"]
mod tests;

const CERT_LIMIT: usize = 16_384;
const CA_FILE: &CStr = c"peer-relay-ca.der";
const PROFILE_CHANGED: Error = Error::Identity("peer_profile_changed");
const CA_ERROR: Error = Error::Transport("peer_tls_ca");
const CERT_ERROR: Error = Error::Transport("peer_tls_certificate");
const PIN_ERROR: Error = Error::Transport("peer_tls_pin");

pub(crate) fn qualification_status(state: &Path, identity: &str) -> Result<QualificationReport> {
    let profile = ProfileFiles::open(Arc::new(ProfileLease::acquire_ready(state, identity)?))?;
    let config = profile.command_config()?;
    let relay = config.bound_peer_relay(&config)?;
    Ok(qualification::show(&profile, relay))
}

pub(crate) async fn qualify(
    state: &Path,
    identity: &str,
    attestation: PathBuf,
) -> Result<QualificationReport> {
    qualification::check_build_scope()?;
    let lease = Arc::new(ProfileLease::acquire_ready(state, identity)?);
    let profile = ProfileFiles::open(lease.clone())?;
    let (config, snapshot) = profile.command_snapshot()?;
    let relay = config.bound_peer_relay(&config)?;
    let clock = || qualification::wall().ok_or(Error::Invalid("peer_qualification_clock"));
    let checked = qualification::CheckedAttestation::read(&attestation, relay, clock()?)?;
    let owner = PeerRelayOwner::bind_with_limits(
        lease,
        &config,
        TransportLimits {
            max_frame_bytes: 128 * 1024,
            ..TransportLimits::default()
        },
    )?;
    let plan = probe::Plan::new()?;
    let affirmative = format!("qualify {}", relay.origin);
    let prompt = format!(
        "Private relay qualification for {}\nTLS trust: {}\n{}\n{}Type {affirmative} to authorize the bounded relay probe: ",
        relay.origin,
        qualification::trust_expression(&relay.trust),
        checked.operator_view(),
        plan.operator_view(),
    );
    let answer =
        tokio::task::spawn_blocking(move || crate::native_setup::Tty::open()?.line_exact(&prompt))
            .await
            .map_err(|_| Error::Invalid("peer_qualification_confirmation"))??;
    if answer != affirmative {
        return Err(Error::Invalid("peer_qualification_confirmation"));
    }
    let authority = ProbeAuthority {
        profile: &owner.profile,
        relay,
        snapshot,
        attestation: &checked,
        attestation_path: &attestation,
    };
    let started = authority.check()?;
    let completed = plan.execute(&owner.connector, &authority, started).await?;
    qualification::issue(&authority, completed)
}

/// Created only by the selected profile's confirmed operator turn. A raw URL
/// cannot obtain a probe connection and this permission cannot open live Peer.
pub(crate) struct ProbeAuthority<'a> {
    profile: &'a ProfileFiles,
    relay: &'a Relay,
    snapshot: ConfigWitness,
    attestation: &'a qualification::CheckedAttestation,
    attestation_path: &'a Path,
}

impl ProbeAuthority<'_> {
    pub(crate) fn endpoint(&self, tls: &BoundPeerTls) -> Result<&Url> {
        if tls.origin != self.relay.origin
            || tls.trust != self.relay.trust
            || !std::ptr::eq(tls.profile.as_ptr(), self.profile)
        {
            return Err(Error::Transport("relay_not_configured"));
        }
        self.check()?;
        Ok(&self.relay.origin)
    }

    fn check(&self) -> Result<u64> {
        let now = qualification::wall().ok_or(Error::Invalid("peer_qualification_clock"))?;
        self.attestation
            .recheck(self.attestation_path, self.relay, now)?;
        self.profile.check_command_snapshot(self.snapshot)?;
        Ok(now)
    }
}

pub(crate) struct PeerRelayOwner {
    profile: Arc<ProfileFiles>,
    connector: PeerRelayConnector,
}

impl PeerRelayOwner {
    pub(crate) fn bind(lease: Arc<ProfileLease>, config: &CommandConfig) -> Result<Self> {
        Self::bind_with_limits(lease, config, TransportLimits::default())
    }

    fn bind_with_limits(
        lease: Arc<ProfileLease>,
        config: &CommandConfig,
        mut limits: TransportLimits,
    ) -> Result<Self> {
        let profile = Arc::new(ProfileFiles::open(lease)?);
        let loaded = profile.command_config()?;
        let relay = config.bound_peer_relay(&loaded)?;
        limits.max_relays = 1;
        let policy = RelayPolicy::parse(&[relay.origin.to_string()], RouteMode::Off, limits)?;
        let tls = BoundPeerTls {
            origin: relay.origin.clone(),
            trust: relay.trust.clone(),
            profile: Arc::downgrade(&profile),
            #[cfg(test)]
            spki_roots: None,
        };
        // Reject unusable CA settings when binding the policy too. Every future
        // handshake validates the currently opened file separately.
        tls.tls_roots(&profile)?;
        let dirs = ArtiDirs {
            state: profile.lease.arti_state().to_owned(),
            cache: profile.lease.arti_cache().to_owned(),
        };
        let connector = PeerRelayConnector::bound(policy, dirs, tls, profile.cancel.clone())?;
        Ok(Self { profile, connector })
    }
}

impl Drop for PeerRelayOwner {
    fn drop(&mut self) {
        // All cloned connectors and established socket halves share this token.
        self.profile.cancel.cancel();
    }
}

pub(crate) struct BoundPeerTls {
    origin: Url,
    trust: Trust,
    profile: Weak<ProfileFiles>,
    #[cfg(test)]
    spki_roots: Option<RootCertStore>,
}

pub(crate) struct PeerTlsAttempt {
    profile: Arc<ProfileFiles>,
    config: ClientConfig,
    trust: Trust,
    anchor_window: Option<std::ops::RangeInclusive<i64>>,
}

impl BoundPeerTls {
    #[cfg(test)]
    pub(crate) fn use_test_spki_roots(&mut self, roots: RootCertStore) {
        assert!(matches!(self.trust, Trust::SpkiSha256(_)));
        self.spki_roots = Some(roots);
    }

    pub(crate) fn attempt(&self, url: &Url) -> Result<PeerTlsAttempt> {
        if url.as_str() != self.origin.as_str() {
            return Err(Error::Transport("relay_not_configured"));
        }
        let profile = self.profile.upgrade().ok_or(Error::Transport("shutdown"))?;
        profile.recheck()?;
        let (roots, anchor_window) = self.tls_roots(&profile)?;
        let mut config = crate::transport::client_tls_config(roots)?;
        config.enable_early_data = false;
        config.resumption = rustls::client::Resumption::disabled();
        Ok(PeerTlsAttempt {
            profile,
            config,
            trust: self.trust.clone(),
            anchor_window,
        })
    }

    fn tls_roots(
        &self,
        profile: &ProfileFiles,
    ) -> Result<(RootCertStore, Option<std::ops::RangeInclusive<i64>>)> {
        match self.trust {
            Trust::SpkiSha256(_) => {
                #[cfg(test)]
                if let Some(roots) = &self.spki_roots {
                    return Ok((roots.clone(), None));
                }
                Ok((crate::transport::bundled_tls_roots(), None))
            }
            Trust::CaSha256(pin) => {
                let file = profile.required(CA_FILE).map_err(|_| CA_ERROR)?;
                let bytes = read_bounded(file, CERT_LIMIT).map_err(|_| CA_ERROR)?;
                if Sha256::digest(&bytes)[..] != pin {
                    return Err(CA_ERROR);
                }
                let window = ca_window(&bytes)
                    .filter(|window| wall_in(window, UnixTime::now().as_secs()))
                    .ok_or(CA_ERROR)?;
                let mut root = RootCertStore::empty();
                root.add(CertificateDer::from(bytes))
                    .map_err(|_| CA_ERROR)?;
                Ok((root, Some(window)))
            }
        }
    }
}

impl PeerTlsAttempt {
    pub(crate) async fn connect(
        &self,
        stream: Box<dyn NetworkStream>,
        host: &str,
    ) -> Result<Box<dyn NetworkStream>> {
        self.connect_using(stream, host, || UnixTime::now().as_secs())
            .await
    }

    async fn connect_using(
        &self,
        stream: Box<dyn NetworkStream>,
        host: &str,
        wall: impl FnOnce() -> u64,
    ) -> Result<Box<dyn NetworkStream>> {
        let stream = crate::transport::secure_tls_stream(stream, host, self.config.clone())
            .await
            .map_err(|_| CERT_ERROR)?;
        let (_, session) = stream.get_ref();
        let certificates = session.peer_certificates().ok_or(CERT_ERROR)?;
        validate_live_leaf(certificates, &self.trust)?;
        if self
            .anchor_window
            .as_ref()
            .is_some_and(|window| !wall_in(window, wall()))
        {
            return Err(CA_ERROR);
        }
        self.profile.recheck()?;
        Ok(Box::new(stream))
    }
}

fn validate_live_leaf(certificates: &[CertificateDer<'_>], trust: &Trust) -> Result<()> {
    if !(1..=8).contains(&certificates.len())
        || certificates
            .iter()
            .map(|cert| cert.as_ref().len())
            .sum::<usize>()
            > CERT_LIMIT * 8
    {
        return Err(CERT_ERROR);
    }
    let leaf = certificates[0].as_ref();
    let cert = parse_der(leaf).ok_or(CERT_ERROR)?;
    if let Trust::SpkiSha256(pin) = trust
        && Sha256::digest(cert.public_key().raw)[..] != *pin
    {
        return Err(PIN_ERROR);
    }
    Ok(())
}

fn parse_der(bytes: &[u8]) -> Option<X509Certificate<'_>> {
    if bytes.is_empty() || bytes.len() > CERT_LIMIT {
        return None;
    }
    let (remaining, certificate) = parse_x509_certificate(bytes).ok()?;
    remaining.is_empty().then_some(certificate)
}

fn ca_window(bytes: &[u8]) -> Option<std::ops::RangeInclusive<i64>> {
    let certificate = parse_der(bytes)?;
    let validity = certificate.validity();
    (certificate.subject() == certificate.issuer()
        && certificate
            .basic_constraints()
            .is_ok_and(|bc| bc.is_some_and(|bc| bc.value.ca))
        && certificate
            .key_usage()
            .is_ok_and(|ku| ku.is_none_or(|ku| ku.value.key_cert_sign()))
        && modern_root_signature(&certificate)
        && certificate.verify_signature(None).is_ok())
    .then_some(validity.not_before.timestamp()..=validity.not_after.timestamp())
}

fn wall_in(window: &std::ops::RangeInclusive<i64>, wall: u64) -> bool {
    i64::try_from(wall).is_ok_and(|wall| window.contains(&wall))
}

fn modern_root_signature(cert: &X509Certificate<'_>) -> bool {
    let signature = &cert.signature_algorithm;
    if signature != &cert.tbs_certificate.signature {
        return false;
    }
    let key = cert.public_key();
    match signature.algorithm.to_id_string().as_str() {
        "1.2.840.113549.1.1.11" | "1.2.840.113549.1.1.12" | "1.2.840.113549.1.1.13" => {
            signature.parameters.as_ref().is_none_or(|p| {
                p.data.is_empty() && p.tag() == x509_parser::der_parser::asn1_rs::Tag::Null
            }) && key.algorithm.algorithm.to_id_string() == "1.2.840.113549.1.1.1"
                && key.algorithm.parameters.as_ref().is_none_or(|p| {
                    p.data.is_empty() && p.tag() == x509_parser::der_parser::asn1_rs::Tag::Null
                })
                && matches!(key.parsed(), Ok(PublicKey::RSA(rsa)) if rsa.key_size() >= 2048)
        }
        "1.2.840.10045.4.3.2" => matching_ec_parameters(cert, "1.2.840.10045.3.1.7"),
        "1.2.840.10045.4.3.3" => matching_ec_parameters(cert, "1.3.132.0.34"),
        "1.3.101.112" => {
            signature.parameters.is_none()
                && key.algorithm.algorithm.to_id_string() == "1.3.101.112"
                && key.algorithm.parameters.is_none()
        }
        _ => false,
    }
}

fn matching_ec_parameters(cert: &X509Certificate<'_>, curve: &str) -> bool {
    let key = &cert.public_key().algorithm;
    cert.signature_algorithm.parameters.is_none()
        && key.algorithm.to_id_string() == "1.2.840.10045.2.1"
        && key
            .parameters
            .as_ref()
            .and_then(|p| p.as_oid().ok())
            .is_some_and(|oid| oid.to_id_string() == curve)
}

struct ProfileFiles {
    lease: Arc<ProfileLease>,
    dir: File,
    cancel: CancellationToken,
    record_writer: Mutex<()>,
}

impl ProfileFiles {
    fn open(lease: Arc<ProfileLease>) -> Result<Self> {
        let dir = open_profile_directory(&lease)?;
        let owned = Self {
            lease,
            dir,
            cancel: CancellationToken::new(),
            record_writer: Mutex::new(()),
        };
        owned.recheck()?;
        let manifest = owned
            .required(c"profile.json")
            .map_err(|_| PROFILE_CHANGED)?;
        owned
            .lease
            .match_peer_manifest(manifest)
            .map_err(|_| PROFILE_CHANGED)?;
        Ok(owned)
    }

    fn recheck(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(Error::Transport("shutdown"));
        }
        let current = open_profile_directory(&self.lease)?;
        let current = current.metadata().map_err(|_| PROFILE_CHANGED)?;
        let held = self.dir.metadata().map_err(|_| PROFILE_CHANGED)?;
        if inode(&current) != inode(&held) || !private_mode(&held, 0o700) {
            return Err(PROFILE_CHANGED);
        }
        let lock = self.required(c"owner.lock").map_err(|_| PROFILE_CHANGED)?;
        self.lease
            .match_peer_lock(&lock)
            .map_err(|_| PROFILE_CHANGED)
    }

    fn required(&self, name: &CStr) -> io::Result<File> {
        open_regular(&self.dir, name, libc::O_RDONLY)
    }

    fn command_config(&self) -> Result<CommandConfig> {
        self.command_snapshot().map(|(config, _)| config)
    }

    fn command_snapshot(&self) -> Result<(CommandConfig, ConfigWitness)> {
        self.recheck()?;
        let file = self.required(c"config.json").map_err(|_| PROFILE_CHANGED)?;
        let file_id = inode(&file.metadata().map_err(|_| PROFILE_CHANGED)?);
        let bytes = read_bounded(file, crate::command_config::MAX_CONFIG as usize)
            .map_err(|_| PROFILE_CHANGED)?;
        self.recheck()?;
        let live = self.required(c"config.json").map_err(|_| PROFILE_CHANGED)?;
        if inode(&live.metadata().map_err(|_| PROFILE_CHANGED)?) != file_id {
            return Err(PROFILE_CHANGED);
        }
        let config = CommandConfig::from_bytes(&bytes).map_err(|_| PROFILE_CHANGED)?;
        Ok((
            config,
            ConfigWitness {
                file_id,
                digest: Sha256::digest(bytes).into(),
            },
        ))
    }

    fn check_command_snapshot(&self, expected: ConfigWitness) -> Result<()> {
        if self.command_snapshot()?.1 != expected {
            return Err(PROFILE_CHANGED);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ConfigWitness {
    file_id: (u64, u64),
    digest: [u8; 32],
}

fn open_profile_directory(lease: &ProfileLease) -> Result<File> {
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(lease.root())
        .map_err(|_| PROFILE_CHANGED)?;
    let meta = dir.metadata().map_err(|_| PROFILE_CHANGED)?;
    if !meta.is_dir() || !private_mode(&meta, 0o700) {
        return Err(PROFILE_CHANGED);
    }
    Ok(dir)
}

fn open_regular(dir: &File, name: &CStr, flags: libc::c_int) -> io::Result<File> {
    let flags = flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    // SAFETY: The directory descriptor is live and name is NUL-terminated.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags, 0o600 as libc::c_uint) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a new descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || !private_mode(&meta, 0o600) {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(file)
}

fn private_mode(meta: &Metadata, mode: u32) -> bool {
    // SAFETY: geteuid reads the current effective user.
    meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o7777 == mode
}

fn inode(meta: &Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}

fn read_bounded(file: File, limit: usize) -> io::Result<Vec<u8>> {
    if file.metadata()?.len() > limit as u64 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(bytes)
}
