use super::*;
use nostr::key::Keys;
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use serde_json::json;
use std::{fs, net::SocketAddr, path::Path, time::Duration};
use tokio::{
    net::TcpListener,
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

fn config_fixture(lease: &ProfileLease, origin: &str, trust: &str, mode: &str) -> CommandConfig {
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
        let mut ca = CertificateParams::default();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages.push(KeyUsagePurpose::KeyCertSign);
        let key = KeyPair::generate().unwrap();
        let der = ca.self_signed(&key).unwrap().der().clone();
        Self {
            der,
            issuer: Issuer::new(ca, key),
        }
    }

    fn server(&self, san: &str, key: &KeyPair, serial: u64) -> (TlsAcceptor, String) {
        let mut params = CertificateParams::new(vec![san.into()]).unwrap();
        params.serial_number = Some(serial.into());
        let cert = params.signed_by(key, &self.issuer).unwrap();
        let parsed = parse_der(cert.der().as_ref()).unwrap();
        let hash = hex::encode(Sha256::digest(parsed.public_key().raw));
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
