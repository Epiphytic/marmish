//! The receipt uses the same private enrollment, with separate durable decisions.

use super::*;
use crate::peer_invitation::{
    accept_receipt_verify as founder_proof,
    accept_receipt_wire::{self, Receipt},
    publication_socket::ReceiptGate,
    receipt_publication_verify as outbound,
    socket::wire::Failure as SocketFailure,
    store::consent_checkpoint::provisioning::prepared_consent::custody::publication::{
        FounderEnrollment, ReceiptReservation,
    },
};
use cgka_traits::{GroupEvent, OutboundFanoutStorage, engine::SendIntent};

#[test]
#[ignore = "requires native staged SQLCipher and two independent private WSS accounts"]
fn private_wss_cold_acceptance_receipt_projects_exactly_once() {
    run_native(Duration::from_secs(180), || prove_lost_ack(true));
}

pub(super) async fn start(
    owner: &mut PreparedConsentOwner<'_>,
    stage: &Stage,
    round: &Round,
    image: &Path,
    reserved: ReceiptReservation,
) -> [u8; 32] {
    let wall = round.wall.load(Ordering::Acquire);
    let held = stage.read(owner, wall);
    assert!(held.is_active());
    assert_eq!(reserved.publication().step(), Step::Reserved);

    let mut recipient = open_recipient_with_package_gate(
        owner,
        &held,
        &round.wall,
        &stage.group,
        &stage.routing,
        ReceiptGate::closed(round.wall.clone()),
        Gate::unscoped_closed(round.wall.clone()),
    );
    let start = owner
        .state
        .checkpoint
        .start_native_receipt(&owner.state.source, reserved.publication(), wall)
        .unwrap();
    assert_eq!(start.publication().step(), Step::NativeStarted);
    assert!(outbound::capture(&recipient, image, start.publication(), wall).is_err());
    let pin = stable_pin(&recipient, &stage.group);
    assert_eq!(pin.epoch.0, start.publication().source_pin().ordinal);
    assert_eq!(pin.identity, start.publication().source_pin().authenticator);
    assert!(
        owner
            .state
            .checkpoint
            .start_native_receipt(&owner.state.source, reserved.publication(), wall,)
            .is_err()
    );
    let effects = recipient
        .send(SendIntent::AppMessage {
            group_id: stage.group.clone(),
            payload: reserved.prepared().payload().to_vec(),
            expected_epoch: Some(pin),
        })
        .await
        .unwrap();
    assert!(effects.published_app_messages.is_empty());
    assert_eq!(round.relay.group_writes().len(), 3);
    reserved.prepared().event_id()
}

pub(super) struct ReceiptRun<'a, 'profile> {
    pub(super) stage: &'a Stage,
    pub(super) round: &'a Round,
    pub(super) image: &'a Path,
    pub(super) reopen: &'a dyn Fn() -> PreparedConsentOwner<'profile>,
    pub(super) founder: PeerFounderReopen,
    pub(super) founder_runtime: Option<PeerFounderRuntime<FounderTransport>>,
    pub(super) founder_socket: &'a mut Owner,
    pub(super) recipient_signer: Arc<Keys>,
    pub(super) inner: [u8; 32],
    pub(super) started: Instant,
}

impl ReceiptRun<'_, '_> {
    fn wall(&self) -> u64 {
        self.round.wall.load(Ordering::Acquire)
    }

    fn open(
        &self,
        owner: &PreparedConsentOwner<'_>,
        row: &crate::peer_invitation::HeldEvidence,
        gate: ReceiptGate,
    ) -> AccountDeviceRuntime<ReceiptGate, StaticTransportRouting, Gate> {
        open_recipient_with_package_gate(
            owner,
            row,
            &self.round.wall,
            &self.stage.group,
            &self.stage.routing,
            gate,
            Gate::unscoped_closed(self.round.wall.clone()),
        )
    }

