use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn root_locks_are_ofd_and_presence_does_not_acquire() {
    const ROOT_ENV: &str = "MARMISH_OFD_PRESENCE_TEST_ROOT";
    let Some(test_root) = std::env::var_os(ROOT_ENV) else {
        use std::process::{Command, Stdio};
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        // Start before creating locks: unrelated parallel forks must not retain
        // the isolated child's open descriptions after its strict drop checks.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "launcher::tests::root_locks_are_ofd_and_presence_does_not_acquire",
                "--nocapture",
            ])
            .env_clear()
            .env(ROOT_ENV, temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "isolated OFD test failed: {status}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                child.wait().unwrap();
                panic!("isolated OFD test timed out");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read(temp.path().join("completed")).unwrap(), b"ok");
        return;
    };
    let test_root = PathBuf::from(test_root);
    let root = StateRoot::open(&test_root.join("state")).unwrap();
    let guard = root.lock_frontend().unwrap();
    assert!(matches!(
        root.lock_frontend(),
        Err(EngineError::AlreadyOpen)
    ));
    drop(guard);
    root.lock_frontend().unwrap();
    let job = JobId([1; 16]);
    root.prepare_job(job, &WorkerCredential([2; 32])).unwrap();
    assert!(matches!(
        root.presence(job, None).unwrap(),
        WorkerPresence::Absent
    ));
    let held = root.lock_job(job).unwrap();
    assert!(matches!(
        root.presence(job, None).unwrap(),
        WorkerPresence::Initializing
    ));
    let identity = process_identity(std::process::id() as i32).unwrap();
    assert!(matches!(
        root.presence(job, Some(&identity)).unwrap(),
        WorkerPresence::Live(_)
    ));
    drop(held);
    assert!(matches!(
        root.presence(job, Some(&identity)).unwrap(),
        WorkerPresence::Absent
    ));
    std::fs::write(test_root.join("completed"), b"ok").unwrap();
}

