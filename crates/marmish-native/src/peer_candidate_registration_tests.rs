use super::{super::qualification, *};
use crate::{
    command_engine::Admission,
    dispatcher_fatal::Trip,
    identity::BackendChoice,
    mdk_adapter::MdkOwner,
    transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits},
};
use rusqlite::{Connection, ffi as sql};
use std::{
    ffi::CString,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Stdio},
    ptr,
    time::{Duration, Instant},
};

const DIR_ENV: &str = "MARMISH_TEST_CANDIDATE_REGISTRATION_ROOT";
const CASE_ENV: &str = "MARMISH_TEST_CANDIDATE_REGISTRATION_CASE";
const CHILD: &str = "peer_relay_trust::candidate_registration::tests::registration_subprocess";

fn prepare(lease: &mut ProfileLease) -> IdentityMaterial {
    lease
        .prepare(
            BackendChoice::File,
            Path::new("/unused"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap()
}

fn start_profile() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let mut lease = ProfileLease::acquire(root.path(), "test").unwrap();
    let material = prepare(&mut lease);
    let connector = RelayConnector::new(
        RelayPolicy::parse(
            &["wss://127.0.0.1:9/".into()],
            RouteMode::Off,
            TransportLimits::default(),
        )
        .unwrap(),
        ArtiDirs {
            state: lease.arti_state().into(),
            cache: lease.arti_cache().into(),
        },
    )
    .unwrap();
    let owner = MdkOwner::open(&lease, &material, connector, vec![]).unwrap();
    lease.mark_ready(owner.opened_binding()).unwrap();
    drop(owner);
    lease.cleanup_runtime().unwrap();
    let config = serde_json::json!({
        "version":1, "role":"both", "relays":["wss://127.0.0.1:9/"], "tor":"off",
        "trusted_remotes":[], "execution_clients":[], "command_policy":{"version":1},
        "cwd":null, "limits":{}, "aliases":{}, "peer":{
            "receive":{"mode":"manual"}, "relay":"wss://private.example/",
            "relay_trust":format!("spki-sha256:{}", "ab".repeat(32))
        }
    });
    super::super::tests::private_write(
        &lease.root().join("config.json"),
        &serde_json::to_vec(&config).unwrap(),
    );
    root
}

fn scope(root: &Path) -> (Arc<ProfileLease>, IdentityMaterial, CommandConfig) {
    let mut lease = ProfileLease::acquire_ready(root, "test").unwrap();
    let material = prepare(&mut lease);
    let config = CommandConfig::load(lease.root()).unwrap();
    (Arc::new(lease), material, config)
}

#[test]
fn candidate_registration_scope_and_irreversible_dispatch() {
    for mode in [
        "local",
        "ordinary",
        "host_storage_healthy",
        "host_storage_displaced",
        "host_preflight_open_displaced",
        "host_preflight_key_displaced",
        "host_preflight_close_displaced",
        "host_preflight_next_displaced",
        "host_journal_open_displaced",
        "host_journal_key_displaced",
        "topology",
        "displaced",
        "dead_lease",
        "displaced_before_take",
        "displaced_after_take",
        "duplicate_displacement",
        "wake_after_unlock",
        "delayed_notifier",
        "delayed_custody_notifier",
        "guard_unwind",
        "poison_backstop",
        "custody_under_lock",
    ] {
        let root = start_profile();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", CHILD, "--nocapture"])
            .env_clear()
            .env(DIR_ENV, root.path())
            .env(CASE_ENV, mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("registration test did not exit: {mode}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{mode}: {stdout}{}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[test]
#[ignore = "run only through candidate_registration_scope_and_irreversible_dispatch"]
fn registration_subprocess() {
    let Some(root) = std::env::var_os(DIR_ENV) else {
        return;
    };
    let mode = std::env::var(CASE_ENV).unwrap();
    let (lease, material, config) = scope(Path::new(&root));
    let profile = ProfileFiles::open(Arc::clone(&lease)).unwrap();
    let key = lease
        .peer_qualification_key_from_material(&material)
        .unwrap();
    // Authenticated on-disk records use the real checked route reader.
    // This controlled fixture has conducted no relay traffic.
    qualification::write_candidate_fixture(&profile, &key);
    drop(profile);
    let peer = lease.root().join("peer");
    assert!(!peer.exists());
    let prepare = || {
        ReceiverPreflight::prepare(&lease, &material, &config)
            .unwrap()
            .unwrap()
    };
    let prepared = prepare();
    assert!(!peer.exists(), "prepare must remain read-only");
    if mode == "local" {
        let mut wrong_root = zeroize::Zeroizing::new(*material.sqlcipher_root());
        wrong_root[0] ^= 1;
        let wrong = material.with_test_sqlcipher_root(*wrong_root);
        assert!(matches!(
            prepared.register(&lease, &wrong),
            Err(RegistrationFailure::Untouched)
        ));
        assert!(!peer.exists());
        local_refusals(&lease, &material, prepare);
        corrupt_records_refuse(&lease, &material, &config);
        lease.cleanup_runtime().unwrap();
        return;
    }

    let mut registered = prepared.register(&lease, &material).unwrap();
    let forwarding = registered.forwarding_verifier();
    registered.recheck(&lease, &material).unwrap();
    let fence = registered.take_command_fence().unwrap();
    let ordinary_fatal = forwarding.fatal();
    let ordinary = Admission::from_registered(fence);
    assert!(matches!(
        registered.take_command_fence(),
        Err(RegistrationFailure::LocalUnavailable)
    ));
    assert!(!ordinary_fatal.is_tripped());
    assert!(ordinary.admit());
    assert_eq!(fs::read_dir(&peer).unwrap().count(), 0);
    assert!(matches!(
        prepare().register(&lease, &material),
        Err(RegistrationFailure::LocalUnavailable)
    ));
    match mode.as_str() {
        "ordinary" => {
            let independent = forwarding.clone();
            let mut attached = registered.attach(&lease, &material).unwrap();
            let _quota = attached.quota();
            attached.recheck(&lease, &material).unwrap();

            let config = lease.root().join("config.json");
            let mut changed = fs::read(&config).unwrap();
            changed.push(b'\n');
            fs::write(config, changed).unwrap();
            assert_eq!(
                attached.recheck(&lease, &material),
                Err(RegistrationFailure::LocalUnavailable)
            );
            drop(attached);
            independent.recheck_default().unwrap();
            assert!(!ordinary_fatal.is_tripped());
            assert!(ordinary.admit());
            verify_forwarded_ordinary_and_fenced_names(lease.root());
        }
        "host_storage_healthy"
        | "host_storage_displaced"
        | "host_preflight_open_displaced"
        | "host_preflight_key_displaced"
        | "host_preflight_close_displaced"
        | "host_preflight_next_displaced"
        | "host_journal_open_displaced"
        | "host_journal_key_displaced" => {
            verify_host_storage(&ordinary, &lease, &material, &mode);
        }
        "topology" => {
            let mut attached = registered.attach(&lease, &material).unwrap();
            let moved = lease.root().join("moved");
            fs::rename(&peer, &moved).unwrap();
            fs::create_dir(&peer).unwrap();
            fs::set_permissions(&peer, fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(
                attached.recheck(&lease, &material),
                Err(RegistrationFailure::LocalUnavailable)
            );
            forwarding.recheck_default().unwrap();
            assert!(!ordinary_fatal.is_tripped());
        }
        "displaced" => {
            verify_sticky_displacement(registered, &lease, &material, &forwarding);
            assert!(matches!(
                prepare().register(&lease, &material),
                Err(RegistrationFailure::Indeterminate)
            ));
        }
        "displaced_before_take" => {
            let mut attached = registered.attach(&lease, &material).unwrap();
            let cancel = attached.route_scope.scope.ready.profile.cancel.clone();
            let config = lease.root().join("config.json");
            let mut changed = fs::read(&config).unwrap();
            changed.push(b'\n');
            fs::write(config, changed).unwrap();
            displace_default();
            assert!(matches!(
                attached.take_route(&lease, &material),
                Err(RegistrationFailure::Indeterminate)
            ));
            assert!(attached.route_scope.route.is_some());
            assert!(!cancel.is_cancelled());
        }
        "displaced_after_take" => {
            let mut attached = registered.attach(&lease, &material).unwrap();
            let cancel = attached.route_scope.scope.ready.profile.cancel.clone();
            let (quota, mut control) = attached.actor_parts();
            assert!(std::ptr::eq(quota, control.quota));
            assert!(matches!(
                control.take_route_checked(&lease, &material, || {
                    assert!(!cancel.is_cancelled());
                    displace_default();
                }),
                Err(RegistrationFailure::Indeterminate)
            ));
            assert!(attached.route_scope.route.is_none());
            assert!(cancel.is_cancelled());
        }
        "duplicate_displacement" => {
            let mut attached = registered.attach(&lease, &material).unwrap();
            let _route = attached.take_route(&lease, &material).unwrap();
            let duplicate = prepare();
            displace_default();
            assert!(!ordinary_fatal.is_tripped());
            assert!(matches!(
                duplicate.register(&lease, &material),
                Err(RegistrationFailure::Indeterminate)
            ));
            assert!(ordinary_fatal.is_tripped());
            assert!(forwarding.fatal().is_tripped());
        }
        "wake_after_unlock"
        | "delayed_notifier"
        | "delayed_custody_notifier"
        | "guard_unwind"
        | "poison_backstop"
        | "custody_under_lock" => {
            let (_, unrelated) = Trip::new();
            quota::verify_notification_case(&mode, &forwarding, || {
                ordinary.report_test_custody_failure();
                assert!(ordinary_fatal.is_tripped());
                assert!(forwarding.fatal().is_tripped());
                assert!(!ordinary.admit());
            });
            assert!(ordinary_fatal.is_tripped());
            assert!(!unrelated.is_tripped());
        }
        "dead_lease" => {
            let RegisteredReceiver {
                scope: old_scope,
                dispatch,
                route,
                ..
            } = registered;
            lease.cleanup_runtime().unwrap();
            drop(old_scope);
            drop(route);
            drop(lease);
            let (new, material, config) = scope(Path::new(&root));
            let mut prepared = ReceiverPreflight::prepare(&new, &material, &config)
                .unwrap()
                .unwrap();
            let directories = prepared.admit_directory(&new, &material).unwrap();
            assert_eq!(
                dispatch.recheck_lease(&new, &directories),
                Err("dispatch_lease_conflict")
            );
            forwarding.recheck_default().unwrap();
            // With the old lease gone, the process still cannot register again.
            assert!(matches!(
                prepared.register(&new, &material),
                Err(RegistrationFailure::LocalUnavailable)
            ));
            new.cleanup_runtime().unwrap();
            return;
        }
        _ => panic!("unrecognized registration test case"),
    }
    if ordinary_fatal.is_tripped() {
        assert!(!ordinary.admit());
    }
    if mode.starts_with("displaced") || mode.ends_with("_displaced") {
        assert!(ordinary_fatal.is_tripped());
        assert!(forwarding.fatal().is_tripped());
    }
    lease.cleanup_runtime().unwrap();
}

fn verify_host_storage(
    admission: &Admission,
    lease: &ProfileLease,
    material: &IdentityMaterial,
    mode: &str,
) {
    use crate::journal::Journal;
    use hkdf::Hkdf;
    use sha2::Sha256;
    use zeroize::Zeroizing;

    use std::sync::atomic::{AtomicUsize, Ordering};

    let limit = match mode {
        // Admission at scanner entry, before file access, twice at raw open
        // and before keying; the next check is the first read-only close.
        "host_preflight_open_displaced" => Some(4),
        "host_preflight_key_displaced" => Some(5),
        "host_preflight_close_displaced" => Some(6),
        "host_preflight_next_displaced" => Some(7),
        // Before file preparation, twice at raw open, then before keying.
        "host_journal_open_displaced" => Some(3),
        "host_journal_key_displaced" => Some(4),
        _ => None,
    };
    let visits = Arc::new(AtomicUsize::new(0));
    let fence = if let Some(limit) = limit {
        let visits = visits.clone();
        let admission = admission.clone();
        marmish_engine::ConnectionFence::new(
            move || {
                if visits.fetch_add(1, Ordering::Relaxed) + 1 == limit {
                    displace_default();
                }
                admission.admit()
            },
            || panic!("unexpected SQLite close failure"),
        )
    } else {
        admission.connection_fence()
    };
    if mode.starts_with("host_preflight_") {
        let following = lease.journal_database();
        let journal_bytes = || {
            ["", "-wal", "-shm"].map(|suffix| {
                let mut path = following.as_os_str().to_os_string();
                path.push(suffix);
                match fs::read(path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => panic!("reading fixture Journal files: {error}"),
                }
            })
        };
        let before = journal_bytes();
        assert!(before[0].is_some());
        assert!(matches!(
            crate::native_preflight::check_with_fence(lease, material, Some(&fence)),
            Err(crate::Error::Storage("dispatcher_admission"))
        ));
        assert_eq!(visits.load(Ordering::Relaxed), limit.unwrap());
        assert_eq!(journal_bytes(), before);
        return;
    }
    if !mode.starts_with("host_journal_") {
        crate::native_preflight::check_with_fence(lease, material, Some(&fence)).unwrap();
    }
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), material.sqlcipher_root())
        .expand(b"journal", key.as_mut())
        .unwrap();
    let account = material.public_key().to_hex();
    let path = lease.journal_database();
    if mode.starts_with("host_journal_") {
        let path = path.with_file_name("refused.sqlite3");
        assert!(matches!(
            Journal::open_with_fence(&path, &key, &account, Some(&fence)),
            Err(crate::Error::Storage("dispatcher_admission"))
        ));
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
        assert_eq!(visits.load(Ordering::Relaxed), limit.unwrap());
        return;
    }
    let journal = Journal::open_with_fence(path, &key, &account, Some(&fence)).unwrap();
    if mode == "host_storage_displaced" {
        let unopened = path.with_file_name("refused.sqlite3");
        displace_default();
        assert!(matches!(
            journal.close(),
            Err(crate::Error::Storage("dispatcher_admission"))
        ));
        assert!(!admission.admit());
        assert!(matches!(
            Journal::open_with_fence(&unopened, &key, &account, Some(&fence)),
            Err(crate::Error::Storage("dispatcher_admission"))
        ));
        assert!(!unopened.exists());
        assert!(matches!(
            crate::native_preflight::check_with_fence(lease, material, Some(&fence)),
            Err(crate::Error::Storage("dispatcher_admission"))
        ));
    } else {
        journal.close().unwrap();
        Journal::open_with_fence(path, &key, &account, Some(&fence))
            .unwrap()
            .close()
            .unwrap();
        assert!(admission.admit());
    }
}

fn displace_default() {
    // SAFETY: the isolated subprocess owns the default mutation and the
    // initialized stock VFS and registered shim outlive the subprocess.
    unsafe {
        let stock = sql::sqlite3_vfs_find(c"unix".as_ptr());
        assert!(!stock.is_null());
        assert_ne!(sql::sqlite3_vfs_find(ptr::null()), stock);
        assert_eq!(sql::sqlite3_vfs_register(stock, 1), sql::SQLITE_OK);
    }
}

fn verify_sticky_displacement(
    registered: RegisteredReceiver,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    forwarding: &ForwardingVerifier,
) {
    let independent = forwarding.clone();
    let mut attached = registered.attach(lease, material).unwrap();
    // SAFETY: the isolated test owns these default mutations and the
    // stock and registered descriptors live for this process.
    let wrapper = unsafe {
        let wrapper = sql::sqlite3_vfs_find(ptr::null());
        let stock = sql::sqlite3_vfs_find(c"unix".as_ptr());
        assert_ne!(wrapper, stock);
        assert_eq!(sql::sqlite3_vfs_register(stock, 1), sql::SQLITE_OK);
        wrapper
    };
    assert_eq!(
        attached.recheck(lease, material),
        Err(RegistrationFailure::Indeterminate)
    );
    assert_eq!(
        forwarding.recheck_default(),
        Err(RegistrationFailure::Indeterminate)
    );
    // SAFETY: the wrapper was retained for process life by registration.
    unsafe {
        assert_eq!(sql::sqlite3_vfs_register(wrapper, 1), sql::SQLITE_OK);
        assert_eq!(sql::sqlite3_vfs_find(ptr::null()), wrapper);
    }
    drop(attached);
    assert_eq!(
        independent.recheck_default(),
        Err(RegistrationFailure::Indeterminate)
    );
}

fn local_refusals(
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    prepare: impl Fn() -> ReceiverPreflight,
) {
    let peer = lease.root().join("peer");
    fs::write(&peer, b"file").unwrap();
    assert_untouched(prepare(), lease, material);
    fs::remove_file(&peer).unwrap();

    let other = tempfile::tempdir().unwrap();
    symlink(other.path(), &peer).unwrap();
    assert_untouched(prepare(), lease, material);
    fs::remove_file(&peer).unwrap();

    fs::create_dir(&peer).unwrap();
    fs::set_permissions(&peer, fs::Permissions::from_mode(0o755)).unwrap();
    assert_untouched(prepare(), lease, material);

    fs::set_permissions(&peer, fs::Permissions::from_mode(0o700)).unwrap();
    let directories = prepare().admit_directory(lease, material).unwrap();
    fs::rename(&peer, lease.root().join("moved")).unwrap();
    fs::create_dir(&peer).unwrap();
    fs::set_permissions(&peer, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(directories.identities(lease).is_err());

    let pending = prepare();
    let manifest = lease.root().join("profile.json");
    let original = fs::read(&manifest).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    fs::write(&manifest, &changed).unwrap();
    assert_untouched(pending, lease, material);
    fs::write(manifest, original).unwrap();

    let pending = prepare();
    let config = lease.root().join("config.json");
    let original = fs::read(&config).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    fs::write(&config, changed).unwrap();
    assert_untouched(pending, lease, material);
    fs::write(config, original).unwrap();
}

fn corrupt_records_refuse(
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    config: &CommandConfig,
) {
    let file = lease.root().join("peer-relay-qualification.json");
    let original = fs::read_to_string(&file).unwrap();
    let mut prior: serde_json::Value = serde_json::from_str(&original).unwrap();
    let tag = prior["issuer_tag"].as_str().unwrap();
    let mut bad = tag.as_bytes().to_vec();
    bad[0] = if bad[0] == b'0' { b'1' } else { b'0' };
    let changed_tag = String::from_utf8(bad).unwrap();
    let tampered = original.replace(tag, &changed_tag);
    let missing = original.replace(&format!(r#","issuer_tag":"{tag}""#), "");
    assert_ne!(missing, original);
    prior["schema"] = serde_json::json!(2);
    prior.as_object_mut().unwrap().remove("issuer_tag");
    prior.as_object_mut().unwrap().remove("build_commit");
    for data in [tampered, missing, serde_json::to_string(&prior).unwrap()] {
        fs::write(&file, data).unwrap();
        assert!(ReceiverPreflight::prepare(lease, material, config).is_err());
    }
    fs::write(file, original).unwrap();
}

fn assert_untouched(
    prepared: ReceiverPreflight,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
) {
    assert!(matches!(
        prepared.register(lease, material),
        Err(RegistrationFailure::Untouched)
    ));
    // SAFETY: SQLite is initialized by registration and this only queries it.
    unsafe {
        assert_eq!(
            sql::sqlite3_vfs_find(ptr::null()),
            sql::sqlite3_vfs_find(c"unix".as_ptr())
        );
    }
}

fn verify_forwarded_ordinary_and_fenced_names(root: &Path) {
    let forwarded = root.join("ordinary-test.db");
    let database = Connection::open(&forwarded).unwrap();
    database
        .execute_batch("CREATE TABLE t (v); INSERT INTO t VALUES (11);")
        .unwrap();
    drop(database);
    let read = Connection::open(&forwarded).unwrap();
    assert_eq!(
        read.query_row("SELECT v from t", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        11
    );
    for main in [
        "provisional-known.db",
        "provisional-unknown.db",
        "receiver-join-snapshot.db",
    ] {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let name = root.join("peer").join(format!("{main}{suffix}"));
            assert!(Connection::open(&name).is_err());
            let c_name = CString::new(name.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: bound default lives for process lifetime. Refusal never
            // dereferences or changes any fixture database file.
            unsafe {
                let vfs = sql::sqlite3_vfs_find(ptr::null());
                assert_eq!(
                    (*vfs).xDelete.unwrap()(vfs, c_name.as_ptr(), 0),
                    sql::SQLITE_IOERR_DELETE
                );
            }
            assert!(!name.exists());
        }
    }
}

pub(crate) fn package_fixture_root() -> tempfile::TempDir {
    start_profile()
}

pub(crate) fn package_fixture_registration(
    root: &Path,
) -> (Arc<ProfileLease>, IdentityMaterial, RegisteredReceiver) {
    let (lease, material, config) = scope(root);
    let profile = ProfileFiles::open(Arc::clone(&lease)).unwrap();
    let key = lease
        .peer_qualification_key_from_material(&material)
        .unwrap();
    if !lease.root().join("peer-relay-qualification.json").exists() {
        qualification::write_candidate_fixture(&profile, &key);
    }
    drop(profile);
    let registered = ReceiverPreflight::prepare(&lease, &material, &config)
        .unwrap()
        .unwrap()
        .register(&lease, &material)
        .unwrap();
    (lease, material, registered)
}
