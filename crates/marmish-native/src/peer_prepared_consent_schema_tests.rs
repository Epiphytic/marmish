use rusqlite::Connection;

use super::super::super::{Policy, Route};
use super::super::retirement::schema::inventory;
use super::*;

const URL: &str = "wss://consent-schema.example/";

fn setup() -> (tempfile::TempDir, [u8; 32], Keys, Origin) {
    let private = tempfile::tempdir().unwrap();
    let key = [61; 32];
    let signer = Keys::generate();
    let origin = Route::configured(URL).unwrap().id;
    (private, key, signer, origin)
}

#[test]
fn prepared_consent_exact_epoch_attachment_and_exclusion() {
    let (private, key, signer, origin) = setup();
    let path = private.path().join("cursor");
    std::fs::File::create(&path).unwrap();
    let open = || HardenedPeerConnection::open(&path, key).unwrap();
    let account = signer.public_key().to_bytes();
    let expected = Error::Cursor(CursorError::Binding);
    let export = || {
        Checkpoint::attach_prepared_consent_fixture(
            open(),
            signer.clone(),
            0,
            &[URL],
            Policy::default(),
        )
    };
    assert_eq!(export().err(), Some(expected));
    let bare = CursorStore::attach(open(), account, &[origin]).unwrap();
    let base = Checkpoint::attach_for_provisional_proof(
        bare,
        signer.clone(),
        0,
        &[URL],
        Policy::default(),
    )
    .unwrap();
    base.cursor
        .conn
        .execute_batch("CREATE TABLE extra(a)")
        .unwrap();
    assert_eq!(export().err(), Some(expected));
    base.cursor.conn.execute_batch("DROP TABLE extra").unwrap();
    drop(base);

    let checkpoint = export().unwrap();
    assert!(
        matches!(checkpoint.export_epoch, Some(Epoch::Prepared)) && checkpoint.provisional_schema
    );
    assert!(!checkpoint.retirement_epoch);
    drop(checkpoint);
    drop(export().unwrap());
    assert_eq!(
        Checkpoint::attach(
            CursorStore::attach(open(), account, &[origin]).unwrap(),
            signer.clone(),
            0,
            &[URL],
            Policy::default(),
        )
        .err(),
        Some(expected)
    );
    assert_eq!(
        Checkpoint::attach_for_provisional_proof(
            CursorStore::attach(open(), account, &[origin]).unwrap(),
            signer.clone(),
            0,
            &[URL],
            Policy::default(),
        )
        .err(),
        Some(expected)
    );
    assert_eq!(
        Checkpoint::retirement_cursor(open(), account, vec![origin]).err(),
        Some(expected)
    );

    let other_path = private.path().join("retirement-cursor");
    std::fs::File::create(&other_path).unwrap();
    let other = || HardenedPeerConnection::open(&other_path, key).unwrap();
    drop(
        Checkpoint::attach_for_provisional_proof(
            CursorStore::attach(other(), account, &[origin]).unwrap(),
            signer.clone(),
            0,
            &[URL],
            Policy::default(),
        )
        .unwrap(),
    );
    drop(Checkpoint::retirement_cursor(other(), account, vec![origin]).unwrap());
    assert_eq!(
        Checkpoint::attach_prepared_consent_fixture(
            other(),
            signer.clone(),
            0,
            &[URL],
            Policy::default()
        )
        .err(),
        Some(expected)
    );
    assert_eq!(
        Checkpoint::attach_prepared_consent_fixture(
            open(),
            Keys::generate(),
            0,
            &[URL],
            Policy::default()
        )
        .err(),
        Some(expected)
    );
    assert_eq!(
        Checkpoint::attach_prepared_consent_fixture(
            open(),
            signer,
            0,
            &["wss://other.example/"],
            Policy::default()
        )
        .err(),
        Some(expected)
    );
}