    async fn open_sender(&self) -> Arc<Mutex<Owner>> {
        let config = QualifiedOrigin::for_daemon_peer_fixture(
            self.round.relay.peer_mode().unwrap(),
            self.round.root.path(),
            self.round.signer,
            self.round.founder.public_key(),
        )
        .unwrap();
        Arc::new(Mutex::new(
            Owner::open(
                &config.connector,
                self.round.relay.url.as_str(),
                self.round.signer,
                &self.recipient_signer,
                Instant::now() + WAIT,
            )
            .await
            .unwrap(),
        ))
    }

    fn read(
        &self,
        owner: &mut PreparedConsentOwner<'_>,
        step: Step,
    ) -> (crate::peer_invitation::HeldEvidence, Publication) {
        let row = self.stage.read(owner, self.wall());
        assert!(row.is_active());
        let saved = owner
            .state
            .checkpoint
            .publication(&owner.state.source, &row, Kind::AcceptReceipt, self.wall())
            .unwrap()
            .unwrap();
        assert_eq!(saved.step(), step);
        assert_eq!(saved.semantic_id(), self.inner);
        (row, saved)
    }
}

pub(super) async fn finish(mut run: ReceiptRun<'_, '_>) {
    let projection = FounderProjection::create(&run);
    let prior = expected_founder_enrollment(&run, run.founder_runtime.as_ref().unwrap());
    {
        let mut db = projection.checkpoint(&run);
        let source = db.bind_source(run.round.relay.url.as_str()).unwrap();
        db.record_founder_enrollment(&source, &prior, run.wall())
            .unwrap();
    }
    drop(prior);
    let mut owner = (run.reopen)();
    let (row, native_started) = run.read(&mut owner, Step::NativeStarted);
    let gate = ReceiptGate::closed(run.round.wall.clone());
    let recipient = run.open(&owner, &row, gate.clone());
    let captured = outbound::capture(&recipient, run.image, &native_started, run.wall()).unwrap();
    let candidate = captured.event_id();
    let digest = captured.signed_digest();
    let recorded = owner
        .state
        .checkpoint
        .capture_receipt(&owner.state.source, &captured, run.wall())
        .unwrap();
    assert_eq!(recorded.step(), Step::Captured);
    let deadline = recorded.deadline();
    assert!(outbound::capture(&recipient, run.image, &native_started, deadline).is_err());
    drop(captured);
    drop((recipient, gate, native_started, recorded, owner));

    let reopen_started = Instant::now();
    let mut owner = (run.reopen)();
    let (row, recorded) = run.read(&mut owner, Step::Captured);
    let gate = ReceiptGate::closed(run.round.wall.clone());
    let mut recipient = run.open(&owner, &row, gate.clone());
    assert!(
        outbound::check_captured(
            &recipient,
            &run.image.with_file_name("wrong-receipt-stage"),
            &recorded,
            run.wall(),
        )
        .is_err()
    );
    let request = outbound::check_captured(&recipient, run.image, &recorded, run.wall()).unwrap();
    let capture_reopen_ms = reopen_started.elapsed().as_millis();
    advance_retry(&recipient, &run.stage.group, &run.round.wall, deadline);
    let sender = run.open_sender().await;
    let socket = ReservedSocket::reserve(
        sender.clone(),
        &recorded,
        run.round.wall.clone(),
        Instant::now() + STEP,
    )
    .await
    .unwrap();
    let attempt = owner
        .state
        .checkpoint
        .arm_publication(&owner.state.source, &recorded, run.wall())
        .unwrap();
    let turn = socket
        .bind(
            &mut owner.state.checkpoint,
            &owner.state.source,
            attempt,
            &request,
            Kind::AcceptReceipt,
        )
        .unwrap();
    gate.arm(turn).unwrap();
    run.round.relay.group_fault(GroupFault::LoseNextAck);
    let ack_loss_start = Instant::now();
    let (effects, record) = tokio::join!(
        recipient.resume_outbound_fanouts(),
        run.founder_socket.next_group_record(Instant::now() + STEP)
    );
    assert!(effects.unwrap().published_app_messages.is_empty());
    let ack_loss_ms = ack_loss_start.elapsed().as_millis();
    assert!(matches!(
        sender.lock().await.stop(Instant::now() + STEP).await,
        Err(SocketFailure::Retired)
    ));
    drop(sender);

    // Stop the recipient before its terminal SQL decision. The founder has
    // not yet changed the durable acceptance journal.
    drop((recipient, gate, request, recorded, owner));
    let record = record.unwrap();
    assert!(run.founder_socket.accepts_group_record(&record));
    assert_eq!(record.fingerprint().id, candidate);
    assert_eq!(record.fingerprint().bytes_sha256, digest);
    let delivery = record.delivery(&run.stage.group, run.wall());
    let mut founder = run.founder_runtime.take().unwrap();
    let ingress = founder.ingest_delivery(&delivery).await.unwrap();
    assert_eq!(ingress.outcome, IngestOutcome::Processed);
    assert!(!ingress.left_object_unpersisted);
    let native_ids: Vec<_> = ingress
        .effects
        .events
        .iter()
        .filter_map(|event| match event {
            GroupEvent::MessageReceived {
                message_id,
                payload,
                ..
            } if accept_receipt_wire::decode(payload)
                .is_ok_and(|item| item.event_id == run.inner) =>
            {
                Some(message_id.clone())
            }
            _ => None,
        })
        .collect();
    let [native_id] = native_ids.as_slice() else {
        panic!("one authenticated native acceptance receipt source");
    };
    drop(founder);

    // The founder can durably accept a receipt it ingested despite the
    // publisher losing its WSS ACK. The recipient has not retried yet.
    let mut owner = (run.reopen)();
    run.read(&mut owner, Step::Armed { number: 1 });
    drop(owner);
    let first_acceptance = project_founder(&run, &projection, native_id);
    let founder_acceptance_ms = run.started.elapsed().as_millis();
    assert_eq!(run.round.relay.group_writes().len(), 4);

    let mut owner = (run.reopen)();
    let (row, first_armed) = run.read(&mut owner, Step::Armed { number: 1 });
    let gate = ReceiptGate::closed(run.round.wall.clone());
    let mut recipient = run.open(&owner, &row, gate.clone());
    assert!(outbound::confirm(&recipient, run.image, &first_armed, run.wall()).is_err());
    let uncertain = owner
        .state
        .checkpoint
        .record_publication_uncertain(&owner.state.source, &first_armed, run.wall())
        .unwrap();
    assert!(outbound::recover(&recipient, run.image, &uncertain, deadline).is_err());
    let writes = run.round.relay.group_writes();
    assert_eq!(writes.len(), 4);
    assert_eq!(writes[3].record, record.fingerprint());
    assert!(!writes[3].acknowledged && !writes[3].duplicate);

    let retry = run.open_sender().await;
    let retry_started = Instant::now();
    advance_retry(&recipient, &run.stage.group, &run.round.wall, deadline);
    let recovered = outbound::recover(&recipient, run.image, &uncertain, run.wall()).unwrap();
    assert_eq!(
        (recovered.event_id(), recovered.signed_digest()),
        (candidate, digest)
    );
    let socket = ReservedSocket::reserve(
        retry.clone(),
        &uncertain,
        run.round.wall.clone(),
        Instant::now() + STEP,
    )
    .await
    .unwrap();
    let attempt = owner
        .state
        .checkpoint
        .arm_recovered_receipt(&owner.state.source, &recovered, run.wall())
        .unwrap();
    let turn = socket
        .bind(
            &mut owner.state.checkpoint,
            &owner.state.source,
            attempt,
            recovered.request(),
            Kind::AcceptReceipt,
        )
        .unwrap();
    gate.arm(turn).unwrap();
    let effects = recipient.resume_outbound_fanouts().await.unwrap();
    assert_eq!(effects.published_app_messages.len(), 1);
    let retry_ms = retry_started.elapsed().as_millis();
    drop((recipient, gate, recovered, owner));
    retry
        .lock()
        .await
        .stop(Instant::now() + STEP)
        .await
        .unwrap();

    let mut owner = (run.reopen)();
    let (row, armed) = run.read(&mut owner, Step::Armed { number: 2 });
    let recipient = run.open(&owner, &row, ReceiptGate::closed(run.round.wall.clone()));
    assert!(outbound::acknowledge_published(&recipient, run.image, &armed, run.wall()).is_err());
    let accepted = outbound::confirm(&recipient, run.image, &armed, run.wall()).unwrap();
    assert_eq!(accepted.event_id(), candidate);
    assert_eq!(accepted.signed_digest(), digest);
    let published = owner
        .state
        .checkpoint
        .confirm_receipt(&owner.state.source, &accepted, run.wall())
        .unwrap();
    assert_eq!(published.step(), Step::Published { number: 2 });
    drop((recipient, owner, accepted, published));

    // Reopen after the recipient SQL commit and acknowledge exactly the
    // MessageId/app-id selected by the retained native publication evidence.
    let mut owner = (run.reopen)();
    let (row, published) = run.read(&mut owner, Step::Published { number: 2 });
    let mut recipient = run.open(&owner, &row, ReceiptGate::closed(run.round.wall.clone()));
    outbound::acknowledge_published(&recipient, run.image, &published, run.wall()).unwrap();
    outbound::acknowledge_published(&recipient, run.image, &published, run.wall()).unwrap();
    let no_fanout = recipient
        .group_authority_view()
        .with_group_authority_snapshot(&run.stage.group, |storage, _| {
            Ok::<_, marmot_session::SessionError>(storage.list_outbound_fanouts()?.is_empty())
        })
        .unwrap();
    assert!(no_fanout);
    assert!(
        recipient
            .resume_outbound_fanouts()
            .await
            .unwrap()
            .published_app_messages
            .is_empty()
    );
    let writes = run.round.relay.group_writes();
    assert_eq!(writes.len(), 5);
    assert_eq!(writes[4].sender, run.round.signer);
    assert_eq!(writes[4].record, writes[3].record);
    assert!(writes[4].acknowledged && writes[4].duplicate);

    retire_founder(&run, &projection, native_id, first_acceptance, delivery).await;
    assert_eq!(run.round.relay.group_writes(), writes);
    eprintln!(
        "peer_receipt_durable acceptance_path_ms={} founder_acceptance_ms={founder_acceptance_ms} capture_reopen_ms={capture_reopen_ms} ack_loss_ms={ack_loss_ms} retry_ms={retry_ms} group_writes=5",
        run.started.elapsed().as_millis()
    );
}

