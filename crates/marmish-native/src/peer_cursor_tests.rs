use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use super::pages::Cursor;
use super::*;

const A: Origin = [0xaa; 32];
const B: Origin = [0xbb; 32];
const ACCOUNT: [u8; 32] = [0xcc; 32];
const KEY: [u8; 32] = [0x65; 32];
const W: u64 = 1_800_000_000;
const TEST_BYTES: &[u8] = b"opaque receiver fixture with distinctive exact event bytes";

struct Fixture(tempfile::NamedTempFile);

impl Fixture {
    fn new() -> Self {
        Self(tempfile::NamedTempFile::new().unwrap())
    }

    fn connection(&self) -> HardenedPeerConnection {
        HardenedPeerConnection::open(self.0.path(), KEY).unwrap()
    }

    fn open(&self) -> CursorStore {
        CursorStore::attach(self.connection(), ACCOUNT, &[B, A]).unwrap()
    }
}

fn id(value: u16) -> EventId {
    let mut id = [0; 32];
    id[..2].copy_from_slice(&value.to_be_bytes());
    id
}

fn record(number: u16, stamp: u64) -> RawRecord<'static> {
    RawRecord {
        id: id(number),
        stamp,
        bytes: TEST_BYTES,
    }
}

fn metadata(auth: Auth, prerequisite: Option<[u8; 32]>) -> Metadata {
    Metadata {
        auth,
        expires: W + 900,
        prerequisite,
    }
}

fn request<'a>(cursor: &Cursor, records: &'a [RawRecord<'a>]) -> Page<'a> {
    Page {
        origin: cursor.origin,
        since: cursor.lower,
        until: cursor.until,
        records,
    }
}

fn begin(store: &mut CursorStore, origin: Origin, wall: u64) -> Cursor {
    store
        .scan(origin, ScanStart::Recovery, wall)
        .unwrap()
        .unwrap()
}

fn invalid_receipts(
    store: &mut CursorStore,
    origin: Origin,
    rows: &[RawRecord<'_>],
    wall: u64,
) -> Vec<Receipt> {
    rows.iter()
        .map(|row| store.commit_invalid(origin, *row, wall).unwrap())
        .collect()
}

#[test]
fn peer_cursor_core_ties_cutoffs_and_periodic_restart_survive_reopen() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let start = begin(&mut store, A, W);
    assert_eq!(
        (start.lower, start.upper, start.until),
        (W - 259_500, W + 300, W + 300)
    );
    let first = [
        record(1, start.upper),
        record(4, W + 90),
        record(2, W + 90),
        record(3, W + 90),
    ];
    let mut disposed = invalid_receipts(&mut store, A, &first, W);
    disposed.reverse();
    let current = store
        .commit_page(&start, &request(&start, &first), &disposed, W)
        .unwrap()
        .unwrap();
    assert_eq!(current.visible, 4);
    assert_eq!(current.ties, [id(2), id(3), id(4)]);
    assert_eq!(current.until, W + 90);
    assert_eq!(
        store
            .commit_page(&start, &request(&start, &first), &disposed, W)
            .err(),
        Some(Error::Stale)
    );
    drop(store);

    let mut store = fixture.open();
    let resume = begin(&mut store, A, W + 12);
    assert_eq!(resume, current);
    let second = [
        record(3, W + 90),
        record(5, W + 90),
        record(4, W + 90),
        record(6, start.lower),
    ];
    let disposed = invalid_receipts(&mut store, A, &second, W + 12);
    let lower = store
        .commit_page(&resume, &request(&resume, &second), &disposed, W + 12)
        .unwrap()
        .unwrap();
    assert_eq!((lower.until, lower.visible), (start.lower, 6));
    assert_eq!(lower.ties, [id(6)]);
    let last = [record(6, start.lower)];
    let disposed = invalid_receipts(&mut store, A, &last, W + 12);
    assert!(
        store
            .commit_page(&lower, &request(&lower, &last), &disposed, W + 12)
            .unwrap()
            .is_none()
    );
    let previous_anchor: i64 = store
        .conn
        .query_row(
            "SELECT completed_anchor FROM peer_cursor_origin WHERE origin=?1",
            [A],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(previous_anchor, seconds(W).unwrap());
    assert!(
        store
            .scan(A, ScanStart::Periodic, W + 611)
            .unwrap()
            .is_none()
    );
    let next = store
        .scan(A, ScanStart::Periodic, W + 612)
        .unwrap()
        .unwrap();
    assert_eq!(
        (next.anchor, next.lower, next.until),
        (W + 612, W + 612 - 259_500, W + 912)
    );
    assert!(next.revision > lower.revision);
    assert!(
        store
            .commit_page(&next, &request(&next, &[]), &[], W + 612)
            .unwrap()
            .is_none()
    );
    let same_second = begin(&mut store, A, W + 612);
    assert_eq!(same_second.until, next.until);
    assert_ne!(same_second.revision, next.revision);
    assert_eq!(
        store
            .commit_page(&next, &request(&next, &[]), &[], W + 612)
            .err(),
        Some(Error::Stale)
    );
}

#[test]
fn peer_cursor_core_invalid_page_and_relay_quarantine_are_closed() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let current = begin(&mut store, A, W);
    let records = [record(1, W)];
    let wrong_origin = Page {
        origin: B,
        ..request(&current, &records)
    };
    assert_eq!(
        store.commit_page(&current, &wrong_origin, &[], W).err(),
        Some(Error::Page)
    );
    assert_eq!(
        store
            .commit_page(&current, &request(&current, &records), &[], W)
            .err(),
        Some(Error::Page)
    );
    let competing = RawRecord {
        bytes: b"different raw bytes",
        ..records[0]
    };
    let disposed = invalid_receipts(&mut store, A, &[competing], W);
    assert_eq!(
        store
            .commit_page(&current, &request(&current, &records), &disposed, W)
            .err(),
        Some(Error::Page)
    );
    assert_eq!(begin(&mut store, A, W), current);

    let wrong_shape = [
        vec![record(1, W), record(1, W - 1)],
        vec![record(2, W - 1), record(3, W)],
        vec![record(1, W), record(2, W), record(3, W), record(4, W)],
        vec![record(1, W + 301)],
        vec![record(1, W - 259_501)],
        vec![
            record(1, W),
            record(2, W - 1),
            record(3, W - 2),
            record(4, W - 3),
            record(5, W - 4),
        ],
    ];
    for (offset, records) in wrong_shape.iter().enumerate() {
        let wall = W + offset as u64 * 60;
        let active = begin(&mut store, A, wall);
        // Each input case uses the same anchored bounds, translated to this pass.
        let translated: Vec<_> = records
            .iter()
            .map(|event| RawRecord {
                stamp: event.stamp + (wall - W),
                ..*event
            })
            .collect();
        assert_eq!(
            store
                .commit_page(&active, &request(&active, &translated), &[], wall)
                .err(),
            Some(Error::RelayContract)
        );
        drop(store);
        store = fixture.open();
        assert_eq!(
            store.scan(A, ScanStart::Recovery, wall + 59).err(),
            Some(Error::RelayContract)
        );
        assert_eq!(store.requalify(A, wall + 59), Err(Error::RelayContract));
        store.requalify(A, wall + 60).unwrap();
    }
    let active = begin(&mut store, A, W + 360);
    let oversize = vec![0; EVENT_BYTES + 1];
    let records = [RawRecord {
        bytes: &oversize,
        ..record(11, W + 360)
    }];
    assert_eq!(
        store
            .commit_page(&active, &request(&active, &records), &[], W + 360)
            .err(),
        Some(Error::RelayContract)
    );
    assert_eq!(begin(&mut store, B, W).origin, B);
}

#[test]
fn peer_cursor_core_visible_budget_does_not_fake_exhaustion() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let mut current = begin(&mut store, A, W);
    for index in 0..64_u16 {
        let rows: [_; 4] = std::array::from_fn(|offset| {
            let n = index * 4 + offset as u16;
            record(n, W - u64::from(n))
        });
        let receipts = invalid_receipts(&mut store, A, &rows, W);
        current = store
            .commit_page(&current, &request(&current, &rows), &receipts, W)
            .unwrap()
            .unwrap();
    }
    assert_eq!(current.visible, VISIBLE);
    let new = [record(300, W - 256)];
    let receipts = invalid_receipts(&mut store, A, &new, W);
    assert_eq!(
        store
            .commit_page(&current, &request(&current, &new), &receipts, W)
            .err(),
        Some(Error::RelayContract)
    );
    let persisted: (i64, Option<i64>) = store
        .conn
        .query_row(
            "SELECT visible, completed_anchor FROM peer_cursor_origin WHERE origin=?1",
            [A],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(persisted, (256, None));
    drop(store);
    let mut store = fixture.open();
    assert_eq!(
        store.scan(A, ScanStart::Recovery, W + 60).err(),
        Some(Error::RelayContract)
    );
    store.requalify(A, W + 60).unwrap();
    let opened = begin(&mut store, A, W + 60);
    assert_eq!(opened.visible, 0);
    assert!(opened.revision > current.revision);
}

fn effect_tables(store: &CursorStore) {
    store
        .conn
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS owner_fence (origin BLOB, id BLOB, PRIMARY KEY(origin,id));
         CREATE TABLE IF NOT EXISTS owner_effect (n INTEGER);",
        )
        .unwrap();
}

