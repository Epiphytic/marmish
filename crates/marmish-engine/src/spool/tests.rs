use super::*;
use crate::store::WorkerCredential;
use std::os::unix::fs::PermissionsExt;

fn empty() -> OutputStatus {
    OutputStatus {
        retained: 0,
        produced: 0,
        closed: false,
        gaps: vec![],
    }
}
fn status(commit: &OutputCommit) -> OutputStatus {
    OutputStatus {
        retained: commit.retained,
        produced: commit.produced,
        closed: commit.closed,
        gaps: commit.gaps.clone(),
    }
}
fn setup() -> (tempfile::TempDir, StateRoot, StorageKeys, JobId) {
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = StateRoot::open(temp.path()).unwrap();
    let front = root.lock_frontend().unwrap();
    let keys = root.initialize_storage(&front).unwrap();
    let job = JobId([3; 16]);
    root.prepare_job(job, &WorkerCredential([4; 32])).unwrap();
    (temp, root, keys, job)
}

#[test]
fn cap_binary_roundtrip_and_expiry_are_explicit() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let mut spool = Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 4, &guard).unwrap();
    let commit = spool.append(b"a\0\xffbmore", &guard).unwrap();
    assert_eq!((commit.retained, commit.produced), (4, 8));
    assert_eq!(commit.gaps[0].reason, MissingReason::Limit);
    let saved = status(&commit);
    assert_eq!(
        Spool::read(&root, &keys, job, Stream::Stdout, &saved, 0, 16, &guard)
            .unwrap()
            .bytes,
        b"a\0\xffb"
    );
    assert_eq!(
        Spool::read(&root, &keys, job, Stream::Stdout, &saved, 4, 16, &guard)
            .unwrap()
            .next_cursor,
        8
    );
    drop(spool);
    root.expire_spool(job, &guard).unwrap();
    let chunk = Spool::read(&root, &keys, job, Stream::Stdout, &saved, 0, 16, &guard).unwrap();
    assert!(chunk.bytes.is_empty());
    assert_eq!(chunk.status.retained, 0);
    assert!(chunk
        .status
        .gaps
        .iter()
        .any(|g| g.reason == MissingReason::StorageFailure));
}

#[test]
fn tiny_appends_are_compact_and_bound_physical_storage() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let cap = CHUNK * 2 + 1;
    let mut spool = Spool::open(&root, &keys, job, Stream::Stdout, &empty(), cap, &guard).unwrap();
    // Thousands of tiny appends must fill a row, rather than allocate per write.
    for _ in 0..1000 {
        spool.append(b"abc", &guard).unwrap();
    }
    spool
        .append(&vec![0xff; (cap - 3000) as usize], &guard)
        .unwrap();
    let db = spool.db.as_ref().unwrap();
    let rows: i64 = db
        .query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, cap.div_ceil(CHUNK) as i64);
    assert_eq!(prefix(db, cap).unwrap(), cap);
    let limit: i64 = db
        .query_row("PRAGMA max_page_count", [], |r| r.get(0))
        .unwrap();
    assert_eq!(limit, max_pages(cap) as i64);
    bounds(spool.path.as_ref().unwrap(), cap).unwrap();
    let saved = status(&spool.commit);
    let chunk = Spool::read(
        &root,
        &keys,
        job,
        Stream::Stdout,
        &saved,
        CHUNK - 2,
        5,
        &guard,
    )
    .unwrap();
    assert_eq!(chunk.bytes, vec![0xff; 5]);
}

#[test]
fn suffix_recovery_trims_midrow_and_multiple_rows() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let cap = CHUNK * 3;
    let mut spool = Spool::open(&root, &keys, job, Stream::Stdout, &empty(), cap, &guard).unwrap();
    let committed = spool.append(b"prefix", &guard).unwrap();
    spool
        .append(&vec![b'x'; (CHUNK * 2) as usize], &guard)
        .unwrap();
    drop(spool);
    let saved = status(&committed);
    let mut recovered =
        Spool::open(&root, &keys, job, Stream::Stdout, &saved, cap, &guard).unwrap();
    assert_eq!(prefix(recovered.db.as_ref().unwrap(), cap).unwrap(), 6);
    let commit = recovered.append(b"new", &guard).unwrap();
    assert_eq!(
        Spool::read(
            &root,
            &keys,
            job,
            Stream::Stdout,
            &status(&commit),
            0,
            20,
            &guard
        )
        .unwrap()
        .bytes,
        b"prefixnew"
    );
}