fn expected_founder_enrollment(
    run: &ReceiptRun<'_, '_>,
    founder: &PeerFounderRuntime<FounderTransport>,
) -> FounderEnrollment {
    let founder = founder.runtime();
    let epoch = stable_pin(founder, &run.stage.group);
    let recipient_leaf = founder
        .canonical_group_membership(&run.stage.group)
        .unwrap()
        .member_leaves
        .into_iter()
        .find(|leaf| leaf.member.as_slice() == run.round.signer.to_bytes())
        .unwrap()
        .leaf_index;
    let delivery = &run.stage.delivery;
    let offer = delivery.offer();
    let flight = offer.flight();
    assert_eq!(founder.self_id().as_slice(), flight.author.to_bytes());
    assert_eq!(run.stage.routing, *offer.routing());
    assert_eq!(
        run.stage.welcome.as_slice(),
        delivery.welcome_id().to_bytes()
    );
    FounderEnrollment {
        receipt: Receipt {
            founder: flight.author,
            recipient: flight.recipient,
            room_id: offer.room_id(),
            offer_id: offer.offer_id(),
            offer_revision: flight.revision.to_bytes(),
            welcome_id: delivery.welcome_id().to_bytes(),
            epoch: epoch.epoch.0,
            epoch_identity: epoch.identity,
        },
        group: run.stage.group.as_slice().to_vec(),
        route: offer.routing().nostr_group_id,
        leaf: recipient_leaf,
        valid_from: delivery.flight().created_at,
        deadline: delivery.flight().expires_at,
    }
}