fn effect(tx: &Transaction<'_>, origin: Origin, id: EventId) -> Result<TrustedOutcome> {
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO owner_fence VALUES (?1,?2)",
        params![origin, id],
    )?;
    if inserted == 0 {
        return Ok(TrustedOutcome::AlreadyApplied);
    }
    tx.execute("INSERT INTO owner_effect VALUES (1)", [])?;
    Ok(TrustedOutcome::Applied)
}

fn effect_count(store: &CursorStore) -> i64 {
    store
        .conn
        .query_row("SELECT COUNT(*) FROM owner_effect", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn peer_cursor_core_effect_fence_cleanup_rolls_back_and_page_replays() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    effect_tables(&store);
    let active = begin(&mut store, A, W);
    let raw = record(1, W - 1);
    let other = RawRecord {
        bytes: b"competing bytes under the same claimed ID",
        ..raw
    };
    store
        .defer(A, other, metadata(Auth::Unverified, None), W)
        .unwrap();
    store
        .defer(B, raw, metadata(Auth::Unverified, None), W)
        .unwrap();
    assert_eq!(
        store
            .commit_trusted(A, raw, W, |tx| {
                effect(tx, A, raw.id)?;
                Err(Error::Storage)
            })
            .err(),
        Some(Error::Storage)
    );
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.commit_trusted(A, raw, W, |tx| {
            effect(tx, A, raw.id)?;
            panic!("disposable owner callback failure")
        });
    }));
    assert!(panicked.is_err());
    assert_eq!(effect_count(&store), 0);
    assert!(store.existing(A, raw.id).unwrap().is_some());
    let (_, applied) = store
        .commit_trusted(A, raw, W, |tx| effect(tx, A, raw.id))
        .unwrap();
    assert_eq!(applied, TrustedOutcome::Applied);
    assert!(store.existing(A, raw.id).unwrap().is_none());
    assert!(store.existing(B, raw.id).unwrap().is_some());
    assert_eq!(effect_count(&store), 1);
    drop(store);

    let mut store = fixture.open();
    assert_eq!(begin(&mut store, A, W), active);
    let (receipt, replay) = store
        .commit_trusted(A, raw, W, |tx| effect(tx, A, raw.id))
        .unwrap();
    assert_eq!(replay, TrustedOutcome::AlreadyApplied);
    assert!(
        store
            .commit_page(&active, &request(&active, &[raw]), &[receipt], W)
            .unwrap()
            .is_none()
    );
    assert_eq!(effect_count(&store), 1);
}

