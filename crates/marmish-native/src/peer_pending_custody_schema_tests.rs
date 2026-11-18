use cgka_traits::app_components::NostrRoutingV1;
use rusqlite::{Connection, TransactionBehavior};

use super::super::super::super::super::{Auth, Metadata};
use super::super::super::retirement::schema::inventory;
use super::*;

const URL: &str = "wss://custody-schema.example/";
const DB_KEY: [u8; 32] = [61; 32];
const UID: u32 = 10_000;
const WALL: u64 = 1_760_000_000;

struct Fixture {
    file: tempfile::NamedTempFile,
    client: Keys,
    origin: Origin,
}

impl Fixture {
    fn new() -> Self {
        Self {
            file: tempfile::NamedTempFile::new().unwrap(),
            client: Keys::generate(),
            origin: super::super::super::super::Route::configured(URL)
                .unwrap()
                .id,
        }
    }

    fn open(&self) -> HardenedPeerConnection {
        HardenedPeerConnection::open(self.file.path(), DB_KEY).unwrap()
    }

    fn provisional(&self) {
        let cursor = CursorStore::attach(
            self.open(),
            self.client.public_key().to_bytes(),
            &[self.origin],
        )
        .unwrap();
        Checkpoint::attach_for_provisional_proof(
            cursor,
            self.client.clone(),
            UID,
            &[URL],
            Policy::default(),
        )
        .unwrap();
    }

    fn prepared(&self) -> Result<Checkpoint> {
        Checkpoint::attach_prepared_consent_fixture(
            self.open(),
            self.client.clone(),
            UID,
            &[URL],
            Policy::default(),
        )
    }

    fn custody(&self) -> Result<Checkpoint> {
        Checkpoint::attach_pending_custody_fixture(
            self.open(),
            self.client.clone(),
            UID,
            &[URL],
            Policy::default(),
        )
    }

    fn retirement(&self) -> Result<CursorStore> {
        Checkpoint::retirement_cursor(
            self.open(),
            self.client.public_key().to_bytes(),
            vec![self.origin],
        )
    }

    fn inventory(&self) -> Vec<super::super::super::retirement::schema::Entry> {
        let HardenedPeerConnection(conn) = self.open();
        inventory(&conn).unwrap()
    }
}

#[test]
fn pending_custody_only_upgrades_prepared_and_rejects_older_owners() {
    let f = Fixture::new();
    let binding = Error::Cursor(CursorError::Binding);
    f.provisional();
    let provisional = f.inventory();
    assert_eq!(f.custody().err(), Some(binding));
    assert_eq!(f.inventory(), provisional);

    drop(f.prepared().unwrap());
    let epoch1 = f.inventory();
    assert_eq!(f.retirement().err(), Some(binding));
    assert_eq!(f.inventory(), epoch1);
    assert_eq!(
        Checkpoint::attach_pending_custody_fixture(
            f.open(),
            f.client.clone(),
            UID,
            &["wss://wrong-custody-origin.example/"],
            Policy::default(),
        )
        .err(),
        Some(binding)
    );
    assert_eq!(f.inventory(), epoch1);
    let HardenedPeerConnection(conn) = f.open();
    conn.execute_batch("CREATE TEMP TABLE shadow(a)").unwrap();
    assert_eq!(
        Checkpoint::attach_pending_custody_fixture(
            HardenedPeerConnection(conn),
            f.client.clone(),
            UID,
            &[URL],
            Policy::default(),
        )
        .err(),
        Some(binding)
    );
    assert_eq!(f.inventory(), epoch1);

    let checkpoint = f.custody().unwrap();
    let epoch2 = f.inventory();
    assert_ne!(epoch2, epoch1);
    assert_eq!(f.prepared().err(), Some(binding));
    assert_eq!(f.retirement().err(), Some(binding));
    assert_eq!(f.inventory(), epoch2);
    let HardenedPeerConnection(external) = f.open();
    assert!(
        external
            .execute("UPDATE peer_pending_custody_epoch SET epoch=2", [])
            .is_err()
    );
    assert!(
        external
            .execute("DELETE FROM peer_pending_custody_epoch", [])
            .is_err()
    );
    drop(checkpoint);
    drop(f.custody().unwrap());
    assert_eq!(f.inventory(), epoch2);

    let retiring = Fixture::new();
    retiring.provisional();
    drop(retiring.retirement().unwrap());
    let retirement = retiring.inventory();
    assert_eq!(retiring.custody().err(), Some(binding));
    assert_eq!(retiring.inventory(), retirement);
}

