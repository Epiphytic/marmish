use super::*;
use rusqlite::ErrorCode;
use std::{io::Write, os::unix::fs::OpenOptionsExt};
use tempfile::TempDir;

const KEY: [u8; 32] = [42; 32];

fn cursor() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cursor.db");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    (directory, path)
}

fn extent(path: &Path) -> u64 {
    fs::metadata(path).unwrap().len()
}

#[test]
fn peer_installed_cursor_refuses_preexisting_sidecars_before_open() {
    let (_directory, path) = cursor();
    let issued = "CREATE TABLE proof(value); INSERT INTO proof VALUES('original');";
    let original = HardenedPeerConnection::open_installed(&path, KEY).unwrap();
    original.0.execute_batch(issued).unwrap();
    drop(original);
    let before = fs::read(&path).unwrap();

    // A sector-sized rollback header with an original-page count above the
    // cap is still under any physical length check. Do not send it to SQLite.
    let mut header = [0; 512];
    header[..8].copy_from_slice(&[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]);
    header[16..20].copy_from_slice(&((MAX_PAGES as u32) + 1).to_be_bytes());
    header[20..24].copy_from_slice(&(512u32).to_be_bytes());
    header[24..28].copy_from_slice(&(PAGE_SIZE as u32).to_be_bytes());
    for (suffix, bytes) in [
        ("-journal", &header[..]),
        ("-wal", &[][..]),
        ("-shm", &[][..]),
    ] {
        let file = sidecar(&path, suffix);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        assert_eq!(
            HardenedPeerConnection::open_installed(&path, KEY).err(),
            Some(Error::Storage)
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read(&file).unwrap(), bytes);
        fs::remove_file(&file).unwrap();
    }
    HardenedPeerConnection::open_installed(&path, KEY).unwrap();
}

#[test]
fn peer_installed_cursor_refuses_oversized_and_unaligned_main() {
    let (_directory, path) = cursor();
    for bytes in [MAX_MAIN_BYTES + PAGE_SIZE, PAGE_SIZE - 1] {
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(bytes)
            .unwrap();
        assert_eq!(check_closed_image(&path), Err(Error::Storage));
        assert_eq!(
            HardenedPeerConnection::open_installed(&path, KEY).err(),
            Some(Error::Storage)
        );
        assert_eq!(extent(&path), bytes);
    }
}

#[test]
fn peer_installed_cursor_full_rolls_back_and_reopen_keeps_bound() {
    let (_directory, path) = cursor();
    let HardenedPeerConnection(mut conn) =
        HardenedPeerConnection::open_installed(&path, KEY).unwrap();
    conn.execute_batch(
        "CREATE TABLE proof (bytes BLOB NOT NULL) STRICT;
         WITH RECURSIVE count(n) AS (
           SELECT 1 UNION ALL SELECT n+1 FROM count WHERE n<1450
         )
         INSERT INTO proof SELECT zeroblob(16000) FROM count;",
    )
    .unwrap();
    let original = extent(&path);
    assert!(original > MAX_MAIN_BYTES * 9 / 10);
    let journal = sidecar(&path, "-journal");
    assert!(!journal.exists());

    let transaction = conn.transaction().unwrap();
    transaction
        .execute("UPDATE proof SET bytes=randomblob(16000)", [])
        .unwrap();
    let journal_extent = extent(&journal);
    assert!(journal_extent > MAX_MAIN_BYTES * 9 / 10);
    assert!(journal_extent <= MAX_JOURNAL_BYTES);
    assert!(extent(&path) + journal_extent <= RESERVED_BYTES);
    let err = transaction
        .execute(
            "INSERT INTO proof VALUES(zeroblob(?1))",
            [MAX_MAIN_BYTES as i64],
        )
        .unwrap_err();
    assert_eq!(err.sqlite_error_code(), Some(ErrorCode::DiskFull));
    drop(transaction);
    assert_eq!(extent(&path), original);
    assert!(!journal.exists());
    drop(conn);

    let HardenedPeerConnection(reopened) =
        HardenedPeerConnection::open_installed(&path, KEY).unwrap();
    let count: i64 = reopened
        .query_row(
            "SELECT count(*) FROM proof WHERE bytes=zeroblob(16000)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1450);
    assert_eq!(pragma(&reopened, "max_page_count").unwrap(), MAX_PAGES);
    assert_eq!(pragma(&reopened, "cache_spill").unwrap(), 0);
}

#[test]
fn peer_installed_cursor_retains_full_deferred_queue_on_reopen() {
    use super::super::{Auth, CursorStore, EVENT_BYTES, Metadata, RawRecord, VISIBLE};

    const ACCOUNT: [u8; 32] = [1; 32];
    const ORIGIN: [u8; 32] = [2; 32];
    const WALL: u64 = 1_800_000_000;

    let (_directory, path) = cursor();
    let connection = HardenedPeerConnection::open_installed(&path, KEY).unwrap();
    let mut store = CursorStore::attach(connection, ACCOUNT, &[ORIGIN]).unwrap();
    let payload = vec![b'x'; EVENT_BYTES];
    let metadata = Metadata {
        auth: Auth::Unverified,
        expires: WALL + 60,
        prerequisite: None,
    };
    let record = |index: usize| {
        let mut id = [0; 32];
        id[..8].copy_from_slice(&(index as u64).to_be_bytes());
        RawRecord {
            id,
            stamp: WALL,
            bytes: &payload,
        }
    };
    for index in 0..VISIBLE {
        store.defer(ORIGIN, record(index), metadata, WALL).unwrap();
    }
    assert_eq!(
        store.defer(ORIGIN, record(VISIBLE), metadata, WALL).err(),
        Some(Error::Backlog)
    );
    drop(store);

    assert!(extent(&path) >= (VISIBLE * EVENT_BYTES) as u64);
    let connection = HardenedPeerConnection::open_installed(&path, KEY).unwrap();
    let mut store = CursorStore::attach(connection, ACCOUNT, &[ORIGIN]).unwrap();
    assert!(!store.capacity(ORIGIN).unwrap());
    for index in 0..VISIBLE {
        let expected = record(index);
        let saved = store.existing(ORIGIN, expected.id).unwrap().unwrap();
        assert_eq!(saved.record().bytes, expected.bytes);
    }
}
