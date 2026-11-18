use super::super::tests::{Fixture, UID, URL, WALL, joined_pending};
use super::*;

fn publication_checkpoint(fixture: &Fixture) -> Result<Checkpoint> {
    Checkpoint::attach_publication_fixture(
        fixture.open(),
        fixture.client.clone(),
        UID,
        URL,
        Policy::default(),
    )
}

fn activating_fixture(fixture: &Fixture) -> (Checkpoint, Source, Selector) {
    let (mut cp, source, selector) = joined_pending(fixture, &Keys::generate());
    let tx = cp.cursor.with_wall(source.origin, WALL + 3).unwrap();
    let writer = custody_schema::Writer::Update;
    writer.guard(&tx, false).unwrap();
    tx.execute(
        "UPDATE peer_pending_custody SET state=3,approval=0,activating_at=qualified_at+1",
        [],
    )
    .unwrap();
    writer.guard(&tx, true).unwrap();
    rows::advance_wall(&tx, WALL + 3).unwrap();
    tx.commit().unwrap();
    drop(cp);
    let cp = publication_checkpoint(fixture).unwrap();
    let source = cp.bind_source(URL).unwrap();
    (cp, source, selector)
}

fn held(cp: &mut Checkpoint, source: &Source, selector: Selector, wall: u64) -> HeldEvidence {
    cp.read_custody(source, selector, 1, wall).unwrap().unwrap()
}

fn pin(ordinal: u64) -> Pin {
    Pin {
        ordinal,
        authenticator: [u8::try_from(ordinal).unwrap(); 32],
        recipient_leaf: 1,
    }
}

#[test]
fn epoch_four_upgrades_only_exact_epoch_three_and_refuses_older_owner_reopen() {
    let fixture = Fixture::new();
    let binding = Error::Cursor(CursorError::Binding);
    fixture.provisional();
    assert_eq!(publication_checkpoint(&fixture).err(), Some(binding));
    drop(fixture.prepared().unwrap());
    assert_eq!(publication_checkpoint(&fixture).err(), Some(binding));
    drop(fixture.custody().unwrap());
    let cp = publication_checkpoint(&fixture).unwrap();
    let HardenedPeerConnection(external) = fixture.open();
    assert_eq!(
        external
            .query_row("SELECT epoch FROM peer_publication_epoch", [], |row| row
                .get::<_, u8>(0))
            .unwrap(),
        4
    );
    assert!(
        external
            .execute("UPDATE peer_publication_epoch SET epoch=3", [])
            .is_err()
    );
    assert!(
        external
            .execute("DELETE FROM peer_publication_epoch", [])
            .is_err()
    );
    assert_eq!(fixture.custody().err(), Some(binding));
    drop(cp);
    drop(publication_checkpoint(&fixture).unwrap());
    external
        .execute_batch("CREATE TABLE unexpected_epoch_four_sibling(i INTEGER) STRICT")
        .unwrap();
    assert_eq!(publication_checkpoint(&fixture).err(), Some(binding));
}