fn exact_offer(cp: &mut Checkpoint, source: &Source, client: &Keys, server: &Keys) -> Selector {
    let frame = wire::Codec::new(wire::MAX_OUTER_BYTES)
        .unwrap()
        .prepare_offer(
            server,
            client.public_key(),
            [0x51; 16],
            &NostrRoutingV1::new([0xc4; 32], vec![URL.to_owned()]).unwrap(),
            WALL,
        )
        .unwrap();
    let signed: nostr::event::Event = serde_json::from_slice(frame.frame()).unwrap();
    let inserted = cp
        .admit(
            source,
            RawRecord {
                id: signed.id.to_bytes(),
                stamp: signed.created_at.as_secs(),
                bytes: frame.frame(),
            },
            WALL,
        )
        .unwrap();
    assert_eq!(inserted.status, Status::Accepted(TrustedOutcome::Applied));
    Selector::from(&frame.flight)
}

// This prepares a storage row graph without a live native or signed Delivery
// fixture. The integrated test separately uses the real attestation API.
fn seed_exact_outbox(
    cp: &mut Checkpoint,
    source: &Source,
    selector: Selector,
    settle: bool,
) -> Reservation {
    let reservation = cp.reserve_provisioning(source, selector, WALL).unwrap();
    let stage = cp.bind_stage(source, &reservation, WALL).unwrap();
    let witness = FileWitness {
        device: [0x12; 8],
        inode: [0x34; 8],
    };
    cp.start_stage(source, &stage, witness, WALL).unwrap();
    cp.record_prepared(
        source,
        &stage,
        witness,
        Artifact::new([0x25; 32], b"only a storage fixture", WALL + 172_800),
        WALL,
    )
    .unwrap();
    let tx = cp.cursor.with_wall(source.origin, WALL).unwrap();
    let prepared = Candidate::read(
        &tx,
        reservation.stage_binding(),
        cp.signer.public_key().to_bytes(),
        wire::Codec::new(wire::MAX_OUTER_BYTES)
            .unwrap()
            .open_offer(
                &cp.signer,
                &Invitation::load(&tx, selector, source.origin)
                    .unwrap()
                    .frame,
                Some(selector.server),
                &[URL.to_owned()],
                WALL,
            )
            .unwrap(),
    )
    .unwrap();
    let mut out = outbox::Outbox::new(
        &prepared,
        Transcript {
            carrier: [0x83; 32],
            revision: [0x64; 32],
            created_at: WALL,
            expires: WALL + 70_000,
            package_ref: [0x58; 32],
            package_slot: [0x19; 16],
        },
        b"prepared Consent row for schema only",
    );
    out.insert(&tx, &prepared).unwrap();
    if settle {
        outbox::advance(
            &tx,
            &prepared,
            &out,
            outbox::Advance::Reserve {
                attempt: 1,
                retry_at: WALL + 60,
            },
        )
        .unwrap();
        out.attempt = 1;
        out.retry_at = Some(WALL + 60);
        outbox::advance(
            &tx,
            &prepared,
            &out,
            outbox::Advance::Acknowledge { wall: WALL },
        )
        .unwrap();
    }
    tx.commit().unwrap();
    reservation
}

fn held_insert(tx: &Transaction<'_>, consent_revision: [u8; 32]) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO peer_pending_custody
         (class,generation,account,server,offer_id,offer_revision,origin,consent_carrier,
          consent_revision,delivery_carrier,outer_stamp,flight_created,expires,frame,replay_until,
          welcome_id,welcome_digest,package_id,package_digest,package_ref,package_slot,
          package_not_before,package_not_after,mode,held_at,held_deadline,state)
         SELECT class,generation,account,server,offer_id,offer_revision,origin,carrier,
          ?1,?2,?3,?3,?3+3600,CAST('schema fixture bytes' AS BLOB),?3+7200,
          zeroblob(32),zeroblob(32),package_id,package_digest,package_ref,package_slot,
          ?3-600,package_expires,0,?3,?3+3300,0 FROM peer_prepared_consent_outbox",
        params![consent_revision, [0x56u8; 32], seconds(WALL).unwrap()],
    )
}