struct DurableExpectation {
    saved: FounderEnrollment,
    group: GroupId,
    route: NostrRoutingV1,
}

impl DurableExpectation {
    fn read(
        run: &ReceiptRun<'_, '_>,
        db: &mut Checkpoint,
        source: &consent_checkpoint::Source,
    ) -> Self {
        let saved = db
            .read_founder_enrollment(source, run.round.signer, run.wall())
            .unwrap();
        let group = GroupId::new(saved.group.clone());
        let route = NostrRoutingV1 {
            nostr_group_id: saved.route,
            relays: vec![run.round.relay.url.as_str().to_owned()],
        };
        Self {
            saved,
            group,
            route,
        }
    }

    fn expected(&self) -> founder_proof::Expectation<'_> {
        founder_proof::Expectation {
            receipt: &self.saved.receipt,
            group: &self.group,
            route: &self.route,
            recipient_leaf: self.saved.leaf,
            valid_from: self.saved.valid_from,
            deadline: self.saved.deadline,
        }
    }
}

struct FounderProjection {
    path: PathBuf,
    key: zeroize::Zeroizing<[u8; 32]>,
}

impl FounderProjection {
    fn create(run: &ReceiptRun<'_, '_>) -> Self {
        let projection = Self {
            path: run.round.founder_root.path().join("acceptance.db"),
            key: zeroize::Zeroizing::new(random()),
        };
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&projection.path)
            .unwrap();
        let origin = run.round.relay.url.as_str();
        let (signer, uid) = Self::account(run);
        let origin_id = Route::configured(origin).unwrap().id;
        let bare = CursorStore::attach(
            projection.connect(),
            signer.public_key().to_bytes(),
            &[origin_id],
        )
        .unwrap();
        drop(
            Checkpoint::attach_for_provisional_proof(
                bare,
                signer.clone(),
                uid,
                &[origin],
                Policy::default(),
            )
            .unwrap(),
        );
        drop(
            Checkpoint::attach_prepared_consent_fixture(
                projection.connect(),
                signer.clone(),
                uid,
                &[origin],
                Policy::default(),
            )
            .unwrap(),
        );
        drop(
            Checkpoint::attach_pending_custody_fixture(
                projection.connect(),
                signer,
                uid,
                &[origin],
                Policy::default(),
            )
            .unwrap(),
        );
        projection
    }

    fn account(run: &ReceiptRun<'_, '_>) -> (Keys, u32) {
        // SAFETY: reads the process's effective Unix UID without mutating state.
        ((*run.round.founder).clone(), unsafe { libc::geteuid() })
    }

    fn connect(&self) -> HardenedPeerConnection {
        HardenedPeerConnection::open(&self.path, *self.key).unwrap()
    }

    fn checkpoint(&self, run: &ReceiptRun<'_, '_>) -> Checkpoint {
        let (signer, uid) = Self::account(run);
        Checkpoint::attach_publication_fixture(
            self.connect(),
            signer,
            uid,
            run.round.relay.url.as_str(),
            Policy::default(),
        )
        .unwrap()
    }
}

