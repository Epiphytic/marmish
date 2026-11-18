use super::terminal::{TerminalReason, TerminalStep, advance_local_wall, insert_terminal_parent};
use super::terminal_leave::{Recovery, WINDOW, deadline, insert_terminal_and_hold};
use super::tests::{Fixture, UID, URL, WALL, joined_pending_in, mark_pending, seed_pending};
use super::*;

const STALE: Error = Error::Cursor(CursorError::Stale);
const BINDING: Error = Error::Cursor(CursorError::Binding);
const NOW: u64 = WALL + 3;

pub(super) fn leave_checkpoint(f: &Fixture) -> Result<Checkpoint> {
    Checkpoint::attach_terminal_leave_fixture(
        f.open(),
        f.client.clone(),
        UID,
        URL,
        Policy::default(),
    )
}

fn pending(f: &Fixture) -> (Checkpoint, Source, Selector) {
    joined_pending_in(f, &Keys::generate(), leave_checkpoint)
}

fn pending_pair(f: &Fixture) -> (Checkpoint, Source, Selector, Selector) {
    f.provisional();
    drop(f.prepared().unwrap());
    let mut cp = leave_checkpoint(f).unwrap();
    let source = cp.bind_source(URL).unwrap();
    let first = Keys::generate();
    let second = Keys::generate();
    cp.policy.reserved.insert(second.public_key());
    let first = seed_pending(&mut cp, &source, &f.client, &first, [0x83; 32]);
    let second = seed_pending(&mut cp, &source, &f.client, &second, [0x84; 32]);
    mark_pending(&mut cp, &source, [0x83; 32], [0x56; 32]);
    mark_pending(&mut cp, &source, [0x84; 32], [0x57; 32]);
    (cp, source, first, second)
}