#[test]
fn transaction_rollback_never_publishes_a_partial_append() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let mut spool =
        Spool::open(&root, &keys, job, Stream::Stdout, &empty(), CHUNK, &guard).unwrap();
    let committed = spool.append(b"prefix", &guard).unwrap();
    {
        let tx = spool.db.as_mut().unwrap().transaction().unwrap();
        tx.execute(
            "UPDATE chunks SET data=CAST(data || X'7878' AS BLOB),end=end+2",
            [],
        )
        .unwrap();
        // Simulate pre-COMMIT failure by dropping the transaction.
    }
    assert_eq!(prefix(spool.db.as_ref().unwrap(), CHUNK).unwrap(), 6);
    assert_eq!(
        Spool::read(
            &root,
            &keys,
            job,
            Stream::Stdout,
            &status(&committed),
            0,
            20,
            &guard
        )
        .unwrap()
        .bytes,
        b"prefix"
    );
}

#[test]
fn busy_append_preserves_every_memory_field_and_retries() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let mut spool = Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 100, &guard).unwrap();
    spool.append(b"prefix", &guard).unwrap();
    let path = spool.path.as_ref().unwrap();
    let other = cipher::open(
        path,
        &keys,
        Context::Spool(job, Stream::Stdout),
        Journal::Delete,
    )
    .unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let before = spool.commit.clone();
    assert!(matches!(
        spool.append(b"retry", &guard),
        Err(EngineError::Busy)
    ));
    assert_eq!(
        (
            spool.commit.retained,
            spool.commit.produced,
            spool.commit.closed
        ),
        (before.retained, before.produced, before.closed)
    );
    assert_eq!(spool.commit.gaps, before.gaps);
    assert!(!spool.failed);
    other.execute_batch("ROLLBACK").unwrap();
    drop(other);
    let after = spool.append(b"retry", &guard).unwrap();
    assert_eq!((after.retained, after.produced), (11, 11));
}

#[test]
fn encrypted_bytes_require_the_right_key_and_stream() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let canary = b"spool-secret-canary-8bf2c8b70";
    let mut spool = Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 100, &guard).unwrap();
    let committed = spool.append(canary, &guard).unwrap();
    let path = spool.path.clone().unwrap();
    drop(spool);
    let contents = std::fs::read(&path).unwrap();
    assert!(!contents.windows(canary.len()).any(|w| w == canary));
    assert_ne!(&contents[..16], b"SQLite format 3\0");
    let plain = Connection::open(&path).unwrap();
    assert!(plain
        .query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))
        .is_err());
    drop(plain);
    let wrong = StorageKeys(zeroize::Zeroizing::new([42; 32]));
    let chunk = Spool::read(
        &root,
        &wrong,
        job,
        Stream::Stdout,
        &status(&committed),
        0,
        100,
        &guard,
    )
    .unwrap();
    assert!(chunk.bytes.is_empty());
    assert!(cipher::open(
        &path,
        &keys,
        Context::Spool(job, Stream::Stderr),
        Journal::Delete
    )
    .is_err());
    assert_eq!(std::fs::read(path).unwrap(), contents);
}

#[test]
fn missing_retained_database_is_not_recreated() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let mut spool = Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 100, &guard).unwrap();
    let committed = spool.append(b"prefix", &guard).unwrap();
    let path = spool.path.clone().unwrap();
    drop(spool);
    std::fs::remove_file(&path).unwrap();
    let mut reopened = Spool::open(
        &root,
        &keys,
        job,
        Stream::Stdout,
        &status(&committed),
        100,
        &guard,
    )
    .unwrap();
    let after = reopened.append(b"discard", &guard).unwrap();
    assert_eq!((after.retained, after.produced), (0, 13));
    assert!(!path.exists());
}

