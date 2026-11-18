use super::*;

fn admit(cp: &mut Checkpoint, f: &Fixture, source: &Source, server: &Keys, url: &str) -> Selector {
    let offer = f.offer(server, url, f.wall);
    assert_eq!(
        cp.admit(source, raw(offer.frame()), f.wall).unwrap().status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    Selector::from(&offer.flight)
}

#[test]
fn peer_provisioning_classes_and_clock_survive_reopen() {
    let f = Fixture::new();
    let pinned = Keys::generate();
    let candidate = Keys::generate();
    let mut cp = f.start(Policy {
        reserved: [pinned.public_key()].into(),
        ..Policy::default()
    });
    let a = cp.bind_source(A).unwrap();
    let b = cp.bind_source(B).unwrap();
    let unknown = admit(&mut cp, &f, &a, &f.server, A);
    let known = admit(&mut cp, &f, &b, &pinned, B);
    let candidate = admit(&mut cp, &f, &b, &candidate, B);
    assert_eq!(
        cp.recover_provisioning(&a, unknown, f.wall).unwrap_err(),
        Error::Missing
    );
    assert_eq!(
        cp.reserve_provisioning(&b, unknown, f.wall).unwrap_err(),
        Error::Missing
    );
    let first = cp.reserve_provisioning(&a, unknown, f.wall).unwrap();
    assert_eq!(first.binding.key, unknown);
    assert_eq!(first.binding.origin, a.origin);
    assert!(!first.binding.generation.known);
    assert_eq!(first.binding.generation.count, 1);
    assert!(Arc::ptr_eq(&first.owner, &cp.owner));
    assert_eq!(
        cp.reserve_provisioning(&b, candidate, f.wall).unwrap_err(),
        Error::Cursor(CursorError::Backlog)
    );
    let trusted = cp.reserve_provisioning(&b, known, f.wall).unwrap();
    assert!(trusted.binding.generation.known);
    assert_eq!(trusted.binding.generation.count, 1);
    drop(cp);

    let mut cp = f.start(Policy {
        reserved: [unknown.server].into(),
        ..Policy::default()
    });
    let b = cp.bind_source(B).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(
        cp.recover_provisioning(&a, unknown, f.wall + 5)
            .unwrap_err(),
        Error::Source
    );
    let a = cp.bind_source(A).unwrap();
    let recovered = cp.reserve_provisioning(&a, unknown, f.wall + 5).unwrap();
    assert_eq!(recovered.binding, first.binding);
    assert!(!Arc::ptr_eq(&recovered.owner, &first.owner));
    assert_eq!(
        cp.recover_provisioning(&b, known, f.wall + 4).unwrap_err(),
        Error::Cursor(CursorError::ClockRegressed)
    );
    let abandon = cp.retirement_target(&local, unknown).unwrap();
    cp.retire_provisioning(&local, abandon, f.wall + 5).unwrap();
    assert_eq!(
        cp.reserve_provisioning(&b, candidate, f.wall + 599)
            .unwrap_err(),
        Error::Cursor(CursorError::Backlog)
    );
    let next = cp
        .reserve_provisioning(&b, candidate, f.wall + 600)
        .unwrap();
    assert!(!next.binding.generation.known);
    assert_eq!(next.binding.generation.count, 2);
}

#[test]
fn peer_provisioning_policy_quarantine_and_expiry() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let short_offer = f.offer(&f.server, A, f.wall - 86_400 + 10);
    let other_source = cp.bind_source(B).unwrap();
    let other_server = Keys::generate();
    let other = f.offer(&other_server, B, f.wall - 86_400 + 10);
    assert_eq!(
        f.submit(&mut cp, &other_source, &other),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let key = Selector::from(&short_offer.flight);
    let expires = short_offer.flight.flight().expires_at;
    assert_eq!(
        f.submit(&mut cp, &a, &short_offer),
        Status::Accepted(TrustedOutcome::Applied)
    );
    cp.cursor
        .conn
        .execute(
            "UPDATE peer_cursor_origin SET quarantine_until=?2 WHERE origin=?1",
            params![a.origin, i64::try_from(f.wall + 100).unwrap()],
        )
        .unwrap();
    assert_eq!(
        cp.reserve_provisioning(&a, key, f.wall).unwrap_err(),
        Error::Cursor(CursorError::RelayContract)
    );
    cp.cursor
        .conn
        .execute(
            "UPDATE peer_cursor_origin SET quarantine_until=NULL WHERE origin=?1",
            params![a.origin],
        )
        .unwrap();
    cp.reserve_provisioning(&a, key, f.wall).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    let old_target = cp.retirement_target(&local, key).unwrap();
    drop(cp);

    for policy in [
        Policy {
            mode: Mode::Off,
            ..Policy::default()
        },
        Policy {
            blocked: [key.server].into(),
            ..Policy::default()
        },
    ] {
        let mut cp = f.start(policy);
        let a = cp.bind_source(A).unwrap();
        assert_eq!(
            cp.reserve_provisioning(&a, key, f.wall).unwrap_err(),
            Error::Policy
        );
        assert_eq!(
            cp.recover_provisioning(&a, key, f.wall).unwrap_err(),
            Error::Policy
        );
    }

    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(
        cp.retire_provisioning(&local, old_target, f.wall),
        Err(Error::LocalOwner)
    );
    let a = cp.bind_source(A).unwrap();
    assert_eq!(
        cp.recover_provisioning(&a, key, expires + 301).unwrap_err(),
        Error::Expired
    );
    assert_eq!(
        cp.recover_provisioning(&a, key, expires - 1).unwrap_err(),
        Error::Cursor(CursorError::ClockRegressed)
    );
    let b = cp.bind_source(B).unwrap();
    cp.sweep(&b, expires + 301).unwrap();
    assert_eq!(counts(&cp), (1, 2, 0, 0));
    assert_eq!(
        cp.discard(&local, key, expires + 301),
        Err(Error::Cursor(CursorError::Conflict))
    );
    drop(cp);

    let mut cp = f.start(Policy {
        mode: Mode::Off,
        blocked: [key.server].into(),
        ..Policy::default()
    });
    let local = cp.authorize_local(UID).unwrap();
    cp.fixture_statement("UPDATE peer_cursor_origin SET quarantine_until=1");
    let current = cp.retirement_target(&local, key).unwrap();
    cp.retire_provisioning(&local, current, expires + 301)
        .unwrap();
    assert_eq!(counts(&cp), (0, 2, 0, 0));
}

#[test]
fn peer_provisioning_retirement_preserves_signed_replay_fence() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    let a = cp.bind_source(A).unwrap();
    let signed = f.offer(&f.server, A, f.wall);
    assert_eq!(
        f.submit(&mut cp, &a, &signed),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let key = Selector::from(&signed.flight);
    cp.reserve_provisioning(&a, key, f.wall).unwrap();
    let old_target = cp.retirement_target(&local, key).unwrap();
    let retire = cp.retirement_target(&local, key).unwrap();
    cp.retire_provisioning(&local, retire, f.wall).unwrap();
    assert_eq!(
        f.submit(&mut cp, &a, &signed),
        Status::Accepted(TrustedOutcome::AlreadyApplied)
    );
    assert_eq!(
        cp.reserve_provisioning(&a, key, f.wall).unwrap_err(),
        Error::Missing
    );

    let revision = f.offer(&f.server, A, f.wall + 600);
    let next = Selector::from(&revision.flight);
    assert_eq!(key.server, next.server);
    assert_ne!(key.id, next.id);
    assert_ne!(key.revision, next.revision);
    let disposition = cp.admit(&a, raw(revision.frame()), f.wall + 600).unwrap();
    assert_eq!(
        disposition.status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert_eq!(
        cp.reserve_provisioning(&a, Selector { id: key.id, ..next }, f.wall + 600)
            .unwrap_err(),
        Error::Missing,
    );
    let second = cp.reserve_provisioning(&a, next, f.wall + 600).unwrap();
    assert_eq!(second.binding.generation.count, 2);
    assert_eq!(
        cp.retire_provisioning(&local, old_target, f.wall + 600),
        Err(Error::Missing)
    );
}

const RAW_OUTBOX: &str = "
    INSERT INTO peer_consent_outbox
        (server, offer_id, offer_revision, origin, carrier, revision, expires, frame,
         package_id, package_digest)
    SELECT server, offer_id, revision, origin, carrier, revision, expires, X'01',
        zeroblob(32), zeroblob(32) FROM peer_consent_invite";

#[tokio::test]
async fn peer_provisioning_mutual_exclusion_with_legacy_approval_and_outbox() {
    for mode in [Mode::Manual, Mode::Automatic] {
        let f = Fixture::new();
        let mut cp = f.start(Policy {
            mode,
            ..Policy::default()
        });
        let a = cp.bind_source(A).unwrap();
        let local = cp.authorize_local(UID).unwrap();
        let key = admit(&mut cp, &f, &a, &f.server, A);
        if matches!(mode, Mode::Manual) {
            cp.approve(&local, key, f.wall).unwrap();
        }
        assert_eq!(approval_time(&cp, key), Some(f.wall));
        cp.reserve_provisioning(&a, key, f.wall).unwrap();
        assert_eq!(approval_time(&cp, key), None);
        assert!(cp.summaries(&local, &a, f.wall).unwrap().is_empty());
        assert_eq!(
            cp.approve(&local, key, f.wall),
            Err(Error::Cursor(CursorError::Conflict))
        );
        assert_eq!(
            cp.approved_work(key, f.wall, None).err(),
            Some(Error::Cursor(CursorError::Conflict))
        );
        let direct_insert = cp.cursor.conn.execute_batch(RAW_OUTBOX).unwrap_err();
        assert!(
            direct_insert
                .to_string()
                .contains("provisioning outbox conflict")
        );
        assert_eq!(counts(&cp).2, 0);
    }

    let f = Fixture::new();
    let mut cp = f.start(Policy {
        mode: Mode::Automatic,
        ..Policy::default()
    });
    let a = cp.bind_source(A).unwrap();
    let key = admit(&mut cp, &f, &a, &f.server, A);
    let (package, now) = native_package(&f.client, A).await;
    let now = now.max(f.wall);
    let expires = now + 2_400;
    let outbox = queued(
        cp.approved_work(
            key,
            now,
            Some(Material {
                origin: a.origin,
                selector: key,
                package: &package,
                expires,
            }),
        )
        .unwrap(),
    );
    for wall in [now, expires] {
        assert_eq!(
            cp.reserve_provisioning(&a, key, wall).unwrap_err(),
            Error::Cursor(CursorError::Conflict)
        );
    }
    let direct = cp
        .cursor
        .conn
        .execute_batch(
            "
        UPDATE peer_provisioning_slot SET generation=1, next_start_at=600,
            (server, offer_id, revision, origin) =
                (SELECT server, offer_id, revision, origin FROM peer_consent_invite)
            WHERE class=0",
        )
        .unwrap_err();
    assert!(direct.to_string().contains("provisioning claim conflict"));
    // Recreate the earlier checkpoint's schema, leaving the encrypted consent in place.
    cp.fixture_statement(
        "
        DROP TRIGGER peer_provisioning_reject_outbox;
        DROP TRIGGER peer_provisioning_reject_claim;
        DROP TABLE peer_provisioning_slot;",
    );
    drop(cp);
    let mut cp = f.start(Policy::default());
    let recovered = queued(cp.approved_work(key, now, None).unwrap());
    assert_eq!(*recovered.frame, *outbox.frame);
    let a = cp.bind_source(A).unwrap();
    assert_eq!(
        cp.reserve_provisioning(&a, key, now).unwrap_err(),
        Error::Cursor(CursorError::Conflict)
    );
}

#[test]
fn peer_provisioning_sql_failure_is_atomic_and_schema_cannot_reseed() {
    let f = Fixture::new();
    let mut cp = f.start(Policy {
        mode: Mode::Automatic,
        ..Policy::default()
    });
    let a = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    let key = admit(&mut cp, &f, &a, &f.server, A);
    cp.fixture_statement(
        "
        CREATE TRIGGER fail_approval_clear BEFORE UPDATE OF decision_at ON peer_consent_invite
        BEGIN SELECT RAISE(FAIL, 'fixture clear refusal'); END",
    );
    assert_eq!(
        cp.reserve_provisioning(&a, key, f.wall + 20).unwrap_err(),
        Error::Cursor(CursorError::Storage)
    );
    assert_eq!(approval_time(&cp, key), Some(f.wall));
    cp.fixture_statement("DROP TRIGGER fail_approval_clear");
    let first = cp.reserve_provisioning(&a, key, f.wall).unwrap();
    assert_eq!(first.binding.generation.count, 1);
    cp.fixture_statement(
        "
        CREATE TRIGGER fail_invite_delete BEFORE DELETE ON peer_consent_invite
        BEGIN SELECT RAISE(FAIL, 'fixture retire refusal'); END",
    );
    let target = cp.retirement_target(&local, key).unwrap();
    assert_eq!(
        cp.retire_provisioning(&local, target, f.wall + 20),
        Err(Error::Cursor(CursorError::Storage))
    );
    assert_eq!(
        cp.recover_provisioning(&a, key, f.wall).unwrap().binding,
        first.binding
    );
    cp.fixture_statement("DROP TRIGGER fail_invite_delete");

    let stale = cp.retirement_target(&local, key).unwrap();
    cp.fixture_statement("UPDATE peer_provisioning_slot SET generation=2 WHERE class=0");
    assert_eq!(
        cp.retire_provisioning(&local, stale, f.wall),
        Err(Error::Cursor(CursorError::Stale))
    );
    assert!(
        cp.cursor
            .conn
            .execute(
                "UPDATE peer_provisioning_slot SET origin=NULL WHERE class=0",
                [],
            )
            .is_err()
    );
    assert_eq!(
        cp.recover_provisioning(&a, key, f.wall)
            .unwrap()
            .binding
            .generation
            .count,
        2
    );
    assert_eq!(
        cp.recover_provisioning(&a, key, u64::MAX).unwrap_err(),
        Error::Cursor(CursorError::Range)
    );
    cp.fixture_statement("DELETE FROM peer_provisioning_slot WHERE class=1");
    drop(cp);
    let cursor = f.cursor(DB_KEY, f.client.public_key()).unwrap();
    assert_eq!(
        Checkpoint::attach(cursor, f.client.clone(), UID, &[A, B], Policy::default()).err(),
        Some(Error::Cursor(CursorError::Binding))
    );
}

#[test]
#[ignore = "local encrypted provisioning reservation latency"]
fn peer_provisioning_latency() {
    let mut reserve = Vec::new();
    let mut recover = Vec::new();
    for _ in 0..20 {
        let f = Fixture::new();
        let mut cp = f.start(Policy::default());
        let a = cp.bind_source(A).unwrap();
        let key = admit(&mut cp, &f, &a, &f.server, A);
        let start = Instant::now();
        let first = cp.reserve_provisioning(&a, key, f.wall).unwrap();
        reserve.push(start.elapsed());
        drop(cp);
        let mut cp = f.start(Policy::default());
        let a = cp.bind_source(A).unwrap();
        let start = Instant::now();
        let resumed = cp.recover_provisioning(&a, key, f.wall + 1).unwrap();
        recover.push(start.elapsed());
        assert_eq!(first.binding, resumed.binding);
    }
    for (operation, samples) in [("reserve", &mut reserve), ("recover", &mut recover)] {
        samples.sort_unstable();
        eprintln!(
            "peer_provisioning {operation} n={} p50_us={} p95_us={}",
            samples.len(),
            samples[9].as_micros(),
            samples[18].as_micros(),
        );
    }
}
