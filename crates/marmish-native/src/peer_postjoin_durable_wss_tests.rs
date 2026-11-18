//! Inspect and retry an exact native PostJoin through a private WSS crash cut.

use super::*;
use crate::peer_invitation::{
    postjoin_gate::{Permit, Scope},
    postjoin_verify as verify,
    publication_socket::ReservedSocket,
    store::consent_checkpoint::provisioning::prepared_consent::custody::publication::{
        Kind, Publication, Step,
    },
};
use std::path::PathBuf;

const DURABLE_DEADLINE: Duration = Duration::from_secs(135);

#[test]
#[ignore = "requires native staged SQLCipher and two independent private WSS accounts"]
fn private_wss_recovers_lost_postjoin_ack_without_another_native_send() {
    run_native(DURABLE_DEADLINE, prove_lost_ack);
}

async fn prove_lost_ack() {
    let group_route = random();
    let mut round = Round::initialize_with_group(Some(group_route)).await;
    let setup = prepare_history(&round, group_route, 600).await;
    let mut founder = setup.founder;
    let stage = Stage {
        selector: setup.selector,
        generation: setup.generation,
        delivery: setup.delivery,
        group: setup.group,
        routing: setup.routing,
        welcome: setup.welcome,
    };
    let wall = || round.wall.load(Ordering::Acquire);
    let origin = round.relay.url.as_str();
    let mut lease = ProfileLease::acquire(round.root.path(), PROFILE).unwrap();
    let material = material(&mut lease, &round.daemon);
    let route = Route::configured(origin).unwrap();
    let open_owner = || {
        PreparedConsentOwner::open_publication(
            &lease,
            &material,
            crate::command_config::Role::Client,
            Policy {
                mode: Mode::Automatic,
                ..Policy::default()
            },
            &route,
            Wall::Fixture(round.wall.clone()),
        )
        .unwrap()
    };
    let open_custody = || {
        PreparedConsentOwner::open_custody(
            &lease,
            &material,
            crate::command_config::Role::Client,
            Policy {
                mode: Mode::Automatic,
                ..Policy::default()
            },
            &route,
            Wall::Fixture(round.wall.clone()),
        )
    };
    let before = {
        let mut owner = open_custody().unwrap();
        read_joined(
            &mut owner,
            stage.selector,
            stage.generation,
            wall(),
            &stage.delivery,
            &stage.group,
        )
        .stage()
    };
    let mut owner = open_owner();
    let held = stage.read(&mut owner, wall());
    let (binding, witness) = held.stage();
    assert_eq!((binding, witness), before);
    let image = checked_image(&owner, &held);
    let deadline = held.pending_deadline().unwrap();
    let qualify = || {
        QualifiedOrigin::for_daemon_peer_fixture(
            round.relay.peer_mode().unwrap(),
            lease.root(),
            round.signer,
            round.founder.public_key(),
        )
        .unwrap()
    };
    let founder_connector = qualify().connector;
    let recipient_connector = qualify().connector;

    let mut founder_socket = Owner::open(
        &founder_connector,
        origin,
        round.founder.public_key(),
        &round.founder,
        Instant::now() + WAIT,
    )
    .await
    .unwrap();
    let founder_page = founder_socket
        .group_history(group_route, Instant::now() + STEP)
        .await
        .unwrap();
    assert!(founder_socket.accepts_group_page(&founder_page));
    assert!(founder_page.records().is_empty());
    assert!(
        round
            .relay
            .group_history_cut(round.founder.public_key(), founder_page.request())
            .unwrap()
            .records
            .is_empty()
    );
    assert_eq!(stable_pin(founder.runtime(), &stage.group), setup.next);

    let recipient_signer = material.signer();
    let open_socket = || {
        Owner::open(
            &recipient_connector,
            origin,
            round.signer,
            &recipient_signer,
            Instant::now() + WAIT,
        )
    };
    let old_gate = Gate::unscoped_closed(round.wall.clone());
    let mut recipient = stage.open(&owner, &held, &round.wall, old_gate.clone());
    let mut recipient_socket = open_socket().await.unwrap();
    assert_eq!(stable_pin(&recipient, &stage.group), setup.initial);
    let history_started = Instant::now();
    let mut fetch = None;
    let page = recipient_socket
        .group_history_after_install(group_route, Instant::now() + STEP, |_| {
            recipient
                .mark_post_join_subscription_installed(&stage.group)
                .map_err(|_| Failure::Binding)?;
            fetch = recipient
                .start_post_join_history_fetch(&stage.group)
                .map_err(|_| Failure::Binding)?;
            Ok(())
        })
        .await
        .unwrap();
    let cut = round
        .relay
        .group_history_cut(round.signer, page.request())
        .unwrap();
    let [founder_write] = setup.writes.as_slice() else {
        panic!("one initial distinct Group publication");
    };
    assert_eq!(cut.records, [founder_write.record]);
    assert!(
        process_history(
            &recipient_socket,
            &page,
            &mut recipient,
            &stage.group,
            &cut,
            wall(),
            setup.next,
        )
        .await
    );
    match_stable_roster(&recipient, founder.runtime(), &stage.group, &stage.routing);
    assert_eq!(
        recipient
            .finish_post_join_history_fetch(&stage.group, &fetch.unwrap())
            .unwrap()
            .unwrap()
            .outcome,
        PostJoinHistoryFetchOutcome::Fetched
    );
    recipient.mark_post_join_eose(&stage.group).unwrap();
    // The authenticated intervening founder commit restarted native quiet.
    // One sweep reaches Jitter; the next could author the native update.
    assert!(matches!(
        recipient.maintenance_obligations_for_group(&stage.group).unwrap().as_slice(),
        [obligation] if obligation.phase == cgka_traits::maintenance::MaintenancePhase::Quiet
    ));
    recipient.run_due_maintenance().await.unwrap();
    assert!(matches!(
        recipient.maintenance_obligations_for_group(&stage.group).unwrap().as_slice(),
        [obligation] if obligation.phase == cgka_traits::maintenance::MaintenancePhase::Jitter
    ));
    // MDK also tries to publish a prepared KeyPackage during maintenance.
    // The dedicated deny-only handler cannot reach any relay.
    assert_eq!(
        old_gate.observe().unwrap().refused_key_package_publications,
        1
    );
    assert_gate(&old_gate, 0, 0, false);
    let history_ms = history_started.elapsed().as_millis();
    let baseline = verify::pre_capture(
        &recipient,
        founder.runtime(),
        verify::Scope {
            image: &image,
            inode: (
                u64::from_be_bytes(witness.device),
                u64::from_be_bytes(witness.inode),
            ),
            group: &stage.group,
            account: round.signer,
            founder: round.founder.public_key(),
            route: &stage.routing,
            generation: stage.generation,
            deadline,
            welcome: &stage.welcome,
        },
        wall(),
    )
    .unwrap();
    let scope = Scope::new(
        recipient.self_id(),
        stage.group.clone(),
        group_route,
        TransportEndpoint(origin.into()),
        stage.generation,
        deadline,
        baseline.obligation_id().clone(),
    )
    .unwrap();
    let reservation = owner
        .state
        .checkpoint
        .reserve_postjoin(&owner.state.source, &held, &baseline, wall())
        .unwrap();
    assert_eq!(reservation.step(), Step::Reserved);
    old_gate.begin_capture(scope.clone()).unwrap();
    recipient.run_due_maintenance().await.unwrap();
    assert_gate(&old_gate, 1, 0, false);
    assert_eq!(round.relay.group_writes(), setup.writes);
    // This cut precedes application capture. Recover only native frozen rows.
    drop((
        old_gate,
        recipient,
        owner,
        baseline,
        reservation,
        scope,
        image,
    ));
    let reopen_started = Instant::now();
    let mut owner = open_owner();
    let held = stage.read(&mut owner, wall());
    assert_eq!(held.stage(), (binding, witness));
    let image = checked_image(&owner, &held);
    let reservation = read_publication(&mut owner, &held, wall(), Step::Reserved);
    let gate = Gate::closed(publication_scope(&reservation), round.wall.clone());
    let recipient = stage.open(&owner, &held, &round.wall, gate.clone());
    let initial =
        verify::recover_unexposed(&recipient, founder.runtime(), &image, &reservation, wall())
            .unwrap();
    let publication = owner
        .state
        .checkpoint
        .capture_postjoin(
            &owner.state.source,
            &reservation,
            initial.baseline(),
            initial.capture(),
            wall(),
        )
        .unwrap();
    assert_eq!(publication.step(), Step::Captured);
    let reopen_ms = reopen_started.elapsed().as_millis();
    // The second cut follows durable capture. Exact native readback and the
    // application journal must still agree before the first physical turn.
    assert_gate(&gate, 0, 0, false);
    drop((
        gate,
        recipient,
        owner,
        initial,
        reservation,
        publication,
        image,
    ));
    let mut owner = open_owner();
    let held = stage.read(&mut owner, wall());
    let image = checked_image(&owner, &held);
    let publication = read_publication(&mut owner, &held, wall(), Step::Captured);
    let scope = publication_scope(&publication);
    let deadline = publication.deadline();
    let gate = Gate::closed(scope.clone(), round.wall.clone());
    let mut recipient = stage.open(&owner, &held, &round.wall, gate.clone());
    let recovered =
        verify::recover_unexposed(&recipient, founder.runtime(), &image, &publication, wall())
            .unwrap();
    assert_gate(&gate, 0, 0, false);
    let fresh = recovered.capture();

    let event_id = fresh.event_id();
    let signed_digest = fresh.signed_digest();
    let target = *fresh.target();
    let lost_started = Instant::now();
    round.relay.group_fault(GroupFault::LoseNextAck);
    let recipient_socket = Arc::new(Mutex::new(recipient_socket));
    let reserved = ReservedSocket::reserve(
        recipient_socket.clone(),
        &publication,
        round.wall.clone(),
        Instant::now() + STEP,
    )
    .await
    .unwrap();
    let attempt = owner
        .state
        .checkpoint
        .arm_publication(&owner.state.source, &publication, wall())
        .unwrap();
    let turn = reserved
        .bind(
            &mut owner.state.checkpoint,
            &owner.state.source,
            attempt,
            fresh.request(),
            Kind::PostJoin,
        )
        .unwrap();
    gate.arm(
        Permit::verify_recovery(&scope, &recovered).unwrap(),
        fresh,
        Some(turn.into_postjoin_sink().unwrap()),
    )
    .unwrap();
    advance_retry(&recipient, &stage.group, &round.wall, deadline);
    let (native, founder_record) = tokio::join!(
        recipient.run_due_maintenance(),
        founder_socket.next_group_record(Instant::now() + STEP),
    );
    native.unwrap();
    let founder_record = founder_record.unwrap();
    assert!(founder_socket.accepts_group_record(&founder_record));
    assert_eq!(founder_record.fingerprint().id, event_id);
    assert_gate(&gate, 1, 1, false);
    assert_eq!(round.relay.group_writes().len(), 2);
    let exposed = &round.relay.group_writes()[1];
    assert_eq!(exposed.sender, round.signer);
    assert_eq!(exposed.record, founder_record.fingerprint());
    assert!(!exposed.duplicate && !exposed.acknowledged);
    let lost_ms = lost_started.elapsed().as_millis();

    // Reopen both checked stores with a fresh gate. Seeing only the lost ACK
    // journal is insufficient until the independent founder has processed it.
    drop((
        gate,
        recipient,
        recipient_socket,
        owner,
        recovered,
        publication,
        scope,
        image,
    ));
    assert!(open_custody().is_err());
    let recover_started = Instant::now();
    let mut owner = open_owner();
    let held = stage.read(&mut owner, wall());
    let image = checked_image(&owner, &held);
    let armed = read_publication(&mut owner, &held, wall(), Step::Armed { number: 1 });
    owner
        .state
        .checkpoint
        .record_publication_uncertain(&owner.state.source, &armed, wall())
        .unwrap();
    let uncertain = owner
        .state
        .checkpoint
        .read_uncertain_postjoin(&owner.state.source, &held, wall())
        .unwrap()
        .unwrap();
    let scope = publication_scope(uncertain.publication());
    let deadline = uncertain.publication().deadline();
    let gate = Gate::closed(scope.clone(), round.wall.clone());
    let mut recipient = stage.open(&owner, &held, &round.wall, gate.clone());
    let recover = |recipient: &Recipient, founder: &PeerFounderRuntime<FounderTransport>, wall| {
        verify::recover_exposed(recipient, founder.runtime(), &image, &uncertain, wall)
    };
    assert!(recover(&recipient, &founder, wall()).is_err());
    let delivery = founder_record.delivery(&stage.group, wall());
    assert!(
        founder
            .converge_exact_delivery(&stage.group, &target, &delivery)
            .await
    );
    assert!(recover(&recipient, &founder, deadline).is_err());
    assert_gate(&gate, 0, 0, false);
    assert_eq!(round.relay.group_writes().len(), 2);
    let recovered = recover(&recipient, &founder, wall()).unwrap();
    let retry = recovered.capture();
    assert_eq!(retry.event_id(), event_id);
    assert_eq!(retry.signed_digest(), signed_digest);
    let publication = read_publication(&mut owner, &held, wall(), Step::Uncertain { number: 1 });
    let retry_socket = Arc::new(Mutex::new(open_socket().await.unwrap()));
    let reserved = ReservedSocket::reserve(
        retry_socket.clone(),
        &publication,
        round.wall.clone(),
        Instant::now() + STEP,
    )
    .await
    .unwrap();
    let attempt = owner
        .state
        .checkpoint
        .arm_recovered_postjoin(&owner.state.source, &uncertain, &recovered, wall())
        .unwrap();
    let turn = reserved
        .bind(
            &mut owner.state.checkpoint,
            &owner.state.source,
            attempt,
            retry.request(),
            Kind::PostJoin,
        )
        .unwrap();
    gate.arm(
        Permit::verify_recovery(&scope, &recovered).unwrap(),
        retry,
        Some(turn.into_postjoin_sink().unwrap()),
    )
    .unwrap();
    advance_retry(&recipient, &stage.group, &round.wall, deadline);
    recipient.run_due_maintenance().await.unwrap();
    assert_gate(&gate, 1, 1, true);
    // Finish native bookkeeping under the spent permit before dropping every
    // recipient handle and proving the canonical native result from disk.
    let armed = read_publication(&mut owner, &held, wall(), Step::Armed { number: 2 });
    if verify::confirm_rotation_cold(&recipient, founder.runtime(), &image, &armed, wall()).is_err()
    {
        recipient.run_due_maintenance().await.unwrap();
    }
    assert_gate(&gate, 1, 1, true);
    assert_eq!(
        stable_pin(&recipient, &stage.group),
        stable_pin(founder.runtime(), &stage.group),
    );
    match_stable_roster(&recipient, founder.runtime(), &stage.group, &stage.routing);
    let writes = round.relay.group_writes();
    assert_eq!(writes.len(), 3);
    assert_eq!(writes[2].sender, round.signer);
    assert_eq!(writes[2].record, writes[1].record);
    assert!(writes[2].duplicate && writes[2].acknowledged);
    let recover_ms = recover_started.elapsed().as_millis();

    drop((
        gate,
        recipient,
        owner,
        recovered,
        uncertain,
        publication,
        armed,
        scope,
        image,
    ));
    let mut owner = open_owner();
    let held = stage.read(&mut owner, wall());
    assert_eq!(held.stage(), (binding, witness));
    let image = checked_image(&owner, &held);
    let armed = read_publication(&mut owner, &held, wall(), Step::Armed { number: 2 });
    let gate = Gate::closed(publication_scope(&armed), round.wall.clone());
    let mut recipient = stage.open(&owner, &held, &round.wall, gate.clone());
    let rotation =
        verify::confirm_rotation_cold(&recipient, founder.runtime(), &image, &armed, wall())
            .unwrap();
    let confirmed = owner
        .state
        .checkpoint
        .confirm_recovered_postjoin(&owner.state.source, &rotation, wall())
        .unwrap();
    assert_eq!(confirmed.step(), Step::Published { number: 2 });
    verify::confirm_rotation_cold(&recipient, founder.runtime(), &image, &confirmed, wall())
        .unwrap();
    recipient.resume_outbound_fanouts().await.unwrap();
    assert_gate(&gate, 0, 0, false);
    assert_eq!(round.relay.group_writes(), writes);
    founder_socket.stop(Instant::now() + STEP).await.unwrap();
    retry_socket
        .lock()
        .await
        .stop(Instant::now() + STEP)
        .await
        .unwrap();
    assert_eq!(round.relay.peer_transcript().len(), 3);
    assert_eq!(round.ordinary.publication_attempts(7435), 0);
    assert_eq!(round.ordinary.publication_attempts(445), 0);
    round.relay.stop().await;
    round.ordinary.stop().await;
    eprintln!(
        "peer_postjoin_durable stage_ms={} history_ms={history_ms} reopen_ms={reopen_ms} ack_loss_ms={lost_ms} retry_ms={recover_ms} group_writes=3",
        setup.activation_ms,
    );
}

