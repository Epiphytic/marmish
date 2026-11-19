use super::*;
use crate::command_engine::Readiness;
use crate::dispatcher_fatal::Trip;
use crate::identity::BackendChoice;
use crate::transport::ArtiDirs;
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::Instant;

struct Fixture {
    owner: MdkOwner,
    profile: ProfileLease,
    material: IdentityMaterial,
    config: CommandConfig,
    directory: tempfile::TempDir,
}

impl Fixture {
    fn new(server: bool) -> Self {
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let mut profile = ProfileLease::acquire(directory.path(), "command-start").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let peer = nostr::key::Keys::generate().public_key().to_hex();
        let execution_clients = if server { vec![peer] } else { Vec::new() };
        let config = CommandConfig::from_bytes(
            &serde_json::to_vec(&json!({
                "version": 1,
                "role": if server { "server" } else { "client" },
                "relays": ["wss://command-start.invalid/"],
                "tor": "off",
                "cwd": "/",
                "limits": {"payload_retention": 3600},
                "aliases": {},
                "trusted_remotes": [],
                "execution_clients": execution_clients,
                "command_policy": {"version": 1, "defaults": {"allow_exec": ["/usr/bin/cat"]}},
            }))
            .unwrap(),
        )
        .unwrap();
        let connector = RelayConnector::new(
            config.relay_policy().clone(),
            ArtiDirs {
                state: profile.root().join("test-arti-state"),
                cache: profile.root().join("test-arti-cache"),
            },
        )
        .unwrap();
        let owner = MdkOwner::open(&profile, &material, connector, vec![]).unwrap();
        Self {
            owner,
            profile,
            material,
            config,
            directory,
        }
    }

    fn journal_snapshot(&self) -> (Vec<u8>, bool) {
        let hkdf = Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), self.material.sqlcipher_root());
        let mut key = Zeroizing::new([0u8; 32]);
        hkdf.expand(b"journal", key.as_mut()).unwrap();
        let reader = Connection::open_with_flags(
            self.profile.journal_database(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .unwrap();
        let raw_key = Zeroizing::new(format!("x'{}'", hex::encode(*key)));
        reader.pragma_update(None, "key", raw_key.as_str()).unwrap();
        let settings = reader
            .query_row(
                "SELECT settings FROM command_settings WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let bound = reader
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM command_engine_binding)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (settings, bound)
    }

    fn layout(&self) -> PathBuf {
        for name in ["marmish", "marmishd", "marmish-worker"] {
            let path = self.directory.path().join(name);
            // Only layout resolution and engine readiness are exercised here.
            std::fs::copy("/usr/bin/true", &path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500)).unwrap();
        }
        self.directory.path().join("marmishd")
    }

    fn start_with_layout(&mut self, startup: CommandStartup) -> Result<()> {
        let current = self.layout();
        let worker = self.directory.path().join("marmish-worker");
        self.owner.enable_commands_inner(
            &self.profile,
            &self.material,
            self.config.clone(),
            worker,
            startup,
            move || Ok(crate::native_layout::resolve(&current)?.helper),
        )
    }
}

#[tokio::test]
async fn command_startup_explicit_cut_keeps_exact_durable_prefix() {
    for cut in 1..=5 {
        let mut fixture = Fixture::new(true);
        let before = fixture.journal_snapshot();
        let (trip, fatal) = Trip::new();
        let trip = Arc::new(trip);
        let checks = Arc::new(AtomicUsize::new(0));
        let check_count = Arc::clone(&checks);
        let cut_trip = Arc::clone(&trip);
        let admission = Admission::new(&trip, move || {
            if check_count.fetch_add(1, Ordering::Relaxed) + 1 == cut {
                cut_trip.trip();
                return false;
            }
            true
        });

        assert!(matches!(
            fixture.start_with_layout(CommandStartup::Candidate(admission)),
            Err(Error::Storage("candidate_dispatcher_lost"))
        ));
        assert_eq!(checks.load(Ordering::Relaxed), cut);
        assert!(fatal.is_tripped());
        assert!(fixture.owner.commands.is_none());
        assert!(fixture.owner.take_engine_for_stop().is_none());
        assert!(!fixture.profile.root().join("commands").exists());
        let after = fixture.journal_snapshot();
        assert_eq!(after.0 != before.0, cut >= 3);
        assert!(!before.1);
        assert_eq!(after.1, cut >= 4);
    }
}