#[test]
fn peer_cursor_exact_custody_cleanup_preserves_a_competing_raw_tuple() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    effect_tables(&store);
    let raw = record(1, W - 1);
    let competing = RawRecord {
        bytes: b"competing bytes under the same claimed ID",
        ..raw
    };
    store
        .defer(A, competing, metadata(Auth::Unverified, None), W)
        .unwrap();
    store
        .commit_trusted_exact(A, raw, W, |tx| effect(tx, A, raw.id))
        .unwrap();
    assert_eq!(effect_count(&store), 1);
    let retained = store.existing(A, raw.id).unwrap().unwrap();
    assert_eq!(retained.record().bytes, competing.bytes);

    let (_, replay) = store
        .commit_trusted_exact(A, competing, W, |tx| effect(tx, A, raw.id))
        .unwrap();
    assert_eq!(replay, TrustedOutcome::AlreadyApplied);
    assert!(store.existing(A, raw.id).unwrap().is_none());
    assert_eq!(effect_count(&store), 1);
}

#[test]
fn peer_cursor_core_failed_writes_and_missing_deferrals_leave_cursor_unmoved() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let active = begin(&mut store, A, W);
    let raw = record(1, W - 1);
    store
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER fail_defer BEFORE INSERT ON peer_cursor_deferred
         BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
        )
        .unwrap();
    assert_eq!(
        store
            .defer(A, raw, metadata(Auth::Unverified, None), W)
            .err(),
        Some(Error::Storage)
    );
    assert_eq!(begin(&mut store, A, W), active);
    store.conn.execute_batch("DROP TRIGGER fail_defer").unwrap();
    let receipt = store
        .defer(A, raw, metadata(Auth::Unverified, None), W)
        .unwrap();
    let rejected = store
        .existing(A, raw.id)
        .unwrap()
        .unwrap()
        .reverify(|_| Some(RetryFinding::Invalid))
        .unwrap();
    store.finish_retry(rejected, W).unwrap();
    assert_eq!(
        store
            .commit_page(&active, &request(&active, &[raw]), &[receipt], W)
            .err(),
        Some(Error::Stale)
    );
    let receipt = store.commit_invalid(A, raw, W).unwrap();
    store
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER fail_page BEFORE UPDATE OF completed_anchor ON peer_cursor_origin
         BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;",
        )
        .unwrap();
    assert_eq!(
        store
            .commit_page(&active, &request(&active, &[raw]), &[receipt], W)
            .err(),
        Some(Error::Storage)
    );
    drop(store);
    assert_eq!(begin(&mut fixture.open(), A, W), active);
}

fn owner_wall_table(store: &CursorStore) {
    store
        .conn
        .execute_batch("CREATE TABLE fixture_owner_wall (wall INTEGER NOT NULL);")
        .unwrap();
    store
        .conn
        .execute(
            "INSERT INTO fixture_owner_wall VALUES (?1)",
            [seconds(W).unwrap()],
        )
        .unwrap();
}

fn advance_owner_wall(tx: &Transaction<'_>, wall: u64) -> Result<()> {
    tx.execute("UPDATE fixture_owner_wall SET wall=?1", [seconds(wall)?])?;
    Ok(())
}

fn assert_walls(store: &CursorStore, wall: u64) {
    let persisted: (i64, i64) = store
        .conn
        .query_row(
            "SELECT wall, (SELECT wall FROM fixture_owner_wall)
             FROM peer_cursor_origin WHERE origin=?1",
            [A],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let expected = seconds(wall).unwrap();
    assert_eq!(persisted, (expected, expected));
}

#[test]
fn peer_cursor_page_owner_fence_rolls_back_and_precedes_quarantine() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let active = begin(&mut store, A, W);
    owner_wall_table(&store);
    let raw = record(1, W);
    let bad_order = [raw, raw];
    let ordinary = [raw];
    let receipt = store.commit_invalid(A, raw, W).unwrap();

    for (records, receipts) in [
        (ordinary.as_slice(), std::slice::from_ref(&receipt)),
        (bad_order.as_slice(), &[]),
    ] {
        assert_eq!(
            store
                .commit_page_with_fence(
                    &active,
                    &request(&active, records),
                    receipts,
                    W + 1,
                    |tx| {
                        advance_owner_wall(tx, W + 1)?;
                        Err(Error::Binding)
                    },
                )
                .err(),
            Some(Error::Binding)
        );
        assert_walls(&store, W);
        assert_eq!(begin(&mut store, A, W), active);
    }

    store.conn.execute_batch(
        "CREATE TEMP TRIGGER fail_fenced_page BEFORE UPDATE OF completed_anchor ON peer_cursor_origin
         BEGIN SELECT RAISE(ABORT,'fixture page write failure'); END;",
    ).unwrap();
    assert_eq!(
        store
            .commit_page_with_fence(
                &active,
                &request(&active, &ordinary),
                std::slice::from_ref(&receipt),
                W + 1,
                |tx| advance_owner_wall(tx, W + 1),
            )
            .err(),
        Some(Error::Storage)
    );
    assert_walls(&store, W);
    assert_eq!(begin(&mut store, A, W), active);
    store
        .conn
        .execute_batch("DROP TRIGGER fail_fenced_page")
        .unwrap();

    let extra = store.commit_invalid(A, record(2, W), W).unwrap();
    for receipts in [&[][..], &[receipt, extra][..]] {
        assert_eq!(
            store
                .commit_page_with_fence(
                    &active,
                    &request(&active, &ordinary),
                    receipts,
                    W + 1,
                    |_| { panic!("mismatched receipt counts must refuse before the owner fence") }
                )
                .err(),
            Some(Error::Page)
        );
        assert_walls(&store, W);
        assert_eq!(begin(&mut store, A, W), active);
    }
    assert_eq!(
        store
            .commit_page_with_fence(&active, &request(&active, &bad_order), &[], W + 1, |tx| {
                let already_quarantined: bool = tx.query_row(
                    "SELECT quarantine_until IS NOT NULL FROM peer_cursor_origin WHERE origin=?1",
                    [A],
                    |row| row.get(0),
                )?;
                assert!(!already_quarantined);
                advance_owner_wall(tx, W + 1)
            },)
            .err(),
        Some(Error::RelayContract)
    );
    drop(store);

    let mut store = fixture.open();
    assert_walls(&store, W + 1);
    assert_eq!(
        store
            .commit_page_with_fence(&active, &request(&active, &[]), &[], W + 2, |_| {
                panic!("active quarantine must refuse before a new fence write")
            })
            .err(),
        Some(Error::RelayContract)
    );
    assert_walls(&store, W + 1);
    store.requalify(A, W + 61).unwrap();
    let current = begin(&mut store, A, W + 61);
    assert_eq!(
        store
            .commit_page_with_fence(&active, &request(&active, &[]), &[], W + 61, |_| {
                panic!("stale page must refuse before the owner fence")
            })
            .err(),
        Some(Error::Stale)
    );
    assert!(
        store
            .commit_page_with_fence(&current, &request(&current, &[]), &[], W + 62, |tx| {
                advance_owner_wall(tx, W + 62)
            },)
            .unwrap()
            .is_none()
    );
    assert_walls(&store, W + 62);
}

