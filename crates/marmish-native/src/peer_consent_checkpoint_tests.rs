use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use rusqlite::{Connection, params};

use super::super::{HardenedPeerConnection, Page, ReceiptKind, ScanStart};
use super::*;

#[path = "peer_consent_checkpoint_fixture.rs"]
mod fixture;
#[path = "peer_installed_offer_store_tests.rs"]
mod installed_layout;
#[path = "peer_provisioning_tests.rs"]
mod provisioning;

use fixture::{A, B, DB_KEY, Fixture, UID, native_package, raw, rewrap};

fn counts(checkpoint: &Checkpoint) -> (u16, u16, u16, u16) {
    checkpoint
        .cursor
        .conn
        .query_row(
            "SELECT (SELECT count(*) FROM peer_consent_invite),
                    (SELECT count(*) FROM peer_consent_fence),
                    (SELECT count(*) FROM peer_consent_outbox),
                    (SELECT count(*) FROM peer_cursor_deferred)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

fn queued(work: Work) -> Outbox {
    let Work::Queued(value) = work else {
        panic!("expected committed Consent");
    };
    value
}

fn approval_time(checkpoint: &Checkpoint, key: Selector) -> Option<u64> {
    checkpoint
        .cursor
        .conn
        .query_row(
            "SELECT decision_at FROM peer_consent_invite WHERE server=?1 AND offer_id=?2 AND revision=?3",
            params![key.server.to_bytes(), key.id, key.revision],
            |row| super::super::read_optional_seconds(row, 0),
        )
        .unwrap()
}

#[test]
fn peer_consent_checkpoint_core_exact_verified_source_and_cursor_commit() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let b = cp.bind_source(B).unwrap();
    let different = Fixture::new().start(Policy::default());
    let elsewhere = different.bind_source(A).unwrap();
    let offer = f.offer(&f.server, A, f.wall);
    let rewritten = rewrap(offer.frame(), &f.client);
    let original = raw(offer.frame());
    let wrapped = raw(&rewritten);
    assert_ne!(original.id, wrapped.id);
    assert_eq!(
        cp.admit(&elsewhere, original, f.wall).err(),
        Some(Error::Source)
    );
    let wrong = cp.admit(&b, original, f.wall).unwrap();
    assert!(wrong.receipt.kind == ReceiptKind::Invalid);
    assert_eq!(
        cp.admit(&b, wrapped, f.wall).unwrap().status,
        Status::Invalid
    );
    assert_eq!(counts(&cp), (0, 0, 0, 0));

    for forged in [
        RawRecord {
            stamp: original.stamp + 1,
            ..original
        },
        RawRecord {
            id: [0x78; 32],
            ..original
        },
    ] {
        assert_eq!(cp.admit(&a, forged, f.wall).err(), Some(Error::Index));
    }
    let malformed = RawRecord {
        bytes: b"{invalid JSON",
        ..original
    };
    assert_eq!(
        cp.admit(&a, malformed, f.wall).unwrap().status,
        Status::Invalid
    );
    let mut corrupt: serde_json::Value = serde_json::from_slice(offer.frame()).unwrap();
    corrupt["sig"] = serde_json::Value::String("72".repeat(64));
    let corrupt = serde_json::to_vec(&corrupt).unwrap();
    assert_eq!(
        cp.admit(&a, raw(&corrupt), f.wall).unwrap().status,
        Status::Invalid
    );
    let other = Fixture::new();
    let wrong_recipient = other.offer(&f.server, A, f.wall);
    assert_eq!(f.submit(&mut cp, &a, &wrong_recipient), Status::Invalid);
    assert_eq!(counts(&cp), (0, 0, 0, 0));

    let c = cp
        .cursor
        .scan(a.origin, ScanStart::Recovery, f.wall)
        .unwrap()
        .unwrap();
    let records = [original, wrapped];
    let page = Page {
        origin: a.origin,
        since: c.lower,
        until: c.until,
        records: &records,
    };
    assert_eq!(
        cp.cursor.commit_page(&c, &page, &[], f.wall).err(),
        Some(CursorError::Page)
    );
    let incoming = cp.admit(&a, original, f.wall).unwrap();
    assert_eq!(incoming.status, Status::Accepted(TrustedOutcome::Applied));
    let repeat = cp.admit(&a, wrapped, f.wall).unwrap();
    assert_eq!(
        repeat.status,
        Status::Accepted(TrustedOutcome::AlreadyApplied)
    );
    assert!(
        cp.cursor
            .commit_page(&c, &page, &[incoming.receipt, repeat.receipt], f.wall)
            .unwrap()
            .is_none()
    );
    assert_eq!(counts(&cp), (1, 1, 0, 0));
    let selector = Selector::from(&offer.flight);
    assert_eq!(approval_time(&cp, selector), None);
}

#[test]
fn peer_consent_checkpoint_core_policy_reload_enforces_current_local_owner() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(cp.authorize_local(UID + 1).err(), Some(Error::LocalOwner));
    let offer = f.offer(&f.server, A, f.wall);
    let selector = Selector::from(&offer.flight);
    assert_eq!(
        f.submit(&mut cp, &a, &offer),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let stale = Selector {
        revision: [0xbc; 32],
        ..selector
    };
    assert_eq!(
        cp.approve(&local, stale, f.wall).err(),
        Some(Error::Missing)
    );
    assert_eq!(
        cp.discard(&local, stale, f.wall).err(),
        Some(Error::Missing)
    );
    assert!(matches!(
        cp.work(
            &local,
            selector,
            f.wall,
            Some(Material {
                origin: a.origin,
                selector,
                package: b"unused",
                expires: 0
            })
        )
        .unwrap(),
        Work::Pending
    ));
    drop(cp);

    let mut cp = f.start(Policy {
        mode: Mode::Automatic,
        reserved: [f.server.public_key()].into(),
        ..Default::default()
    });
    let a = cp.bind_source(A).unwrap();
    assert_eq!(
        cp.approve(&local, selector, f.wall).err(),
        Some(Error::LocalOwner)
    );
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(approval_time(&cp, selector), None);
    cp.approve(&local, selector, f.wall + 1).unwrap();
    cp.approve(&local, selector, f.wall + 2).unwrap();
    assert_eq!(approval_time(&cp, selector), Some(f.wall + 1));
    let pinned = Keys::generate();
    let unknown = Keys::generate();
    drop(cp);

    let mut cp = f.start(Policy {
        mode: Mode::Automatic,
        reserved: [pinned.public_key()].into(),
        ..Default::default()
    });
    let source = cp.bind_source(A).unwrap();
    assert_eq!(
        cp.admit(&a, raw(offer.frame()), f.wall + 2).err(),
        Some(Error::Source)
    );
    for server in [&pinned, &unknown] {
        let incoming = f.offer(server, A, f.wall);
        assert_eq!(
            cp.admit(&source, raw(incoming.frame()), f.wall + 2)
                .unwrap()
                .status,
            Status::Accepted(TrustedOutcome::Applied)
        );
        assert_eq!(
            approval_time(&cp, Selector::from(&incoming.flight)),
            Some(f.wall + 2)
        );
    }
    drop(cp);

    let mut cp = f.start(Policy {
        mode: Mode::Off,
        ..Default::default()
    });
    let source = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    let new = f.offer(&Keys::generate(), A, f.wall);
    assert_eq!(
        cp.admit(&source, raw(new.frame()), f.wall + 2)
            .unwrap()
            .status,
        Status::Invalid
    );
    assert_eq!(
        cp.work(&local, selector, f.wall + 2, None).err(),
        Some(Error::Policy)
    );
    assert_eq!(
        cp.approve(&local, selector, f.wall + 2).err(),
        Some(Error::Policy)
    );
    drop(cp);

    let mut cp = f.start(Policy {
        mode: Mode::Automatic,
        blocked: [f.server.public_key()].into(),
        reserved: [f.server.public_key()].into(),
    });
    let local = cp.authorize_local(UID).unwrap();
    let source = cp.bind_source(A).unwrap();
    let denied = f.offer(&f.server, A, f.wall);
    assert_eq!(
        cp.admit(&source, raw(denied.frame()), f.wall + 2)
            .unwrap()
            .status,
        Status::Invalid
    );
    assert_eq!(
        cp.work(&local, selector, f.wall + 2, None).err(),
        Some(Error::Policy)
    );
    cp.discard(&local, selector, f.wall + 2).unwrap();
    assert_eq!(counts(&cp), (2, 3, 0, 0));
}

#[test]
fn peer_consent_checkpoint_core_capacity_and_deferred_recovery() {
    let f = Fixture::new();
    let pinned: Vec<_> = (0..9).map(|_| Keys::generate()).collect();
    let strangers: Vec<_> = (0..9).map(|_| Keys::generate()).collect();
    let policy = || Policy {
        reserved: pinned.iter().map(Keys::public_key).collect(),
        ..Default::default()
    };
    let mut cp = f.start(policy());
    let a = cp.bind_source(A).unwrap();
    let b = cp.bind_source(B).unwrap();
    for peers in [&strangers, &pinned] {
        for (index, server) in peers.iter().enumerate() {
            let offer = f.offer(server, A, f.wall);
            assert_eq!(
                f.submit(&mut cp, &a, &offer),
                if index < 8 {
                    Status::Accepted(TrustedOutcome::Applied)
                } else {
                    Status::Deferred
                }
            );
        }
    }
    assert_eq!(counts(&cp), (16, 16, 0, 2));
    let contended = f.offer(&pinned[0], B, f.wall);
    assert_eq!(f.submit(&mut cp, &b, &contended), Status::Deferred);
    let deferred = cp
        .cursor
        .existing(b.origin, raw(contended.frame()).id)
        .unwrap()
        .unwrap();
    assert_eq!(deferred.metadata.auth, Auth::FlightVerified);
    assert_eq!(
        deferred.metadata.expires,
        contended.flight.flight().expires_at
    );
    let first = cp
        .cursor
        .conn
        .query_row(
            "SELECT offer_id, revision FROM peer_consent_invite WHERE server=?1",
            [pinned[0].public_key().to_bytes()],
            |row| {
                Ok(Selector {
                    server: pinned[0].public_key(),
                    id: row.get(0)?,
                    revision: row.get(1)?,
                })
            },
        )
        .unwrap();
    assert_eq!(counts(&cp), (16, 16, 0, 3));
    let second = Selector::from(&contended.flight);
    assert_ne!(first, second);
    drop(cp);

    let mut cp = f.start(policy());
    let local = cp.authorize_local(UID).unwrap();
    cp.discard(&local, first, f.wall).unwrap();
    let source = cp.bind_source(B).unwrap();
    let disposition = cp
        .admit(&source, deferred.record(), deferred.retry_at)
        .unwrap();
    assert_eq!(
        disposition.status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert!(
        cp.cursor
            .existing(source.origin, deferred.record().id)
            .unwrap()
            .is_none()
    );
    assert_eq!(counts(&cp), (16, 17, 0, 2));
    assert_eq!(
        cp.approve(&local, first, deferred.retry_at).err(),
        Some(Error::Missing)
    );
    assert_eq!(approval_time(&cp, second), None);
}

#[test]
fn peer_consent_checkpoint_core_fence_cap_expiry_and_clock_rebase() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let b = cp.bind_source(B).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    let original = f.offer(&f.server, A, f.wall);
    assert_eq!(
        f.submit(&mut cp, &a, &original),
        Status::Accepted(TrustedOutcome::Applied)
    );
    cp.discard(&local, Selector::from(&original.flight), f.wall)
        .unwrap();
    // Seed the already-committed history in one transaction. The original,
    // duplicate, cap refusal and post-expiry admission use real signed Offers.
    let tx = cp.cursor.begin(a.origin).unwrap();
    let deadline = original.flight.flight().replay_safe_until;
    for index in 1u16..256 {
        let mut revision = [0; 32];
        revision[..2].copy_from_slice(&index.to_le_bytes());
        tx.execute(
            "INSERT INTO peer_consent_fence VALUES (?1, ?2, 0, ?3)",
            params![a.origin, revision, super::super::seconds(deadline).unwrap()],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    let extra = f.offer(&f.server, A, f.wall);
    assert_eq!(f.submit(&mut cp, &a, &extra), Status::Deferred);
    assert_eq!(
        f.submit(&mut cp, &a, &original),
        Status::Accepted(TrustedOutcome::AlreadyApplied)
    );
    assert_eq!(counts(&cp), (0, 256, 0, 1));
    cp.sweep(&b, deadline - 1).unwrap();
    assert_eq!(counts(&cp), (0, 256, 0, 1));
    cp.sweep(&b, deadline).unwrap();
    assert_eq!(counts(&cp), (0, 0, 0, 1));
    cp.cursor.rebase_wall(a.origin, f.wall).unwrap();
    cp.cursor.rebase_wall(b.origin, f.wall).unwrap();
    drop(cp);

    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    assert_eq!(
        cp.admit(&a, raw(original.frame()), f.wall).err(),
        Some(Error::Cursor(CursorError::ClockRegressed))
    );
    assert_eq!(
        cp.admit(&a, raw(original.frame()), deadline)
            .unwrap()
            .status,
        Status::Invalid
    );
    let new = f.offer(&f.server, A, deadline);
    assert_eq!(
        cp.admit(&a, raw(new.frame()), deadline).unwrap().status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    let local = cp.authorize_local(UID).unwrap();
    cp.cursor.rebase_wall(a.origin, f.wall).unwrap();
    assert_eq!(
        cp.approve(&local, Selector::from(&new.flight), deadline - 1)
            .err(),
        Some(Error::Cursor(CursorError::ClockRegressed))
    );
}

#[test]
fn peer_consent_checkpoint_core_ingest_rollback_and_encryption() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let offer = f.offer(&f.server, A, f.wall);
    let record = raw(offer.frame());
    cp.cursor
        .defer(
            a.origin,
            record,
            Metadata {
                auth: Auth::FlightVerified,
                expires: offer.flight.flight().expires_at,
                prerequisite: None,
            },
            f.wall,
        )
        .unwrap();
    cp.cursor
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER abort_fence BEFORE INSERT ON peer_consent_fence
        BEGIN SELECT RAISE(ABORT, 'fixture rollback'); END;",
        )
        .unwrap();
    assert_eq!(
        cp.admit(&a, record, f.wall).err(),
        Some(Error::Cursor(CursorError::Storage))
    );
    assert_eq!(counts(&cp), (0, 0, 0, 1));
    drop(cp);

    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        cp.cursor.commit_trusted(a.origin, record, f.wall, |tx| {
            rows::sweep(tx, f.wall)?;
            rows::admit(tx, a.origin, record, &offer.flight, false, None)?;
            panic!("fixture abort after both writes");
        })
    }));
    assert!(panic.is_err());
    assert_eq!(counts(&cp), (0, 0, 0, 1));
    assert_eq!(
        f.submit(&mut cp, &a, &offer),
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert_eq!(counts(&cp), (1, 1, 0, 0));
    drop(cp);
    let file = std::fs::read(f.file.path()).unwrap();
    assert!(!file.starts_with(b"SQLite format 3\0"));
    let key = Selector::from(&offer.flight);
    for secret in [
        key.id.as_slice(),
        key.revision.as_slice(),
        &record.id,
        offer.frame(),
    ] {
        assert!(!file.windows(secret.len()).any(|part| part == secret));
    }
    assert!(HardenedPeerConnection::open(f.file.path(), [0x09; 32]).is_err());
    assert_eq!(
        f.cursor(DB_KEY, Keys::generate().public_key()).err(),
        Some(CursorError::Binding)
    );
    let cursor = f.cursor(DB_KEY, f.client.public_key()).unwrap();
    assert_eq!(
        Checkpoint::attach(cursor, Keys::generate(), UID, &[A, B], Policy::default()).err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    let cursor = f.cursor(DB_KEY, f.client.public_key()).unwrap();
    assert_eq!(
        Checkpoint::attach(cursor, f.client.clone(), UID, &[A, A], Policy::default()).err(),
        Some(Error::Source)
    );
    let plain = tempfile::NamedTempFile::new().unwrap();
    Connection::open(plain.path())
        .unwrap()
        .execute_batch("CREATE TABLE fixture (n INTEGER);")
        .unwrap();
    assert!(HardenedPeerConnection::open(plain.path(), DB_KEY).is_err());
}

#[tokio::test]
async fn peer_consent_checkpoint_core_exact_outbox_rollback_restart_and_discard() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    let offer = f.offer(&f.server, A, f.wall);
    let selector = Selector::from(&offer.flight);
    assert_eq!(
        f.submit(&mut cp, &a, &offer),
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert!(matches!(
        cp.work(&local, selector, f.wall, None).unwrap(),
        Work::Pending
    ));
    cp.cursor
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER abort_decision BEFORE UPDATE OF decision_at
        ON peer_consent_invite BEGIN SELECT RAISE(ABORT, 'fixture rollback'); END;",
        )
        .unwrap();
    assert_eq!(
        cp.approve(&local, selector, f.wall + 1).err(),
        Some(Error::Cursor(CursorError::Storage))
    );
    drop(cp);

    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(approval_time(&cp, selector), None);
    assert!(matches!(
        cp.work(&local, selector, f.wall, None).unwrap(),
        Work::Pending
    ));
    cp.approve(&local, selector, f.wall).unwrap();
    drop(cp);

    let mut cp = f.start(Policy::default());
    assert_eq!(
        cp.work(&local, selector, f.wall, None).err(),
        Some(Error::LocalOwner)
    );
    let local = cp.authorize_local(UID).unwrap();
    assert!(matches!(
        cp.work(&local, selector, f.wall, None).unwrap(),
        Work::NeedPackage { origin } if origin == a.origin
    ));
    let (package, now) = native_package(&f.client, A).await;
    let (wrong_signer, other_now) = native_package(&Keys::generate(), A).await;
    let now = now.max(other_now).max(f.wall);
    let expiry = now + 120;
    let material = || {
        Some(Material {
            origin: a.origin,
            selector,
            package: &package,
            expires: expiry,
        })
    };
    assert_eq!(
        cp.work(
            &local,
            selector,
            now,
            Some(Material {
                origin: a.origin,
                selector,
                package: b"invalid",
                expires: expiry
            })
        )
        .err(),
        Some(Error::Package)
    );
    assert_eq!(
        cp.work(
            &local,
            selector,
            now,
            Some(Material {
                origin: Route::configured(B).unwrap().id,
                selector,
                package: &package,
                expires: expiry
            })
        )
        .err(),
        Some(Error::Package)
    );
    assert_eq!(
        cp.work(
            &local,
            selector,
            now,
            Some(Material {
                origin: a.origin,
                selector,
                package: &package,
                expires: now + 86_401
            })
        )
        .err(),
        Some(Error::Package)
    );
    assert_eq!(
        cp.work(
            &local,
            selector,
            now,
            Some(Material {
                origin: a.origin,
                selector,
                package: &wrong_signer,
                expires: expiry
            })
        )
        .err(),
        Some(Error::Package)
    );
    assert_eq!(
        cp.work(
            &local,
            selector,
            now,
            Some(Material {
                origin: a.origin,
                selector: Selector {
                    revision: [0xb1; 32],
                    ..selector
                },
                package: &package,
                expires: expiry,
            })
        )
        .err(),
        Some(Error::Package)
    );
    cp.cursor
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER abort_outbox BEFORE INSERT ON peer_consent_outbox
        BEGIN SELECT RAISE(ABORT, 'fixture rollback'); END;",
        )
        .unwrap();
    assert_eq!(
        cp.work(&local, selector, now, material()).err(),
        Some(Error::Cursor(CursorError::Storage))
    );
    assert_eq!(counts(&cp), (1, 1, 0, 0));
    drop(cp);

    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    let outbox = queued(cp.work(&local, selector, now, material()).unwrap());
    assert_eq!(outbox.origin, a.origin);
    assert_eq!(outbox.expires, expiry);
    let opened = cp
        .codec
        .open_consent(&f.server, &outbox.frame, &offer.flight, now)
        .unwrap();
    assert_eq!(opened.flight().carrier_id.to_bytes(), outbox.carrier);
    assert_eq!(opened.flight().revision.to_bytes(), outbox.revision);
    assert_eq!(Selector::from(opened.offer()), selector);
    assert_eq!(opened.package_bytes(), &package);
    drop(cp);

    let mut cp = f.start(Policy {
        mode: Mode::Off,
        ..Default::default()
    });
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(
        cp.work(&local, selector, now, None).err(),
        Some(Error::Policy)
    );
    assert_eq!(counts(&cp), (1, 1, 1, 0));
    drop(cp);

    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    cp.cursor
        .conn
        .execute_batch(
            "CREATE TEMP TRIGGER abort_discard BEFORE DELETE
        ON peer_consent_invite BEGIN SELECT RAISE(ABORT, 'fixture rollback'); END;",
        )
        .unwrap();
    assert_eq!(
        cp.discard(&local, selector, now + 1).err(),
        Some(Error::Cursor(CursorError::Storage))
    );
    drop(cp);

    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    assert_eq!(counts(&cp), (1, 1, 1, 0));
    let recovered = queued(cp.work(&local, selector, now + 1, None).unwrap());
    let duplicate = queued(
        cp.work(
            &local,
            selector,
            now + 1,
            Some(Material {
                origin: a.origin,
                selector,
                package: &wrong_signer,
                expires: 0,
            }),
        )
        .unwrap(),
    );
    assert_eq!(&*recovered.frame, &*outbox.frame);
    assert_eq!(&*duplicate.frame, &*outbox.frame);
    assert_eq!(recovered.carrier, outbox.carrier);
    assert_eq!(recovered.revision, outbox.revision);
    let db = std::fs::read(f.file.path()).unwrap();
    for bytes in [
        outbox.frame.as_slice(),
        outbox.carrier.as_slice(),
        outbox.revision.as_slice(),
    ] {
        assert!(!db.windows(bytes.len()).any(|part| part == bytes));
    }
    assert_eq!(
        cp.work(&local, selector, expiry, material()).err(),
        Some(Error::Expired)
    );
    assert_eq!(counts(&cp), (1, 1, 1, 0));

    let other = f.offer(&Keys::generate(), B, now);
    let other_selector = Selector::from(&other.flight);
    let b = cp.bind_source(B).unwrap();
    assert_eq!(
        cp.admit(&b, raw(other.frame()), expiry).unwrap().status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    cp.discard(&local, selector, expiry).unwrap();
    assert_eq!(counts(&cp), (1, 2, 0, 0));
    assert_eq!(
        cp.work(&local, selector, expiry, material()).err(),
        Some(Error::Missing)
    );
    assert!(matches!(
        cp.work(&local, other_selector, expiry, None).unwrap(),
        Work::Pending
    ));
    drop(cp);

    let mut cp = f.start(Policy::default());
    let source = cp.bind_source(A).unwrap();
    assert_eq!(
        cp.admit(&source, raw(offer.frame()), expiry)
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::AlreadyApplied)
    );
    let another_origin = cp.bind_source(B).unwrap();
    assert_eq!(
        cp.admit(&another_origin, raw(offer.frame()), expiry)
            .unwrap()
            .status,
        Status::Invalid
    );
    assert_eq!(counts(&cp), (1, 2, 0, 0));
}

