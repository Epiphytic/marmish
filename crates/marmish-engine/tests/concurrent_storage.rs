#![cfg(any(target_os = "linux", target_os = "macos"))]

use rusqlite::Connection;
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

const CHILD: &str = "MARMISH_CONCURRENT_STORAGE_CHILD";
const CANARY: &[u8] = b"parallel encrypted storage\0\xff";

fn keyed(path: &std::path::Path, byte: u8) -> Connection {
    let connection = Connection::open(path).unwrap();
    let raw = format!("x'{}'", format!("{byte:02x}").repeat(32));
    // Key bytes remain data, matching the production raw-key representation.
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_key(connection.handle(), raw.as_ptr().cast(), raw.len() as i32)
        },
        rusqlite::ffi::SQLITE_OK
    );
    connection
}

#[test]
fn concurrent_storage_child() {
    let Some(completed) = std::env::var_os(CHILD) else {
        return;
    };
    // No SQLite API is called by setup: the synchronized initializer below is
    // each thread's first SQLite use in this fresh process.
    let directory = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(16));
    let threads: Vec<_> = (1..=16)
        .map(|byte| {
            let path = directory.path().join(format!("{byte}.sqlite3"));
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                marmish_engine::initialize_storage().unwrap();
                let connection = keyed(&path, byte);
                connection
                    .execute_batch("CREATE TABLE probe(value BLOB)")
                    .unwrap();
                connection
                    .execute("INSERT INTO probe VALUES(?1)", [CANARY])
                    .unwrap();
                drop(connection);
                let connection = keyed(&path, byte);
                let value: Vec<u8> = connection
                    .query_row("SELECT value FROM probe", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(value, CANARY);
                drop(connection);
                let bytes = std::fs::read(&path).unwrap();
                assert!(!bytes.starts_with(b"SQLite format 3\0"));
                assert!(!bytes.windows(CANARY.len()).any(|w| w == CANARY));
                let wrong = keyed(&path, 0);
                assert!(wrong
                    .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                        .get::<_, i64>(0))
                    .is_err());
            })
        })
        .collect();
    let failed = threads
        .into_iter()
        .filter_map(|thread| thread.join().err())
        .count();
    assert_eq!(failed, 0, "concurrent encrypted open failed");
    std::fs::write(completed, b"16 encrypted opens verified").unwrap();
}

#[test]
fn first_encrypted_opens_are_safe_in_fresh_processes() {
    let sentinels = tempfile::tempdir().unwrap();
    for index in 0..3 {
        let completed = sentinels.path().join(index.to_string());
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "concurrent_storage_child", "--nocapture"])
            .env(CHILD, &completed)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    assert!(status.success(), "fresh-process concurrent storage failed");
                    assert_eq!(
                        std::fs::read(&completed).expect("child test did not complete"),
                        b"16 encrypted opens verified"
                    );
                    break;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                other => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("concurrent storage deadlock/child observation failure: {other:?}");
                }
            }
        }
    }
}