#[track_caller]
fn assert_gate(gate: &Gate, commits: u8, deliveries: u8, accepted: bool) {
    let observation = gate.observe().unwrap();
    assert_eq!(
        (
            observation.commit_calls,
            observation.deliveries,
            observation.accepted,
            observation.denied,
            observation.violation
        ),
        (commits, deliveries, accepted, 0, false),
    );
}

fn publication_scope(publication: &Publication) -> Scope {
    Scope::new(
        cgka_traits::MemberId::new(publication.recipient().to_bytes()),
        GroupId::new(publication.group()),
        publication.route(),
        TransportEndpoint(publication.origin().into()),
        publication.generation(),
        publication.deadline(),
        MessageId::new(publication.semantic_id()),
    )
    .unwrap()
}

struct Stage {
    selector: Selector,
    generation: u64,
    delivery: Delivery,
    group: GroupId,
    routing: NostrRoutingV1,
    welcome: MessageId,
}

impl Stage {
    fn read(
        &self,
        owner: &mut PreparedConsentOwner<'_>,
        wall: u64,
    ) -> crate::peer_invitation::HeldEvidence {
        let row = owner
            .state
            .checkpoint
            .read_custody(&owner.state.source, self.selector, self.generation, wall)
            .unwrap()
            .expect("exact durable Activating custody");
        assert_eq!(row.state(), CustodyState::Activating);
        assert_eq!(
            row.delivery_carrier(),
            self.delivery.flight().carrier_id.to_bytes()
        );
        assert_eq!(row.welcome_id().as_slice(), self.welcome.as_slice());
        assert_eq!(row.offer().routing(), &self.routing);
        row
    }

