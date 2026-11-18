use cgka_traits::app_components::NostrRoutingV1;

use super::super::provisioning::prepared_consent::custody::ReceivePolicy;
use super::*;
use wire::Transcript;

fn open(f: &Fixture, signer: &Keys, url: &str, policy: Policy) -> Result<Checkpoint> {
    let conn = HardenedPeerConnection::open(f.file.path(), DB_KEY)?;
    Checkpoint::attach_installed_v2(conn, signer.clone(), UID, url, policy)
}

fn codec() -> Codec {
    Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2).unwrap()
}

fn offer(f: &Fixture, founder: &Keys, url: &str, wall: u64) -> wire::Prepared<Offer> {
    let routing = NostrRoutingV1::new([0xc4; 32], vec![url.to_owned()]).unwrap();
    codec()
        .prepare_offer(founder, f.client.public_key(), [0x2a; 16], &routing, wall)
        .unwrap()
}

fn marks(cp: &Checkpoint) -> u16 {
    cp.cursor
        .conn
        .query_row("SELECT count(*) FROM peer_installed_offer_v2", [], |r| {
            r.get(0)
        })
        .unwrap()
}

#[tokio::test]
async fn peer_installed_offer_automatic_restarts_with_only_exact_v2_ancestry() {
    let f = Fixture::new();
    let policy = || Policy {
        mode: Mode::Automatic,
        ..Default::default()
    };
    let (package, wall) = native_package(&f.client, A).await;
    let incoming = offer(&f, &f.server, A, wall);
    let key = Selector::from(&incoming.flight);
    let v1 = f.offer(&f.server, A, wall);
    let stale_source = {
        let mut cp = open(&f, &f.client, A, policy()).unwrap();
        let source = cp.bind_source(A).unwrap();
        assert_eq!(cp.bind_source(B).err(), Some(Error::Source));
        assert_eq!(
            cp.admit(&source, raw(v1.frame()), wall).unwrap().status,
            Status::Invalid
        );
        assert_eq!(marks(&cp), 0);
        assert_eq!(
            cp.admit(&source, raw(incoming.frame()), wall)
                .unwrap()
                .status,
            Status::Accepted(TrustedOutcome::Applied)
        );
        assert_eq!(approval_time(&cp, key), None);
        assert_eq!(marks(&cp), 1);
        source
    };
    assert!(open(&f, &f.client, B, policy()).is_err());
    assert!(open(&f, &Keys::generate(), A, policy()).is_err());
    let mut cp = open(&f, &f.client, A, policy()).unwrap();
    let source = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(
        cp.installed_live_offer(&stale_source, key, wall).err(),
        Some(Error::Source)
    );
    let wrong_founder = Selector {
        server: Keys::generate().public_key(),
        ..key
    };
    assert_eq!(
        cp.installed_live_offer(&source, wrong_founder, wall).err(),
        Some(Error::Missing)
    );
    assert_eq!(cp.approve(&local, key, wall).err(), Some(Error::Layout));
    assert_eq!(cp.discard(&local, key, wall).err(), Some(Error::Layout));
    assert_eq!(cp.approved_work(key, wall, None).err(), Some(Error::Layout));
    assert_eq!(
        cp.reserve_attempt(&source, key, [0; 32], b"unused", wall)
            .err(),
        Some(Error::Layout)
    );
    assert_eq!(
        cp.summaries(&local, &source, wall).err(),
        Some(Error::Layout)
    );

    let reopened = cp.installed_live_offer(&source, key, wall).unwrap();
    let prepared = codec()
        .prepare_capped_fixture_consent(&f.client, &reopened, &package, wall + 300, wall)
        .unwrap();
    let transcript = Transcript::from_consent(&prepared.flight);
    transcript
        .verify(&f.client, &reopened, prepared.frame(), &package, wall)
        .unwrap();
    assert!(
        transcript
            .verify(&f.client, &v1.flight, prepared.frame(), &package, wall)
            .is_err()
    );

    let repeated = rewrap(incoming.frame(), &f.client);
    assert_eq!(
        cp.admit(&source, raw(&repeated), wall).unwrap().status,
        Status::Accepted(TrustedOutcome::AlreadyApplied)
    );
    let expires = reopened.flight().expires_at;
    let horizon = reopened.flight().replay_safe_until;
    assert_eq!(
        cp.installed_live_offer(&source, key, expires).err(),
        Some(Error::Expired)
    );
    let past = cp
        .installed_historical_offer(&source, key, expires)
        .unwrap();
    assert_eq!(past.selector, key);
    assert_eq!(past.expires, expires);
    assert_eq!(
        cp.installed_historical_offer(&source, key, horizon).err(),
        Some(Error::Expired)
    );
    assert_eq!(counts(&cp), (1, 1, 0, 0));
}

