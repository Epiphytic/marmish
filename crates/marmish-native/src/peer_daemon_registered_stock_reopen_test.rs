//! Stock libtest child, launched only with the parent's private WSS fixture.
use super::*;
use crate::transport::{ArtiDirs, RelayConnector};
use rustls::pki_types::CertificateDer;
use std::io::Read;

const PROFILE: &str = "test";

pub(super) fn run() {
    run_native(Duration::from_secs(90), async || {
        let setting = |name| std::env::var_os(name).expect("stock child fixture input");
        let root = PathBuf::from(setting("MARMISH_TEST_STOCK_REOPEN_ROOT"));
        let layout = PathBuf::from(setting("MARMISH_TEST_STOCK_REOPEN_DAEMON"));
        let candidate = PathBuf::from(setting("MARMISH_TEST_STOCK_REOPEN_CANDIDATE"));
        assert!(matches!(
            crate::native_layout::resolve(&candidate),
            Err(crate::Error::Invalid("install_manifest_class"))
        ));
        let relay = setting("MARMISH_TEST_STOCK_REOPEN_RELAY")
            .into_string()
            .unwrap();
        let ca = PathBuf::from(setting("MARMISH_TEST_STOCK_REOPEN_CA"));
        assert!(root.is_absolute() && layout.is_absolute() && ca.starts_with(&root));
        let profile = root.join(PROFILE);
        let mut roots = rustls::RootCertStore::empty();
        roots.add(fs_read_fixture_ca(&ca)).unwrap();
        let policy = RelayPolicy::parse(&[relay], RouteMode::Off, TransportLimits::default())
            .expect("disposable stock WSS policy");
        let connector = RelayConnector::for_loopback_fixture(
            policy.clone(),
            ArtiDirs {
                state: profile.join("arti_state"),
                cache: profile.join("arti_cache"),
            },
            policy.urls()[0].clone(),
            roots,
        )
        .expect("disposable stock WSS trust");
        let config = Config {
            state_dir: root,
            identity: PROFILE.into(),
            backend: BackendChoice::File,
            helper: crate::native_layout::resolve(&layout).unwrap().helper,
            relay_policy: policy,
            allowed_peers: vec![],
            retention: Duration::from_secs(86_400),
            max_commands: None,
            max_uptime: None,
        };
        let (stop, receiver) = oneshot::channel();
        let input = tokio::task::spawn_blocking(move || {
            let mut input = [0];
            let result = std::io::stdin().read_exact(&mut input);
            let _ = stop.send(());
            assert!(result.is_ok() && input == *b"S", "stock child stop byte");
        });
        let fixture = PeerFixture {
            bootstrap: None,
            connector: Some(connector),
            stop: receiver,
            layout,
        };
        daemon::run_peer_fixture(config, None, fixture)
            .await
            .expect("clean stock fixture server");
        input.await.expect("stock child normal stop");
    });
}

fn fs_read_fixture_ca(path: &Path) -> CertificateDer<'static> {
    let mut bytes = Vec::new();
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .unwrap()
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!((1..=16 * 1024).contains(&bytes.len()));
    CertificateDer::from(bytes)
}