#[tokio::test]
async fn command_startup_late_refusal_retains_and_joins_the_started_worker() {
    let mut fixture = Fixture::new(true);
    let (trip, fatal) = Trip::new();
    let trip = Arc::new(trip);
    let checks = Arc::new(AtomicUsize::new(0));
    let frontend_checks = Arc::clone(&checks);
    let cut_trip = Arc::clone(&trip);
    let frontend = thread::current().id();
    let (worker_arrived, frontend_wait) = mpsc::sync_channel(1);
    let frontend_wait = Mutex::new(frontend_wait);
    let admission = Admission::new(&trip, move || {
        if thread::current().id() != frontend {
            worker_arrived.try_send(()).unwrap();
            return false;
        }
        if frontend_checks.fetch_add(1, Ordering::Relaxed) + 1 == 6 {
            frontend_wait
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            cut_trip.trip();
            return false;
        }
        true
    });

    assert!(matches!(
        fixture.start_with_layout(CommandStartup::Candidate(admission)),
        Err(Error::Storage("candidate_dispatcher_lost"))
    ));
    assert_eq!(checks.load(Ordering::Relaxed), 6);
    assert!(fatal.is_tripped());
    assert!(fixture.owner.commands.is_some());
    assert!(fixture.journal_snapshot().1);
    assert!(!fixture.profile.root().join("commands").exists());
    let engine = fixture
        .owner
        .take_engine_for_stop()
        .expect("retained engine");
    engine
        .into_fatal_join()
        .ok()
        .expect("exact fatal engine")
        .wait()
        .unwrap();
    assert!(fixture.owner.take_engine_for_stop().is_none());
}

#[tokio::test]
async fn command_startup_stock_layout_fails_after_durable_setup() {
    let mut fixture = Fixture::new(true);
    let before = fixture.journal_snapshot();
    assert!(matches!(
        fixture.owner.enable_commands(
            &fixture.profile,
            &fixture.material,
            fixture.config.clone(),
            PathBuf::new(),
        ),
        Err(Error::Invalid("worker_installation"))
    ));
    assert!(fixture.owner.commands.is_none());
    let after = fixture.journal_snapshot();
    assert_ne!(before.0, after.0);
    assert!(after.1);
}

#[tokio::test]
async fn command_startup_stock_server_starts_and_stops_its_real_engine() {
    let mut fixture = Fixture::new(true);
    let before = fixture.journal_snapshot();
    fixture.start_with_layout(CommandStartup::Stock).unwrap();
    let after = fixture.journal_snapshot();
    assert_ne!(before.0, after.0);
    assert!(after.1);
    let mut engine = fixture.owner.take_engine_for_stop().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.readiness() == Readiness::Opening && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(engine.readiness() == Readiness::Ready);
    engine.shutdown(deadline).unwrap();
}

#[tokio::test]
async fn command_startup_client_entrypoints_keep_the_no_engine_path() {
    for candidate in [false, true] {
        let mut fixture = Fixture::new(false);
        let before = fixture.journal_snapshot();
        let (trip, fatal) = Trip::new();
        let trip = Arc::new(trip);
        let checks = Arc::new(AtomicUsize::new(0));
        let check_count = Arc::clone(&checks);
        if candidate {
            fixture
                .owner
                .enable_candidate_commands(
                    &fixture.profile,
                    &fixture.material,
                    fixture.config.clone(),
                    PathBuf::new(),
                    Admission::new(&trip, move || {
                        check_count.fetch_add(1, Ordering::Relaxed);
                        true
                    }),
                )
                .unwrap();
        } else {
            fixture
                .owner
                .enable_commands(
                    &fixture.profile,
                    &fixture.material,
                    fixture.config.clone(),
                    PathBuf::new(),
                )
                .unwrap();
        }
        assert!(!fatal.is_tripped());
        assert_eq!(
            checks.load(Ordering::Relaxed),
            if candidate { 4 } else { 0 }
        );
        assert!(fixture.owner.commands.is_some());
        assert!(fixture.owner.take_engine_for_stop().is_none());
        let after = fixture.journal_snapshot();
        assert_ne!(before.0, after.0);
        assert!(after.1);
    }
}