fn row_counts(conn: &rusqlite::Connection) -> (u32, u32, u32) {
    conn.query_row(
        "SELECT (SELECT count(*) FROM peer_pending_terminal),
                (SELECT count(*) FROM peer_terminal_native_leave),
                (SELECT count(*) FROM peer_provisioning_slot WHERE server IS NOT NULL)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn epoch_five_requires_exact_prepared_or_existing_composed_schema() {
    let f = Fixture::new();
    f.provisional();
    let provisional = f.inventory();
    assert_eq!(leave_checkpoint(&f).err(), Some(BINDING));
    assert_eq!(f.inventory(), provisional);
    drop(f.prepared().unwrap());
    let prepared = f.inventory();
    for (keys, url) in [
        (Keys::generate(), URL),
        (f.client.clone(), "wss://other-origin.example/"),
    ] {
        assert_eq!(
            Checkpoint::attach_terminal_leave_fixture(f.open(), keys, UID, url, Policy::default())
                .err(),
            Some(BINDING)
        );
        assert_eq!(f.inventory(), prepared);
    }
    let HardenedPeerConnection(wrong) = f.open();
    wrong.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    assert_eq!(
        Checkpoint::attach_terminal_leave_fixture(
            HardenedPeerConnection(wrong),
            f.client.clone(),
            UID,
            URL,
            Policy::default()
        )
        .err(),
        Some(Error::Cursor(CursorError::Storage))
    );
    drop(leave_checkpoint(&f).unwrap());
    let installed = f.inventory();
    assert_ne!(installed, prepared);
    assert_eq!(f.prepared().err(), Some(BINDING));
    assert_eq!(f.custody().err(), Some(BINDING));
    assert_eq!(
        Checkpoint::attach_publication_fixture(
            f.open(),
            f.client.clone(),
            UID,
            URL,
            Policy::default()
        )
        .err(),
        Some(BINDING)
    );
    drop(leave_checkpoint(&f).unwrap());
    assert_eq!(f.inventory(), installed);

    for (keys, url) in [
        (Keys::generate(), URL),
        (f.client.clone(), "wss://other-origin.example/"),
    ] {
        assert_eq!(
            Checkpoint::attach_terminal_leave_fixture(f.open(), keys, UID, url, Policy::default())
                .err(),
            Some(BINDING)
        );
        assert_eq!(f.inventory(), installed);
    }

    let HardenedPeerConnection(tamper) = f.open();
    tamper
        .execute_batch("DROP TRIGGER peer_terminal_leave_deny_UPDATE")
        .unwrap();
    let partial = f.inventory();
    assert_eq!(leave_checkpoint(&f).err(), Some(BINDING));
    assert_eq!(f.inventory(), partial);

    let f = Fixture::new();
    f.provisional();
    drop(f.prepared().unwrap());
    drop(f.custody().unwrap());
    let old = f.inventory();
    assert_eq!(leave_checkpoint(&f).err(), Some(BINDING));
    assert_eq!(f.inventory(), old);
    drop(
        Checkpoint::attach_publication_fixture(
            f.open(),
            f.client.clone(),
            UID,
            URL,
            Policy::default(),
        )
        .unwrap(),
    );
    let old = f.inventory();
    assert_eq!(leave_checkpoint(&f).err(), Some(BINDING));
    assert_eq!(f.inventory(), old);
}

#[test]
fn epoch_five_hold_decision_is_atomic_and_both_entry_points_refuse_retroactive_hold() {
    let f = Fixture::new();
    let (mut cp, source, key) = pending(&f);
    let local = cp.authorize_local(UID).unwrap();
    let prior = cp.read_custody(&source, key, 1, NOW).unwrap().unwrap();

    assert_eq!(
        cp.hold_pending_decline(&source, key, 2, NOW, &local, || Ok(()))
            .err(),
        Some(STALE)
    );
    assert_eq!(
        cp.hold_pending_decline(&source, key, 1, NOW, &local, || Err(Error::LocalOwner))
            .err(),
        Some(Error::LocalOwner)
    );
    let tx = cp.cursor.begin(source.origin).unwrap();
    advance_local_wall(&tx, source.origin, NOW).unwrap();
    assert_eq!(
        insert_terminal_and_hold(&tx, &prior, NOW, || {
            insert_terminal_parent(&tx, &prior, TerminalReason::Declined, NOW, || Ok(()))?;
            assert_eq!(row_counts(&tx), (1, 0, 1));
            Err(Error::LocalOwner)
        })
        .err(),
        Some(Error::LocalOwner)
    );
    drop(tx);
    assert_eq!(row_counts(&cp.cursor.conn), (0, 0, 1));

    let record = cp
        .decline_pending_checked(&source, key, 1, NOW, &local, || Ok(()))
        .unwrap();
    assert_eq!(record.reason(), TerminalReason::Declined);
    assert_eq!(
        cp.hold_pending_decline(&source, key, 1, NOW, &local, || panic!(
            "retroactive authorization"
        ))
        .err(),
        Some(STALE)
    );
    let tx = cp.cursor.begin(source.origin).unwrap();
    assert_eq!(
        insert_terminal_and_hold(&tx, &prior, NOW, || panic!("retroactive parent insertion")).err(),
        Some(STALE)
    );
    assert_eq!(row_counts(&tx), (1, 0, 1));
    let last_eligible = i64::MAX as u64 - WINDOW;
    assert_eq!(deadline(last_eligible).unwrap(), i64::MAX as u64);
    assert_eq!(
        deadline(last_eligible + 1).err(),
        Some(Error::Cursor(CursorError::Range))
    );
}

#[test]
fn epoch_five_live_and_expired_hold_never_open_ordinary_retirement() {
    let f = Fixture::new();
    let (mut cp, source, key) = pending(&f);
    let local = cp.authorize_local(UID).unwrap();
    let record = cp
        .hold_pending_decline(&source, key, 1, NOW, &local, || Ok(()))
        .unwrap();
    assert_eq!(
        cp.hold_pending_decline(&source, key, 1, NOW + 1, &local, || Ok(()))
            .unwrap()
            .decided_at(),
        NOW
    );
    let end = NOW + WINDOW;
    for wall in [end - 1, end, end + 1] {
        let hold = cp.read_leave_hold(&source, key, 1, wall).unwrap().unwrap();
        assert_eq!((hold.deadline(), hold.recovery()), (end, Recovery::Idle));
        assert!(cp.read_leave_hold(&source, key, 2, wall).unwrap().is_none());
        assert!(cp.terminal_cleanup_due(&source, wall).unwrap().is_none());
        assert_eq!(
            cp.terminal_retirement_check(&source, &record, wall).err(),
            Some(STALE)
        );
        assert_eq!(
            cp.terminal_intent(&source, &record, wall, &[0; 48]).err(),
            Some(STALE)
        );
        assert_eq!(
            cp.terminal_blocked(&source, &record, wall).err(),
            Some(STALE)
        );
    }
    let tx = cp.cursor.begin(source.origin).unwrap();
    for sql in [
        "DELETE FROM peer_terminal_native_leave",
        "UPDATE peer_terminal_native_leave SET route=zeroblob(32)",
        "INSERT INTO peer_terminal_native_leave SELECT * FROM peer_terminal_native_leave",
    ] {
        assert_denied(tx.execute(sql, []), "terminal leave owner required");
    }
    terminal_schema::Writer::Progress.guard(&tx, false).unwrap();
    assert_denied(
        tx.execute("UPDATE peer_pending_terminal SET step=4", []),
        "terminal leave hold requires reconciled authority",
    );
    drop(tx);
    drop(cp);

    let mut cp = leave_checkpoint(&f).unwrap();
    let source = cp.bind_source(URL).unwrap();
    assert!(
        cp.read_leave_hold(&source, key, 1, end + 1)
            .unwrap()
            .is_some()
    );
    assert_eq!(row_counts(&cp.cursor.conn), (1, 1, 1));
}

#[test]
fn epoch_five_recovery_uses_exact_1s_2s_floors_and_durable_third_stop() {
    let f = Fixture::new();
    let (mut cp, source, key) = pending(&f);
    let local = cp.authorize_local(UID).unwrap();
    cp.hold_pending_decline(&source, key, 1, NOW, &local, || Ok(()))
        .unwrap();
    let mut source = source;
    let mut prior = cp.leave_recovery_due(&source, NOW).unwrap().unwrap();
    assert_eq!(
        (prior.recovery(), prior.not_before()),
        (Recovery::Idle, NOW)
    );
    for (attempts, at, floor) in [
        (1, NOW, NOW + 1),
        (2, NOW + 1, NOW + 3),
        (3, NOW + 3, NOW + 3),
    ] {
        let next = cp
            .reserve_or_park_leave_recovery(&source, &prior, at)
            .unwrap();
        assert_eq!(next.recovery(), Recovery::Reserved { attempts, at });
        assert_eq!(next.not_before(), floor);
        assert_eq!(
            cp.reserve_or_park_leave_recovery(&source, &prior, at).err(),
            Some(STALE)
        );
        if at < floor {
            assert!(cp.leave_recovery_due(&source, floor - 1).unwrap().is_none());
            assert_eq!(
                cp.reserve_or_park_leave_recovery(&source, &next, floor - 1)
                    .err(),
                Some(STALE)
            );
        }
        drop(cp);
        cp = leave_checkpoint(&f).unwrap();
        let rebound = cp.bind_source(URL).unwrap();
        assert_eq!(
            cp.reserve_or_park_leave_recovery(&rebound, &next, floor)
                .err(),
            Some(Error::LocalOwner)
        );
        assert_eq!(
            cp.read_leave_hold(&rebound, key, 1, at - 1).err(),
            Some(Error::Cursor(CursorError::ClockRegressed))
        );
        prior = cp.leave_recovery_due(&rebound, floor).unwrap().unwrap();
        assert_eq!(prior.recovery(), next.recovery());
        source = rebound;
    }
    let parked = cp
        .reserve_or_park_leave_recovery(&source, &prior, NOW + 3)
        .unwrap();
    assert_eq!(parked.recovery(), Recovery::Parked { at: NOW + 3 });
    assert!(
        cp.leave_recovery_due(&source, NOW + WINDOW)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cp.reserve_or_park_leave_recovery(&source, &parked, NOW + 4)
            .err(),
        Some(STALE)
    );

    let wrong = Selector {
        id: [0x98; 16],
        ..key
    };
    assert!(
        cp.read_leave_hold(&source, wrong, 1, NOW + 4)
            .unwrap()
            .is_none()
    );
    let tx = cp.cursor.begin(source.origin).unwrap();
    tx.execute(
        "UPDATE peer_cursor_origin SET wall=?1 WHERE origin=?2",
        params![seconds(NOW + 10).unwrap(), source.origin],
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(
        cp.read_leave_hold(&source, key, 1, NOW + 4).err(),
        Some(Error::Cursor(CursorError::ClockRegressed))
    );
}

#[test]
fn epoch_five_filtered_holds_do_not_starve_other_class() {
    let f = Fixture::new();
    let (mut cp, source, first, second) = pending_pair(&f);
    let local = cp.authorize_local(UID).unwrap();
    cp.hold_pending_decline(&source, first, 1, NOW, &local, || Ok(()))
        .unwrap();
    cp.hold_pending_decline(&source, second, 1, NOW, &local, || Ok(()))
        .unwrap();
    let due = cp.leave_recovery_due(&source, NOW).unwrap().unwrap();
    assert_eq!(due.selector(), first);
    cp.reserve_or_park_leave_recovery(&source, &due, NOW)
        .unwrap();
    assert_eq!(
        cp.leave_recovery_due(&source, NOW)
            .unwrap()
            .unwrap()
            .selector(),
        second
    );
    for at in [NOW + 1, NOW + 3, NOW + 3] {
        let hold = cp.read_leave_hold(&source, first, 1, at).unwrap().unwrap();
        cp.reserve_or_park_leave_recovery(&source, &hold, at)
            .unwrap();
    }
    assert_eq!(
        cp.leave_recovery_due(&source, NOW + 3)
            .unwrap()
            .unwrap()
            .selector(),
        second
    );

    let f = Fixture::new();
    let (mut cp, source, first, second) = pending_pair(&f);
    let local = cp.authorize_local(UID).unwrap();
    cp.hold_pending_decline(&source, first, 1, NOW, &local, || Ok(()))
        .unwrap();
    let late = NOW + WINDOW + 1;
    assert_eq!(
        cp.terminal_due(&source, second.server, late).unwrap(),
        Some((second, 1))
    );
    let record = cp.expire_pending(&source, second, 1, late).unwrap();
    assert_eq!(record.reason(), TerminalReason::Expired);
    assert_eq!(
        cp.hold_pending_expiry(&source, second, 1, late).err(),
        Some(STALE)
    );
    assert_eq!(
        cp.terminal_cleanup_due(&source, late)
            .unwrap()
            .unwrap()
            .selector(),
        second
    );
    let witness = [[0x42; 32].as_slice(), &[0x12; 8], &[0x34; 8]].concat();
    cp.terminal_retirement_check(&source, &record, late)
        .unwrap();
    let intent = cp
        .terminal_intent(&source, &record, late, &witness)
        .unwrap();
    let removed = cp.terminal_files_removed(&source, &intent, late).unwrap();
    let released = cp.terminal_release(&source, &removed, late).unwrap();
    assert_eq!(released.step(), &TerminalStep::Released);
    assert_eq!(row_counts(&cp.cursor.conn), (2, 1, 1));
}

#[test]
fn epoch_five_expiry_hold_rechecks_the_original_signed_route() {
    let f = Fixture::new();
    let (mut cp, source, key) = pending(&f);
    let at = WALL + 3_300;
    assert_eq!(
        cp.hold_pending_expiry(&source, key, 1, at - 1).err(),
        Some(Error::NotExpired)
    );
    let terminal = cp.hold_pending_expiry(&source, key, 1, at).unwrap();
    assert_eq!(terminal.reason(), TerminalReason::Expired);
    assert_eq!(
        cp.read_leave_hold(&source, key, 1, at)
            .unwrap()
            .unwrap()
            .deadline(),
        at + WINDOW
    );

    // Restore the exact inventory after seeding a storage-integrity failure.
    // SQL can check route shape, but only the authenticated Offer can bind its value.
    let tx = cp.cursor.begin(source.origin).unwrap();
    let sql: String = tx
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name='peer_terminal_leave_progress'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    leave_schema::Writer::Update.guard(&tx, false).unwrap();
    tx.execute_batch("DROP TRIGGER peer_terminal_leave_progress")
        .unwrap();
    tx.execute(
        "UPDATE peer_terminal_native_leave SET route=zeroblob(32)",
        [],
    )
    .unwrap();
    tx.execute_batch(&sql).unwrap();
    leave_schema::Writer::Update.guard(&tx, true).unwrap();
    tx.commit().unwrap();
    assert_eq!(cp.read_leave_hold(&source, key, 1, at).err(), Some(BINDING));
    assert_eq!(cp.leave_recovery_due(&source, at).err(), Some(BINDING));
    assert!(cp.terminal_cleanup_due(&source, at).unwrap().is_none());
    assert_eq!(row_counts(&cp.cursor.conn), (1, 1, 1));
}

fn assert_denied(result: rusqlite::Result<usize>, expected: &str) {
    assert!(
        matches!(
            result, Err(rusqlite::Error::SqliteFailure(_, Some(ref message))) if message == expected
        ),
        "{result:?}"
    );
}

#[test]
fn epoch_five_sql_recovery_trigger_checks_each_transition_independently() {
    let f = Fixture::new();
    let (mut cp, source, key) = pending(&f);
    let local = cp.authorize_local(UID).unwrap();
    cp.hold_pending_decline(&source, key, 1, NOW, &local, || Ok(()))
        .unwrap();

    let tx = cp.cursor.begin(source.origin).unwrap();
    leave_schema::Writer::Update.guard(&tx, false).unwrap();
    let update = |(state, attempts, at, floor): (u8, u8, Option<u64>, u64)| {
        tx.execute(
            "UPDATE peer_terminal_native_leave
             SET recovery_state=?1,recovery_attempts=?2,recovery_reserved_at=?3,recovery_not_before=?4",
            params![state, attempts, at.map(|at| seconds(at).unwrap()), seconds(floor).unwrap()],
        )
    };
    let deny = |cases: &[(u8, u8, Option<u64>, u64)]| {
        for case in cases {
            assert_denied(update(*case), "exact leave recovery transition required");
        }
    };

    deny(&[
        (1, 1, Some(NOW), NOW + 2),     // Wrong first delay.
        (1, 1, Some(NOW + 1), NOW + 2), // Timestamp differs from the qualified wall.
        (1, 2, Some(NOW), NOW + 2),     // Skip the first attempt.
        (2, 3, Some(NOW), NOW),         // Idle cannot park.
    ]);
    assert_eq!(update((1, 1, Some(NOW), NOW + 1)).unwrap(), 1);
    deny(&[
        (1, 2, Some(NOW), NOW + 2), // Reservation before the previous floor.
        (2, 1, Some(NOW), NOW + 1), // Cannot park with one attempt.
        (0, 0, None, NOW),          // Cannot reset.
    ]);
    advance_local_wall(&tx, source.origin, NOW + 1).unwrap();
    deny(&[(1, 2, Some(NOW + 1), NOW + 2)]); // Wrong second delay.
    assert_eq!(update((1, 2, Some(NOW + 1), NOW + 3)).unwrap(), 1);
    advance_local_wall(&tx, source.origin, NOW + 3).unwrap();
    deny(&[(1, 3, Some(NOW + 3), NOW + 4)]); // Third reservation must preserve the floor.
    assert_eq!(update((1, 3, Some(NOW + 3), NOW + 3)).unwrap(), 1);
    deny(&[(1, 4, Some(NOW + 3), NOW + 3)]); // No fourth attempt.

    // This artificial origin rollback isolates the trigger's park-time floor.
    // The owner itself rejects a regressed wall before it can reach this aperture.
    tx.execute(
        "UPDATE peer_cursor_origin SET wall=?1 WHERE origin=?2",
        params![seconds(NOW + 2).unwrap(), source.origin],
    )
    .unwrap();
    deny(&[(2, 3, Some(NOW + 3), NOW + 3)]);
    advance_local_wall(&tx, source.origin, NOW + 3).unwrap();
    assert_eq!(update((2, 3, Some(NOW + 3), NOW + 3)).unwrap(), 1);
    deny(&[(1, 3, Some(NOW + 3), NOW + 3)]); // A parked row cannot reopen or change.
    assert_denied(
        tx.execute(
            "UPDATE peer_terminal_native_leave SET route=zeroblob(32)",
            [],
        ),
        "exact leave recovery transition required",
    );
    leave_schema::Writer::Update.guard(&tx, true).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        cp.read_leave_hold(&source, key, 1, NOW + 3)
            .unwrap()
            .unwrap()
            .recovery(),
        Recovery::Parked { at: NOW + 3 },
    );
}

fn effect_checkpoint(f: &Fixture) -> Result<Checkpoint> {
    Checkpoint::attach_leave_effect_fixture(f.open(), f.client.clone(), UID, URL, Policy::default())
}

#[test]
fn peer_terminal_leave_effect_epoch_requires_exact_inventory() {
    let f = Fixture::new();
    f.provisional();
    assert_eq!(effect_checkpoint(&f).err(), Some(BINDING));
    drop(f.prepared().unwrap());
    drop(effect_checkpoint(&f).unwrap());
    let exact = f.inventory();
    assert_eq!(leave_checkpoint(&f).err(), Some(BINDING));
    drop(effect_checkpoint(&f).unwrap());
    assert_eq!(f.inventory(), exact);
    let HardenedPeerConnection(tampered) = f.open();
    assert!(
        tampered
            .execute("DELETE FROM peer_terminal_leave_effect_epoch", [])
            .is_err()
    );
    tampered
        .execute_batch("DROP TRIGGER peer_terminal_leave_effect_deny_INSERT")
        .unwrap();
    let partial = f.inventory();
    assert_eq!(effect_checkpoint(&f).err(), Some(BINDING));
    assert_eq!(f.inventory(), partial);

    let older = Fixture::new();
    older.provisional();
    drop(older.prepared().unwrap());
    drop(leave_checkpoint(&older).unwrap());
    let inventory = older.inventory();
    assert_eq!(effect_checkpoint(&older).err(), Some(BINDING));
    assert_eq!(older.inventory(), inventory);
}

#[test]
fn peer_terminal_leave_effect_apertures_restore_after_uncommitted_writes() {
    use super::leave_effect::Effect;
    use super::leave_effect_schema::set_write;

    let f = Fixture::new();
    let (mut cp, source, key) = joined_pending_in(&f, &Keys::generate(), effect_checkpoint);
    let local = cp.authorize_local(UID).unwrap();
    cp.hold_pending_decline(&source, key, 1, NOW, &local, || Ok(()))
        .unwrap();
    let hold = cp.read_leave_hold(&source, key, 1, NOW).unwrap().unwrap();
    assert_eq!(
        cp.native_leave_effect(&source, &hold, NOW),
        Ok(Effect::Absent)
    );
    let insert = |conn: &rusqlite::Connection, wall| {
        conn.execute(
            "INSERT INTO peer_terminal_leave_effect (class,generation,armed_at,image_digest)
         VALUES ((SELECT class FROM peer_terminal_native_leave),1,?1,zeroblob(32))",
            [i64::try_from(wall).unwrap()],
        )
    };
    let capture = |conn: &rusqlite::Connection, wall| {
        conn.execute(
            "UPDATE peer_terminal_leave_effect SET captured_at=?1, event_id=zeroblob(32),
         event_digest=zeroblob(32), signed_event=x'45'",
            [i64::try_from(wall).unwrap()],
        )
    };
    assert!(insert(&cp.cursor.conn, NOW).is_err());
    {
        let tx = cp.cursor.begin(source.origin).unwrap();
        set_write(&tx, false, true).unwrap();
        assert!(insert(&tx, NOW + 1).is_err(), "cursor floor must match");
        assert_eq!(insert(&tx, NOW).unwrap(), 1);
    }
    assert_eq!(
        cp.native_leave_effect(&source, &hold, NOW),
        Ok(Effect::Absent)
    );
    assert!(insert(&cp.cursor.conn, NOW).is_err());

    {
        let tx = cp.cursor.begin(source.origin).unwrap();
        set_write(&tx, false, true).unwrap();
        assert_eq!(insert(&tx, NOW).unwrap(), 1);
        set_write(&tx, false, false).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(
        cp.native_leave_effect(&source, &hold, NOW),
        Ok(Effect::Armed)
    );
    assert_eq!(
        cp.native_leave_effect(&source, &hold, NOW - 1),
        Err(Error::Cursor(CursorError::ClockRegressed))
    );
    assert!(capture(&cp.cursor.conn, NOW).is_err());
    assert!(
        cp.cursor
            .conn
            .execute("DELETE FROM peer_terminal_leave_effect", [])
            .is_err()
    );
    {
        let tx = cp.cursor.begin(source.origin).unwrap();
        set_write(&tx, true, true).unwrap();
        assert!(capture(&tx, hold.deadline()).is_err());
        assert_eq!(capture(&tx, NOW).unwrap(), 1);
    }
    assert_eq!(
        cp.native_leave_effect(&source, &hold, NOW),
        Ok(Effect::Armed)
    );
    assert!(capture(&cp.cursor.conn, NOW).is_err());
    assert!(
        cp.terminal_cleanup_due(&source, hold.deadline())
            .unwrap()
            .is_none()
    );
    assert_eq!(row_counts(&cp.cursor.conn), (1, 1, 1));
}