#[test]
fn peer_cursor_retry_owner_fences_preserve_exact_rows_on_refusal() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    begin(&mut store, A, W);
    owner_wall_table(&store);
    let raw = record(1, W);
    let binding = id(2);
    let verified = metadata(Auth::FlightVerified, Some(binding));
    store
        .defer_with_fence(A, raw, verified, W + 1, |tx| advance_owner_wall(tx, W + 1))
        .unwrap();
    assert_walls(&store, W + 1);
    let due = store.existing(A, raw.id).unwrap().unwrap();
    let wall = W + 2;
    let refuse = |tx: &Transaction<'_>| {
        advance_owner_wall(tx, wall)?;
        Err(Error::Binding)
    };

    assert_eq!(
        store.due_with_fence(A, wall, refuse).err(),
        Some(Error::Binding)
    );
    assert_eq!(
        store.commit_invalid_with_fence(A, raw, wall, refuse).err(),
        Some(Error::Binding)
    );
    assert_eq!(
        store.wake_with_fence(A, binding, wall, refuse),
        Err(Error::Binding)
    );
    assert_eq!(
        store.retry_again_with_fence(&due, wall, refuse),
        Err(Error::Binding)
    );
    let invalid = due.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
    assert_eq!(
        store.finish_retry_with_fence(invalid, wall, refuse),
        Err(Error::Binding)
    );
    assert_walls(&store, W + 1);
    assert_eq!(store.existing(A, raw.id).unwrap().unwrap().retry_at, wall);

    let competing = RawRecord {
        bytes: b"alternate lexical rendering",
        ..raw
    };
    assert_eq!(
        store
            .defer_with_fence(A, competing, verified, wall, |tx| advance_owner_wall(
                tx, wall
            ))
            .err(),
        Some(Error::Conflict)
    );
    assert_walls(&store, W + 1);

    let due = store
        .due_with_fence(A, wall, |tx| advance_owner_wall(tx, wall))
        .unwrap()
        .unwrap();
    assert_eq!(due.record().bytes, raw.bytes);
    assert_walls(&store, wall);
    store
        .retry_again_with_fence(&due, wall, |tx| advance_owner_wall(tx, wall))
        .unwrap();
    assert_eq!(
        store.existing(A, raw.id).unwrap().unwrap().retry_at,
        wall + 2
    );
    assert_eq!(
        store.wake_with_fence(A, binding, wall + 1, |tx| advance_owner_wall(tx, wall + 1)),
        Ok(1)
    );
    assert_eq!(
        store.retry_again_with_fence(&due, wall + 2, |_| {
            panic!("stale retry must refuse before the owner fence")
        }),
        Err(Error::Stale)
    );
    let old = due.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
    assert_eq!(
        store.finish_retry_with_fence(old, wall + 2, |_| {
            panic!("stale retry must refuse before the owner fence")
        }),
        Err(Error::Stale)
    );
    assert_walls(&store, wall + 1);
    let current = store.existing(A, raw.id).unwrap().unwrap();
    let premature = current
        .reverify(|_| {
            Some(RetryFinding::Expired {
                verified_expiry: verified.expires,
            })
        })
        .unwrap();
    assert_eq!(
        store.finish_retry_with_fence(premature, wall + 2, |_| {
            panic!("unexpired retry must refuse before the owner fence")
        }),
        Err(Error::Classification)
    );
    assert_walls(&store, wall + 1);
    let proof = current.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
    store
        .finish_retry_with_fence(proof, wall + 2, |tx| advance_owner_wall(tx, wall + 2))
        .unwrap();
    assert!(store.existing(A, raw.id).unwrap().is_none());

    store.defer(A, raw, verified, wall + 2).unwrap();
    store
        .commit_invalid_with_fence(A, raw, wall + 3, |tx| advance_owner_wall(tx, wall + 3))
        .unwrap();
    assert!(store.existing(A, raw.id).unwrap().is_none());
    assert_walls(&store, wall + 3);
    assert_eq!(
        store
            .due_with_fence(A, wall + 4, |tx| {
                advance_owner_wall(tx, wall + 4)?;
                Err(Error::Binding)
            })
            .err(),
        Some(Error::Binding)
    );
    assert_walls(&store, wall + 3);
    assert!(
        store
            .due_with_fence(A, wall + 4, |tx| advance_owner_wall(tx, wall + 4))
            .unwrap()
            .is_none()
    );
    assert_walls(&store, wall + 4);
}