#[test]
fn private_modes_and_symlinks_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        StateRoot::open(temp.path()),
        Err(EngineError::UnsafePath)
    ));
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(temp.path()).unwrap();
    root.prepare_db(true).unwrap();
    assert_eq!(
        std::fs::metadata(temp.path().join("engine.sqlite3"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    std::os::unix::fs::symlink(
        "/tmp/does-not-exist-marmish",
        temp.path().join("engine.sqlite3-wal"),
    )
    .unwrap();
    assert!(matches!(
        root.verify_db_files(),
        Err(EngineError::UnsafePath)
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn proc_stat_handles_spaces_and_parentheses_and_rejects_forgery() {
    let identity = process_identity(std::process::id() as i32).unwrap();
    assert!(identity.start_ticks > 0);
    use crate::platform::linux::parse_proc_stat;
    let mut fields = vec!["0"; 50];
    fields[0] = "S";
    fields[19] = "12345";
    let text = format!("42 (odd ) (( process) {}", fields.join(" "));
    assert_eq!(parse_proc_stat(text.as_bytes(), 42).unwrap(), 12345);
    assert!(parse_proc_stat(text.as_bytes(), 43).is_err());
    assert!(parse_proc_stat(b"42 (short) S 1", 42).is_err());
}

#[test]
fn db_verification_does_not_release_sqlite_process_locks() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(temp.path()).unwrap();
    let guard = root.lock_frontend().unwrap();
    let keys = root.initialize_storage(&guard).unwrap();
    let path = root.prepare_db(true).unwrap();
    let conn = crate::cipher::open(
        &path,
        &keys,
        crate::cipher::Context::State,
        crate::cipher::Journal::Wal,
    )
    .unwrap();
    conn.execute_batch("CREATE TABLE probe(n); BEGIN IMMEDIATE; INSERT INTO probe VALUES(1);")
        .unwrap();
    root.prepare_db(false).unwrap();
    root.verify_db_files().unwrap();
    // Another process must still be excluded by SQLite's WAL writer lock.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "launcher::tests::sqlite_lock_helper",
            "--nocapture",
        ])
        .env_clear()
        .env("MARMISH_TEST_DB_LOCK", &path)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("SQLite lock probe timed out");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(status.success());
    conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn sqlite_lock_helper() {
    let Some(path) = std::env::var_os("MARMISH_TEST_DB_LOCK") else {
        return;
    };
    let path = PathBuf::from(path);
    let root = StateRoot::open_existing(path.parent().unwrap()).unwrap();
    let keys = root.load_storage_keys().unwrap();
    let conn = crate::cipher::open(
        &path,
        &keys,
        crate::cipher::Context::State,
        crate::cipher::Journal::Wal,
    )
    .unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let error = conn.execute_batch("BEGIN IMMEDIATE").unwrap_err();
    assert!(matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ));
}

#[test]
fn orphan_locks_recover_empty_and_credential_only_preparation() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(temp.path()).unwrap();
    let frontend = root.lock_frontend().unwrap();
    for (job, credential) in [(JobId([20; 16]), false), (JobId([21; 16]), true)] {
        mkdirat(&root.jobs, OsStr::new(&job_hex(job))).unwrap();
        let dir = root.job_dir(job).unwrap();
        if credential {
            let mut file = root.file(&dir, "credential", true).unwrap();
            file.write_all(&[7; 32]).unwrap();
        }
        // Missing locks in admitted-job APIs still fail rather than repairing.
        assert!(matches!(
            root.lock_job(job),
            Err(EngineError::StorageFailure)
        ));
        assert!(matches!(
            root.lock_io(job),
            Err(EngineError::StorageFailure)
        ));
        let (job_guard, io_guard) = root.lock_orphan_job(job, &frontend).unwrap();
        assert!(matches!(root.lock_job(job), Err(EngineError::Busy)));
        assert!(matches!(root.lock_io(job), Err(EngineError::Busy)));
        for name in ["worker.lock", "io.lock"] {
            let meta = root.file(&dir, name, false).unwrap().metadata().unwrap();
            assert_eq!(meta.mode() & 0o7777, 0o600);
            assert_eq!(meta.uid(), root.uid);
        }
        root.remove_job(job, &io_guard).unwrap();
        assert!(!root.job_directory_exists(job).unwrap());
        drop((job_guard, io_guard));
    }
}

#[test]
fn orphan_lock_recovery_rejects_foreign_guard_symlinks_and_busy_worker() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(&temp.path().join("state")).unwrap();
    let other = StateRoot::open(&temp.path().join("other")).unwrap();
    let frontend = root.lock_frontend().unwrap();
    let other_frontend = other.lock_frontend().unwrap();
    let job = JobId([22; 16]);
    root.prepare_job(job, &WorkerCredential([1; 32])).unwrap();
    assert!(matches!(
        root.lock_orphan_job(job, &other_frontend),
        Err(EngineError::UnsafePath)
    ));
    let worker = root.lock_job(job).unwrap();
    assert!(matches!(
        root.lock_orphan_job(job, &frontend),
        Err(EngineError::Busy)
    ));
    drop(worker);
    let directory = root.path().join("jobs").join(job_hex(job));
    for name in ["worker.lock", "io.lock"] {
        let path = directory.join(name);
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("/dev/null", &path).unwrap();
        assert!(matches!(
            root.lock_orphan_job(job, &frontend),
            Err(EngineError::UnsafePath)
        ));
        std::fs::remove_file(path).unwrap();
        root.file(&root.job_dir(job).unwrap(), name, true).unwrap();
    }
    let path = directory.join("worker.lock");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        root.lock_orphan_job(job, &frontend),
        Err(EngineError::UnsafePath)
    ));
}

fn storage_root() -> (tempfile::TempDir, StateRoot, FrontendGuard) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(dir.path()).unwrap();
    let guard = root.lock_frontend().unwrap();
    (dir, root, guard)
}
#[test]
fn storage_bootstrap_staging_recovery_and_missing_key_refusal() {
    let (_dir, root, guard) = storage_root();
    let mut pending = root
        .file(&root.directory, ".engine.key.pending", true)
        .unwrap();
    pending.write_all(b"partial").unwrap();
    drop(pending);
    let _keys = root.initialize_storage(&guard).unwrap();
    assert!(!root.path.join(".engine.key.pending").exists());
    assert_eq!(
        std::fs::metadata(root.path.join("engine.key"))
            .unwrap()
            .len(),
        40
    );
    root.initialize_storage(&guard).unwrap();
    root.prepare_db(true).unwrap();
    root.prepare_job(JobId([8; 16]), &WorkerCredential([1; 32]))
        .unwrap();
    assert!(root.initialize_storage(&guard).is_err());
    std::fs::remove_file(root.path.join("engine.key")).unwrap();
    assert!(root.initialize_storage(&guard).is_err());
    assert!(!root.path.join("engine.key").exists());
    assert!(root
        .path
        .join("jobs")
        .join(job_hex(JobId([8; 16])))
        .is_dir());
}

