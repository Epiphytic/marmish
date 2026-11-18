use super::*;
use nostr::key::Keys;
use rcgen::{
    BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose, PublicKeyData,
    SignatureAlgorithm, date_time_ymd,
};
use serde_json::json;
use std::{fs, net::SocketAddr, path::Path, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::{Instant, timeout},
};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::accept_async;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn private_write(path: &Path, data: &[u8]) {
    use std::io::Write;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(data)
        .unwrap();
}

pub(super) fn test_lease() -> (tempfile::TempDir, Arc<ProfileLease>) {
    // Synthetic ready profile with a disposable public identity only.
    let temp = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let lease = ProfileLease::acquire(temp.path(), "test").unwrap();
    let profile = json!({
        "format":2,"profile_id":lease.profile_id(),"name":"test",
        "transaction":"12".repeat(16),"phase":"ready","backend":"files",
        "nostr_ref":"21".repeat(32),"sqlcipher_ref":"32".repeat(32),
        "device_slot":"46".repeat(16),"public_key":Keys::generate().public_key().to_hex()
    });
    private_write(
        &lease.root().join("profile.json"),
        &serde_json::to_vec(&profile).unwrap(),
    );
    drop(lease);
    let ready = ProfileLease::acquire_ready(temp.path(), "test").unwrap();
    (temp, Arc::new(ready))
}

pub(super) fn config_fixture(
    lease: &ProfileLease,
    origin: &str,
    trust: &str,
    mode: &str,
) -> CommandConfig {
    let bytes = serde_json::to_vec(&json!({
        "version": 1, "role":"both", "relays":["wss://ordinary.invalid/"],
        "tor":mode,"trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},
        "cwd":null,"limits":{},"aliases":{},
        "peer":{"relay":origin,"relay_trust":trust}
    }))
    .unwrap();
    private_write(&lease.root().join("config.json"), &bytes);
    CommandConfig::from_bytes(&bytes).unwrap()
}

struct Authority {
    der: CertificateDer<'static>,
    issuer: Issuer<'static, KeyPair>,
}

impl Authority {
    fn new() -> Self {
        Self::configured(&rcgen::PKCS_ECDSA_P256_SHA256, |_| {})
    }

    fn configured(
        algorithm: &'static SignatureAlgorithm,
        configure: impl FnOnce(&mut CertificateParams),
    ) -> Self {
        let mut ca = CertificateParams::default();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages.push(KeyUsagePurpose::KeyCertSign);
        configure(&mut ca);
        let key = KeyPair::generate_for(algorithm).unwrap();
        let der = ca.self_signed(&key).unwrap().der().clone();
        Self {
            der,
            issuer: Issuer::new(ca, key),
        }
    }

    fn server(&self, san: &str, key: &KeyPair, serial: u64) -> (TlsAcceptor, String) {
        self.server_with(san, key, serial, |_| {})
    }

    fn server_with(
        &self,
        san: &str,
        key: &KeyPair,
        serial: u64,
        configure: impl FnOnce(&mut CertificateParams),
    ) -> (TlsAcceptor, String) {
        let mut params = CertificateParams::new(vec![san.into()]).unwrap();
        params.serial_number = Some(serial.into());
        configure(&mut params);
        let cert = params.signed_by(key, &self.issuer).unwrap();
        let hash = hex::encode(Sha256::digest(key.subject_public_key_info()));
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )
        .unwrap();
        (TlsAcceptor::from(Arc::new(config)), hash)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    TlsRefused,
    NoUpgrade,
    Upgraded,
}