#[tokio::test]
async fn peer_consent_checkpoint_core_live_queued_consent_outlasts_offer() {
    let f = Fixture::new();
    let mut cp = f.start(Policy {
        mode: Mode::Automatic,
        ..Default::default()
    });
    let a = cp.bind_source(A).unwrap();
    let offer = f.offer(&f.server, A, f.wall - 86_400 + 120);
    let selector = Selector::from(&offer.flight);
    assert_eq!(
        f.submit(&mut cp, &a, &offer),
        Status::Accepted(TrustedOutcome::Applied)
    );
    drop(cp);
    let mut cp = f.start(Policy::default());
    let local = cp.authorize_local(UID).unwrap();
    assert!(matches!(
        cp.work(&local, selector, f.wall, None).unwrap(),
        Work::NeedPackage { .. }
    ));
    let (package, now) = native_package(&f.client, A).await;
    let now = now.max(f.wall);
    let expires = now + 2_400;
    let outbox = queued(
        cp.work(
            &local,
            selector,
            now,
            Some(Material {
                origin: a.origin,
                selector,
                package: &package,
                expires,
            }),
        )
        .unwrap(),
    );
    let offer_end = offer.flight.flight().expires_at;
    let fence_end = offer.flight.flight().replay_safe_until;
    assert!(offer_end < fence_end && fence_end < expires);
    drop(cp);

    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    cp.sweep(&a, fence_end).unwrap();
    assert_eq!(counts(&cp), (1, 0, 1, 0));
    assert_eq!(
        cp.approve(&local, selector, fence_end).err(),
        Some(Error::Expired)
    );
    let recovered = queued(cp.work(&local, selector, fence_end, None).unwrap());
    assert_eq!(&*recovered.frame, &*outbox.frame);
    assert!(
        cp.codec
            .open_consent(&f.server, &recovered.frame, &offer.flight, fence_end)
            .is_ok()
    );
    assert_eq!(
        cp.work(&local, selector, expires, None).err(),
        Some(Error::Expired)
    );
    cp.sweep(&a, expires).unwrap();
    assert_eq!(counts(&cp), (0, 0, 0, 0));
}