#[test]
fn corrupt_or_mismatched_database_is_not_reinitialized() {
    for corrupt in [false, true] {
        let (_temp, root, keys, job) = setup();
        let guard = root.lock_io(job).unwrap();
        let mut spool =
            Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 100, &guard).unwrap();
        let committed = spool.append(b"prefix", &guard).unwrap();
        let path = spool.path.clone().unwrap();
        if !corrupt {
            spool
                .db
                .as_ref()
                .unwrap()
                .execute("UPDATE spool_meta SET stream=1", [])
                .unwrap();
        }
        drop(spool);
        if corrupt {
            std::fs::write(&path, b"corrupt encrypted database").unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mut reopened = Spool::open(
            &root,
            &keys,
            job,
            Stream::Stdout,
            &status(&committed),
            100,
            &guard,
        )
        .unwrap();
        assert!(reopened.failed);
        assert_eq!(reopened.append(b"discard", &guard).unwrap().retained, 0);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

#[test]
fn shorter_authenticated_prefix_is_preserved_and_followed_by_gap() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let mut spool = Spool::open(
        &root,
        &keys,
        job,
        Stream::Stdout,
        &empty(),
        CHUNK * 2,
        &guard,
    )
    .unwrap();
    let committed = spool
        .append(&vec![b'a'; (CHUNK + 5) as usize], &guard)
        .unwrap();
    spool
        .db
        .as_ref()
        .unwrap()
        .execute("DELETE FROM chunks WHERE start=?1", [CHUNK as i64])
        .unwrap();
    drop(spool);
    let mut reopened = Spool::open(
        &root,
        &keys,
        job,
        Stream::Stdout,
        &status(&committed),
        CHUNK * 2,
        &guard,
    )
    .unwrap();
    assert_eq!(reopened.commit.retained, CHUNK);
    assert!(reopened.failed);
    let after = reopened.append(b"discard", &guard).unwrap();
    assert_eq!(after.retained, CHUNK);
    assert_eq!(
        Spool::read(
            &root,
            &keys,
            job,
            Stream::Stdout,
            &status(&after),
            0,
            4,
            &guard
        )
        .unwrap()
        .bytes,
        b"aaaa"
    );
}

#[test]
fn wrong_io_guard_and_invalid_limits_fail_closed() {
    let (_temp, root, keys, job) = setup();
    let other = JobId([8; 16]);
    root.prepare_job(other, &WorkerCredential([1; 32])).unwrap();
    let guard = root.lock_io(other).unwrap();
    assert!(matches!(
        Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 8, &guard),
        Err(EngineError::UnsafePath)
    ));
    let guard = root.lock_io(job).unwrap();
    assert!(Spool::open(&root, &keys, job, Stream::Stdout, &empty(), 0, &guard).is_err());
    assert!(Spool::read(&root, &keys, job, Stream::Stdout, &empty(), 0, 0, &guard).is_err());
}

#[test]
fn physical_and_journal_bounds_are_numeric_and_enforced() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("stdout.db");
    let file = std::fs::File::create(&path).unwrap();
    let cap = 17;
    let bound = max_pages(cap) * PAGE;
    file.set_len(bound).unwrap();
    bounds(&path, cap).unwrap();
    file.set_len(bound + 1).unwrap();
    assert!(bounds(&path, cap).is_err());
    file.set_len(bound).unwrap();
    let journal = std::fs::File::create(temp.path().join("stdout.db-journal")).unwrap();
    journal.set_len(2 * bound + 65536).unwrap();
    bounds(&path, cap).unwrap();
    journal.set_len(2 * bound + 65537).unwrap();
    assert!(bounds(&path, cap).is_err());
}

#[test]
fn closed_missing_stream_is_not_created() {
    let (_temp, root, keys, job) = setup();
    let guard = root.lock_io(job).unwrap();
    let mut saved = empty();
    saved.closed = true;
    let spool = Spool::open(&root, &keys, job, Stream::Stdout, &saved, 100, &guard).unwrap();
    assert!(spool.db.is_none());
    assert!(root
        .prepare_spool_db(job, Stream::Stdout, false, &guard)
        .is_err());
}