#[test]
fn prepared_consent_always_denies_unowned_writes_and_temp_shadow() {
    let (private, key, signer, origin) = setup();
    let path = private.path().join("cursor");
    std::fs::File::create(&path).unwrap();
    let open = || HardenedPeerConnection::open(&path, key).unwrap();
    let account = signer.public_key().to_bytes();
    drop(
        Checkpoint::attach_for_provisional_proof(
            CursorStore::attach(open(), account, &[origin]).unwrap(),
            signer.clone(),
            0,
            &[URL],
            Policy::default(),
        )
        .unwrap(),
    );
    let export = |connection| {
        Checkpoint::attach_prepared_consent_fixture(
            connection,
            signer.clone(),
            0,
            &[URL],
            Policy::default(),
        )
    };
    let checkpoint = export(open()).unwrap();
    let HardenedPeerConnection(other) = open();
    let HardenedPeerConnection(temp) = open();
    temp.execute_batch("CREATE TEMP TABLE shadow(a)").unwrap();
    assert_eq!(
        export(HardenedPeerConnection(temp)).err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    other
        .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA recursive_triggers=OFF;")
        .unwrap();
    assert!(
        other
            .execute(
                "INSERT OR REPLACE INTO peer_prepared_consent_epoch VALUES (1)",
                []
            )
            .is_err()
    );
    let denies: i64 = other.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='trigger' AND name LIKE 'peer_prepared_deny_%'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(denies, 3);
    assert!(
        other
            .execute(
                "INSERT OR REPLACE INTO peer_prepared_consent_outbox (class) VALUES (0)",
                []
            )
            .is_err()
    );
    assert_eq!(
        checkpoint
            .cursor
            .conn
            .query_row(
                "SELECT count(*) FROM peer_prepared_consent_outbox",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl Checkpoint {
    pub(crate) fn fixture_assert_managed_consent_boundaries(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        consent: &wire::Prepared<wire::Consent>,
        before_expiry: u64,
        boundary: u64,
        queue_refusal: Error,
    ) {
        let candidate = self
            .inspect_exact_prepared(source, reservation, before_expiry, true)
            .unwrap();
        let signed = || Unsaved {
            offer_carrier: candidate.offer.flight().carrier_id.to_bytes(),
            binding: candidate.binding,
            witness: candidate.witness,
            artifact: candidate.artifact,
            row: outbox::Outbox::new(
                &candidate,
                Transcript::from_consent(&consent.flight),
                consent.frame(),
            ),
        };
        let floors = self.consent_fixture_clocks();
        let origin = candidate.binding.origin;
        let key = candidate.binding.key;
        assert_eq!(
            self.queue_exact_prepared(source, reservation, signed(), boundary, true, |_| {
                panic!("queue reached commit at its exclusive deadline");
            })
            .err(),
            Some(queue_refusal)
        );
        assert_eq!(self.consent_fixture_clocks(), floors);
        assert!(!outbox::exists(&self.cursor.conn, key, origin).unwrap());
        let queued = self
            .queue_exact_prepared(source, reservation, signed(), before_expiry, true, |_| {
                Ok(())
            })
            .unwrap();
        assert_eq!(queued.deadline(), boundary);
        let floors = self.consent_fixture_clocks();
        assert_eq!(
            self.change_installed_consent::<()>(
                source,
                &queued,
                boundary,
                |_, _| panic!("exclusive deadline reached the reserve/settle mutation"),
                || panic!("exclusive deadline reached commit"),
            )
            .err(),
            Some(Error::Expired)
        );
        assert_eq!(self.consent_fixture_clocks(), floors);
        let persisted = outbox::load(&self.cursor.conn, &queued.prepared)
            .unwrap()
            .unwrap();
        assert!(persisted == queued.row);
    }

    pub(crate) fn fixture_assert_managed_consent_custodied(
        &mut self,
        source: &Source,
        queued: &Queued,
        wall: u64,
    ) {
        let floors = self.consent_fixture_clocks();
        assert_eq!(
            self.change_installed_consent::<()>(
                source,
                queued,
                wall,
                |_, _| panic!("custodied tuple reached the reserve/settle mutation"),
                || panic!("custodied tuple reached commit"),
            )
            .err(),
            Some(Error::Custodied)
        );
        assert_eq!(self.consent_fixture_clocks(), floors);
        let persisted = outbox::load(&self.cursor.conn, &queued.prepared)
            .unwrap()
            .unwrap();
        assert!(persisted == queued.row);
    }
}

impl Checkpoint {
    // Stores real signed bytes through the checked writer. The stage coordinates
    // and settlement are fixture inputs; this exercises no native join or relay.
    pub(crate) fn fixture_queue_and_settle_storage(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        consent: &wire::Prepared<wire::Consent>,
        wall: u64,
    ) {
        let prepared = self
            .inspect_prepared_consent(source, reservation, wall)
            .unwrap();
        let signed = Unsaved {
            offer_carrier: [0; 32],
            binding: prepared.binding,
            witness: prepared.witness,
            artifact: prepared.artifact,
            row: outbox::Outbox::new(
                &prepared,
                Transcript::from_consent(&consent.flight),
                consent.frame(),
            ),
        };
        let mut queued = self
            .queue_prepared_consent(source, reservation, signed, wall, |_| Ok(()))
            .unwrap();
        let tx = self
            .cursor
            .with_wall(prepared.binding.origin, wall)
            .unwrap();
        let retry_at = wall + outbox::RETRY_SECONDS;
        outbox::advance(
            &tx,
            &prepared,
            &queued.row,
            outbox::Advance::Reserve {
                attempt: 1,
                retry_at,
            },
        )
        .unwrap();
        queued.row.attempt = 1;
        queued.row.retry_at = Some(retry_at);
        outbox::advance(
            &tx,
            &prepared,
            &queued.row,
            outbox::Advance::Acknowledge { wall },
        )
        .unwrap();
        tx.commit().unwrap();
    }

    pub(crate) fn fixture_check_outbox_guards(&mut self, second: HardenedPeerConnection) {
        use rusqlite::{ErrorCode, TransactionBehavior};
        use std::time::Duration;

        let HardenedPeerConnection(second) = second;
        second.busy_timeout(Duration::ZERO).unwrap();
        second
            .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA recursive_triggers=OFF")
            .unwrap();
        let count = || {
            second
                .query_row(
                    "SELECT count(*) FROM peer_prepared_consent_outbox",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(count(), 2);
        const OUTBOX: &str = "peer_prepared_consent_outbox";
        const COLUMNS: &str = "class,generation,account,server,offer_id,offer_revision,origin,
            package_id,package_digest,package_expires,carrier,frame,frame_digest,revision,
            created_at,expires,package_ref,package_slot,attempt,retry_at,ack_at";
        for mutation in [
            COLUMNS
                .replace("server", "randomblob(32)")
                .replace("carrier", "randomblob(32)"),
            COLUMNS
                .replace("class", "1")
                .replace("carrier", "randomblob(32)"),
            COLUMNS
                .replace("class", "1")
                .replace("server", "randomblob(32)"),
        ] {
            let sql = format!(
                "INSERT OR REPLACE INTO {OUTBOX} ({COLUMNS})
                               SELECT {mutation} FROM {OUTBOX} WHERE class=0"
            );
            let error = second.execute(&sql, []).unwrap_err();
            assert!(
                error.to_string().contains("export owner required"),
                "{error}"
            );
        }
        for set in [
            "class=1",
            "server=(SELECT server FROM peer_prepared_consent_outbox WHERE class=1)",
            "carrier=(SELECT carrier FROM peer_prepared_consent_outbox WHERE class=1)",
        ] {
            let sql = format!("UPDATE OR REPLACE {OUTBOX} SET {set} WHERE class=0");
            let error = second.execute(&sql, []).unwrap_err();
            assert!(
                error.to_string().contains("export owner required"),
                "{error}"
            );
        }
        assert!(
            second
                .execute(&format!("DELETE FROM {OUTBOX}"), [])
                .is_err()
        );
        for sql in [
            "UPDATE OR REPLACE peer_provisioning_slot SET generation=generation+1 WHERE class=0",
            "DELETE FROM peer_provisional_stage_v1 WHERE class=1",
            "UPDATE OR REPLACE peer_consent_invite SET decision_at=expires-1",
        ] {
            assert!(second.execute(sql, []).is_err(), "{sql}");
        }

        let tx = self
            .cursor
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        schema::Writer::Insert.guard(&tx, false).unwrap();
        let wrong_generation = COLUMNS.replace("generation", "generation+1");
        let sql = format!(
            "INSERT INTO {OUTBOX} ({COLUMNS})
                           SELECT {wrong_generation} FROM {OUTBOX} WHERE class=0"
        );
        let error = tx.execute(&sql, []).unwrap_err();
        assert!(
            error.to_string().contains("exact Prepared stage required"),
            "{error}"
        );
        let blocked = second
            .execute(&format!("DELETE FROM {OUTBOX}"), [])
            .unwrap_err();
        assert_eq!(blocked.sqlite_error_code(), Some(ErrorCode::DatabaseBusy));
        tx.rollback().unwrap();
        assert_eq!(count(), 2);
        let blocked = second
            .execute(&format!("DELETE FROM {OUTBOX}"), [])
            .unwrap_err();
        assert!(blocked.to_string().contains("export owner required"));

        let tx = self
            .cursor
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        schema::Writer::Update.guard(&tx, false).unwrap();
        assert!(
            tx.execute(&format!("UPDATE {OUTBOX} SET attempt=1 WHERE class=0"), [],)
                .is_err()
        );
        assert!(
            tx.execute(
                &format!("UPDATE OR REPLACE {OUTBOX} SET class=1 WHERE class=0"),
                [],
            )
            .is_err()
        );
        tx.rollback().unwrap();
        assert_eq!(count(), 2);
        assert_legacy_conflicts(&second);
        self.fixture_assert_corrupt_parent_rejected();
    }

    pub(crate) fn fixture_assert_queue_rollback(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        proof: &crate::peer_invitation::prepared_consent::NativeAttestation,
        wall: u64,
    ) {
        use rusqlite::TransactionBehavior;
        let before = inventory(&self.cursor.conn).unwrap();
        let candidate = self
            .inspect_prepared_consent(source, reservation, wall)
            .unwrap();
        let signed = self.sign_prepared_consent(&candidate, proof, wall).unwrap();
        let (_, _, _, selector) = reservation.stage_binding().coordinates();
        let origin = self.origin(source).unwrap();
        let result = self.queue_prepared_consent(source, reservation, signed, wall, |tx| {
            tx.execute_batch(
                "CREATE TEMP TRIGGER fail_wall_update BEFORE UPDATE OF wall ON main.peer_consent_version
                 BEGIN SELECT RAISE(ABORT,'fixture lost durable cursor write'); END",
            )?;
            Ok(())
        });
        assert_eq!(result.err(), Some(Error::Cursor(CursorError::Storage)));
        let account = self.signer.public_key().to_bytes();
        let origins = self.cursor.origins.clone();
        let tx = self
            .cursor
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(inventory(&tx).unwrap(), before);
        schema::verify(&tx, account, &origins).unwrap();
        assert!(!outbox::exists(&tx, selector, origin).unwrap());
        tx.rollback().unwrap();
    }

    pub(crate) fn fixture_assert_mutated_transcript_rejected(
        &mut self,
        source: &Source,
        selector: Selector,
        proof: &crate::peer_invitation::prepared_consent::NativeAttestation,
        wall: u64,
    ) {
        let mut queued = self
            .recover_queued_consent(source, selector, wall)
            .unwrap()
            .unwrap();
        self.verify_queued_consent(&queued, proof, wall).unwrap();
        queued.row.frame[0] ^= 1;
        assert_eq!(
            self.verify_queued_consent(&queued, proof, wall).err(),
            Some(Error::Package)
        );
        let mut queued = self
            .recover_queued_consent(source, selector, wall)
            .unwrap()
            .unwrap();
        queued.row.transcript.carrier[0] ^= 1;
        assert_eq!(
            self.verify_queued_consent(&queued, proof, wall).err(),
            Some(Error::Package)
        );
    }

    fn fixture_assert_corrupt_parent_rejected(&mut self) {
        use rusqlite::TransactionBehavior;
        let account = self.signer.public_key().to_bytes();
        let origins = self.cursor.origins.clone();
        let tx = self
            .cursor
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        schema::verify(&tx, account, &origins).unwrap();
        let before = inventory(&tx).unwrap();
        for (trigger, delete) in [
            (
                "peer_stage_slot_delete_guard",
                "DELETE FROM peer_provisioning_slot WHERE class=0",
            ),
            (
                "peer_stage_delete_guard",
                "DELETE FROM peer_provisional_stage_v1 WHERE class=0",
            ),
        ] {
            tx.execute_batch("SAVEPOINT corrupt").unwrap();
            let ddl: String = tx
                .query_row(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
                    [trigger],
                    |r| r.get(0),
                )
                .unwrap();
            tx.execute_batch(&format!("DROP TRIGGER {trigger}; {delete}; {ddl}"))
                .unwrap();
            assert_eq!(inventory(&tx).unwrap(), before);
            assert_eq!(
                schema::verify(&tx, account, &origins),
                Err(CursorError::Binding)
            );
            tx.execute_batch("ROLLBACK TO corrupt; RELEASE corrupt")
                .unwrap();
            schema::verify(&tx, account, &origins).unwrap();
        }
        tx.rollback().unwrap();
    }
}

fn assert_legacy_conflicts(conn: &Connection) {
    const LEGACY_INSERT: &str = "INSERT INTO peer_consent_outbox
        (server,offer_id,offer_revision,origin,carrier,revision,expires,frame,package_id,package_digest)
        SELECT server,offer_id,revision,origin,randomblob(32),randomblob(32),expires,frame,randomblob(32),randomblob(32)
        FROM peer_consent_invite i WHERE";
    let owned = format!(
        "{LEGACY_INSERT} EXISTS (SELECT 1 FROM peer_provisioning_slot p WHERE p.server=i.server)"
    );
    let error = conn.execute(&owned, []).unwrap_err();
    assert!(
        error.to_string().contains("provisioning outbox conflict"),
        "{error}"
    );

    conn.execute_batch("SAVEPOINT legacy_claim").unwrap();
    let unowned = format!(
        "{LEGACY_INSERT} NOT EXISTS (SELECT 1 FROM peer_provisioning_slot p WHERE p.server=i.server)"
    );
    assert_eq!(conn.execute(&unowned, []).unwrap(), 1);
    let error = conn
        .execute(
            "UPDATE peer_provisioning_slot SET (server,offer_id,revision,origin) =
            (SELECT server,offer_id,offer_revision,origin FROM peer_consent_outbox) WHERE class=0",
            [],
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("provisioning claim conflict"),
        "{error}"
    );
    conn.execute_batch("ROLLBACK TO legacy_claim; RELEASE legacy_claim")
        .unwrap();
}