#[test]
fn storage_bootstrap_preserves_key_permissions_under_restrictive_umask() {
    const ROOT_ENV: &str = "MARMISH_TEST_RESTRICTIVE_UMASK_ROOT";
    if let Some(path) = std::env::var_os(ROOT_ENV) {
        let root = StateRoot::open_existing(Path::new(&path)).unwrap();
        let guard = root.lock_frontend().unwrap();
        root.initialize_storage(&guard).unwrap();
        let key_path = root.path.join("engine.key");
        let metadata = std::fs::metadata(&key_path).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.len(), 40);
        let original = Zeroizing::new(std::fs::read(&key_path).unwrap());
        root.initialize_storage(&guard).unwrap();
        let reloaded = Zeroizing::new(std::fs::read(key_path).unwrap());
        assert!(original.as_slice() == reloaded.as_slice());
        assert!(!root.path.join(".engine.key.pending").exists());
        return;
    }

    // Create unrelated root/lock files before applying the mask. Only the
    // isolated child changes its umask; parallel test threads remain unaffected.
    let (_dir, root, guard) = storage_root();
    let path = root.path.clone();
    drop((guard, root));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "launcher::tests::storage_bootstrap_preserves_key_permissions_under_restrictive_umask",
            "--nocapture",
        ])
        .env_clear()
        .env(ROOT_ENV, path)
        .stdin(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o277);
            Ok(())
        });
    }
    assert!(command.status().unwrap().success());
}

#[test]
fn storage_key_rejects_partial_unsafe_links_and_fifo_promptly() {
    for kind in ["partial", "permissions", "symlink", "hardlink", "fifo"] {
        let (_dir, root, guard) = storage_root();
        let path = root.path.join("engine.key");
        match kind {
            "symlink" => std::os::unix::fs::symlink("missing", &path).unwrap(),
            "fifo" => {
                let name = CString::new(path.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
            _ => {
                let file = root.file(&root.directory, "engine.key", true).unwrap();
                drop(file);
                if kind == "hardlink" {
                    std::fs::hard_link(&path, root.path.join("linked-key")).unwrap();
                }
                if kind == "permissions" {
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                        .unwrap();
                }
            }
        }
        let before = std::time::Instant::now();
        assert!(root.initialize_storage(&guard).is_err());
        assert!(before.elapsed() < std::time::Duration::from_secs(1));
    }
}
#[test]
fn legacy_content_rejected_even_with_valid_key() {
    let (_dir, root, guard) = storage_root();
    root.initialize_storage(&guard).unwrap();
    let path = root.prepare_db(true).unwrap();
    std::fs::write(&path, b"SQLite format 3\0preserve this legacy data").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(root.initialize_storage(&guard).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn journal_only_expiry_remainder_is_removed_but_not_opened() {
    let (_dir, root, guard) = storage_root();
    root.initialize_storage(&guard).unwrap();
    let job = JobId([12; 16]);
    root.prepare_job(job, &WorkerCredential([3; 32])).unwrap();
    let _job = root.lock_job(job).unwrap();
    let io = root.lock_io(job).unwrap();
    let dir = root.job_dir(job).unwrap();
    drop(root.file(&dir, "stdout.db-journal", true).unwrap());
    assert!(root
        .prepare_spool_db(job, Stream::Stdout, true, &io)
        .is_err());
    assert!(!exists_at(&dir, "stdout.db").unwrap());
    root.expire_spool(job, &io).unwrap();
    assert!(!exists_at(&dir, "stdout.db-journal").unwrap());
    root.expire_spool(job, &io).unwrap();
}
#[test]
fn unsafe_staging_file_is_left_intact() {
    for link in [false, true] {
        let (_dir, root, guard) = storage_root();
        let pending = root.path.join(".engine.key.pending");
        if link {
            let file = root
                .file(&root.directory, ".engine.key.pending", true)
                .unwrap();
            drop(file);
            std::fs::hard_link(&pending, root.path.join("staging-linked")).unwrap();
        } else {
            std::os::unix::fs::symlink("missing", &pending).unwrap();
        }
        assert!(root.initialize_storage(&guard).is_err());
        assert!(std::fs::symlink_metadata(&pending).is_ok());
        assert!(!root.path.join("engine.key").exists());
    }
}