#[test]
fn peer_cursor_saved_flight_checks_current_row_before_custody_effect() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    effect_tables(&store);
    owner_wall_table(&store);
    let raw = record(1, W);
    let consent = id(2);
    let verified = metadata(Auth::FlightVerified, Some(consent));
    store.defer(A, raw, verified, W).unwrap();
    store.defer(B, raw, verified, W).unwrap();
    let foreign = store.existing(B, raw.id).unwrap().unwrap();
    let initial = store
        .due_with_fence(A, W + 1, |tx| advance_owner_wall(tx, W + 1))
        .unwrap()
        .unwrap();

    assert_eq!(
        store
            .commit_trusted_exact(A, raw, W + 1, |tx| {
                assert_eq!(
                    initial.require_current_flight(tx, B, consent),
                    Err(Error::Binding)
                );
                initial.require_current_flight(tx, A, id(3))?;
                unreachable!("a mismatched Consent cannot execute its effect")
            })
            .err(),
        Some(Error::Binding)
    );
    store.retry_again(&initial, W + 1).unwrap();
    let current = store.existing(A, raw.id).unwrap().unwrap();
    assert_eq!(
        store
            .commit_trusted_exact(A, raw, W + 3, |tx| {
                initial.require_current_flight(tx, A, consent)?;
                effect(tx, A, raw.id)
            })
            .err(),
        Some(Error::Stale)
    );
    assert_eq!(
        store
            .commit_trusted_exact(A, raw, W + 3, |tx| {
                advance_owner_wall(tx, W + 3)?;
                foreign.require_current_flight(tx, A, consent)?;
                effect(tx, A, raw.id)
            })
            .err(),
        Some(Error::Binding)
    );
    assert_eq!(effect_count(&store), 0);
    assert_walls(&store, W + 1);

    // A damaged row can retain the revision. Independently check its state and bytes.
    let wall = W + 3;
    let changed = [
        (
            "UPDATE peer_cursor_deferred SET stamp=?1 WHERE origin=?2 AND id=?3",
            seconds(W - 1).unwrap(),
        ),
        (
            "UPDATE peer_cursor_deferred SET expires=?1 WHERE origin=?2 AND id=?3",
            seconds(verified.expires + 1).unwrap(),
        ),
        (
            "UPDATE peer_cursor_deferred SET retry_at=?1 WHERE origin=?2 AND id=?3",
            seconds(wall + 1).unwrap(),
        ),
        (
            "UPDATE peer_cursor_deferred SET step=?1 WHERE origin=?2 AND id=?3",
            2,
        ),
    ];
    for (update, unexpected) in changed {
        assert_eq!(
            store
                .commit_trusted_exact(A, raw, wall, |tx| {
                    advance_owner_wall(tx, wall)?;
                    tx.execute(update, params![unexpected, A, raw.id])?;
                    current.require_current_flight(tx, A, consent)?;
                    effect(tx, A, raw.id)
                })
                .err(),
            Some(Error::Stale)
        );
    }
    for (update, unexpected) in [
        (
            "UPDATE peer_cursor_deferred SET prerequisite=?1 WHERE origin=?2 AND id=?3",
            &[9; 32][..],
        ),
        (
            "UPDATE peer_cursor_deferred SET frame=?1 WHERE origin=?2 AND id=?3",
            b"different lexical bytes".as_slice(),
        ),
    ] {
        assert_eq!(
            store
                .commit_trusted_exact(A, raw, wall, |tx| {
                    advance_owner_wall(tx, wall)?;
                    tx.execute(update, params![unexpected, A, raw.id])?;
                    current.require_current_flight(tx, A, consent)?;
                    effect(tx, A, raw.id)
                })
                .err(),
            Some(Error::Stale)
        );
    }
    assert_eq!(
        store
            .commit_trusted_exact(A, raw, wall, |tx| {
                advance_owner_wall(tx, wall)?;
                tx.execute(
                    "DELETE FROM peer_cursor_deferred WHERE origin=?1 AND id=?2",
                    params![A, raw.id],
                )?;
                current.require_current_flight(tx, A, consent)?;
                effect(tx, A, raw.id)
            })
            .err(),
        Some(Error::Stale)
    );
    assert_eq!(effect_count(&store), 0);
    assert_walls(&store, W + 1);

    let (_, applied) = store
        .commit_trusted_exact(A, raw, wall, |tx| {
            advance_owner_wall(tx, wall)?;
            current.require_current_flight(tx, A, consent)?;
            effect(tx, A, raw.id)
        })
        .unwrap();
    assert_eq!(applied, TrustedOutcome::Applied);
    assert_eq!(effect_count(&store), 1);
    assert_walls(&store, wall);
    assert!(store.existing(A, raw.id).unwrap().is_none());
}

#[test]
fn peer_cursor_core_clock_floor_and_rebase_preserve_retry_evidence() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let before = begin(&mut store, A, W);
    let raw = record(5, W);
    let _ = store
        .defer(A, raw, metadata(Auth::FlightVerified, Some(id(7))), W + 300)
        .unwrap();
    let saved = store.existing(A, raw.id).unwrap().unwrap();
    let proof = saved.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
    assert_eq!(
        store.scan(A, ScanStart::Recovery, W).err(),
        Some(Error::ClockRegressed)
    );
    assert_eq!(
        store
            .commit_page(&before, &request(&before, &[]), &[], W)
            .err(),
        Some(Error::ClockRegressed)
    );
    assert_eq!(
        store
            .defer(A, raw, metadata(Auth::Unverified, None), W)
            .err(),
        Some(Error::ClockRegressed)
    );
    assert_eq!(store.due(A, W).err(), Some(Error::ClockRegressed));
    assert_eq!(store.wake(A, id(7), W), Err(Error::ClockRegressed));
    assert_eq!(store.retry_again(&saved, W), Err(Error::ClockRegressed));
    assert_eq!(
        store.commit_invalid(A, raw, W).err(),
        Some(Error::ClockRegressed)
    );
    assert_eq!(
        store.commit_trusted(A, raw, W, |_| unreachable!()).err(),
        Some(Error::ClockRegressed)
    );
    assert_eq!(store.finish_retry(proof, W), Err(Error::ClockRegressed));
    drop(store);

    let mut store = fixture.open();
    assert_eq!(store.due(A, W).err(), Some(Error::ClockRegressed));
    let stale = saved.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
    store.rebase_wall(A, W).unwrap();
    let due = store.due(A, W).unwrap().unwrap();
    assert_eq!(due.record().bytes, raw.bytes);
    assert_eq!(due.metadata, saved.metadata);
    assert_eq!(due.retry_at, W);
    assert_eq!(store.finish_retry(stale, W), Err(Error::Stale));
    let fresh = begin(&mut store, A, W);
    assert!(fresh.revision > before.revision);
    assert_eq!(fresh.lower, before.lower);
    assert_eq!(
        store
            .commit_page(&before, &request(&before, &[]), &[], W)
            .err(),
        Some(Error::Stale)
    );
    store.retry_again(&due, W).unwrap();
    assert_eq!(store.existing(A, raw.id).unwrap().unwrap().retry_at, W + 2);
}