fn counts(conn: &Connection) -> (i64, i64, i64, i64, i64) {
    conn.query_row(
        "SELECT (SELECT count(*) FROM peer_pending_custody),
                (SELECT count(*) FROM peer_prepared_consent_outbox),
                (SELECT count(*) FROM peer_provisioning_slot WHERE server IS NOT NULL),
                (SELECT count(*) FROM peer_provisional_stage_v1),
                (SELECT count(*) FROM peer_consent_invite WHERE decision_at IS NULL)",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )
    .unwrap()
}

fn probe_cross_owner_denials(conn: &Connection) {
    for (table, column, condition) in [
        ("peer_pending_custody", "state", "1"),
        ("peer_prepared_consent_outbox", "revision", "1"),
        ("peer_provisioning_slot", "server", "server IS NOT NULL"),
        ("peer_provisional_stage_v1", "state", "1"),
        ("peer_consent_invite", "decision_at", "1"),
    ] {
        assert!(
            conn.execute(&format!("DELETE FROM {table} WHERE {condition}"), [])
                .is_err(),
            "{table} deletion was allowed"
        );
        assert!(
            conn.execute(
                &format!("UPDATE {table} SET {column}={column} WHERE {condition}"),
                []
            )
            .is_err(),
            "{table} update was allowed"
        );
    }
    assert_eq!(counts(conn), (1, 1, 1, 1, 1));
}

#[test]
fn pending_custody_exact_graph_and_legacy_sweep_preserve_each_state() {
    let f = Fixture::new();
    f.provisional();
    let mut cp = f.prepared().unwrap();
    let source = cp.bind_source(URL).unwrap();
    let selector = exact_offer(&mut cp, &source, &f.client, &Keys::generate());
    let reservation = seed_exact_outbox(&mut cp, &source, selector, true);
    drop(cp);
    let mut cp = f.custody().unwrap();
    let source = cp.bind_source(URL).unwrap();
    let writer = custody_schema::Writer::Insert;
    let HardenedPeerConnection(outside) = f.open();
    {
        let tx = cp.cursor.with_wall(source.origin, WALL).unwrap();
        assert!(held_insert(&tx, [0x64; 32]).is_err());
        writer.guard(&tx, false).unwrap();
        assert!(held_insert(&tx, [0x62; 32]).is_err());
        assert_eq!(held_insert(&tx, [0x64; 32]).unwrap(), 1);
        // Simulated failed owner turn leaves neither a Held row nor an open aperture.
    }
    assert_eq!(counts(&outside), (0, 1, 1, 1, 1));
    {
        let tx = cp.cursor.with_wall(source.origin, WALL).unwrap();
        assert!(held_insert(&tx, [0x64; 32]).is_err());
        writer.guard(&tx, false).unwrap();
        assert_eq!(held_insert(&tx, [0x64; 32]).unwrap(), 1);
        writer.guard(&tx, true).unwrap();
        tx.commit().unwrap();
    }
    assert!(cp.custody_owned(&source, selector, WALL).unwrap());
    assert_eq!(
        cp.reserve_provisioning(&source, selector, WALL).err(),
        Some(Error::Custodied)
    );
    assert_eq!(
        cp.stage_transaction(&source, reservation.stage_binding(), WALL)
            .err(),
        Some(Error::Custodied)
    );
    let evidence = cp
        .read_custody(&source, selector, 1, WALL)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.state(), CustodyState::Held);
    assert!(cp.daemon_inventory(&source, WALL).unwrap().is_empty());
    assert_eq!(evidence.package_bounds(), (WALL - 600, WALL + 172_800));
    assert!(
        cp.read_custody(&source, selector, 2, WALL)
            .unwrap()
            .is_none()
    );
    probe_cross_owner_denials(&outside);

    assert!(matches!(
        cp.start_custody_join(&source, &evidence, WALL + 3300),
        Err(Error::Expired)
    ));
    let first = cp.start_custody_join(&source, &evidence, WALL + 1).unwrap();
    assert!(first.matches(&evidence));
    assert!(cp.daemon_inventory(&source, WALL + 1).unwrap().is_empty());
    assert!(matches!(
        cp.start_custody_join(&source, &evidence, WALL + 1),
        Err(Error::Cursor(CursorError::Stale))
    ));
    probe_cross_owner_denials(&outside);
    let writer = custody_schema::Writer::Update;
    let tx = cp.cursor.with_wall(source.origin, WALL + 2).unwrap();
    writer.guard(&tx, false).unwrap();
    assert!(
        tx.execute(
            "UPDATE peer_pending_custody SET state=3,approval=0,activating_at=?1",
            [seconds(WALL + 2).unwrap()]
        )
        .is_err()
    );
    assert!(
        tx.execute(
            "UPDATE peer_pending_custody SET state=2,group_id=zeroblob(16),epoch_number=1,
           epoch_identity=zeroblob(32),own_leaf=1,qualified_at=?1,pending_deadline=?2,
           consent_revision=zeroblob(32)",
            params![seconds(WALL + 2).unwrap(), seconds(WALL + 3300).unwrap()],
        )
        .is_err()
    );
    assert_eq!(
        tx.execute(
            "UPDATE peer_pending_custody SET state=2,group_id=zeroblob(16),epoch_number=1,
           epoch_identity=zeroblob(32),own_leaf=1,qualified_at=?1,pending_deadline=?2",
            params![seconds(WALL + 2).unwrap(), seconds(WALL + 3300).unwrap()],
        )
        .unwrap(),
        1
    );
    writer.guard(&tx, true).unwrap();
    tx.commit().unwrap();
    probe_cross_owner_denials(&outside);
    let pending = cp
        .read_custody(&source, selector, 1, WALL + 2)
        .unwrap()
        .unwrap();
    assert_eq!(pending.state(), CustodyState::Pending);
    assert_eq!(pending.pending_deadline(), Some(WALL + 3300));
    let inventory = cp.daemon_inventory(&source, WALL + 2).unwrap();
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].key, selector);
    assert_eq!(inventory[0].generation, 1);
    assert_eq!(inventory[0].state, CustodyState::Pending);
    assert_eq!(inventory[0].qualified_at, WALL + 2);
    assert_eq!(inventory[0].expires_at, WALL + 3300);
    assert_eq!(inventory[0].activating_at, None);
    let tx = cp.cursor.with_wall(source.origin, WALL + 3).unwrap();
    writer.guard(&tx, false).unwrap();
    assert!(
        tx.execute(
            "UPDATE peer_pending_custody SET state=3,approval=1,activating_at=?1",
            [seconds(WALL + 3).unwrap()]
        )
        .is_err()
    );
    assert_eq!(
        tx.execute(
            "UPDATE peer_pending_custody SET state=3,approval=0,activating_at=?1",
            [seconds(WALL + 3).unwrap()]
        )
        .unwrap(),
        1
    );
    writer.guard(&tx, true).unwrap();
    tx.commit().unwrap();
    probe_cross_owner_denials(&outside);

    assert_eq!(
        f.prepared().err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    assert_eq!(
        f.retirement().err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    probe_cross_owner_denials(&outside);

    // The fixture holds an exact byte tuple without using the signed ingress
    // parser. Exercise only the storage-side replay and deferred apertures.
    let raw = RawRecord {
        id: [0x56; 32],
        stamp: WALL,
        bytes: b"schema fixture bytes",
    };
    let expired = WALL + 3600;
    cp.cursor
        .defer(
            source.origin,
            raw,
            Metadata {
                auth: Auth::FlightVerified,
                expires: expired + 200,
                prerequisite: Some([0x83; 32]),
            },
            expired,
        )
        .unwrap();
    assert!(matches!(
        cp.custody_replay(
            &source,
            RawRecord {
                bytes: b"different raw record",
                ..raw
            },
            expired
        )
        .unwrap(),
        Replay::Invalid(_)
    ));
    assert!(cp.cursor.due(source.origin, expired + 1).unwrap().is_some());
    assert!(matches!(
        cp.custody_replay(&source, raw, expired + 1).unwrap(),
        Replay::AlreadyApplied(_)
    ));
    assert!(cp.cursor.due(source.origin, expired + 1).unwrap().is_none());
    assert!(matches!(
        cp.custody_replay(&source, raw, WALL + 7200).unwrap(),
        Replay::Invalid(_)
    ));
    let after = WALL + 1_000_000;
    let inventory = cp.daemon_inventory(&source, after).unwrap();
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].state, CustodyState::Activating);
    assert_eq!(inventory[0].activating_at, Some(WALL + 3));
    assert_eq!(inventory[0].held_at, WALL);
    assert_eq!(inventory[0].expires_at, WALL + 3300);
    let tx = cp.cursor.with_wall(source.origin, after).unwrap();
    rows::sweep(&tx, after).unwrap();
    tx.commit().unwrap();
    probe_cross_owner_denials(&outside);
    drop(cp);
    assert_eq!(counts(&f.custody().unwrap().cursor.conn), (1, 1, 1, 1, 1));

    // Model damaged disk contents under an otherwise byte-identical inventory.
    // The checkpoint must reject instead of minting recoverable authority.
    let mut outside = outside;
    let tx = outside
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let mut guards = Vec::new();
    for name in [
        "peer_custody_next",
        "peer_custody_reference_UPDATE",
        "peer_custody_deny_UPDATE",
    ] {
        let ddl: String = tx
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .unwrap();
        tx.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
        guards.push(ddl);
    }
    tx.execute(
        "UPDATE peer_pending_custody SET consent_revision=zeroblob(32)",
        [],
    )
    .unwrap();
    for ddl in guards {
        tx.execute_batch(&ddl).unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(f.custody().err(), Some(Error::Cursor(CursorError::Binding)));
}