#[test]
fn peer_installed_offer_bad_remote_routes_do_not_block_the_cursor_page() {
    let f = Fixture::new();
    let blocked = Keys::generate();
    let policy = Policy {
        blocked: BTreeSet::from([blocked.public_key()]),
        ..Default::default()
    };
    let mut cp = open(&f, &f.client, A, policy).unwrap();
    let source = cp.bind_source(A).unwrap();
    let wrong_origin = offer(&f, &f.server, B, f.wall);
    let blocked_wrong_origin = offer(&f, &blocked, B, f.wall);
    let unmodified = offer(&f, &f.server, A, f.wall);
    let malformed =
        wire::replace_offer_relays(&codec(), unmodified.frame(), &f.server, &f.client, &[A, A]);
    let admitted = offer(&f, &f.server, A, f.wall - 1);
    assert_eq!(
        codec()
            .open_offer(&f.client, &malformed, None, &[A.to_owned()], f.wall)
            .err(),
        Some(wire::Error::Route)
    );
    let admitted_record = raw(admitted.frame());
    let mut records = [
        raw(wrong_origin.frame()),
        raw(blocked_wrong_origin.frame()),
        raw(&malformed),
        admitted_record,
    ];
    records.sort_by_key(|r| std::cmp::Reverse(r.stamp));
    let scan = cp
        .cursor
        .scan(source.origin, ScanStart::Recovery, f.wall)
        .unwrap()
        .unwrap();
    let page = Page {
        origin: source.origin,
        since: scan.lower,
        until: scan.until,
        records: &records,
    };
    let receipts: Vec<_> = records
        .iter()
        .map(|record| {
            let result = cp.admit(&source, *record, f.wall).unwrap();
            if record.id == admitted_record.id {
                assert_eq!(result.status, Status::Accepted(TrustedOutcome::Applied));
            } else {
                assert_eq!(result.status, Status::Invalid);
                assert!(result.receipt.kind == ReceiptKind::Invalid);
            }
            result.receipt
        })
        .collect();
    let next = cp
        .cursor
        .commit_page(&scan, &page, &receipts, f.wall)
        .unwrap()
        .unwrap();
    assert_eq!(next.until, records.last().unwrap().stamp);
    let exhausted = Page {
        origin: source.origin,
        since: next.lower,
        until: next.until,
        records: &[],
    };
    assert!(
        cp.cursor
            .commit_page(&next, &exhausted, &[], f.wall)
            .unwrap()
            .is_none()
    );
    assert_eq!(marks(&cp), 1);
}