fn project_founder(
    run: &ReceiptRun<'_, '_>,
    projection: &FounderProjection,
    native_id: &MessageId,
) -> u64 {
    let founder = run.founder.closed();
    let mut db = projection.checkpoint(run);
    let source = db.bind_source(run.round.relay.url.as_str()).unwrap();
    let durable = DurableExpectation::read(run, &mut db, &source);
    let expected = &durable.expected();

    assert!(matches!(
        founder_proof::pending(founder.runtime(), expected, expected.deadline),
        Err(founder_proof::Refusal::Expired)
    ));
    let wrong = Receipt {
        offer_id: random(),
        ..*expected.receipt
    };
    assert!(
        founder_proof::pending(
            founder.runtime(),
            &founder_proof::Expectation {
                receipt: &wrong,
                ..*expected
            },
            run.wall()
        )
        .unwrap()
        .is_none()
    );
    let wrong_leaf = founder_proof::Expectation {
        recipient_leaf: expected.recipient_leaf ^ 1,
        ..*expected
    };
    assert!(matches!(
        founder_proof::pending(founder.runtime(), &wrong_leaf, run.wall()),
        Err(founder_proof::Refusal::Binding)
    ));
    let mut wrong_pin = *expected.receipt;
    wrong_pin.epoch_identity[0] ^= 1;
    let wrong_epoch = founder_proof::Expectation {
        receipt: &wrong_pin,
        ..*expected
    };
    assert!(matches!(
        founder_proof::pending(founder.runtime(), &wrong_epoch, run.wall()),
        Err(founder_proof::Refusal::Binding)
    ));
    let proof = founder_proof::pending(founder.runtime(), expected, run.wall())
        .unwrap()
        .unwrap();
    assert_eq!(proof.native_id(), native_id);
    assert_eq!(proof.app_id(), run.inner);
    let durable = db
        .accept_verified_receipt(&source, &proof, run.wall())
        .unwrap();
    assert_eq!(durable.accepted_at(), run.wall());
    assert_eq!(durable.source().native_id, native_id.as_slice());
    durable.accepted_at()
}

