#![cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "support/executable.rs"]
mod executable;
use marmish_engine::*;
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
const P: PrincipalId = PrincipalId([51; 32]);
const S: ServerId = ServerId([52; 32]);
fn policy(root: &Path, worker: &Path) -> EnginePolicy {
    EnginePolicy {
        state_root: root.join("state"),
        worker_executable: worker.to_owned(),
        server: S,
        allowed_principals: vec![P],
        allow_shell_command: false,
        command_policy: None,
        cwd: Some(root.to_owned()),
        limits: EngineLimits::default(),
    }
}
fn binding() -> NativeBinding {
    NativeBinding::new([61; 32], [62; 32])
}
fn master() -> ProvidedMaster {
    ProvidedMaster::new([63; 32])
}
fn reopen<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match operation() {
            Err(EngineError::AlreadyOpen) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(10))
            }
            other => return other,
        }
    }
}
fn run(engine: &mut Engine) {
    let ctx = engine
        .authorize(P, EndpointId([53; 32]), S, unsafe { libc::getuid() })
        .unwrap();
    let token = engine.issue_execute_token(&ctx).unwrap();
    let id = InvocationId([54; 16]);
    #[cfg(target_os="linux")]
    let script=b"test ! -t 0 && test \"$(readlink /proc/$PPID/fd/0)\" = /dev/null && printf 'native;\\047\\n'".to_vec();
    #[cfg(target_os = "macos")]
    let script = b"test ! -t 0; printf 'native;\\047\\n'".to_vec();
    let request = CommandRequest::Exec {
        argv: vec![b"/bin/sh".to_vec(), b"-c".to_vec(), script],
    };
    engine.submit(&ctx, &token, id, &request).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let status = engine.status(&ctx, id).unwrap();
        if status.state == InvocationState::Exited {
            assert_eq!(status.outcome, Some(ExitOutcome::Code(0)));
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn native_binding_wrong_key_modes_and_restart() {
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let dir = executable::private_tempdir();
    let policy = policy(dir.path(), &worker.path);
    let mut engine =
        reopen(|| Engine::open_with_master(policy.clone(), binding(), master())).unwrap();
    assert!(!policy.state_root.join("engine.key").exists());
    assert_eq!(
        fs::metadata(policy.state_root.join("engine.native.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    run(&mut engine);
    drop(engine);
    let ciphertext = fs::read(policy.state_root.join("engine.sqlite3")).unwrap();
    assert!(reopen(|| Engine::open(policy.clone())).is_err());
    assert!(reopen(|| Engine::open_with_master(
        policy.clone(),
        NativeBinding::new([0; 32], [62; 32]),
        master()
    ))
    .is_err());
    assert!(reopen(|| Engine::open_with_master(
        policy.clone(),
        binding(),
        ProvidedMaster::new([9; 32])
    ))
    .is_err());
    assert!(
        fs::read(policy.state_root.join("engine.sqlite3")).unwrap() == ciphertext,
        "rejected key or mode changed encrypted database"
    );
    let mut engine =
        reopen(|| Engine::open_with_master(policy.clone(), binding(), master())).unwrap();
    let ctx = engine
        .authorize(P, EndpointId([53; 32]), S, unsafe { libc::getuid() })
        .unwrap();
    assert_eq!(
        engine.status(&ctx, InvocationId([54; 16])).unwrap().outcome,
        Some(ExitOutcome::Code(0))
    );
    drop(engine);
    // A native root cannot silently adopt a legacy file, even with matching bytes.
    fs::write(
        policy.state_root.join("engine.key"),
        [b"MRMSKEY1".as_slice(), &[63; 32]].concat(),
    )
    .unwrap();
    fs::set_permissions(
        policy.state_root.join("engine.key"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(reopen(|| Engine::open_with_master(policy.clone(), binding(), master())).is_err());
}
#[test]
fn migration_authenticates_quiesces_and_recovers_committed_mixed_state() {
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let dir = executable::private_tempdir();
    let policy = policy(dir.path(), &worker.path);
    let mut engine = reopen(|| Engine::open(policy.clone())).unwrap();
    run(&mut engine);
    drop(engine);
    let bytes = zeroize::Zeroizing::new(fs::read(policy.state_root.join("engine.key")).unwrap());
    let key: [u8; 32] = bytes[8..].try_into().unwrap();
    // Interrupted marker creation may leave a torn pending file alongside the
    // authenticated legacy key; only the explicit migration path may recover.
    let pending = policy.state_root.join(".engine.native.pending");
    fs::write(&pending, b"{\"version\":").unwrap();
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        reopen(|| Engine::prepare_native_migration(policy.clone(), binding(), master())).is_err()
    );
    let guard = reopen(|| {
        Engine::prepare_native_migration(policy.clone(), binding(), ProvidedMaster::new(key))
    })
    .unwrap();
    assert!(policy.state_root.join("engine.key").exists());
    assert!(!policy.state_root.join("engine.native.json").exists());
    // Before commitment, dropping the guard retains the legacy root for retry.
    drop(guard);
    let mut guard = reopen(|| {
        Engine::prepare_native_migration(policy.clone(), binding(), ProvidedMaster::new(key))
    })
    .unwrap();
    let mut changed = zeroize::Zeroizing::new(bytes.to_vec());
    changed[8] ^= 1;
    fs::write(policy.state_root.join("engine.key"), changed.as_slice()).unwrap();
    assert!(guard.commit_native().is_err());
    assert!(!policy.state_root.join("engine.native.json").exists());
    fs::write(policy.state_root.join("engine.key"), bytes.as_slice()).unwrap();
    guard.commit_native().unwrap();
    guard.commit_native().unwrap();
    assert!(!policy.state_root.join("engine.key").exists());
    drop(guard);
    let engine =
        reopen(|| Engine::open_with_master(policy.clone(), binding(), ProvidedMaster::new(key)))
            .unwrap();
    drop(engine);
    fs::write(policy.state_root.join("engine.key"), bytes.as_slice()).unwrap();
    // Also recover a published marker plus legacy key and torn staging record.
    fs::write(&pending, b"{").unwrap();
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(
        policy.state_root.join("engine.key"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(reopen(|| Engine::open_with_master(
        policy.clone(),
        binding(),
        ProvidedMaster::new(key)
    ))
    .is_err());
    let mut guard = reopen(|| {
        Engine::prepare_native_migration(policy.clone(), binding(), ProvidedMaster::new(key))
    })
    .unwrap();
    guard.commit_native().unwrap();
    drop(guard);
    let mut guard = reopen(|| {
        Engine::prepare_native_migration(policy.clone(), binding(), ProvidedMaster::new(key))
    })
    .unwrap();
    guard.commit_native().unwrap();
}

fn worker_attempt(
    worker: &Path,
    root: &Path,
    job: &str,
    frame: &[u8],
    keep_open: bool,
    expect_success: bool,
) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(worker)
        .arg(root)
        .arg(job)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(frame).unwrap();
    let held = if keep_open {
        Some(input)
    } else {
        drop(input);
        None
    };
    let until = Instant::now() + Duration::from_secs(7);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.success(), expect_success);
            break;
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("worker frame deadline exceeded");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(held);
}
#[test]
fn worker_rejects_truncated_trailing_wrong_binding_and_open_pipe_without_db_mutation() {
    use std::os::unix::fs::MetadataExt;
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let dir = executable::private_tempdir();
    let policy = policy(dir.path(), &worker.path);
    let mut engine =
        reopen(|| Engine::open_with_master(policy.clone(), binding(), master())).unwrap();
    run(&mut engine);
    drop(engine);
    let job = fs::read_dir(policy.state_root.join("jobs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .into_string()
        .unwrap();
    let db = fs::read(policy.state_root.join("engine.sqlite3")).unwrap();
    let mut frame = zeroize::Zeroizing::new([0u8; 145]);
    frame[..8].copy_from_slice(b"MRMKEY02");
    frame[8] = 1;
    for i in 0..16 {
        frame[16 + i] = u8::from_str_radix(&job[i * 2..i * 2 + 2], 16).unwrap();
    }
    let meta = fs::metadata(&policy.state_root).unwrap();
    frame[32..40].copy_from_slice(&meta.dev().to_le_bytes());
    frame[40..48].copy_from_slice(&meta.ino().to_le_bytes());
    frame[48..80].fill(61);
    frame[80..112].fill(62);
    frame[112..144].fill(63);
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..143],
        false,
        false,
    );
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        frame.as_slice(),
        false,
        false,
    );
    for index in [0, 9, 16, 40, 80] {
        frame[index] ^= 1;
        worker_attempt(
            &worker.path,
            &policy.state_root,
            &job,
            &frame[..144],
            false,
            false,
        );
        frame[index] ^= 1;
    }
    frame[48] ^= 1;
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..144],
        false,
        false,
    );
    frame[48] ^= 1;
    frame[32] ^= 1;
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..144],
        false,
        false,
    );
    frame[32] ^= 1;
    frame[8] = 0;
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..144],
        false,
        false,
    );
    frame[8] = 1;
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..144],
        true,
        false,
    );
    frame[112] ^= 1;
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..144],
        false,
        false,
    );
    assert!(
        fs::read(policy.state_root.join("engine.sqlite3")).unwrap() == db,
        "rejected frame changed encrypted database"
    );
    frame[112] ^= 1;
    worker_attempt(
        &worker.path,
        &policy.state_root,
        &job,
        &frame[..144],
        false,
        true,
    );
}