async fn serve_fixture(servers: Vec<TlsAcceptor>) -> (Url, JoinHandle<Vec<Outcome>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = Url::parse(&format!("wss://{}/", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move {
        let mut results = Vec::new();
        for server in servers {
            let outcome = timeout(TEST_TIMEOUT, async {
                let (stream, _) = listener.accept().await.unwrap();
                let Ok(tls) = server.accept(stream).await else {
                    return Outcome::TlsRefused;
                };
                if accept_async(tls).await.is_ok() {
                    Outcome::Upgraded
                } else {
                    Outcome::NoUpgrade
                }
            })
            .await
            .unwrap();
            results.push(outcome);
        }
        results
    });
    (origin, task)
}

#[tokio::test]
async fn peer_tls_private_ca_checks_pin_chain_name_and_fresh_socket() {
    let ca = Authority::new();
    let other = Authority::new();
    let key = KeyPair::generate().unwrap();
    let (valid, _) = ca.server("127.0.0.1", &key, 1);
    let (wrong_san, _) = ca.server("localhost", &key, 2);
    let (wrong_root, _) = other.server("127.0.0.1", &key, 3);
    let (origin, server) = serve_fixture(vec![valid, wrong_san, wrong_root]).await;

    let (_temp, lease) = test_lease();
    let ca_path = lease.root().join("peer-relay-ca.der");
    let config = config_fixture(
        &lease,
        origin.as_str(),
        &format!("ca-sha256:{}", hex::encode(Sha256::digest(&ca.der))),
        "auto",
    );
    private_write(&ca_path, ca.der.as_ref());
    let owner = PeerRelayOwner::bind(lease.clone(), &config).unwrap();
    let connector = owner.connector.test_transport();
    connector
        .bootstrap(Instant::now() + TEST_TIMEOUT)
        .await
        .unwrap();

    let url_mismatch = Url::parse("wss://127.0.0.1:9/").unwrap();
    assert!(connector.peer_test_open(&url_mismatch, None).await.is_err());
    let proxy: SocketAddr = "127.0.0.1:9".parse().unwrap();
    assert!(matches!(
        connector.peer_test_open(&origin, Some(proxy)).await,
        Err(Error::Transport("proxy_forbidden"))
    ));
    assert_eq!(connector.direct_attempt_count(), 0);

    drop(connector.peer_test_open(&origin, None).await.unwrap());
    for _ in 0..2 {
        assert!(matches!(
            connector.peer_test_open(&origin, None).await,
            Err(Error::Transport("peer_tls_certificate"))
        ));
    }
    assert_eq!(
        server.await.unwrap(),
        [Outcome::Upgraded, Outcome::TlsRefused, Outcome::TlsRefused]
    );

    fs::remove_file(&ca_path).unwrap();
    private_write(&ca_path, other.der.as_ref());
    assert!(matches!(
        connector.peer_test_open(&origin, None).await,
        Err(Error::Transport("peer_tls_ca"))
    ));
    assert_eq!(connector.direct_attempt_count(), 3);
    drop(owner);
    assert!(matches!(
        connector.peer_test_open(&origin, None).await,
        Err(Error::Transport("shutdown"))
    ));
}

#[tokio::test]
async fn peer_tls_spki_checks_same_key_reissue_and_rotation_before_upgrade() {
    let ca = Authority::new();
    let leaf = KeyPair::generate().unwrap();
    let rotated = KeyPair::generate().unwrap();
    let (valid, pin) = ca.server("127.0.0.1", &leaf, 1);
    let (reissued, again) = ca.server("127.0.0.1", &leaf, 2);
    let (wrong_pin, other) = ca.server("127.0.0.1", &rotated, 3);
    assert_eq!(pin, again);
    assert_ne!(pin, other);
    let (origin, server) = serve_fixture(vec![valid, reissued, wrong_pin]).await;
    let (_temp, lease) = test_lease();
    let config = config_fixture(
        &lease,
        origin.as_str(),
        &format!("spki-sha256:{pin}"),
        "off",
    );
    let mut owner = PeerRelayOwner::bind(lease, &config).unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(ca.der).unwrap();
    owner.connector.test_spki_roots(roots);
    let connector = owner.connector.test_transport();
    connector
        .bootstrap(Instant::now() + TEST_TIMEOUT)
        .await
        .unwrap();
    drop(connector.peer_test_open(&origin, None).await.unwrap());
    drop(connector.peer_test_open(&origin, None).await.unwrap());
    assert!(matches!(
        connector.peer_test_open(&origin, None).await,
        Err(Error::Transport("peer_tls_pin"))
    ));
    assert_eq!(
        server.await.unwrap(),
        [Outcome::Upgraded, Outcome::Upgraded, Outcome::NoUpgrade]
    );
}

#[test]
fn peer_tls_anchor_and_strict_profile_checks() {
    let ca = Authority::new();
    let window = ca_window(ca.der.as_ref()).unwrap();
    assert!(wall_in(&window, UnixTime::now().as_secs()));
    assert!(!wall_in(&window, u64::MAX));
    let mut corrupt = ca.der.as_ref().to_vec();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(ca_window(&corrupt).is_none());
    corrupt = ca.der.as_ref().to_vec();
    corrupt.push(0);
    assert!(ca_window(&corrupt).is_none());
    assert!(parse_der(&vec![0; CERT_LIMIT + 1]).is_none());
    let key = KeyPair::generate().unwrap();
    let unsigned_ca = CertificateParams::default().self_signed(&key).unwrap();
    assert!(ca_window(unsigned_ca.der().as_ref()).is_none());

    let (temp, lease) = test_lease();
    // App config permits a saved Peer pair with Tor required, so trust is
    // refused only when the private direct connector would be constructed.
    let config = config_fixture(
        &lease,
        "wss://127.0.0.1:9999/",
        &format!("ca-sha256:{}", "a1".repeat(32)),
        "always",
    );
    assert!(matches!(
        PeerRelayOwner::bind(lease.clone(), &config),
        Err(Error::Transport("routing_unsupported"))
    ));
    let root = lease.root();
    let moved = temp.path().join("moved");
    fs::rename(root, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, root).unwrap();
    assert!(matches!(
        PeerRelayOwner::bind(lease.clone(), &config),
        Err(Error::Identity("peer_profile_changed"))
    ));
    fs::remove_file(root).unwrap();
    fs::rename(moved, root).unwrap();
    fs::rename(root.join("owner.lock"), root.join("old.lock")).unwrap();
    private_write(&root.join("owner.lock"), b"");
    assert!(matches!(
        PeerRelayOwner::bind(lease.clone(), &config),
        Err(Error::Identity("peer_profile_changed"))
    ));
}

#[tokio::test]
async fn peer_tls_refuses_expired_leaf_and_anchor_before_upgrade() {
    let ca = Authority::new();
    let key = KeyPair::generate().unwrap();
    let (expired_leaf, _) = ca.server_with("127.0.0.1", &key, 1, |params| {
        params.not_after = date_time_ymd(2001, 1, 1);
    });
    let (live_leaf, _) = ca.server("127.0.0.1", &key, 2);
    let (origin, server) = serve_fixture(vec![expired_leaf, live_leaf]).await;
    let (_temp, lease) = test_lease();
    let ca_pin: [u8; 32] = Sha256::digest(&ca.der).into();
    private_write(&lease.root().join("peer-relay-ca.der"), ca.der.as_ref());
    let config = config_fixture(
        &lease,
        origin.as_str(),
        &format!("ca-sha256:{}", hex::encode(ca_pin)),
        "off",
    );
    let owner = PeerRelayOwner::bind(lease, &config).unwrap();
    let connector = owner.connector.test_transport();
    connector
        .bootstrap(Instant::now() + TEST_TIMEOUT)
        .await
        .unwrap();
    assert!(matches!(
        connector.peer_test_open(&origin, None).await,
        Err(Error::Transport("peer_tls_certificate"))
    ));

    // Reuse the exact TLS stream verifier with a simulated wall step after
    // the handshake. This makes the anchor expire without a real-time race.
    let tls = BoundPeerTls {
        origin: origin.clone(),
        trust: Trust::CaSha256(ca_pin),
        profile: Arc::downgrade(&owner.profile),
        spki_roots: None,
    };
    let attempt = tls.attempt(&origin).unwrap();
    let expires = u64::try_from(*attempt.anchor_window.as_ref().unwrap().end()).unwrap();
    let stream = TcpStream::connect(("127.0.0.1", origin.port().unwrap()))
        .await
        .unwrap();
    assert!(matches!(
        attempt
            .connect_using(Box::new(stream), "127.0.0.1", || expires + 1)
            .await,
        Err(Error::Transport("peer_tls_ca"))
    ));
    assert_eq!(
        server.await.unwrap(),
        [Outcome::TlsRefused, Outcome::NoUpgrade]
    );

    for configure in [
        |params: &mut CertificateParams| params.not_after = date_time_ymd(2001, 1, 1),
        |params: &mut CertificateParams| params.not_before = date_time_ymd(2099, 1, 1),
    ] {
        let invalid = Authority::configured(&rcgen::PKCS_ECDSA_P256_SHA256, configure);
        let ca_file = owner.profile.lease.root().join("peer-relay-ca.der");
        fs::remove_file(&ca_file).unwrap();
        private_write(&ca_file, invalid.der.as_ref());
        let invalid = BoundPeerTls {
            trust: Trust::CaSha256(Sha256::digest(&invalid.der).into()),
            origin: origin.clone(),
            profile: tls.profile.clone(),
            spki_roots: None,
        };
        assert!(matches!(
            invalid.attempt(&origin),
            Err(Error::Transport("peer_tls_ca"))
        ));
    }
}

#[test]
fn peer_tls_caps_chain_and_requires_private_ca_descriptor() {
    let ca = Authority::new();
    let trust = Trust::CaSha256(Sha256::digest(&ca.der).into());
    for chain in [
        Vec::new(),
        vec![ca.der.clone(); 9],
        vec![
            ca.der.clone(),
            CertificateDer::from(vec![0; CERT_LIMIT * 8]),
        ],
        vec![CertificateDer::from(vec![0; CERT_LIMIT + 1])],
    ] {
        assert!(matches!(
            validate_live_leaf(&chain, &trust),
            Err(Error::Transport("peer_tls_certificate"))
        ));
    }
    for algorithm in [&rcgen::PKCS_ECDSA_P384_SHA384, &rcgen::PKCS_ED25519] {
        let ca = Authority::configured(algorithm, |_| {});
        assert!(ca_window(ca.der.as_ref()).is_some());
    }
    let no_sign = Authority::configured(&rcgen::PKCS_ECDSA_P256_SHA256, |params| {
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    });
    assert!(ca_window(no_sign.der.as_ref()).is_none());

    let (_temp, lease) = test_lease();
    let ca_file = lease.root().join("peer-relay-ca.der");
    let alias = lease.root().join("alias.der");
    let config = config_fixture(
        &lease,
        "wss://127.0.0.1:9999/",
        &format!("ca-sha256:{}", hex::encode(Sha256::digest(&ca.der))),
        "off",
    );
    private_write(&alias, ca.der.as_ref());
    use std::os::unix::fs::{PermissionsExt, symlink};
    symlink(&alias, &ca_file).unwrap();
    assert!(matches!(
        PeerRelayOwner::bind(lease.clone(), &config),
        Err(Error::Transport("peer_tls_ca"))
    ));
    fs::remove_file(&ca_file).unwrap();
    fs::hard_link(&alias, &ca_file).unwrap();
    assert!(matches!(
        PeerRelayOwner::bind(lease.clone(), &config),
        Err(Error::Transport("peer_tls_ca"))
    ));
    fs::remove_file(&alias).unwrap();
    fs::set_permissions(&ca_file, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(
        PeerRelayOwner::bind(lease.clone(), &config),
        Err(Error::Transport("peer_tls_ca"))
    ));
    fs::remove_file(&ca_file).unwrap();
    private_write(&ca_file, &vec![0; CERT_LIMIT + 1]);
    assert!(matches!(
        PeerRelayOwner::bind(lease, &config),
        Err(Error::Transport("peer_tls_ca"))
    ));
}

use crate::mdk_adapter::terminal_relay_fixture::{GroupFault, QualificationFault, Relay};

fn write_issuer_root(lease: &ProfileLease) {
    // Exercise the ordinary read-only file provider without a profile signer.
    let keys = lease.root().join("keys");
    fs::create_dir(&keys).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    private_write(
        &keys.join(format!("sqlcipher-{}", "32".repeat(32))),
        &[42; 32],
    );
}

struct QualificationFixture {
    temp: tempfile::TempDir,
    owner: PeerRelayOwner,
    config: CommandConfig,
    attestation_path: std::path::PathBuf,
    attestation: serde_json::Value,
}

impl QualificationFixture {
    fn new(transport: &Relay) -> Self {
        let (temp, lease) = test_lease();
        write_issuer_root(&lease);
        let trust = format!(
            "ca-sha256:{}",
            hex::encode(Sha256::digest(transport.qualification_ca()))
        );
        let config = config_fixture(&lease, transport.url.as_str(), &trust, "off");
        private_write(
            &lease.root().join("peer-relay-ca.der"),
            transport.qualification_ca(),
        );
        let attestation_path = temp.path().join("attestation.json");
        let now = qualification::wall().unwrap();
        let attestation = json!({
            "schema":1, "origin":transport.url, "trust":trust, "enforcement_source":"fixture contract",
            "valid_from":now-1, "valid_until":now+90,
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
        private_write(&attestation_path, attestation.to_string().as_bytes());
        let owner = PeerRelayOwner::bind_with_limits(
            lease,
            &config,
            TransportLimits {
                max_frame_bytes: 128 * 1024,
                ..TransportLimits::default()
            },
        )
        .unwrap();
        Self {
            temp,
            owner,
            config,
            attestation_path,
            attestation,
        }
    }

    fn authority<'a>(
        &'a self,
        checked: &'a qualification::CheckedAttestation,
    ) -> ProbeAuthority<'a> {
        ProbeAuthority {
            profile: &self.owner.profile,
            relay: self.relay(),
            snapshot: self.owner.profile.command_snapshot().unwrap().1,
            ca: self
                .owner
                .profile
                .ca_snapshot(&self.relay().trust, qualification::wall().unwrap())
                .unwrap(),
            attestation: checked,
            attestation_path: &self.attestation_path,
        }
    }

    fn relay(&self) -> &crate::command_config::peer_intent::Relay {
        self.config.bound_peer_relay(&self.config).unwrap()
    }

    fn issuer_key(&self) -> zeroize::Zeroizing<[u8; 32]> {
        self.owner
            .profile
            .lease
            .peer_qualification_key(
                Path::new("/unused"),
                std::time::Instant::now() + TEST_TIMEOUT,
            )
            .unwrap()
    }

    fn record(&self) -> std::path::PathBuf {
        self.owner
            .profile
            .lease
            .root()
            .join("peer-relay-qualification.json")
    }
}

#[tokio::test]
async fn peer_qualification_loopback_requires_all_evidence_and_rechecks_profile() {
    let plan = probe::Plan::new().unwrap();
    let (accounts, route) = plan.relay_registration();
    let mut transport = Relay::start_peer_qualification(accounts, route).await;
    let fixture = QualificationFixture::new(&transport);
    let attestation_path = &fixture.attestation_path;
    let mut attestation = fixture.attestation.clone();
    let now = qualification::wall().unwrap();
    let relay = fixture.relay();
    let owner = &fixture.owner;
    let save = |value: &serde_json::Value| fs::write(attestation_path, value.to_string()).unwrap();
    attestation["enforcement"]["group_reconnect_history"] = json!(false);
    save(&attestation);
    assert!(qualification::CheckedAttestation::read(attestation_path, relay, now).is_err());
    attestation["enforcement"]["group_reconnect_history"] = json!(true);
    save(&attestation);
    let checked = qualification::CheckedAttestation::read(attestation_path, relay, now).unwrap();
    assert!(qualification::CheckedAttestation::read(attestation_path, relay, now + 90).is_err());
    let alias = fixture.temp.path().join("attestation-link.json");
    std::os::unix::fs::symlink(attestation_path, &alias).unwrap();
    assert!(qualification::CheckedAttestation::read(&alias, relay, now).is_err());

    let authority = fixture.authority(&checked);
    probe::check_test_budget(&authority);
    let record = fixture.record();
    assert!(!record.exists());
    let report = |value| serde_json::to_value(value).unwrap();
    assert_eq!(
        report(qualification::show(&owner.profile, relay))["status"],
        "unqualified"
    );

    attestation["enforcement_source"] = json!("changed after confirmation");
    save(&attestation);
    assert!(authority.check().is_err());
    assert!(!transport.authenticated(accounts[0]));
    assert!(!record.exists());
    attestation["enforcement_source"] = json!("fixture contract");
    save(&attestation);
    assert!(matches!(
        qualification_status(fixture.temp.path(), "test"),
        Err(Error::Identity("profile_in_use"))
    ));

    let result = plan
        .execute(&owner.connector, &authority, authority.check().unwrap())
        .await;
    let completed = result.unwrap_or_else(|error| {
        panic!("qualification failed {error:?}, writer authenticated {}, recipient authenticated {}, control writes {}, group writes {}, Group publications attempted {}, Control publications attempted {}", transport.authenticated(accounts[0]), transport.authenticated(accounts[1]), transport.peer_transcript().len(), transport.group_writes().len(), transport.publication_attempts(445), transport.publication_attempts(7435))
    });
    assert_eq!(transport.peer_transcript().len(), 1);
    assert_eq!(transport.group_writes().len(), 1);
    assert!(transport.authenticated(accounts[0]) && transport.authenticated(accounts[1]));
    assert!(!record.exists());
    let issued = qualification::issue(&authority, completed, &fixture.issuer_key()).unwrap();
    assert_eq!(report(issued)["status"], "qualified");
    let bytes = fs::read(&record).unwrap();
    let mut issued: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(issued["schema"], 3);
    assert!(
        !std::str::from_utf8(&bytes)
            .unwrap()
            .contains("fixture contract")
    );
    let qualified = report(qualification::show(&owner.profile, relay));
    assert_eq!(qualified["status"], "current_metadata");
    assert_eq!(qualified["origin"], transport.url.as_str());
    assert!(
        qualified["trust"].as_str().unwrap().len() < attestation["trust"].as_str().unwrap().len()
    );

    let key = fixture.issuer_key();
    let current = qualification::wall().unwrap();
    let readback = |key, wall| qualification::read_issued(&owner.profile, relay, key, wall);
    assert!(readback(&key, current).is_ok());
    let (_other_temp, other) = test_lease();
    write_issuer_root(&other);
    let other_key = other
        .peer_qualification_key(
            Path::new("/unused"),
            std::time::Instant::now() + TEST_TIMEOUT,
        )
        .unwrap();
    assert!(readback(&other_key, current).is_err());
    assert!(readback(&key, issued["last_observed_at"].as_u64().unwrap() - 1).is_err());
    assert!(readback(&key, issued["expires_at"].as_u64().unwrap()).is_err());

    for field in [
        "issuer_tag",
        "probe_evidence",
        "operator_evidence",
        "build_commit",
    ] {
        let original = issued[field].as_str().unwrap();
        let alternative = if original.starts_with('a') { 'b' } else { 'a' };
        let forged = format!("{alternative}{}", &original[1..]);
        let forged_bytes = std::str::from_utf8(&bytes)
            .unwrap()
            .replace(original, &forged);
        fs::write(&record, forged_bytes).unwrap();
        assert!(readback(&key, current).is_err());
        if field != "build_commit" {
            assert_eq!(
                report(qualification::show(&owner.profile, relay))["status"],
                "current_metadata"
            );
        }
    }
    fs::write(&record, &bytes).unwrap();
    let managed = owner
        .profile
        .lease
        .root()
        .join("peer-relay-attestation.json");
    let managed_bytes = fs::read(&managed).unwrap();
    assert_eq!(fs::metadata(&managed).unwrap().mode() & 0o777, 0o600);
    let changed_attestation = std::str::from_utf8(&managed_bytes)
        .unwrap()
        .replace("fixture contract", "different contract");
    fs::write(&managed, changed_attestation).unwrap();
    assert!(readback(&key, current).is_err());
    fs::write(&managed, managed_bytes).unwrap();
    assert!(readback(&key, current).is_ok());

    issued.as_object_mut().unwrap().remove("build_commit");
    issued.as_object_mut().unwrap().remove("issuer_tag");
    for (schema, status) in [(2, "requalify_required"), (1, "unqualified")] {
        issued["schema"] = json!(schema);
        fs::write(&record, issued.to_string()).unwrap();
        assert_eq!(
            report(qualification::show(&owner.profile, relay))["status"],
            status
        );
        assert!(readback(&key, current).is_err());
    }
    issued["last_observed_at"] = issued["expires_at"].clone();
    fs::write(&record, issued.to_string()).unwrap();
    assert_eq!(
        report(qualification::show(&owner.profile, relay))["status"],
        "unqualified"
    );
    let QualificationFixture { temp, owner, .. } = fixture;
    drop(owner);
    assert_eq!(
        report(qualification_status(temp.path(), "test").unwrap())["status"],
        "unqualified"
    );
    transport.stop().await;
}

#[tokio::test]
async fn peer_qualification_rejects_faulty_private_wss_and_never_writes() {
    use QualificationFault::*;
    for (fault, expected) in [
        (Some(WrongAuthAck(1)), "peer_qualification_writer_auth"),
        (
            Some(WrongWriteAck(7435)),
            "peer_qualification_control_write",
        ),
        (Some(AcceptOversize), "peer_qualification_control_size"),
        (Some(WrongSizeRefusal), "peer_qualification_control_size"),
        (Some(WrongWriteAck(445)), "peer_qualification_group_write"),
        (
            Some(AcceptWrongGroupWrite),
            "peer_qualification_group_foreign_write",
        ),
        (
            Some(LeakHistory("foreign-recipient", 7435)),
            "peer_qualification_control_foreign_read",
        ),
        (
            Some(WrongReadRefusal("foreign-recipient")),
            "peer_qualification_control_foreign_read",
        ),
        (
            Some(AcceptUnregisteredAuth),
            "peer_qualification_outsider_auth",
        ),
        (
            Some(AcceptUnauthenticatedControlWrite),
            "peer_qualification_control_unauthenticated_write",
        ),
        (
            Some(LeakHistory("unauth-control", 7435)),
            "peer_qualification_control_unauthenticated_read",
        ),
        (
            Some(EmptyHistory("unauth-control")),
            "peer_qualification_control_unauthenticated_read",
        ),
        (
            Some(LeakHistory("unauth-group", 445)),
            "peer_qualification_group_unauthenticated_read",
        ),
        (Some(WrongAuthAck(3)), "peer_qualification_recipient_auth"),
        (
            Some(AlterControlHistory),
            "peer_qualification_control_history",
        ),
        (
            Some(ExtraControlHistory),
            "peer_qualification_control_history",
        ),
        (
            Some(EmptyHistory("group")),
            "peer_qualification_group_history",
        ),
        (None, "peer_qualification_group_history"),
        (
            Some(LeakHistory("control-wide", 7435)),
            "peer_qualification_control_wide_read",
        ),
        (
            Some(LeakHistory("group-wide", 445)),
            "peer_qualification_group_wide_read",
        ),
        (
            Some(LeakHistory("group-wrong", 445)),
            "peer_qualification_group_foreign_read",
        ),
        (Some(WrongAuthAck(4)), "peer_qualification_reconnect_auth"),
        (
            Some(EmptyHistory("control-reopen")),
            "peer_qualification_control_reconnect",
        ),
        (
            Some(EmptyHistory("group-reopen")),
            "peer_qualification_group_reconnect",
        ),
    ] {
        let plan = probe::Plan::new().unwrap();
        let (accounts, route) = plan.relay_registration();
        let mut transport = Relay::start_peer_qualification(accounts, route).await;
        if let Some(fault) = fault {
            transport.qualification_fault(fault);
        } else {
            transport.group_fault(GroupFault::Disconnect);
        }
        let fixture = QualificationFixture::new(&transport);
        let now = qualification::wall().unwrap();
        let checked = qualification::CheckedAttestation::read(
            &fixture.attestation_path,
            fixture.relay(),
            now,
        )
        .unwrap();
        let authority = fixture.authority(&checked);
        let result = plan
            .execute(
                &fixture.owner.connector,
                &authority,
                authority.check().unwrap(),
            )
            .await;
        assert!(
            matches!(result, Err(Error::Transport(category)) if category == expected),
            "fault {fault:?} at {expected} returned {:?}",
            result.err()
        );
        if let Some(fault) = fault {
            assert!(
                transport.qualification_fault_fired(),
                "fixture fault not triggered {fault:?}"
            );
        }
        assert!(!fixture.record().exists());
        transport.stop().await;
    }
}

#[tokio::test]
async fn peer_qualification_accepts_correlated_empty_wide_history() {
    for subscription in ["control-wide", "group-wide"] {
        let plan = probe::Plan::new().unwrap();
        let (accounts, route) = plan.relay_registration();
        let mut relay = Relay::start_peer_qualification(accounts, route).await;
        relay.qualification_fault(QualificationFault::EmptyHistory(subscription));
        let fixture = QualificationFixture::new(&relay);
        let checked = qualification::CheckedAttestation::read(
            &fixture.attestation_path,
            fixture.relay(),
            qualification::wall().unwrap(),
        )
        .unwrap();
        let authority = fixture.authority(&checked);
        let completed = plan
            .execute(
                &fixture.owner.connector,
                &authority,
                authority.check().unwrap(),
            )
            .await
            .unwrap();
        assert!(relay.qualification_fault_fired());
        qualification::issue(&authority, completed, &fixture.issuer_key()).unwrap();
        assert!(fixture.record().is_file());
        relay.stop().await;
    }
}

#[tokio::test]
async fn peer_qualification_rejects_config_and_ca_drift_before_network() {
    let plan = probe::Plan::new().unwrap();
    let (accounts, route) = plan.relay_registration();
    let mut transport = Relay::start_peer_qualification(accounts, route).await;
    let fixture = QualificationFixture::new(&transport);
    let now = qualification::wall().unwrap();
    let checked =
        qualification::CheckedAttestation::read(&fixture.attestation_path, fixture.relay(), now)
            .unwrap();
    let authority = fixture.authority(&checked);
    let config_path = fixture.owner.profile.lease.root().join("config.json");
    let config = fs::read(&config_path).unwrap();
    let mut different: serde_json::Value = serde_json::from_slice(&config).unwrap();
    different["peer"]["relay"] = json!("wss://also.invalid/");
    fs::write(&config_path, different.to_string()).unwrap();
    assert!(matches!(
        authority.check(),
        Err(Error::Identity("peer_profile_changed"))
    ));
    fs::write(&config_path, config).unwrap();

    let ca_path = fixture.owner.profile.lease.root().join("peer-relay-ca.der");
    let ca_bytes = fs::read(&ca_path).unwrap();
    fs::write(&ca_path, Authority::new().der.as_ref()).unwrap();
    assert!(matches!(
        authority.check(),
        Err(Error::Transport("peer_tls_ca"))
    ));
    fs::write(&ca_path, &ca_bytes).unwrap();

    let manifest = fixture.owner.profile.lease.root().join("profile.json");
    let manifest_bytes = fs::read(&manifest).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    changed["device_slot"] = json!("54".repeat(16));
    fs::write(&manifest, changed.to_string()).unwrap();
    assert!(matches!(
        authority.check(),
        Err(Error::Identity("peer_profile_changed"))
    ));
    fs::write(&manifest, &manifest_bytes).unwrap();
    assert!(authority.check().is_ok());

    let new_ca = fixture.temp.path().join("same-ca");
    private_write(&new_ca, &ca_bytes);
    fs::rename(new_ca, &ca_path).unwrap();
    assert!(matches!(
        authority.check(),
        Err(Error::Transport("peer_tls_ca"))
    ));

    let new_manifest = fixture.temp.path().join("same-manifest");
    private_write(&new_manifest, &manifest_bytes);
    fs::rename(new_manifest, manifest).unwrap();
    assert!(matches!(
        authority.check(),
        Err(Error::Identity("peer_profile_changed"))
    ));
    assert!(!transport.authenticated(accounts[0]));
    assert!(!fixture.record().exists());
    transport.stop().await;
}

#[tokio::test]
async fn peer_qualification_credential_failure_precedes_tty_and_preserves_files() {
    use std::os::unix::fs::PermissionsExt;
    let plan = probe::Plan::new().unwrap();
    let (accounts, route) = plan.relay_registration();
    let mut transport = Relay::start_peer_qualification(accounts, route).await;
    let fixture = QualificationFixture::new(&transport);
    let root = fixture.owner.profile.lease.root().to_owned();
    let record = fixture.record();
    let managed = root.join("peer-relay-attestation.json");
    private_write(&record, b"prior local record");
    private_write(&managed, b"prior managed attestation");
    let QualificationFixture {
        temp,
        owner,
        attestation_path,
        ..
    } = fixture;
    drop(owner);
    let manifest_path = root.join("profile.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["backend"] = json!("keychain");
    let manifest = serde_json::to_vec(&manifest).unwrap();
    fs::write(&manifest_path, &manifest).unwrap();
    let helper = temp.path().join("provider-fixture");
    let trace_path = temp.path().join("provider-trace");
    let read_and_trace = format!(
        "IFS= read -r request\nprintf '%s' \"$request\" > '{}'",
        trace_path.display()
    );
    private_write(&helper, b"#!/bin/sh\n");
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    for (reply, timed_out) in [
        (
            r#"printf '%s' '{"version":1,"ok":false,"error":"locked_or_denied"}'"#,
            false,
        ),
        ("exec /bin/sleep 30", true),
    ] {
        fs::write(&helper, format!("#!/bin/sh\n{read_and_trace}\n{reply}\n")).unwrap();
        let result = timeout(
            Duration::from_secs(14),
            qualify(temp.path(), "test", attestation_path.clone(), &helper),
        )
        .await
        .unwrap();
        if timed_out {
            assert!(matches!(result, Err(Error::Timeout)));
        } else {
            assert!(matches!(result, Err(Error::Identity("locked_or_denied"))));
        }
        let trace: serde_json::Value =
            serde_json::from_slice(&fs::read(&trace_path).unwrap()).unwrap();
        fs::remove_file(&trace_path).unwrap();
        assert_eq!(trace["action"], "load");
        assert_eq!(trace["reference"]["purpose"], "sqlcipher");
        assert_eq!(fs::read(&record).unwrap(), b"prior local record");
        assert_eq!(fs::read(&managed).unwrap(), b"prior managed attestation");
        assert_eq!(fs::read(&manifest_path).unwrap(), manifest);
        assert_eq!(transport.qualification_connections(), 0);
    }
    transport.stop().await;
}

#[tokio::test]
#[ignore = "requires MARMISH_TEST_PEER_CLI pointing to the matching native marmish executable"]
async fn peer_qualification_native_cli_requires_exact_controlling_tty_confirmation() {
    use std::{io::Write as _, process::Stdio};
    use tokio::io::{AsyncReadExt as _, BufReader};
    use tokio::process::Command;

    let binary = std::env::var_os("MARMISH_TEST_PEER_CLI").expect("matching native CLI executable");
    let plan = probe::Plan::new().unwrap();
    let (decoys, decoy_route) = plan.relay_registration();
    let mut transport = Relay::start_peer_qualification(decoys, decoy_route).await;
    let fixture = QualificationFixture::new(&transport);
    let QualificationFixture {
        temp,
        owner,
        attestation_path,
        ..
    } = fixture;
    let record = owner
        .profile
        .lease
        .root()
        .join("peer-relay-qualification.json");
    drop(owner);

    let cli = || {
        let mut command = Command::new(&binary);
        command
            .arg("--state-dir")
            .arg(temp.path())
            .args(["-i", "test", "peer", "relay"]);
        command.env_clear().kill_on_drop(true);
        command
    };
    let show = || async {
        let output = timeout(TEST_TIMEOUT, cli().arg("show").output())
            .await
            .unwrap()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    assert_eq!(show().await["status"], "unqualified");
    let mut no_terminal = cli();
    no_terminal
        .args(["qualify", "--attestation"])
        .arg(&attestation_path)
        .stdin(Stdio::null());
    // SAFETY: setsid is async-signal-safe and detaches the child from any
    // controlling terminal inherited by this opt-in test runner.
    unsafe {
        no_terminal.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let without_tty = timeout(TEST_TIMEOUT, no_terminal.output())
        .await
        .unwrap()
        .unwrap();
    assert!(!without_tty.status.success());
    assert!(without_tty.stdout.is_empty());
    assert!(String::from_utf8_lossy(&without_tty.stderr).contains("no_tty"));
    assert_eq!(transport.qualification_connections(), 0);
    assert!(!record.exists());

    for affirmative in [false, true] {
        let (mut master, slave) = crate::mdk_adapter::open_cli_pty();
        let mut command = cli();
        command
            .args(["qualify", "--attestation"])
            .arg(&attestation_path)
            .stdin(slave)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: after fork the closure makes only async-signal-safe tty
        // calls and returns immediately on success or failure.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1
                {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let child = command.spawn().unwrap();
        drop(command);
        let mut reader = BufReader::new(tokio::fs::File::from_std(master.try_clone().unwrap()));
        let mut bytes = Vec::new();
        timeout(TEST_TIMEOUT, async {
            while bytes.len() < 4096 {
                bytes.push(reader.read_u8().await.unwrap());
                if bytes.ends_with(b"authorize the bounded relay probe: ") {
                    return;
                }
            }
            panic!("bounded foreground prompt missing");
        })
        .await
        .unwrap();
        assert_eq!(transport.qualification_connections(), 0);
        assert!(!record.exists());
        let prompt = std::str::from_utf8(&bytes).unwrap();
        if affirmative {
            let keys = prompt
                .lines()
                .find_map(|line| {
                    let registration = line.strip_prefix("Register disposable writer ")?;
                    let (writer, recipient) = registration
                        .strip_suffix(" for this test.")?
                        .split_once(" and recipient ")?;
                    Some([
                        nostr::key::PublicKey::parse(writer).unwrap(),
                        nostr::key::PublicKey::parse(recipient).unwrap(),
                    ])
                })
                .unwrap();
            let route = prompt
                .lines()
                .find_map(|line| {
                    let route = line
                        .strip_prefix("Authorize the two registered principals on Group route ")?;
                    hex::decode(route.strip_suffix('.')?).ok()?.try_into().ok()
                })
                .unwrap();
            transport.qualification_register(keys, route);
        }
        let suffix = if affirmative { "" } else { " " };
        writeln!(master, "qualify {}{suffix}", transport.url).unwrap();
        let output = timeout(Duration::from_secs(35), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(output.status.success(), affirmative);
        if affirmative {
            assert!(output.stderr.is_empty());
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["status"],
                "qualified"
            );
            assert_eq!(transport.qualification_connections(), 4);
            assert_eq!(show().await["status"], "current_metadata");
            assert!(record.exists());
            let lease = Arc::new(ProfileLease::acquire_ready(temp.path(), "test").unwrap());
            let key = lease
                .peer_qualification_key(
                    Path::new("/unused"),
                    std::time::Instant::now() + TEST_TIMEOUT,
                )
                .unwrap();
            let profile = ProfileFiles::open(lease).unwrap();
            let config = profile.command_snapshot().unwrap().0;
            let relay = config.bound_peer_relay(&config).unwrap();
            assert!(
                qualification::read_issued(&profile, relay, &key, qualification::wall().unwrap())
                    .is_ok()
            );
        } else {
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("peer_qualification_confirmation")
            );
            assert_eq!(transport.qualification_connections(), 0);
            assert!(!record.exists());
        }
    }
    transport.stop().await;
}
