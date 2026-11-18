use super::*;
use crate::mdk_adapter::peer_native_signer::OpenStage;
use nostr::{key::Keys, types::Timestamp};
use rusqlite::{Connection, Error as SqlError, ErrorCode, OpenFlags};
use std::{
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use storage_sqlite::SqlCipherKey;

const CHILD_TEST: &str = "peer_quota_vfs::tests::quota_child";
const CHILD_MODE: &str = "MARMISH_QUOTA_VFS_PROOF_MODE";
const STAGE_ROOT: &str = "MARMISH_QUOTA_VFS_PROOF_ROOT";
const LIMIT: &str = "MARMISH_QUOTA_VFS_PROOF_LIMIT";
const GENEROUS: u64 = 4 * 1024 * 1024;
const BASE_KEY: [u8; 32] = [0xac; 32];
const SLOT: &str = "c70107d45ede1b729a50208c06382c01";

struct TrialChild(Child);

impl Drop for TrialChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn temp() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("fixture_directory")
}

fn run(dir: &Path, mode: &str, capacity: u64, crash: bool) {
    let mut child = TrialChild(
        Command::new(std::env::current_exe().expect("fixture_executable"))
            .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
            .env_clear()
            .env(CHILD_MODE, mode)
            .env(STAGE_ROOT, dir)
            .env(LIMIT, capacity.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture_spawn"),
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("fixture_wait") {
            break status;
        }
        assert!(Instant::now() < deadline, "fixture_timeout {mode}");
        thread::sleep(Duration::from_millis(10));
    };
    let mut output = String::new();
    child
        .0
        .stdout
        .take()
        .expect("fixture_stdout")
        .take(4097)
        .read_to_string(&mut output)
        .expect("fixture_report");
    assert!(output.len() <= 4096, "fixture_report_size");
    // Forward only a bounded allowlist of fixed diagnostic markers and the
    // numeric report shape. Never print the raw child output.
    let reports = output.lines().filter(|line| sanitized(line));
    for report in reports {
        println!("{report}");
    }
    if crash {
        assert_eq!(status.code(), Some(80), "fixture_crash_boundary");
    } else {
        assert!(status.success(), "fixture_case_failed {mode}");
        assert!(output.lines().any(is_measurement), "fixture_missing_report");
    }
}

fn sanitized(line: &str) -> bool {
    matches!(
        line,
        "quota-progress install"
            | "quota-progress native_open"
            | "quota-progress native_generate"
            | "quota-progress native_reopen"
            | "quota-progress tiny_rejected"
            | "quota-progress raw_open"
            | "quota-progress raw_transaction"
            | "quota-progress raw_reopen"
            | "quota-progress path_checks"
            | "quota-progress raw_recovery"
            | "quota-progress raw_seed"
            | "quota-progress ffi_boundary"
    ) || is_measurement(line)
}

fn is_measurement(line: &str) -> bool {
    let mut fields = line.split_ascii_whitespace();
    fields.next() == Some("quota-report")
        && matches!(fields.next(), Some("linux" | "macos"))
        && matches!(fields.next(), Some("x86_64" | "aarch64"))
        && matches!(
            fields.next(),
            Some(
                "native"
                    | "tiny"
                    | "preexisting"
                    | "boundary"
                    | "seed"
                    | "journal_full"
                    | "main_full"
                    | "recover"
                    | "path"
                    | "wal_exclusive"
            )
        )
        && fields.next().is_some_and(|version| {
            version.chars().next().is_some_and(|c| c.is_ascii_digit())
                && version.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        && fields.count() == 8
        && line
            .split_ascii_whitespace()
            .skip(5)
            .all(|field| field.parse::<u64>().is_ok())
}

fn progress(stage: &'static str) {
    println!("quota-progress {stage}");
}

#[test]
#[ignore = "opt-in bundled SQLCipher quota proof"]
fn quota_native_migrations_and_provisional_lifecycle() {
    let tiny = temp();
    let normal = temp();
    marmish_engine::initialize_storage().expect("sqlite_initialize");
    let parent_default = unsafe { sql::sqlite3_vfs_find(ptr::null()) };
    run(tiny.path(), "tiny", 0, false);
    run(normal.path(), "native", GENEROUS, false);
    assert_eq!(
        unsafe { sql::sqlite3_vfs_find(ptr::null()) },
        parent_default
    );
}

#[test]
#[ignore = "opt-in bundled SQLCipher quota proof"]
fn quota_transactions_and_hot_journal_recovery() {
    let directory = temp();
    let main = directory.path().join("provisional.db");
    run(directory.path(), "seed", GENEROUS, false);
    let baseline = bytes(&main).unwrap().unwrap();
    run(directory.path(), "journal_full", baseline + 12_288, false);
    run(directory.path(), "main_full", baseline + 20_480, false);
    let before = fs::read(&main).expect("fixture_committed_bytes");
    run(directory.path(), "crash", baseline + 20_480, true);
    let after = fs::read(&main).expect("fixture_interrupted_bytes");
    assert!(before != after, "fixture_no_interrupted_write");
    assert!(bytes(&sidecar(&main, "-journal")).unwrap().unwrap() > 512);
    run(directory.path(), "recover", baseline + 20_480, false);
}

#[test]
#[ignore = "opt-in bundled SQLCipher quota proof"]
fn quota_path_and_exact_growth_boundaries() {
    let directory = temp();
    let main = directory.path().join("provisional.db");
    run(directory.path(), "seed", GENEROUS, false);
    run(directory.path(), "path", GENEROUS, false);
    run(
        directory.path(),
        "preexisting",
        bytes(&main).unwrap().unwrap() - 1,
        false,
    );
    run(directory.path(), "wal_exclusive", GENEROUS, false);
    let boundary = temp();
    run(boundary.path(), "boundary", 32_768, false);
}

#[test]
#[ignore = "subprocess only"]
fn quota_child() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let dir = PathBuf::from(std::env::var_os(STAGE_ROOT).expect("child_directory"))
        .canonicalize()
        .unwrap_or_else(|_| panic!("canonical_stage_directory"));
    let capacity = std::env::var(LIMIT)
        .expect("fixture_limit")
        .parse()
        .expect("fixture_limit_number");
    let main = dir.join("provisional.db");
    progress("install");
    if mode == "preexisting" {
        assert!(matches!(
            Quota::install(&main, capacity, false, false),
            Err("initial_quota")
        ));
        report(&mode, capacity, Evidence::default(), "0", 0);
        return;
    }
    if matches!(mode.as_str(), "seed" | "boundary") {
        // Only the raw fixtures need precreation. The MDK opener creates
        // its own private, zero-length main file before VFS xOpen.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&main)
            .unwrap_or_else(|_| panic!("fixture_private_raw_file"));
    }
    let quota = Quota::install(&main, capacity, mode == "recover", mode == "crash")
        .unwrap_or_else(|reason| panic!("{reason}"));
    if mode == "recover" {
        assert_eq!(
            quota.admitted_length(),
            bytes(&main).unwrap().unwrap() + bytes(&sidecar(&main, "-journal")).unwrap().unwrap()
        );
    }
    let mut version = String::from("0");
    let recovered = match mode.as_str() {
        "tiny" => {
            native_tiny(&main, quota);
            false
        }
        "native" => {
            native_stage(&main, quota);
            let db = keyed(&main, BASE_KEY);
            version = cipher_version(&db);
            false
        }
        "boundary" => {
            exact_growth(&main, quota);
            false
        }
        "seed" | "journal_full" | "main_full" | "crash" | "recover" | "path" | "wal_exclusive" => {
            let db = keyed(&main, BASE_KEY);
            version = cipher_version(&db);
            match mode.as_str() {
                "seed" => seed(&db),
                "journal_full" => exhaust(db, &main, quota, 1),
                "main_full" => exhaust(db, &main, quota, 0),
                "crash" => {
                    // The stage VFS exits directly after the synced journal
                    // and a write to an actual DB page.
                    update(&db, "bad");
                    panic!("missing_crash_barrier");
                }
                "recover" => {
                    progress("raw_recovery");
                    committed(&db);
                    update(&db, "old");
                }
                "path" => path_checks(&db, &dir, &main, quota),
                "wal_exclusive" => exclusive_wal(&db, quota),
                _ => unreachable!(),
            }
            mode == "recover"
        }
        _ => panic!("unknown_fixture_mode"),
    };
    assert!(bytes(&sidecar(&main, "-journal")).unwrap().is_none());
    assert!(bytes(&sidecar(&main, "-wal")).unwrap().is_none());
    assert!(bytes(&sidecar(&main, "-shm")).unwrap().is_none());
    assert_eq!(bytes(&main).unwrap().unwrap_or(0), quota.admitted_length());
    assert_eq!(fs::read_dir(&dir).expect("fixture_files").count(), 1);
    let evidence = quota.evidence();
    assert!(evidence.peak <= capacity);
    assert_eq!(evidence.ordinary_errors, 0);
    report(&mode, capacity, evidence, &version, u8::from(recovered));
}

fn report(mode: &str, capacity: u64, evidence: Evidence, version: &str, recovered: u8) {
    println!(
        "quota-report {} {} {mode} {version} {capacity} {} {} {} {} {} {} {recovered}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        evidence.peak,
        evidence.rejected[0],
        evidence.rejected[1],
        evidence.journal_writes,
        evidence.database_opens,
        evidence.refused_opens,
    );
}

fn native_open(
    path: &Path,
    key: [u8; 32],
    signer: &Arc<Keys>,
    first: bool,
) -> Result<OpenStage, &'static str> {
    OpenStage::open_provisional(
        path,
        &SqlCipherKey::raw(key),
        Arc::clone(signer),
        "wss://provisional-quota.invalid/",
        SLOT.to_owned(),
        first,
    )
    .map_err(|_| "native_open")
}

fn native_tiny(path: &Path, quota: &Quota) {
    let signer = Arc::new(Keys::generate());
    progress("native_open");
    assert!(native_open(path, BASE_KEY, &signer, true).is_err());
    progress("tiny_rejected");
    let evidence = quota.evidence();
    assert_eq!(evidence.database_opens, 1);
    assert!(evidence.rejected.iter().sum::<u64>() > 0);
    assert_eq!(evidence.peak, 0);
    assert_eq!(bytes(path).unwrap(), Some(0));
    // Retry with the same zero cap and prove the existing empty main file
    // cannot bypass the VFS before migrations.
    assert!(native_open(path, BASE_KEY, &signer, true).is_err());
    assert!(quota.evidence().rejected.iter().sum::<u64>() > evidence.rejected.iter().sum());
}

fn native_stage(path: &Path, quota: &Quota) {
    let signer = Arc::new(Keys::generate());
    progress("native_open");
    let stage =
        native_open(path, BASE_KEY, &signer, true).unwrap_or_else(|reason| panic!("{reason}"));
    let after_migrations = quota.evidence();
    assert_eq!(after_migrations.database_opens, 1);
    assert!(after_migrations.peak > 0);
    let sample = Duration::from_secs(Timestamp::now().as_secs());
    progress("native_generate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("quota_fixture_runtime");
    let event = runtime
        .block_on(stage.generate_provisional(sample, false))
        .unwrap_or_else(|_| panic!("provisional_generate"));
    assert!(quota.evidence().journal_writes > after_migrations.journal_writes);
    drop(stage);
    progress("native_reopen");
    let stage =
        native_open(path, BASE_KEY, &signer, false).unwrap_or_else(|reason| panic!("{reason}"));
    let persisted = stage
        .read_provisional(sample)
        .unwrap_or_else(|_| panic!("provisional_reopen"));
    assert!(persisted.event.id == event.event.id);
    drop(stage);
    let before_wrong_key = quota.evidence().rejected;
    assert!(native_open(path, [0x17; 32], &signer, false).is_err());
    assert_eq!(quota.evidence().rejected, before_wrong_key);
    assert_eq!(quota.evidence().database_opens, 3);
    assert_eq!(quota.evidence().refused_opens, 0);
}

fn keyed(path: &Path, key: [u8; 32]) -> Connection {
    progress("raw_open");
    let db = Connection::open(path).unwrap_or_else(|_| panic!("raw_open"));
    let setup = format!(
        "PRAGMA key = \"x'{}'\"; PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL;          PRAGMA temp_store = MEMORY;",
        hex::encode(key)
    );
    db.execute_batch(&setup)
        .unwrap_or_else(|_| panic!("raw_pragmas"));
    db
}

fn cipher_version(db: &Connection) -> String {
    let version: String = db
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .unwrap_or_else(|_| panic!("fixture_cipher_version"));
    version
        .split_ascii_whitespace()
        .next()
        .filter(|version| version.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .expect("fixture_cipher_version_shape")
        .to_owned()
}

fn seed(db: &Connection) {
    progress("raw_seed");
    db.execute_batch(
        "CREATE TABLE fixture(id INTEGER PRIMARY KEY, value BLOB NOT NULL);          INSERT INTO fixture VALUES (1, 'old'); \
         CREATE TABLE pressure(value BLOB NOT NULL); \
         INSERT INTO pressure VALUES (randomblob(32768));",
    )
    .unwrap_or_else(|_| panic!("fixture_seed"));
    committed(db);
}

fn committed(db: &Connection) {
    verify_row(db, "old");
}

fn verify_row(db: &Connection, expected: &str) {
    let value: String = db
        .query_row(
            "SELECT value FROM fixture WHERE (SELECT count(*) FROM fixture) = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| panic!("fixture_committed_row"));
    assert!(value == expected, "fixture_committed_value");
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap_or_else(|_| panic!("fixture_integrity_read"));
    assert!(integrity == "ok", "fixture_integrity");
}

fn update(db: &Connection, value: &str) {
    db.execute("UPDATE fixture SET value = ?1 WHERE id = 1", [value])
        .unwrap_or_else(|_| panic!("fixture_update"));
}

fn exhaust(mut db: Connection, path: &Path, quota: &Quota, rejected: usize) {
    progress("raw_transaction");
    let expected_pressure = pressure(&db);
    let result = (|| {
        let txn = db.transaction()?;
        txn.execute("UPDATE fixture SET value = 'bad' WHERE id = 1", [])?;
        if rejected == 0 {
            txn.execute("INSERT INTO fixture VALUES (2, zeroblob(131072))", [])?;
        } else {
            txn.execute("UPDATE pressure SET value = zeroblob(32768)", [])?;
        }
        txn.commit()
    })();
    assert!(
        matches!(result, Err(SqlError::SqliteFailure(err, _)) if err.code == ErrorCode::DiskFull),
        "fixture_disk_full"
    );
    let evidence = quota.evidence();
    assert!(evidence.rejected[rejected] > 0, "fixture_wrong_rejection");
    assert_eq!(
        evidence.rejected[1 - rejected],
        0,
        "fixture_extra_rejection"
    );
    if rejected == 0 {
        assert!(evidence.journal_writes > 0);
    }
    committed(&db);
    assert!(
        pressure(&db) == expected_pressure,
        "fixture_pressure_rollback"
    );
    drop(db);
    progress("raw_reopen");
    let db = keyed(path, BASE_KEY);
    committed(&db);
    assert!(
        pressure(&db) == expected_pressure,
        "fixture_pressure_reopen"
    );
    update(&db, "new");
    verify_row(&db, "new");
    update(&db, "old");
    committed(&db);
}

fn pressure(db: &Connection) -> Vec<u8> {
    db.query_row("SELECT value FROM pressure", [], |row| row.get(0))
        .unwrap_or_else(|_| panic!("fixture_pressure_row"))
}

fn path_checks(db: &Connection, dir: &Path, main: &Path, quota: &Quota) {
    progress("path_checks");
    let before = quota.evidence().refused_opens;
    let other = dir.join("sibling.db");
    assert!(Connection::open(&other).is_err());
    assert!(
        db.execute(
            "ATTACH DATABASE ?1 AS alternate",
            [other.to_str().expect("fixture_utf8_path")]
        )
        .is_err()
    );
    assert_eq!(quota.evidence().refused_opens - before, 2);
    assert!(bytes(&other).unwrap().is_none());
    let alias_root = temp();
    let link = alias_root.path().join("linked");
    symlink(dir, &link).expect("fixture_alias");
    let traversal = dir
        .join("..")
        .join(dir.file_name().expect("fixture_dir_name"));
    for spelling in [link, traversal] {
        let before = quota.evidence().database_opens;
        let candidate = Connection::open_with_flags(
            spelling.join("provisional.db"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        );
        if let Ok(candidate) = candidate {
            // A canonicalized spelling can be admitted, but only through the
            // same file counters. It must never get an alternate connection.
            assert_eq!(quota.evidence().database_opens, before + 1);
            drop(candidate);
        }
    }
    let journal = db.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0));
    assert!(!journal.is_ok_and(|result| result.eq_ignore_ascii_case("wal")));
    committed(db);
    assert!(quota.evidence().refused_opens >= 1);
    assert_eq!(bytes(main).unwrap().unwrap(), quota.admitted_length());
}

fn exclusive_wal(db: &Connection, quota: &Quota) {
    progress("path_checks");
    let mode: String = db
        .query_row("PRAGMA locking_mode=EXCLUSIVE", [], |row| row.get(0))
        .unwrap_or_else(|_| panic!("fixture_exclusive_lock"));
    assert!(mode.eq_ignore_ascii_case("exclusive"));
    let before = quota.evidence().refused_opens;
    let transition = db.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0));
    if transition.is_ok_and(|mode| mode.eq_ignore_ascii_case("wal")) {
        assert!(db.execute("UPDATE fixture SET value = 'bad'", []).is_err());
    }
    assert!(
        quota.evidence().refused_opens > before,
        "fixture_wal_not_refused"
    );
}

fn exact_growth(path: &Path, quota: &Quota) {
    progress("ffi_boundary");
    // SQLite pages cannot express a one-byte overflow, so this uses the
    // same registered VFS entry points on scratch main and journal files.
    let vfs = unsafe { sql::sqlite3_vfs_find(ptr::null()) };
    let journal = sidecar(path, "-journal");
    let main = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("fixture_c_path");
    let journal = std::ffi::CString::new(journal.as_os_str().as_bytes()).expect("fixture_c_path");
    unsafe {
        let name = sql::sqlite3_create_filename(
            main.as_ptr(),
            journal.as_ptr(),
            c"".as_ptr(),
            0,
            ptr::null_mut(),
        );
        assert!(!name.is_null());
        let journal_name = sql::sqlite3_filename_journal(name);
        let main_file = boundary_file(vfs, name, sql::SQLITE_OPEN_MAIN_DB);
        let journal_file = boundary_file(vfs, journal_name, sql::SQLITE_OPEN_MAIN_JOURNAL);
        let methods = &*(*main_file).pMethods;
        let resize = methods.xTruncate.unwrap();
        let write = methods.xWrite.unwrap();
        let byte = 0xabu8;
        let data = ptr::from_ref(&byte).cast();
        assert_eq!(resize(main_file, 24_575), sql::SQLITE_OK);
        assert_eq!(resize(journal_file, 8_192), sql::SQLITE_OK);
        assert_eq!(write(main_file, data, 1, 24_575), sql::SQLITE_OK);
        assert_eq!(write(main_file, data, 1, 0), sql::SQLITE_OK);
        assert_eq!(write(main_file, data, 1, 24_576), sql::SQLITE_FULL);
        assert_eq!(resize(main_file, 24_577), sql::SQLITE_FULL);
        assert_eq!(write(journal_file, data, 1, 8_192), sql::SQLITE_FULL);
        for file in [journal_file, main_file] {
            assert_eq!(methods.xClose.unwrap()(file), sql::SQLITE_OK);
            sql::sqlite3_free(file.cast());
        }
        assert_eq!(
            (*vfs).xDelete.unwrap()(vfs, journal_name, 0),
            sql::SQLITE_OK
        );
        sql::sqlite3_free_filename(name);
    }
    let evidence = quota.evidence();
    assert_eq!(evidence.peak, 32_768);
    assert_eq!(evidence.growth_truncates, 2);
    assert_eq!(evidence.rejected, [2, 1]);
}

unsafe fn boundary_file(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    kind: c_int,
) -> *mut sql::sqlite3_file {
    let flags = kind | sql::SQLITE_OPEN_READWRITE | sql::SQLITE_OPEN_CREATE;
    let mut out_flags = 0;
    unsafe {
        let file = sql::sqlite3_malloc64((*vfs).szOsFile as u64).cast::<sql::sqlite3_file>();
        assert!(!file.is_null());
        assert_eq!(
            (*vfs).xOpen.unwrap()(vfs, name, file, flags, &mut out_flags),
            sql::SQLITE_OK
        );
        file
    }
}