#[test]
fn peer_cursor_core_backoff_wake_coalescence_and_fresh_expiry() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let raw = record(2, W - 20);
    let known = metadata(Auth::FlightVerified, Some(id(6)));
    store.defer(A, raw, known, W).unwrap();
    assert!(store.due(A, W).unwrap().is_none());
    let mut wall = W + 1;
    for interval in [2, 4, 8, 16, 32, 60, 60] {
        let due = store.due(A, wall).unwrap().unwrap();
        assert_eq!(due.retry_at, wall);
        store.retry_again(&due, wall).unwrap();
        store
            .defer(A, raw, metadata(Auth::Unverified, None), wall)
            .unwrap();
        let saved = store.existing(A, raw.id).unwrap().unwrap();
        assert_eq!(saved.metadata, known);
        assert_eq!(saved.retry_at, wall + interval);
        wall += interval;
    }
    let snapshot = store.due(A, wall).unwrap().unwrap();
    store.retry_again(&snapshot, wall).unwrap();
    let saved = store.existing(A, raw.id).unwrap().unwrap();
    assert_eq!(store.wake(B, id(6), wall), Ok(0));
    assert_eq!(store.wake(A, id(3), wall), Ok(0));
    assert_eq!(store.wake(A, id(6), wall), Ok(1));
    assert_eq!(store.wake(A, id(6), wall), Ok(0));
    assert_eq!(store.retry_again(&saved, wall), Err(Error::Stale));
    let current = store.due(A, wall).unwrap().unwrap();
    assert_eq!(current.retry_at, wall);
    let classify = |raw: RawRecord<'_>| {
        assert_eq!(raw.bytes, TEST_BYTES);
        Some(RetryFinding::Expired {
            verified_expiry: known.expires,
        })
    };
    assert_eq!(
        store.finish_retry(current.reverify(classify).unwrap(), wall),
        Err(Error::Classification)
    );
    store
        .finish_retry(current.reverify(classify).unwrap(), known.expires)
        .unwrap();
    assert!(store.existing(A, raw.id).unwrap().is_none());
    store.defer(A, raw, known, known.expires).unwrap();
    assert_eq!(
        store.finish_retry(current.reverify(classify).unwrap(), known.expires),
        Err(Error::Stale)
    );
}