#[test]
fn peer_installed_offer_replay_bound_claimed_slot_and_capacity() {
    let f = Fixture::new();
    let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
    let source = cp.bind_source(A).unwrap();
    let first = offer(&f, &f.server, A, f.wall);
    assert_eq!(
        f.submit(&mut cp, &source, &first),
        Status::Accepted(TrustedOutcome::Applied)
    );
    cp.reserve_provisioning(&source, Selector::from(&first.flight), f.wall)
        .unwrap();
    let competing = offer(&f, &f.server, A, f.wall + 1);
    assert_ne!(
        Selector::from(&first.flight),
        Selector::from(&competing.flight)
    );
    assert_eq!(
        cp.admit(&source, raw(competing.frame()), f.wall + 1)
            .unwrap()
            .status,
        Status::Deferred
    );
    assert_eq!(marks(&cp), 1);
    for _ in 1..8 {
        let next = offer(&f, &Keys::generate(), A, f.wall);
        assert_eq!(
            cp.admit(&source, raw(next.frame()), f.wall + 1)
                .unwrap()
                .status,
            Status::Accepted(TrustedOutcome::Applied)
        );
    }
    assert_eq!(marks(&cp), 8);

    let expires = first.flight.flight().expires_at;
    let horizon = first.flight.flight().replay_safe_until;
    cp.sweep(&source, expires).unwrap();
    let extra = offer(&f, &Keys::generate(), A, expires);
    assert_eq!(
        cp.admit(&source, raw(extra.frame()), expires)
            .unwrap()
            .status,
        Status::Deferred
    );
    assert_eq!(counts(&cp), (8, 8, 0, 2));
    cp.sweep(&source, horizon).unwrap();
    assert_eq!(marks(&cp), 1);
    assert_eq!(
        cp.admit(&source, raw(extra.frame()), horizon)
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert_eq!(counts(&cp), (2, 1, 0, 1));
}

#[test]
fn peer_installed_offer_transaction_rolls_back_parent_marker_and_guard() {
    let f = Fixture::new();
    let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
    let source = cp.bind_source(A).unwrap();
    let incoming = offer(&f, &f.server, A, f.wall);
    let record = raw(incoming.frame());
    let result = cp
        .cursor
        .commit_trusted_exact(source.origin, record, f.wall, |tx| {
            assert_eq!(
                rows::admit(tx, source.origin, record, &incoming.flight, false, None)?,
                TrustedOutcome::Applied
            );
            installed::mark(tx, source.origin, &incoming.flight)?;
            let count: u16 =
                tx.query_row("SELECT count(*) FROM peer_installed_offer_v2", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(count, 1);
            Err(CursorError::Stale)
        });
    assert_eq!(result.err(), Some(CursorError::Stale));
    assert_eq!(counts(&cp), (0, 0, 0, 0));
    assert_eq!(marks(&cp), 0);
    assert_eq!(
        cp.admit(&source, raw(incoming.frame()), f.wall)
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    for sql in [
        "INSERT INTO peer_installed_offer_v2 SELECT * FROM peer_installed_offer_v2",
        "UPDATE peer_installed_offer_v2 SET version=2",
        "DELETE FROM peer_installed_offer_v2",
    ] {
        assert!(cp.cursor.conn.execute(sql, []).is_err());
    }
    let horizon = incoming.flight.flight().replay_safe_until;
    {
        let tx = cp.cursor.with_wall(source.origin, horizon).unwrap();
        installed::sweep(&tx, horizon).unwrap();
        assert_eq!(
            tx.query_row("SELECT count(*) FROM peer_installed_offer_v2", [], |r| {
                r.get::<_, u16>(0)
            })
            .unwrap(),
            0
        );
    }
    assert_eq!(counts(&cp), (1, 1, 0, 0));
    assert!(
        cp.cursor
            .conn
            .execute("DELETE FROM peer_installed_offer_v2", [])
            .is_err()
    );
    drop(cp);
    let reopened = open(&f, &f.client, A, Policy::default()).unwrap();
    assert_eq!(marks(&reopened), 1);
}

#[test]
fn peer_installed_offer_mutually_exclusive_layouts_and_corrupt_marker() {
    let f = Fixture::new();
    let origin = Route::configured(A).unwrap().id;
    {
        let conn = HardenedPeerConnection::open(f.file.path(), DB_KEY).unwrap();
        let cursor =
            CursorStore::attach(conn, f.client.public_key().to_bytes(), &[origin]).unwrap();
        let mut legacy =
            Checkpoint::attach(cursor, f.client.clone(), UID, &[A], Policy::default()).unwrap();
        let source = legacy.bind_source(A).unwrap();
        let v2 = offer(&f, &f.server, A, f.wall);
        assert_eq!(f.submit(&mut legacy, &source, &v2), Status::Invalid);
        let frame = f.offer(&f.server, A, f.wall);
        f.submit(&mut legacy, &source, &frame);
        legacy
            .sweep(&source, frame.flight.flight().expires_at)
            .unwrap();
        assert_eq!(counts(&legacy), (0, 1, 0, 0));
    }
    assert_eq!(
        open(&f, &f.client, A, Policy::default()).err(),
        Some(Error::Cursor(CursorError::Binding))
    );

    for version_change in [false, true] {
        let f = Fixture::new();
        let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
        let conn = HardenedPeerConnection::open(f.file.path(), DB_KEY).unwrap();
        let cursor =
            CursorStore::attach(conn, f.client.public_key().to_bytes(), &[origin]).unwrap();
        assert_eq!(
            Checkpoint::attach(cursor, f.client.clone(), UID, &[A], Policy::default()).err(),
            Some(Error::Cursor(CursorError::Binding))
        );
        let source = cp.bind_source(A).unwrap();
        let incoming = offer(&f, &f.server, A, f.wall);
        f.submit(&mut cp, &source, &incoming);
        let (guard, statement) = if version_change {
            (
                "peer_installed_offer_deny_update",
                "UPDATE peer_installed_offer_v2 SET version=1",
            )
        } else {
            (
                "peer_installed_offer_deny_delete",
                "DELETE FROM peer_installed_offer_v2",
            )
        };
        let trigger: String = cp
            .cursor
            .conn
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name=?1",
                [guard],
                |r| r.get(0),
            )
            .unwrap();
        cp.cursor.conn.execute_batch(&format!("DROP TRIGGER {guard}; PRAGMA ignore_check_constraints=ON; {statement}; {trigger}; PRAGMA ignore_check_constraints=OFF;")).unwrap();
        drop(cp);
        assert_eq!(
            open(&f, &f.client, A, Policy::default()).err(),
            Some(Error::Cursor(CursorError::Binding))
        );
    }
}

#[test]
fn peer_installed_offer_signed_parent_corruption_refuses_lookup_and_replay() {
    let f = Fixture::new();
    let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
    let source = cp.bind_source(A).unwrap();
    let initial = offer(&f, &f.server, A, f.wall);
    f.submit(&mut cp, &source, &initial);
    let key = Selector::from(&initial.flight);
    let wrong = offer(&f, &Keys::generate(), A, f.wall);
    cp.cursor
        .conn
        .execute("UPDATE peer_consent_invite SET frame=?1", [wrong.frame()])
        .unwrap();
    assert!(cp.installed_live_offer(&source, key, f.wall).is_err());
    assert!(cp.installed_historical_offer(&source, key, f.wall).is_err());
    assert_eq!(
        cp.admit(&source, raw(initial.frame()), f.wall).err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    assert_eq!(marks(&cp), 1);
    drop(cp);
    assert_eq!(
        open(&f, &f.client, A, Policy::default()).err(),
        Some(Error::Cursor(CursorError::Binding))
    );
}

fn open_custody(
    f: &Fixture,
    signer: &Keys,
    url: &str,
    policy: Policy,
    receive: ReceivePolicy,
) -> Result<Checkpoint> {
    let conn = HardenedPeerConnection::open(f.file.path(), DB_KEY)?;
    Checkpoint::attach_installed_custody(conn, signer.clone(), UID, url, policy, receive)
}

fn cursor_progress(cp: &Checkpoint, source: &Source) -> (i64, i64, i64) {
    cp.cursor
        .conn
        .query_row(
            "SELECT wall, revision, retry_revision FROM peer_cursor_origin WHERE origin=?1",
            [source.origin],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

#[test]
fn peer_installed_custody_preserves_exact_offer_ancestry_and_fences_old_writers() {
    let f = Fixture::new();
    let incoming = offer(&f, &f.server, A, f.wall);
    let key = Selector::from(&incoming.flight);
    let (old_source, before) = {
        let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
        let source = cp.bind_source(A).unwrap();
        assert_eq!(
            f.submit(&mut cp, &source, &incoming),
            Status::Accepted(TrustedOutcome::Applied)
        );
        let before = cursor_progress(&cp, &source);
        (source, before)
    };
    {
        let mut cp =
            open_custody(&f, &f.client, A, Policy::default(), ReceivePolicy::Manual).unwrap();
        let source = cp.bind_source(A).unwrap();
        let local = cp.authorize_local(UID).unwrap();
        assert_eq!(cursor_progress(&cp, &source), before);
        assert_eq!(counts(&cp), (1, 1, 0, 0));
        assert_eq!(
            cp.reserve_provisioning(&old_source, key, f.wall).err(),
            Some(Error::Source)
        );
        assert_eq!(
            cp.admit(&source, raw(incoming.frame()), f.wall).err(),
            Some(Error::Layout)
        );
        assert_eq!(cp.sweep(&source, f.wall).err(), Some(Error::Layout));
        assert_eq!(cp.approve(&local, key, f.wall).err(), Some(Error::Layout));
        assert_eq!(cp.retirement_target(&local, key).err(), Some(Error::Layout));
        let reservation = cp.reserve_provisioning(&source, key, f.wall).unwrap();
        assert!(!cp.has_stage(&source, &reservation, f.wall).unwrap());
        cp.bind_stage(&source, &reservation, f.wall).unwrap();
        assert!(cp.has_stage(&source, &reservation, f.wall).unwrap());
        assert!(open(&f, &f.client, A, Policy::default()).is_err());
        let conn = HardenedPeerConnection::open(f.file.path(), DB_KEY).unwrap();
        assert!(
            Checkpoint::attach_pending_custody_fixture(
                conn,
                f.client.clone(),
                UID,
                &[A],
                Policy::default()
            )
            .is_err()
        );
    }
    assert!(open_custody(&f, &f.client, B, Policy::default(), ReceivePolicy::Manual).is_err());
    assert!(
        open_custody(
            &f,
            &Keys::generate(),
            A,
            Policy::default(),
            ReceivePolicy::Manual
        )
        .is_err()
    );
    let mut reopened =
        open_custody(&f, &f.client, A, Policy::default(), ReceivePolicy::Manual).unwrap();
    let source = reopened.bind_source(A).unwrap();
    assert!(reopened.recover_provisioning(&source, key, f.wall).is_ok());
}

#[test]
fn peer_installed_custody_refuses_charged_predecessor_without_changing_it() {
    let f = Fixture::new();
    let incoming = offer(&f, &f.server, A, f.wall);
    let key = Selector::from(&incoming.flight);
    {
        let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
        let source = cp.bind_source(A).unwrap();
        assert_eq!(
            f.submit(&mut cp, &source, &incoming),
            Status::Accepted(TrustedOutcome::Applied)
        );
        cp.reserve_provisioning(&source, key, f.wall).unwrap();
    }
    assert!(open_custody(&f, &f.client, A, Policy::default(), ReceivePolicy::Manual).is_err());
    let mut unchanged = open(&f, &f.client, A, Policy::default()).unwrap();
    let source = unchanged.bind_source(A).unwrap();
    assert!(unchanged.recover_provisioning(&source, key, f.wall).is_ok());
    assert_eq!(counts(&unchanged), (1, 1, 0, 0));
}

#[test]
fn peer_installed_custody_resolves_receive_for_the_exact_founder() {
    let f = Fixture::new();
    let other = Keys::generate();
    let blocked = Keys::generate();
    let listed = f.server.public_key();
    let unlisted = other.public_key();
    let policy = || Policy {
        blocked: BTreeSet::from([blocked.public_key()]),
        reserved: BTreeSet::from([listed]),
        ..Policy::default()
    };
    let first = offer(&f, &f.server, A, f.wall);
    let second = offer(&f, &other, A, f.wall);
    {
        let mut cp = open(&f, &f.client, A, policy()).unwrap();
        let source = cp.bind_source(A).unwrap();
        for incoming in [&first, &second] {
            assert_eq!(
                f.submit(&mut cp, &source, incoming),
                Status::Accepted(TrustedOutcome::Applied)
            );
        }
    }
    let mut cp = open_custody(
        &f,
        &f.client,
        A,
        policy(),
        ReceivePolicy::AutoFounders(BTreeSet::from([listed])),
    )
    .unwrap();
    assert!(matches!(cp.custody_mode(listed), Ok(Mode::Automatic)));
    assert!(matches!(cp.custody_mode(unlisted), Ok(Mode::Manual)));
    assert!(matches!(
        cp.custody_mode(blocked.public_key()),
        Err(Error::Policy)
    ));
    assert!(matches!(
        cp.custody_mode(f.client.public_key()),
        Err(Error::Policy)
    ));
    let source = cp.bind_source(A).unwrap();
    for incoming in [&first, &second] {
        cp.reserve_provisioning(&source, Selector::from(&incoming.flight), f.wall)
            .unwrap();
    }
    for (receive, automatic) in [(ReceivePolicy::Off, false), (ReceivePolicy::AutoAny, true)] {
        drop(cp);
        cp = open_custody(&f, &f.client, A, policy(), receive).unwrap();
        let source = cp.bind_source(A).unwrap();
        let key = Selector::from(&second.flight);
        let result = cp.recover_provisioning(&source, key, f.wall);
        assert_eq!(result.is_ok(), automatic);
        if !automatic {
            assert_eq!(result.err(), Some(Error::Policy));
        }
    }
}

#[test]
fn peer_installed_custody_refuses_drift_and_unverified_parents() {
    let f = Fixture::new();
    let incoming = offer(&f, &f.server, A, f.wall);
    {
        let mut cp = open(&f, &f.client, A, Policy::default()).unwrap();
        let source = cp.bind_source(A).unwrap();
        assert_eq!(
            f.submit(&mut cp, &source, &incoming),
            Status::Accepted(TrustedOutcome::Applied)
        );
        cp.fixture_statement("UPDATE peer_consent_invite SET frame=X'01'");
    }
    assert!(open_custody(&f, &f.client, A, Policy::default(), ReceivePolicy::Manual).is_err());
    {
        let conn = HardenedPeerConnection::open(f.file.path(), DB_KEY).unwrap();
        let (frame, has_new_ddl): (Vec<u8>, bool) = conn
            .0
            .query_row(
                "SELECT frame, EXISTS(SELECT 1 FROM sqlite_schema
                    WHERE name IN ('peer_installed_custody_v2','peer_provisional_stage_v1'))
                 FROM peer_consent_invite",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(frame, [1]);
        assert!(!has_new_ddl);
        conn.0
            .execute(
                "UPDATE peer_consent_invite SET frame=?1",
                [incoming.frame()],
            )
            .unwrap();
    }
    let mut cp = open_custody(&f, &f.client, A, Policy::default(), ReceivePolicy::Manual).unwrap();
    let source = cp.bind_source(A).unwrap();
    let key = Selector::from(&incoming.flight);
    cp.fixture_statement("CREATE TABLE unrelated (value BLOB) STRICT");
    assert_eq!(
        cp.reserve_provisioning(&source, key, f.wall).err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    drop(cp);
    assert!(open_custody(&f, &f.client, A, Policy::default(), ReceivePolicy::Manual).is_err());
}