#[tokio::test]
#[ignore = "offline cryptography plus synchronous SQLCipher transaction timings"]
async fn peer_consent_checkpoint_benchmark() {
    let f = Fixture::new();
    let mut cp = f.start(Policy::default());
    let a = cp.bind_source(A).unwrap();
    let local = cp.authorize_local(UID).unwrap();
    let warmup = f.offer(&f.server, A, f.wall);
    assert_eq!(
        f.submit(&mut cp, &a, &warmup),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let warmup = Selector::from(&warmup.flight);
    cp.approve(&local, warmup, f.wall).unwrap();
    assert!(matches!(
        cp.work(&local, warmup, f.wall, None).unwrap(),
        Work::NeedPackage { .. }
    ));
    let (package, now) = native_package(&f.client, A).await;
    let now = now.max(f.wall);
    cp.discard(&local, warmup, now).unwrap();

    let mut samples: [Vec<_>; 4] = std::array::from_fn(|_| Vec::with_capacity(100));
    let mut sizes = (0, 0);
    for index in 0..110 {
        let offer = f.offer(&f.server, A, now);
        let selector = Selector::from(&offer.flight);
        let record = raw(offer.frame());
        let start = Instant::now();
        assert_eq!(
            cp.admit(&a, record, now).unwrap().status,
            Status::Accepted(TrustedOutcome::Applied)
        );
        let admit = start.elapsed();
        let start = Instant::now();
        cp.approve(&local, selector, now).unwrap();
        let approval = start.elapsed();
        let start = Instant::now();
        let outbox = queued(
            cp.work(
                &local,
                selector,
                now,
                Some(Material {
                    origin: a.origin,
                    selector,
                    package: &package,
                    expires: now + 600,
                }),
            )
            .unwrap(),
        );
        let write = start.elapsed();
        let start = Instant::now();
        let replay = queued(cp.work(&local, selector, now, None).unwrap());
        let read = start.elapsed();
        assert_eq!(&*outbox.frame, &*replay.frame);
        cp.discard(&local, selector, now).unwrap();
        if index >= 10 {
            for (series, value) in samples.iter_mut().zip([admit, approval, write, read]) {
                series.push(value);
            }
            sizes = (offer.frame().len(), outbox.frame.len());
        }
    }
    println!(
        "signed Offer {} bytes, signed Consent {} bytes, n=100",
        sizes.0, sizes.1
    );
    for (label, mut series) in ["admit", "approve", "queue", "recover work"]
        .into_iter()
        .zip(samples)
    {
        series.sort_unstable();
        let elapsed = |index: usize| series[index].as_secs_f64() * 1000.0;
        println!(
            "{label}: p50={:.3} ms p95={:.3} ms",
            elapsed(49),
            elapsed(94)
        );
    }
}