#[test]
fn peer_cursor_installed_scope_cap_and_exact_expiry_preserve_sql_floors() {
    use super::consent_checkpoint::{
        Checkpoint, Error as CheckpointError, Policy,
        provisioning::prepared_consent::custody::ReceivePolicy,
    };
    use nostr::key::Keys;

    let fixture = Fixture::new();
    let signer = Keys::generate();
    let account = signer.public_key();
    let route = "wss://cursor.example/";
    Checkpoint::attach_installed_v2(
        fixture.connection(),
        signer.clone(),
        123,
        route,
        Policy::default(),
    )
    .unwrap();
    let mut checkpoint = Checkpoint::attach_installed_custody(
        fixture.connection(),
        signer,
        123,
        route,
        Policy::default(),
        ReceivePolicy::Manual,
    )
    .unwrap();
    let source = checkpoint.bind_source(route).unwrap();
    let origin = source.origin;
    let state = |store: &CursorStore| {
        store
            .conn
            .query_row(
                "SELECT wall, (SELECT wall FROM peer_consent_version),
                revision, retry_revision, (SELECT count(*) FROM peer_cursor_deferred)
             FROM peer_cursor_origin WHERE origin=?1",
                [origin],
                |row| {
                    Ok([
                        row.get::<_, i64>(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ])
                },
            )
            .unwrap()
    };
    let fence = |tx: &Transaction<'_>, wall| {
        assert_eq!(
            tx.execute(
                "UPDATE peer_consent_version SET wall=?1 WHERE singleton=1 AND wall<=?1",
                [seconds(wall)?],
            )?,
            1
        );
        Ok(())
    };
    let expires = W + 10;
    let saved_metadata = Metadata {
        expires,
        ..metadata(Auth::FlightVerified, Some(id(6)))
    };
    for n in 0..u16::try_from(VISIBLE).unwrap() {
        checkpoint
            .cursor
            .defer_with_fence(origin, record(n, W), saved_metadata, W, |tx| fence(tx, W))
            .unwrap();
    }
    let baseline = state(&checkpoint.cursor);
    let floor = seconds(W).unwrap();
    assert_eq!(&baseline[..2], &[floor, floor]);
    assert_eq!(baseline[4], i64::try_from(VISIBLE).unwrap());

    for (other_account, other_route) in [
        (Keys::generate().public_key(), route),
        (account, "wss://changed.example/"),
    ] {
        assert_eq!(
            checkpoint.check_installed_consent_scope(&source, other_account, other_route, W + 1),
            Err(CheckpointError::Source)
        );
        assert_eq!(state(&checkpoint.cursor), baseline);
    }
    let advanced = seconds(W + 2).unwrap();
    for (table, column) in [("peer_cursor_origin", 0), ("peer_consent_version", 1)] {
        let sql = format!("UPDATE {table} SET wall=?1");
        checkpoint.cursor.conn.execute(&sql, [advanced]).unwrap();
        let mut changed = baseline;
        changed[column] = advanced;
        assert_eq!(
            checkpoint.check_installed_consent_scope(&source, account, route, W + 1),
            Err(CheckpointError::Cursor(Error::ClockRegressed))
        );
        assert_eq!(state(&checkpoint.cursor), changed);
        checkpoint.cursor.conn.execute(&sql, [floor]).unwrap();
    }
    checkpoint
        .cursor
        .conn
        .execute("UPDATE peer_consent_version SET account=?1", [ACCOUNT])
        .unwrap();
    assert!(
        checkpoint
            .check_installed_consent_scope(&source, account, route, W + 1)
            .is_err()
    );
    assert_eq!(state(&checkpoint.cursor), baseline);
    checkpoint
        .cursor
        .conn
        .execute(
            "UPDATE peer_consent_version SET account=?1",
            [account.to_bytes()],
        )
        .unwrap();
    assert_eq!(
        checkpoint.check_installed_consent_scope(&source, account, route, W + 1),
        Ok(())
    );

    let overflow = record(u16::try_from(VISIBLE).unwrap(), W);
    assert_eq!(
        checkpoint
            .cursor
            .defer_with_fence(origin, overflow, saved_metadata, W + 1, |tx| {
                assert_eq!(
                    tx.query_row(
                        "SELECT wall FROM peer_cursor_origin WHERE origin=?1",
                        [origin],
                        |row| row.get::<_, i64>(0)
                    )?,
                    seconds(W + 1)?
                );
                fence(tx, W + 1)
            })
            .err(),
        Some(Error::Backlog)
    );
    assert_eq!(state(&checkpoint.cursor), baseline);
    assert!(
        checkpoint
            .cursor
            .existing(origin, overflow.id)
            .unwrap()
            .is_none()
    );

    let saved = checkpoint.cursor.existing(origin, id(0)).unwrap().unwrap();
    let expired = || {
        saved
            .reverify(|raw| {
                assert_eq!(raw.id, id(0));
                Some(RetryFinding::Expired {
                    verified_expiry: expires,
                })
            })
            .unwrap()
    };
    assert_eq!(
        checkpoint
            .cursor
            .finish_retry_with_fence(expired(), expires - 1, |_| {
                panic!("a not-yet-expired row must refuse before the owner fence")
            }),
        Err(Error::Classification)
    );
    assert_eq!(state(&checkpoint.cursor), baseline);
    checkpoint
        .cursor
        .finish_retry_with_fence(expired(), expires, |tx| fence(tx, expires))
        .unwrap();
    let expired_wall = seconds(expires).unwrap();
    assert_eq!(
        state(&checkpoint.cursor),
        [
            expired_wall,
            expired_wall,
            baseline[2],
            baseline[3],
            baseline[4] - 1
        ]
    );
    assert!(checkpoint.cursor.existing(origin, id(0)).unwrap().is_none());
}

#[test]
fn peer_cursor_core_conflict_resolution_and_saturation_keep_original_obligations() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let cursor = begin(&mut store, A, W);
    let raw = record(0, W - 30);
    let unverified = metadata(Auth::Unverified, None);
    for n in 0..256 {
        store.defer(A, record(n, W - 30), unverified, W).unwrap();
    }
    assert!(!store.capacity(A).unwrap());
    let overflow = record(256, W - 40);
    owner_wall_table(&store);
    assert_eq!(
        store
            .defer_with_fence(A, overflow, unverified, W + 1, |tx| {
                advance_owner_wall(tx, W + 1)
            })
            .err(),
        Some(Error::Backlog)
    );
    assert_walls(&store, W);
    assert_eq!(
        store
            .commit_page(&cursor, &request(&cursor, &[overflow]), &[], W)
            .err(),
        Some(Error::Page)
    );
    assert_eq!(begin(&mut store, A, W), cursor);
    assert!(store.capacity(B).unwrap());
    let saved = store.existing(A, raw.id).unwrap().unwrap();
    let bytes = vec![0x73; EVENT_BYTES];
    let candidate = RawRecord {
        bytes: &bytes,
        ..raw
    };
    assert_eq!(
        store.defer(A, candidate, unverified, W).err(),
        Some(Error::Conflict)
    );
    assert_eq!(
        store
            .replace_conflict(&saved, candidate, unverified, W)
            .err(),
        Some(Error::Classification)
    );
    let proof = saved.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
    let outer = metadata(Auth::OuterVerified, None);
    let oversized = vec![0; EVENT_BYTES + 1];
    assert_eq!(
        store
            .replace_conflict(
                &saved,
                RawRecord {
                    bytes: &oversized,
                    ..raw
                },
                outer,
                W
            )
            .err(),
        Some(Error::Range)
    );
    let receipt = store.replace_conflict(&saved, candidate, outer, W).unwrap();
    let reopened = store.existing(A, raw.id).unwrap().unwrap();
    assert_eq!(reopened.retry_at, saved.retry_at);
    assert_eq!(reopened.metadata.auth, Auth::OuterVerified);
    assert_eq!(store.finish_retry(proof, W), Err(Error::Stale));
    let newer = RawRecord {
        bytes: b"a later independently verified rendition",
        ..raw
    };
    assert_eq!(
        store.replace_conflict(&saved, newer, outer, W).err(),
        Some(Error::Stale)
    );
    store.commit_invalid(A, raw, W).unwrap();
    assert!(store.existing(A, raw.id).unwrap().is_some());
    assert!(
        store
            .commit_page(&cursor, &request(&cursor, &[candidate]), &[receipt], W)
            .unwrap()
            .is_none()
    );
    drop(store);

    let mut store = fixture.open();
    assert!(!store.capacity(A).unwrap());
    store.commit_invalid(A, candidate, W).unwrap();
    assert!(store.capacity(A).unwrap());
    store.defer(A, overflow, unverified, W).unwrap();
    assert!(!store.capacity(A).unwrap());
}