    fn open(
        &self,
        owner: &PreparedConsentOwner<'_>,
        row: &crate::peer_invitation::HeldEvidence,
        wall: &Arc<AtomicU64>,
        gate: Gate,
    ) -> Recipient {
        open_recipient(owner, row, wall, &self.group, &self.routing, gate)
    }
}

fn checked_image(
    owner: &PreparedConsentOwner<'_>,
    held: &crate::peer_invitation::HeldEvidence,
) -> PathBuf {
    let (binding, witness) = held.stage();
    owner
        .state
        .paths
        .reopen_provisional(binding, witness)
        .unwrap()
        .path()
        .to_owned()
}

fn read_publication(
    owner: &mut PreparedConsentOwner<'_>,
    held: &crate::peer_invitation::HeldEvidence,
    wall: u64,
    step: Step,
) -> Publication {
    let row = owner
        .state
        .checkpoint
        .publication(&owner.state.source, held, Kind::PostJoin, wall)
        .unwrap()
        .unwrap();
    assert_eq!(row.step(), step);
    row
}

fn advance_retry(recipient: &Recipient, group: &GroupId, wall: &AtomicU64, deadline: u64) {
    let delay = recipient
        .outbound_fanout_retry_delay_ms(group)
        .unwrap()
        .unwrap();
    assert!(delay > 0);
    let next = wall.load(Ordering::Acquire) + delay.div_ceil(1_000);
    assert!(
        next < deadline,
        "native retry delay {delay} ms exceeds remaining custody lifetime"
    );
    wall.store(next, Ordering::Release);
}
