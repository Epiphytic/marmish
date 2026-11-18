use super::*;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MODE: &str = "MARMISH_DISPATCH_CHILD_MODE";
const CHILD: &str = "peer_quota_vfs::dispatch::tests::dispatch_child";
const NATIVE_TEST: &str = "peer_invitation::provisional::tests::peer_quota_dispatch_native_child";

#[test]
#[ignore = "opt-in bundled SQLCipher dispatcher proof"]
fn dispatch_native_checkpoint_and_ordinary_routes() {
    for limit in ["0", "32768", "4194304"] {
        run(NATIVE_TEST, "MARMISH_DISPATCH_NATIVE_LIMIT", limit, None);
    }
    for mode in [
        "routes",
        "displaced",
        "postcheck",
        "faults",
        "poisoned_drop",
        "reservation",
    ] {
        run(CHILD, MODE, mode, None);
    }
    let restart = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    for phase in ["crash", "restart"] {
        run(
            NATIVE_TEST,
            "MARMISH_DISPATCH_NATIVE_LIMIT",
            phase,
            Some(restart.path()),
        );
    }
}

fn run(test: &str, var: &str, value: &str, restart: Option<&Path>) {
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut command = Command::new(std::env::current_exe().expect("fixture_executable"));
    command
        .args(["--ignored", "--exact", test, "--nocapture"])
        .env_clear()
        .env(var, value)
        .env("SQLITE_FORCE_PROXY_LOCKING", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(restart) = restart {
        command.env("MARMISH_DISPATCH_RESTART_ROOT", restart);
    }
    let mut child = Child(command.spawn().expect("dispatch_child"));
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("dispatch_wait") {
            break status;
        }
        assert!(Instant::now() < deadline, "dispatch_timeout {value}");
        thread::sleep(Duration::from_millis(10));
    };
    if value == "crash" {
        assert_eq!(status.code(), Some(80), "dispatch_crash_boundary");
        return;
    }
    let mut report = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(4097)
        .read_to_string(&mut report)
        .unwrap();
    assert!(report.len() <= 4096, "dispatch_report_length");
    for line in report.lines().filter(|line| {
        matches!(
            *line,
            "dispatch-proof native 0"
                | "dispatch-proof native 1"
                | "dispatch-proof restart"
                | "dispatch-proof routes"
                | "dispatch-proof displaced"
                | "dispatch-proof postcheck"
                | "dispatch-proof faults"
                | "dispatch-proof poisoned_drop"
                | "dispatch-proof reservation"
        )
    }) {
        println!("{line}");
    }
    assert!(status.success(), "dispatch_child_status {value}");
    assert!(
        report
            .lines()
            .any(|line| line.starts_with("dispatch-proof ")),
        "dispatch_no_report {value}"
    );
}

#[test]
#[ignore = "isolated default SQLCipher VFS subprocess"]
fn dispatch_child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let dispatch = Dispatch::mount().expect("dispatch_mount");
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let lease = ProfileLease::acquire(temp.path(), "fixture").unwrap();
    let parent = lease.root().join("peer");
    fs::DirBuilder::new().mode(0o700).create(&parent).unwrap();
    let profile = ProfileSlots::reserve(&lease, TOTAL_ADMISSION - 4 * 1024 * 1024).unwrap();
    let main = parent.join(NAMES[0]);
    let metadata = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&main)
        .unwrap()
        .metadata()
        .unwrap();
    let binding = RawBinding {
        path: &main,
        generation: NonZeroU64::new(1).unwrap(),
        known: true,
        witness: witness(&metadata),
    };
    let ordinary = temp.path().join("ordinary.db");
    match mode.as_str() {
        "routes" => routes(&profile, binding, &ordinary),
        "faults" => faults(&profile, binding),
        "poisoned_drop" => poisoned_drop(&profile, binding),
        "reservation" => reservation(&profile, &lease, binding),
        "displaced" => {
            unsafe { sql::sqlite3_vfs_register(dispatch.stock, 1) };
            assert_eq!(profile.begin(binding).err(), Some("dispatch_displaced"));
            assert_eq!(bytes(&main), Ok(Some(0)));
            ordinary_work(&ordinary);
        }
        "postcheck" => {
            let turn = profile.begin(binding).unwrap();
            let connection = Connection::open(&main).expect("dispatch_managed");
            drop(connection);
            assert!(turn.quota.verify_dispatch().is_ok());
            unsafe { sql::sqlite3_vfs_register(dispatch.stock, 1) };
            assert_eq!(profile.finish(&turn), Err("dispatch_displaced"));
            ordinary_work(&ordinary);
        }
        _ => panic!("unsupported_dispatch_case"),
    }
    println!("dispatch-proof {mode}");
}