#[test]
fn peer_cursor_core_bounded_retry_turns_progress_despite_reopen_and_rediscovery() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let mut current = begin(&mut store, A, W);
    let old = record(900, W - 200);
    let unverified = metadata(Auth::Unverified, None);
    store.defer(A, old, unverified, W).unwrap();
    for n in 0..8_u16 {
        drop(store);
        store = fixture.open();
        let wall = W + u64::from(n) * 60 + 1;
        let (other, next) = (record(500 + n, W), begin(&mut store, A, wall));
        assert_eq!(next, current);
        store.defer(A, old, unverified, wall).unwrap();
        if n.is_multiple_of(2) {
            store.defer(A, other, unverified, wall).unwrap();
        }
        let retry = store.due(A, wall).unwrap().unwrap();
        let mut turn = ServiceTurn::default();
        assert_eq!(turn.choose(true, true), Turn::Retry);
        if n.is_multiple_of(2) {
            assert_eq!(retry.record().id, old.id);
            store.retry_again(&retry, wall).unwrap();
        } else {
            assert_eq!(retry.record().id, id(500 + n - 1));
            let proof = retry.reverify(|_| Some(RetryFinding::Invalid)).unwrap();
            store.finish_retry(proof, wall).unwrap();
        }
        assert_eq!(turn.choose(true, true), Turn::Page);
        let rows: [_; 4] = std::array::from_fn(|offset| {
            let index = n * 4 + offset as u16;
            record(index, W - u64::from(index))
        });
        let receipts = invalid_receipts(&mut store, A, &rows, wall);
        current = store
            .commit_page(&current, &request(&current, &rows), &receipts, wall)
            .unwrap()
            .unwrap();
    }
    assert_eq!(current.visible, 32);
    let mut turn = ServiceTurn::default();
    assert_eq!(turn.choose(false, true), Turn::Retry);
    assert_eq!(turn.choose(false, false), Turn::Yield);
    let mut turn = ServiceTurn::default();
    for _ in 0..8 {
        assert_eq!(turn.choose(true, true), Turn::Retry);
        assert_eq!(turn.choose(true, true), Turn::Page);
    }
    assert_eq!(turn.choose(true, true), Turn::Yield);
}

#[test]
fn peer_cursor_core_keyed_storage_binding_and_hardening_are_verified() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let cursor = begin(&mut store, A, W);
    store
        .defer(A, record(1, W), metadata(Auth::Unverified, None), W)
        .unwrap();
    drop(store);
    let file = std::fs::read(fixture.0.path()).unwrap();
    assert!(!file.starts_with(b"SQLite format 3"));
    assert!(
        !file
            .windows(TEST_BYTES.len())
            .any(|window| window == TEST_BYTES)
    );
    assert_eq!(
        HardenedPeerConnection::open(fixture.0.path(), [0x98; 32]).err(),
        Some(Error::Storage)
    );
    assert_eq!(
        CursorStore::attach(fixture.connection(), [0; 32], &[A, B]).err(),
        Some(Error::Binding)
    );
    assert_eq!(
        CursorStore::attach(fixture.connection(), ACCOUNT, &[A]).err(),
        Some(Error::Binding)
    );
    assert_eq!(
        CursorStore::attach(fixture.connection(), ACCOUNT, &[A, A]).err(),
        Some(Error::Binding)
    );
    let compromised = fixture.connection();
    compromised
        .0
        .pragma_update(None, "journal_mode", "TRUNCATE")
        .unwrap();
    assert_eq!(
        CursorStore::attach(compromised, ACCOUNT, &[A, B]).err(),
        Some(Error::Storage)
    );
    assert_eq!(begin(&mut fixture.open(), A, W), cursor);
}

#[test]
#[ignore = "offline storage benchmark"]
fn peer_cursor_benchmark() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let mut page_times = Vec::new();
    let mut retry_times = Vec::new();
    let bytes = vec![0x47; EVENT_BYTES];
    for n in 0..110 {
        let wall = W + n * 4;
        let current = begin(&mut store, A, wall);
        let rows: [_; 4] = std::array::from_fn(|offset| RawRecord {
            bytes: &bytes,
            ..record(offset as u16, wall - offset as u64)
        });
        let receipts = invalid_receipts(&mut store, A, &rows, wall);
        let start = Instant::now();
        let next = store
            .commit_page(&current, &request(&current, &rows), &receipts, wall)
            .unwrap()
            .unwrap();
        let page_elapsed = start.elapsed();
        store
            .commit_page(&next, &request(&next, &[]), &[], wall)
            .unwrap();
        store
            .defer(A, rows[0], metadata(Auth::Unverified, None), wall)
            .unwrap();
        let start = Instant::now();
        let retry = store.due(A, wall + 1).unwrap().unwrap();
        store.retry_again(&retry, wall + 1).unwrap();
        let retry_elapsed = start.elapsed();
        store.commit_invalid(A, rows[0], wall + 1).unwrap();
        if n >= 10 {
            page_times.push(page_elapsed.as_secs_f64() * 1000.0);
            retry_times.push(retry_elapsed.as_secs_f64() * 1000.0);
        }
    }
    for (label, bytes, mut samples) in [
        ("full page cursor commit", 4 * EVENT_BYTES, page_times),
        ("due read and retry reschedule", EVENT_BYTES, retry_times),
    ] {
        samples.sort_unstable_by(f64::total_cmp);
        println!(
            "{label}: p50={:.3}ms p95={:.3}ms bytes={bytes} samples={}",
            samples[50],
            samples[94],
            samples.len()
        );
    }
}