#[test]
fn daemon_due_skips_ineligible_classes_and_advances_past_a_stuck_reservation() {
    use super::daemon::{Due, Rotation};

    let f = Fixture::new();
    f.provisional();
    drop(f.prepared().unwrap());
    let mut cp = f.custody().unwrap();
    let source = cp.bind_source(URL).unwrap();
    let old = exact_offer(&mut cp, &source, &f.client, &Keys::generate());
    let extra = exact_offer(&mut cp, &source, &f.client, &Keys::generate());
    let mut rotation = Rotation::default();
    let due = |cp: &mut Checkpoint, rotation: &mut Rotation, online, server| {
        cp.daemon_due(&source, WALL, rotation, online, server)
            .unwrap()
    };
    assert_eq!(
        due(&mut cp, &mut rotation, true, old.server),
        Some(Due::Provision(old))
    );
    cp.reserve_provisioning(&source, old, WALL).unwrap();
    assert_eq!(
        due(&mut cp, &mut rotation, true, old.server),
        Some(Due::Resume(old))
    );
    // A fresh general Offer cannot take the occupied slot. A different
    // eligible class can advance and publish while that reservation stays.
    assert_eq!(due(&mut cp, &mut rotation, true, extra.server), None);
    let reserved = Keys::generate();
    cp.policy.reserved.insert(reserved.public_key());
    let fresh = exact_offer(&mut cp, &source, &f.client, &reserved);
    assert_eq!(
        due(&mut cp, &mut rotation, true, fresh.server),
        Some(Due::Provision(fresh))
    );
    seed_exact_outbox(&mut cp, &source, fresh, false);
    assert_eq!(due(&mut cp, &mut rotation, false, fresh.server), None);
    assert_eq!(
        due(&mut cp, &mut rotation, true, fresh.server),
        Some(Due::Publish(fresh))
    );
    assert_eq!(
        due(&mut cp, &mut rotation, true, old.server),
        Some(Due::Resume(old))
    );
    assert!(cp.daemon_inventory(&source, WALL).unwrap().is_empty());
}