fn reservation(profile: &ProfileSlots<'_>, lease: &ProfileLease, binding: RawBinding<'_>) {
    let another = ProfileSlots::reserve(lease, profile.charged_other).unwrap();
    let reserve = |exact| profile.reservation(exact, profile.charged_other);
    assert_eq!(
        another.owner.admit(reserve(binding)).err(),
        Some("dispatch_reservation_owner")
    );
    let advanced = RawBinding {
        generation: NonZeroU64::new(2).unwrap(),
        ..binding
    };
    let mut turn = profile.owner.admit(reserve(advanced)).unwrap();
    assert!(Connection::open(binding.path).is_err());
    assert_eq!(
        profile.owner.admit(reserve(advanced)).err(),
        Some("dispatch_active_turn")
    );
    turn.enter().unwrap();
    assert_eq!(turn.enter(), Err("dispatch_recursive_turn"));
    drop(Connection::open(binding.path).unwrap());
    turn.finish().unwrap();
    drop(turn);

    let files = profile.file_turn().unwrap();
    assert_eq!(
        another.file_turn().err(),
        Some("dispatch_file_turn_unavailable")
    );
    let managed = files.original(advanced).unwrap();
    let connection = Connection::open(binding.path).unwrap();
    let mut polls = 0;
    let never_ready = std::future::poll_fn(move |_| {
        polls += 1;
        assert_eq!(polls, 1);
        std::hint::black_box(&connection);
        std::task::Poll::<()>::Pending
    });
    assert_eq!(resolve_once(never_ready), Err("dispatch_pending"));
    managed.finish().unwrap();
    assert_eq!(managed.quota.evidence().database_opens, 1);
    drop(managed);
    drop(files);
    assert!(ACTIVE.with(|active| active.borrow().is_none()));
    assert!(Connection::open(binding.path).is_err());
    assert!(profile.file_turn().is_ok());

    assert_eq!(
        profile.owner.admit(reserve(binding)).err(),
        Some("dispatch_stage_generation")
    );
    let wrong_witness = RawBinding {
        witness: (advanced.witness.0, advanced.witness.1 ^ 1),
        ..advanced
    };
    assert_eq!(
        profile.owner.admit(reserve(wrong_witness)).err(),
        Some("dispatch_stage_generation")
    );
    let unentered = profile.owner.admit(reserve(advanced)).unwrap();
    drop(unentered);
    assert!(profile.owner.admit(reserve(advanced)).is_ok());
}

#[test]
fn receiver_eligibility_reads_only_its_lease_config() {
    use serde_json::json;
    use std::io::Write;

    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let lease = Arc::new(ProfileLease::acquire(temp.path(), "receiver").unwrap());
    let other = Arc::new(ProfileLease::acquire(temp.path(), "unrelated").unwrap());
    assert!(ReceiverCandidate::evaluate(&lease).unwrap().is_none());
    let mut config = json!({"version":1,"role":"both","relays":["wss://ordinary.example/"],
        "tor":"auto","trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},
        "cwd":null,"limits":{},"aliases":{}});
    let write = |config: &serde_json::Value| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(lease.root().join("config.json"))
            .unwrap();
        file.write_all(&serde_json::to_vec(config).unwrap())
            .unwrap();
    };
    write(&config);
    assert!(ReceiverCandidate::evaluate(&lease).unwrap().is_none());
    config["role"] = json!("server");
    write(&config);
    assert!(ReceiverCandidate::evaluate(&lease).unwrap().is_none());
    config["role"] = json!("client");
    config["peer"] = json!({
        "receive": {"mode":"manual"},
        "relay": "wss://private.example/",
        "relay_trust": format!("spki-sha256:{}", "ab".repeat(32)),
    });
    write(&config);
    let client = ReceiverCandidate::evaluate(&lease).unwrap();
    assert_eq!(
        client.is_some(),
        cfg!(all(target_os = "linux", target_arch = "x86_64"))
    );
    if let Some(client) = client {
        client.recheck(&lease).unwrap();
        assert!(client.recheck(&other).is_err());
        config["role"] = json!("both");
        write(&config);
        assert!(client.recheck(&lease).is_err());
        let both = ReceiverCandidate::evaluate(&lease).unwrap().unwrap();
        both.recheck(&lease).unwrap();
        config["peer"]["receive"] = json!({"mode":"auto_any"});
        write(&config);
        assert!(both.recheck(&lease).is_err());
        assert!(ReceiverCandidate::evaluate(&lease).unwrap().is_some());
        config["tor"] = json!("always");
        write(&config);
        assert!(ReceiverCandidate::evaluate(&lease).unwrap().is_none());
    }
    assert!(!lease.root().join("peer").exists());
    assert!(!lease.mdk_database().exists());
    assert!(!lease.journal_database().exists());
}

