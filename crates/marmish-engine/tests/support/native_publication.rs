//! Actual abort/restart coverage of the native marker transaction.
use super::*;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
const ROOT: &str = "MARMISH_NATIVE_PUBLICATION_ROOT";
const WORKER: &str = "MARMISH_NATIVE_PUBLICATION_WORKER";
const MIGRATION: &str = "MARMISH_NATIVE_PUBLICATION_MIGRATION";
const TEST: &str = "native_publication::native_publication_crash_boundaries_recover_exact_binding";

fn run_fault_child(path: &std::ffi::OsStr) {
    let fixture = Path::new(path);
    let worker = std::env::var_os(WORKER).unwrap();
    let policy = policy(fixture, Path::new(&worker));
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) }, 0);
    if std::env::var_os(MIGRATION).is_some() {
        let bytes = zeroize::Zeroizing::new(fs::read(fixture.join("fixture-master")).unwrap());
        let key: [u8; 32] = bytes.as_slice().try_into().unwrap();
        let mut guard =
            Engine::prepare_native_migration(policy, binding(), ProvidedMaster::new(key)).unwrap();
        guard.commit_native().unwrap();
    } else {
        drop(Engine::open_with_master(policy, binding(), master()).unwrap());
    }
    panic!("configured publication fault did not abort");
}

fn prepare_fixture(
    dir: &Path,
    policy: &EnginePolicy,
    migration: bool,
) -> zeroize::Zeroizing<[u8; 32]> {
    let mut key = zeroize::Zeroizing::new([63; 32]);
    if migration {
        let mut engine = reopen(|| Engine::open(policy.clone())).unwrap();
        run(&mut engine);
        // Prove worker teardown, without deleting encrypted state.
        drop(engine.into_cleanup().unwrap());
        let bytes =
            zeroize::Zeroizing::new(fs::read(policy.state_root.join("engine.key")).unwrap());
        key.copy_from_slice(&bytes[8..]);
        fs::write(dir.join("fixture-master"), key.as_slice()).unwrap();
        fs::set_permissions(
            dir.join("fixture-master"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    } else {
        fs::create_dir(&policy.state_root).unwrap();
        fs::set_permissions(&policy.state_root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    key
}

fn crash_at(dir: &Path, policy: &EnginePolicy, worker: &Path, migration: bool, point: &str) {
    let fault = policy.state_root.join("fault");
    let mut sentinel = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&fault)
        .unwrap();
    sentinel.write_all(point.as_bytes()).unwrap();
    sentinel.sync_all().unwrap();
    fs::set_permissions(&fault, fs::Permissions::from_mode(0o600)).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST])
        .env_clear()
        .env(ROOT, dir)
        .env(WORKER, worker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if migration {
        command.env(MIGRATION, "1");
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            child.wait().unwrap();
            panic!("publication fault child exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        status.signal(),
        Some(libc::SIGABRT),
        "publication seam {point} did not abort"
    );
    fs::remove_file(fault).unwrap();
}

fn recover(policy: &EnginePolicy, key: &[u8; 32], migration: bool) {
    if migration {
        let mut guard = reopen(|| {
            Engine::prepare_native_migration(policy.clone(), binding(), ProvidedMaster::new(*key))
        })
        .unwrap();
        guard.commit_native().unwrap();
        drop(guard);
    }
    let mut engine =
        reopen(|| Engine::open_with_master(policy.clone(), binding(), ProvidedMaster::new(*key)))
            .unwrap();
    assert!(policy.state_root.join("engine.native.json").exists());
    assert!(!policy.state_root.join(".engine.native.pending").exists());
    assert!(!policy.state_root.join("engine.key").exists());
    if migration {
        let ctx = engine
            .authorize(P, EndpointId([53; 32]), S, unsafe { libc::getuid() })
            .unwrap();
        assert_eq!(
            engine.status(&ctx, InvocationId([54; 16])).unwrap().outcome,
            Some(ExitOutcome::Code(0))
        );
        drop(engine.into_cleanup().unwrap());
    }
}

#[test]
fn native_publication_crash_boundaries_recover_exact_binding() {
    if let Some(path) = std::env::var_os(ROOT) {
        run_fault_child(&path);
    }
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    for migration in [false, true] {
        let points = [
            "AfterNativePendingCreate",
            "AfterNativePendingWrite",
            "AfterNativePendingSync",
            "AfterNativePublish",
            "AfterNativeDirectorySync",
            "AfterNativeLegacyUnlink",
            "AfterNativeLegacyDirectorySync",
        ];
        for point in points.into_iter().take(if migration { 7 } else { 5 }) {
            let dir = executable::private_tempdir();
            let policy = policy(dir.path(), &worker.path);
            let key = prepare_fixture(dir.path(), &policy, migration);
            crash_at(dir.path(), &policy, &worker.path, migration, point);
            recover(&policy, &key, migration);
        }
    }
}