#[test]
fn durable_attempts_are_bounded_and_active_reservation_is_atomic_and_replayable() {
    let fixture = Fixture::new();
    let (mut cp, source, selector) = activating_fixture(&fixture);
    let wall = WALL + 4;
    let evidence = held(&mut cp, &source, selector, wall);
    let scope = Enrollment::from_evidence(&evidence, fixture.client.public_key())
        .unwrap()
        .scope;
    let stale = Error::Cursor(CursorError::Stale);
    let operation = [0x98; 32];
    assert_eq!(
        cp.reserve_postjoin_coordinates(&source, &evidence, &scope, &operation, pin(3), wall)
            .err(),
        Some(stale)
    );
    let reserved = cp
        .reserve_postjoin_coordinates(&source, &evidence, &scope, &operation, pin(2), wall)
        .unwrap();
    assert_eq!(
        cp.reserve_postjoin_coordinates(&source, &evidence, &scope, &operation, pin(2), wall)
            .err(),
        Some(stale)
    );
    assert_eq!(reserved.step(), Step::Reserved);
    assert_eq!(reserved.generation(), 1);
    assert_eq!(reserved.founder(), selector.server);
    assert_eq!(reserved.recipient(), fixture.client.public_key());
    assert_eq!(reserved.origin(), URL);
    assert_eq!(reserved.group(), [0; 16]);
    assert_eq!(reserved.route(), [0xc4; 32]);
    assert!(reserved.stage() == evidence.stage());
    assert_eq!(
        cp.arm_publication(&source, &reserved, wall).err(),
        Some(stale)
    );
    assert!(
        cp.read_receipt_reservation(&source, &evidence, wall)
            .unwrap()
            .is_none()
    );

    let candidate = CandidateBinding {
        event_id: [0x44; 32],
        signed_digest: [0x32; 32],
    };
    let captured = cp
        .update_publication(
            &source,
            &reserved,
            Change::Capture(candidate, Some(pin(3))),
            wall,
        )
        .unwrap();
    assert_eq!(captured.step(), Step::Captured);
    assert_eq!(captured.candidate(), Some(candidate));

    let attempt = cp.arm_publication(&source, &captured, wall + 1).unwrap();
    assert_eq!(attempt.publication().step(), Step::Armed { number: 1 });
    assert_eq!(attempt.publication().attempt_wall(), Some(wall + 1));
    cp.check_armed(&source, &attempt, wall + 1).unwrap();
    drop(cp);

    let mut cp = publication_checkpoint(&fixture).unwrap();
    let source = cp.bind_source(URL).unwrap();
    let evidence = held(&mut cp, &source, selector, wall + 2);
    let armed = cp
        .publication(&source, &evidence, Kind::PostJoin, wall + 2)
        .unwrap()
        .unwrap();
    assert_eq!(armed.step(), Step::Armed { number: 1 });
    assert_eq!(armed.candidate(), Some(candidate));
    assert_eq!(cp.check_armed(&source, &attempt, wall + 2), Err(stale));
    assert_eq!(
        cp.arm_publication(&source, &armed, wall + 2).err(),
        Some(stale)
    );
    let uncertain = cp
        .record_publication_uncertain(&source, &armed, wall + 2)
        .unwrap();
    assert_eq!(uncertain.attempt_wall(), Some(wall + 1));
    let uncertain = cp
        .read_uncertain_postjoin(&source, &evidence, wall + 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        uncertain.publication().step(),
        Step::Uncertain { number: 1 }
    );
    assert_eq!(
        cp.arm_publication(&source, uncertain.publication(), wall + 3)
            .err(),
        Some(stale)
    );
    let second = cp
        .arm_exact(&source, uncertain.publication(), wall + 3)
        .unwrap();
    assert_eq!(second.publication().step(), Step::Armed { number: 2 });
    assert_eq!(second.publication().attempt_wall(), Some(wall + 3));
    assert_eq!(second.publication().candidate(), Some(candidate));
    let twice_uncertain = cp
        .record_publication_uncertain(&source, second.publication(), wall + 4)
        .unwrap();
    assert_eq!(
        cp.arm_exact(&source, &twice_uncertain, wall + 4).err(),
        Some(stale)
    );
    assert!(
        cp.read_uncertain_postjoin(&source, &evidence, wall + 4)
            .unwrap()
            .is_none()
    );
    let mut wrong = candidate;
    wrong.signed_digest = [0x09; 32];
    assert_eq!(
        cp.update_publication(&source, &twice_uncertain, Change::Confirm(wrong), wall + 4)
            .err(),
        Some(stale)
    );

    cp.update_publication(
        &source,
        &twice_uncertain,
        Change::Confirm(candidate),
        wall + 4,
    )
    .unwrap();
    let rotation = RotationCoordinates {
        scope,
        obligation: operation.to_vec(),
        candidate,
        source: pin(2),
        confirmed: pin(3),
    };
    let deadline = evidence.pending_deadline().unwrap();
    assert_eq!(
        cp.activate_and_reserve_coordinates(&source, &evidence, &rotation, deadline, |_| Ok(()))
            .err(),
        Some(Error::Expired)
    );

    // Force an error after Active's CAS but before receipt reservation commits.
    assert_eq!(
        cp.activate_and_reserve_coordinates(&source, &evidence, &rotation, wall + 4, |tx| {
            assert!(activation_schema::active_exists(
                tx,
                rotation.scope.carrier
            )?);
            Err(Error::Bound)
        })
        .err(),
        Some(Error::Bound)
    );
    assert!(
        cp.read_receipt_reservation(&source, &evidence, wall + 4)
            .unwrap()
            .is_none()
    );

    let receipt_stamp = wall + 4;
    let active = cp
        .activate_and_reserve_coordinates(&source, &evidence, &rotation, receipt_stamp, |_| Ok(()))
        .unwrap();
    assert_eq!(active.publication().kind(), Kind::AcceptReceipt);
    assert_eq!(active.publication().step(), Step::Reserved);
    let prepared_id = active.prepared().event_id();
    let payload = active.prepared().payload().to_owned();
    assert_eq!(active.publication().semantic_id(), prepared_id);
    assert!(
        cp.daemon_inventory(&source, receipt_stamp, selector.server)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        cp.activate_and_reserve_coordinates(&source, &evidence, &rotation, wall + 4, |_| Ok(()))
            .err(),
        Some(stale)
    );

    drop(cp);
    let mut cp = publication_checkpoint(&fixture).unwrap();
    let source = cp.bind_source(URL).unwrap();
    let evidence = held(&mut cp, &source, selector, receipt_stamp + 1);
    assert!(evidence.is_active());
    let receipt = cp
        .read_receipt_reservation(&source, &evidence, receipt_stamp + 1)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.prepared().event_id(), prepared_id);
    assert_eq!(receipt.prepared().payload(), payload);
    assert_eq!(
        receipt_wire::decode(&payload).unwrap().created_at,
        receipt_stamp
    );
    assert_eq!(
        cp.arm_publication(&source, receipt.publication(), receipt_stamp + 1)
            .err(),
        Some(stale)
    );
    let started = cp
        .start_native_receipt(&source, receipt.publication(), receipt_stamp + 1)
        .unwrap();
    assert_eq!(started.publication().step(), Step::NativeStarted);
    assert_eq!(
        cp.start_native_receipt(&source, started.publication(), receipt_stamp + 1)
            .err(),
        Some(stale)
    );
    let receipt_candidate = CandidateBinding {
        event_id: [0xb1; 32],
        signed_digest: [0xaf; 32],
    };
    let captured = cp
        .update_publication(
            &source,
            started.publication(),
            Change::Capture(receipt_candidate, None),
            receipt_stamp + 2,
        )
        .unwrap();
    assert!(
        cp.start_native_receipt(&source, &captured, receipt_stamp + 2)
            .is_err()
    );
    let sent = cp
        .arm_publication(&source, &captured, receipt_stamp + 2)
        .unwrap();
    let published = cp
        .update_publication(
            &source,
            sent.publication(),
            Change::Confirm(receipt_candidate),
            receipt_stamp + 2,
        )
        .unwrap();
    assert_eq!(published.step(), Step::Published { number: 1 });
}