fn routes(profile: &ProfileSlots<'_>, binding: RawBinding<'_>, ordinary: &Path) {
    let main = binding.path;
    assert!(Connection::open(main).is_err());
    let alias = main.with_extension("alias");
    symlink(main, &alias).unwrap();
    assert!(Connection::open(&alias).is_err());
    fs::remove_file(&alias).unwrap();
    fs::hard_link(main, &alias).unwrap();
    assert!(Connection::open(&alias).is_err());
    assert_eq!(profile.begin(binding).err(), Some("stage_file_identity"));
    fs::remove_file(&alias).unwrap();
    // The rejected witness quarantines this slot; use the other fixed slot for
    // successful scoped/native and concurrent ordinary routing cases.
    let unknown = main.with_file_name(NAMES[1]);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&unknown)
        .unwrap();
    let binding = RawBinding {
        path: &unknown,
        known: false,
        witness: witness(&file.metadata().unwrap()),
        ..binding
    };
    let turn = profile.begin(binding).unwrap();
    assert_eq!(
        profile.begin(binding).err(),
        Some("dispatch_recursive_turn")
    );
    assert_eq!(
        raw_open(
            profile.dispatch.wrapper,
            ptr::null(),
            sql::SQLITE_OPEN_TEMP_DB
        ),
        sql::SQLITE_CANTOPEN
    );
    assert_eq!(
        raw_open(
            profile.dispatch.wrapper,
            ptr::null(),
            sql::SQLITE_OPEN_SUBJOURNAL
        ),
        sql::SQLITE_CANTOPEN
    );
    let candidate = std::ffi::CString::new(unknown.as_os_str().as_bytes()).unwrap();
    assert_eq!(
        raw_open(
            profile.dispatch.wrapper,
            candidate.as_ptr(),
            RAW_AUTOPROXY | sql::SQLITE_OPEN_MAIN_DB | sql::SQLITE_OPEN_READWRITE
        ),
        sql::SQLITE_CANTOPEN
    );
    assert!(Connection::open(ordinary).is_err());
    thread::scope(|tasks| {
        tasks
            .spawn(|| {
                ordinary_work(ordinary);
                assert_eq!(
                    anonymous_io(profile.dispatch.wrapper),
                    anonymous_io(profile.dispatch.stock)
                );
            })
            .join()
            .unwrap();
    });
    let conn = Connection::open_with_flags(&unknown, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA threads", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        0
    );
    let journal: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "delete");
    assert!(
        conn.execute("ATTACH DATABASE ? AS second", [ordinary.to_str().unwrap()])
            .is_err()
    );
    conn.execute_batch("CREATE TABLE data(id); INSERT INTO data VALUES (2)")
        .unwrap();
    assert!(std::ptr::eq(
        unsafe { (*primary_file(&conn)).pMethods },
        &METHODS
    ));
    assert_eq!(profile.finish(&turn), Err("dispatch_live_handles"));
    drop(conn);
    profile.finish(&turn).unwrap();
    drop(turn);
    assert!(Connection::open(&unknown).is_err());
    assert!(resolve_once(std::future::pending::<()>()).is_err());
    ordinary_work(ordinary);
}

fn ordinary_work(path: &Path) {
    let conn = Connection::open(path).expect("ordinary_open");
    let stock = Connection::open_with_flags_and_vfs(path, OpenFlags::default(), "unix")
        .expect("ordinary_explicit_stock");
    assert!(std::ptr::eq(
        unsafe { (*primary_file(&conn)).pMethods },
        unsafe { (*primary_file(&stock)).pMethods }
    ));
    drop(stock);
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA temp_store=FILE; PRAGMA mmap_size=65536;
         CREATE TABLE IF NOT EXISTS ordinary(value); CREATE TEMP TABLE spool(value);
         INSERT INTO spool VALUES (randomblob(8192));
         BEGIN; INSERT INTO ordinary VALUES (4); COMMIT;
         SAVEPOINT rollback; INSERT INTO ordinary VALUES (9); ROLLBACK TO rollback; RELEASE rollback;"
    ).unwrap();
    assert!(sidecar(path, "-wal").exists());
    assert!(sidecar(path, "-shm").exists());
    assert_eq!(
        conn.query_row("SELECT max(value) FROM ordinary", [], |row| row
            .get::<_, u32>(0))
            .unwrap(),
        4
    );
}