async fn retire_founder(
    run: &ReceiptRun<'_, '_>,
    projection: &FounderProjection,
    native_id: &MessageId,
    saved_at: u64,
    delivery: TransportDelivery,
) {
    run.round.wall.fetch_add(1, Ordering::AcqRel);
    let founder = run.founder.closed();
    let mut db = projection.checkpoint(run);
    let source = db.bind_source(run.round.relay.url.as_str()).unwrap();
    let durable = DurableExpectation::read(run, &mut db, &source);
    let expected = &durable.expected();
    let proof = founder_proof::pending(founder.runtime(), expected, run.wall())
        .unwrap()
        .unwrap();
    let repeated = db
        .accept_verified_receipt(&source, &proof, run.wall())
        .unwrap();
    assert_eq!(repeated.accepted_at(), saved_at);
    assert!(repeated.accepted_at() < run.wall());
    assert_eq!(repeated.source().native_id, proof.native_id().as_slice());
    drop((proof, repeated, db, founder));

    // Already-accepted rows can retire their exact native delivery after
    // the acceptance deadline; no new acceptance becomes possible.
    run.round.wall.store(expected.deadline, Ordering::Release);
    let mut founder = run.founder.closed();
    let mut db = projection.checkpoint(run);
    let source = db.bind_source(run.round.relay.url.as_str()).unwrap();
    let late = db
        .read_founder_acceptance(&source, run.round.signer, run.wall())
        .unwrap();
    assert_eq!(late.accepted_at(), saved_at);
    assert!(matches!(
        founder_proof::pending(founder.runtime(), expected, run.wall()),
        Err(founder_proof::Refusal::Expired)
    ));
    let mut wrong = native_id.as_slice().to_vec();
    wrong[0] ^= 1;
    assert!(matches!(
        founder_proof::retire(&mut founder, &late, expected, &MessageId::new(wrong)),
        Err(founder_proof::Refusal::Binding)
    ));
    assert!(founder_proof::retire(&mut founder, &late, expected, native_id).unwrap());
    assert!(!founder_proof::retire(&mut founder, &late, expected, native_id).unwrap());

    let replay = founder.ingest_delivery(&delivery).await.unwrap();
    assert!(replay.effects.events.iter().all(|event| {
        !matches!(event, GroupEvent::MessageReceived { message_id, .. } if message_id != native_id)
    }));
    // Native can present an already observed delivery again. The application
    // decision is the same durable row, and only that row can retire it.
    let saved = db
        .read_founder_acceptance(&source, run.round.signer, run.wall())
        .unwrap();
    assert_eq!(saved.accepted_at(), saved_at);
    assert_eq!(saved.source().native_id, native_id.as_slice());
    founder_proof::retire(&mut founder, &saved, expected, native_id).unwrap();
    assert!(!founder_proof::retire(&mut founder, &saved, expected, native_id).unwrap());
}