#[test]
fn founder_accepts_only_one_durable_exact_source_and_refuses_late_first_receipt() {
    let fixture = Fixture::new();
    fixture.provisional();
    drop(fixture.prepared().unwrap());
    drop(fixture.custody().unwrap());
    let mut cp = publication_checkpoint(&fixture).unwrap();
    let source = cp.bind_source(URL).unwrap();
    let receipt = Receipt {
        founder: fixture.client.public_key(),
        recipient: Keys::generate().public_key(),
        room_id: [1; 16],
        offer_id: [2; 16],
        offer_revision: [3; 32],
        welcome_id: [4; 32],
        epoch: u64::MAX,
        epoch_identity: [5; 32],
    };
    let prepared = receipt_wire::prepare(&receipt, WALL).unwrap();
    let mut exact = ReceiptSourceCoordinates {
        decoded: receipt_wire::decode(prepared.payload()).unwrap(),
        group: vec![3; 16],
        route: [6; 32],
        sender_leaf: 1,
        native_id: vec![7; 32],
        deadline: WALL + 10,
    };
    assert_eq!(
        cp.accept_receipt_coordinates(&source, &exact, WALL + 1)
            .err(),
        Some(Error::Cursor(CursorError::Binding))
    );
    let mut enrollment = FounderEnrollment {
        receipt,
        group: exact.group.clone(),
        route: exact.route,
        leaf: exact.sender_leaf,
        valid_from: WALL,
        deadline: exact.deadline,
    };
    let binding = Some(Error::Cursor(CursorError::Binding));
    assert_eq!(
        cp.record_founder_enrollment(&source, &enrollment, WALL - 1)
            .err(),
        binding
    );
    assert_eq!(
        cp.record_founder_enrollment(&source, &enrollment, exact.deadline)
            .err(),
        binding
    );
    cp.record_founder_enrollment(&source, &enrollment, WALL)
        .unwrap();
    cp.record_founder_enrollment(&source, &enrollment, WALL)
        .unwrap();
    enrollment.leaf ^= 1;
    assert_eq!(
        cp.record_founder_enrollment(&source, &enrollment, WALL + 1)
            .err(),
        Some(Error::Cursor(CursorError::Stale))
    );

    let source_mismatches: [fn(&mut ReceiptSourceCoordinates); 4] = [
        |candidate| candidate.group[0] ^= 1,
        |candidate| candidate.route[0] ^= 1,
        |candidate| candidate.sender_leaf ^= 1,
        |candidate| candidate.deadline -= 1,
    ];
    let check_refused = |cp: &mut Checkpoint, candidate: &ReceiptSourceCoordinates| {
        assert_eq!(
            cp.accept_receipt_coordinates(&source, candidate, WALL + 1)
                .err(),
            Some(Error::Cursor(CursorError::Binding))
        );
    };
    for mismatch in source_mismatches {
        mismatch(&mut exact);
        check_refused(&mut cp, &exact);
        exact.group = enrollment.group.clone();
        exact.route = enrollment.route;
        exact.sender_leaf = enrollment.leaf ^ 1;
        exact.deadline = enrollment.deadline;
    }
    for (changed, authored_at) in [
        (
            Receipt {
                epoch: receipt.epoch - 1,
                ..receipt
            },
            WALL,
        ),
        (
            Receipt {
                epoch_identity: [9; 32],
                ..receipt
            },
            WALL,
        ),
        (receipt, WALL - 1),
    ] {
        let candidate = receipt_wire::prepare(&changed, authored_at).unwrap();
        exact.decoded = receipt_wire::decode(candidate.payload()).unwrap();
        check_refused(&mut cp, &exact);
    }
    exact.decoded = receipt_wire::decode(prepared.payload()).unwrap();
    assert_eq!(
        cp.accept_receipt_coordinates(&source, &exact, WALL + 10)
            .err(),
        Some(Error::Expired)
    );
    let accepted = cp
        .accept_receipt_coordinates(&source, &exact, WALL + 1)
        .unwrap();
    assert_eq!(accepted.accepted_at(), WALL + 1);
    assert_eq!(accepted.source().decoded.receipt.epoch, u64::MAX);
    drop(cp);

    let mut cp = publication_checkpoint(&fixture).unwrap();
    let source = cp.bind_source(URL).unwrap();
    let accepted = cp
        .read_founder_acceptance(&source, receipt.recipient, WALL + 10)
        .unwrap();
    assert_eq!(accepted.accepted_at(), WALL + 1);
    exact.native_id = vec![8; 32];
    assert_eq!(
        cp.accept_receipt_coordinates(&source, &exact, WALL + 10)
            .err(),
        Some(Error::Cursor(CursorError::Stale))
    );
}