fn raw_open(vfs: *mut sql::sqlite3_vfs, name: *const c_char, flags: c_int) -> c_int {
    unsafe {
        let size = (*vfs).szOsFile as usize;
        let file = sql::sqlite3_malloc64(size as u64).cast::<sql::sqlite3_file>();
        assert!(!file.is_null());
        file.cast::<u8>().write_bytes(0, size);
        let mut out = 0;
        let rc = (*vfs).xOpen.unwrap()(vfs, name, file, flags, &mut out);
        assert!((*file).pMethods.is_null());
        sql::sqlite3_free(file.cast());
        rc
    }
}

fn primary_file(conn: &Connection) -> *mut sql::sqlite3_file {
    let mut file: *mut sql::sqlite3_file = ptr::null_mut();
    let rc = unsafe {
        sql::sqlite3_file_control(
            conn.handle(),
            c"main".as_ptr(),
            sql::SQLITE_FCNTL_FILE_POINTER,
            ptr::from_mut(&mut file).cast(),
        )
    };
    assert_eq!(rc, sql::SQLITE_OK);
    assert!(!file.is_null());
    file
}

fn anonymous_io(vfs: *mut sql::sqlite3_vfs) -> (c_int, usize) {
    unsafe {
        let size = (*vfs).szOsFile as usize;
        let file = sql::sqlite3_malloc64(size as u64).cast::<sql::sqlite3_file>();
        assert!(!file.is_null());
        file.cast::<u8>().write_bytes(0, size);
        let mut out = 0;
        let rc = (*vfs).xOpen.unwrap()(
            vfs,
            ptr::null(),
            file,
            sql::SQLITE_OPEN_TEMP_DB
                | sql::SQLITE_OPEN_CREATE
                | sql::SQLITE_OPEN_READWRITE
                | sql::SQLITE_OPEN_DELETEONCLOSE,
            &mut out,
        );
        assert_eq!(rc, sql::SQLITE_OK);
        let methods = (*file).pMethods;
        assert!(!methods.is_null());
        assert_eq!(close_stock(file), sql::SQLITE_OK);
        (out, methods as usize)
    }
}

fn faults(profile: &ProfileSlots<'_>, binding: RawBinding<'_>) {
    let turn = profile.begin(binding).unwrap();
    let conn = Connection::open(binding.path).unwrap();
    conn.execute_batch("CREATE TABLE test(value)").unwrap();
    let handle = unsafe { Handle::get(primary_file(&conn)) };
    let old = bytes(binding.path).unwrap().unwrap();
    // Model a short stock write followed by I/O failure. Independent stat
    // and the stock xFileSize must agree before a later callback can proceed.
    let file = OpenOptions::new().write(true).open(binding.path).unwrap();
    let rc = handle.resize(old + 64, Resize::Write { offset: old as i64 }, || {
        file.set_len(old + 17).unwrap();
        unsafe { errno_slot().write(libc::ENOSPC) };
        sql::SQLITE_IOERR_WRITE
    });
    let managed_errno = unsafe { forward_error(profile.dispatch.wrapper, 0, ptr::null_mut()) };
    assert_eq!(rc, sql::SQLITE_IOERR_WRITE);
    assert_eq!(managed_errno, libc::ENOSPC);
    assert_eq!(bytes(binding.path), Ok(Some(old + 17)));
    assert_eq!(turn.quota.admitted_length(), old + 17);
    assert_eq!(
        guarded(sql::SQLITE_IOERR, || guarded(sql::SQLITE_IOERR, || panic!(
            "would_reenter"
        ))),
        sql::SQLITE_IOERR
    );
    assert_eq!(
        guarded(sql::SQLITE_IOERR, || panic!("fixture_callback_unwind")),
        sql::SQLITE_IOERR
    );
    assert_eq!(
        handle.resize(old + 65, Resize::Truncate, || panic!("would_grow")),
        sql::SQLITE_IOERR_FSTAT
    );
    assert_eq!(bytes(binding.path), Ok(Some(old + 17)));
    drop(conn);
    assert_eq!(profile.finish(&turn), Err("dispatch_no_clean_receipt"));
    drop(turn);
    assert_eq!(
        profile.begin(binding).err(),
        Some("dispatch_stage_generation")
    );
    assert!(Connection::open(binding.path).is_err());
}

fn poisoned_drop(profile: &ProfileSlots<'_>, binding: RawBinding<'_>) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _turn = profile.begin(binding).unwrap();
        let _slots = profile.dispatch.slots.lock().unwrap();
        panic!("fixture_reservation_unwind");
    }));
    assert!(result.is_err());
    assert!(ACTIVE.with(|active| active.borrow().is_none()));
    let slots = profile.dispatch.slots.lock().err().unwrap().into_inner();
    assert!(slots[0].quarantined);
    assert!(slots[0].active.is_none());
    drop(slots);
    assert_eq!(profile.begin(binding).err(), Some("dispatch_reservations"));
    assert!(Connection::open(binding.path).is_err());
}