#[test]
fn native_pending_recovery_requires_pristine_root_and_exact_binding() {
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let source = executable::private_tempdir();
    let source_policy = policy(source.path(), &worker.path);
    drop(reopen(|| Engine::open_with_master(source_policy.clone(), binding(), master())).unwrap());
    let marker = fs::read(source_policy.state_root.join("engine.native.json")).unwrap();
    for partial in [b"".as_slice(), b"{\"version\":".as_slice()] {
        let torn = executable::private_tempdir();
        let torn_policy = policy(torn.path(), &worker.path);
        fs::create_dir(&torn_policy.state_root).unwrap();
        fs::set_permissions(&torn_policy.state_root, fs::Permissions::from_mode(0o700)).unwrap();
        let pending = torn_policy.state_root.join(".engine.native.pending");
        fs::write(&pending, partial).unwrap();
        fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
        drop(
            reopen(|| Engine::open_with_master(torn_policy.clone(), binding(), master())).unwrap(),
        );
        assert!(!pending.exists());
        assert!(torn_policy.state_root.join("engine.native.json").exists());
    }
    let target = executable::private_tempdir();
    let target_policy = policy(target.path(), &worker.path);
    fs::create_dir(&target_policy.state_root).unwrap();
    fs::set_permissions(&target_policy.state_root, fs::Permissions::from_mode(0o700)).unwrap();
    let pending = target_policy.state_root.join(".engine.native.pending");
    fs::write(&pending, &marker).unwrap();
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(reopen(|| Engine::open_with_master(
        target_policy.clone(),
        NativeBinding::new([0; 32], [62; 32]),
        master()
    ))
    .is_err());
    assert!(pending.exists());
    drop(reopen(|| Engine::open_with_master(target_policy.clone(), binding(), master())).unwrap());
    assert!(!pending.exists());
    assert!(!target_policy.state_root.join("engine.key").exists());
    let final_path = target_policy.state_root.join("engine.native.json");
    fs::rename(&final_path, &pending).unwrap();
    let db = fs::read(target_policy.state_root.join("engine.sqlite3")).unwrap();
    assert!(
        reopen(|| Engine::open_with_master(target_policy.clone(), binding(), master())).is_err()
    );
    assert!(
        fs::read(target_policy.state_root.join("engine.sqlite3")).unwrap() == db,
        "rejected marker recovery changed encrypted database"
    );
    fs::rename(&pending, &final_path).unwrap();
    fs::write(
        target_policy.state_root.join(".engine.key.pending"),
        b"unfinished legacy",
    )
    .unwrap();
    assert!(
        reopen(|| Engine::open_with_master(target_policy.clone(), binding(), master())).is_err()
    );
}
#[test]
fn migration_refuses_active_workers_and_leaves_file_mode_untouched() {
    if unsafe { libc::getuid() } == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let dir = executable::private_tempdir();
    let policy = policy(dir.path(), &worker.path);
    let mut engine = reopen(|| Engine::open(policy.clone())).unwrap();
    let ctx = engine
        .authorize(P, EndpointId([53; 32]), S, unsafe { libc::getuid() })
        .unwrap();
    let token = engine.issue_execute_token(&ctx).unwrap();
    let id = InvocationId([55; 16]);
    engine
        .submit(
            &ctx,
            &token,
            id,
            &CommandRequest::Exec {
                argv: vec![b"/bin/sleep".to_vec(), b"5".to_vec()],
            },
        )
        .unwrap();
    let bytes = zeroize::Zeroizing::new(fs::read(policy.state_root.join("engine.key")).unwrap());
    let key = bytes[8..].try_into().unwrap();
    drop(engine);
    assert!(matches!(
        reopen(|| Engine::prepare_native_migration(
            policy.clone(),
            binding(),
            ProvidedMaster::new(key)
        )),
        Err(EngineError::RecoveryUncertain)
    ));
    assert!(!policy.state_root.join("engine.native.json").exists());
    assert!(
        fs::read(policy.state_root.join("engine.key"))
            .unwrap()
            .as_slice()
            == bytes.as_slice(),
        "refused migration changed legacy key"
    );
    let mut engine = reopen(|| Engine::open(policy.clone())).unwrap();
    let ctx = engine
        .authorize(P, EndpointId([53; 32]), S, unsafe { libc::getuid() })
        .unwrap();
    engine.cancel(&ctx, id).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while engine.status(&ctx, id).unwrap().state != InvocationState::Exited {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(feature = "fault-injection")]
#[path = "support/native_publication.rs"]
mod native_publication;
