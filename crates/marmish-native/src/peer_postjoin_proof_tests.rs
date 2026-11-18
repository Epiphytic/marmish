//! Two native accounts and one guarded publication with no relay connection.

use super::super::provisional::tests::Fixture;
use super::*;
use crate::mdk_adapter::peer_staged_proof_tests::{
    PeerFounderRuntime, peer_enrollment_founder, postjoin_session_config,
};
use crate::peer_invitation::store::{TrustedOutcome, consent_checkpoint::Mode};
use cgka_traits::app_components::NostrRoutingV1;
use cgka_traits::maintenance::{MaintenancePhase, PostJoinHistoryFetchOutcome};
use cgka_traits::{
    GroupId, MemberId, MessageId, Timestamp as NativeTimestamp, TransportAdapter,
    TransportDelivery, TransportDeliveryPlane, TransportDeliverySource, TransportEndpoint,
    TransportMessage, TransportPublishRequest, TransportPublishTarget,
};
use consent_checkpoint::provisioning::prepared_consent::custody::{CustodyState, HeldEvidence};
use consent_checkpoint::wire::{Codec, MAX_OUTER_BYTES};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublication, KeyPackagePublisher, MaintenanceTiming,
    OsMaintenanceRandom, StaticTransportRouting, SystemMonotonicClock, WallClock,
};
use nostr::event::Event;
use postjoin_gate::{DeliveryResult, Gate, Permit};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use storage_sqlite::{SqliteAccountStorage, SqliteJournalMode, SqliteStorageOptions};
use tokio::sync::Mutex as AsyncMutex;

const ORIGIN: &str = "wss://stage-fixture.example/";
const DEADLINE: Duration = Duration::from_secs(120);
type Runtime = AccountDeviceRuntime<Gate, StaticTransportRouting, Gate>;

#[test]
#[ignore = "opt-in two native accounts and closed SQLCipher stage recovery"]
fn exact_postjoin_commit_is_recovered_and_delivered_once() {
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    tokio::time::timeout(DEADLINE, prove_exact_postjoin())
                        .await
                        .expect("bounded native PostJoin proof");
                });
        })
        .unwrap()
        .join();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

struct ActivatingStage {
    row: HeldEvidence,
    selector: Selector,
    generation: u64,
    group: GroupId,
    routing: NostrRoutingV1,
    welcome: MessageId,
    image: PathBuf,
    initial_digest: [u8; 32],
}

fn custody(
    owner: &mut PreparedConsentOwner<'_>,
    selector: Selector,
    generation: u64,
    wall: u64,
    state: CustodyState,
) -> HeldEvidence {
    let row = owner
        .state
        .checkpoint
        .read_custody(&owner.state.source, selector, generation, wall)
        .unwrap()
        .expect("durable fixture custody");
    assert_eq!(row.state(), state);
    row
}

fn record(frame: &[u8]) -> RawRecord<'_> {
    let event: Event = serde_json::from_slice(frame).unwrap();
    RawRecord {
        id: event.id.to_bytes(),
        stamp: event.created_at.as_secs(),
        bytes: frame,
    }
}

async fn reach_activating(
    fixture: &Fixture,
    owner: &mut PreparedConsentOwner<'_>,
    founder_dir: &Path,
    gate: &Gate,
) -> (ActivatingStage, PeerFounderRuntime) {
    let recipient = fixture.identity.signer();
    let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
    let routing = NostrRoutingV1::new([61; 32], vec![ORIGIN.into()]).unwrap();
    let offer = codec
        .prepare_offer(
            &fixture.server,
            recipient.public_key(),
            [63; 16],
            &routing,
            fixture.now,
        )
        .unwrap();
    assert!(matches!(
        owner
            .receive_custody_record(record(offer.frame()))
            .unwrap()
            .status,
        Status::Accepted(_)
    ));
    let selector = Selector::from(&offer.flight);
    let reservation = owner
        .state
        .checkpoint
        .reserve_provisioning(&owner.state.source, selector, fixture.now)
        .unwrap();
    owner
        .state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    owner
        .queue_prepared_consent(&reservation, |_| Ok(()))
        .unwrap();
    let queued = owner
        .state
        .checkpoint
        .recover_queued_consent(&owner.state.source, selector, fixture.now)
        .unwrap()
        .unwrap();
    let generation = queued.stage().0.coordinates().1;
    let mut sink = FixtureSink {
        owner: Arc::new(()),
        origin: owner.state.origin,
        ready: true,
        response: SinkResponse::Positive,
        last: None,
    };
    let reserved = owner
        .reserve_prepared_attempt(selector, &sink)
        .unwrap()
        .unwrap();
    let sent = owner
        .publish_prepared_attempt(reserved, &mut sink)
        .unwrap()
        .unwrap();
    owner.settle_prepared_ack(sent, &sink).unwrap();
    let consent = codec
        .open_consent(
            &fixture.server,
            sink.last.as_deref().unwrap(),
            &offer.flight,
            fixture.now,
        )
        .unwrap();
    let (founder, group, welcome) = peer_enrollment_founder(
        founder_dir,
        Arc::new(fixture.server.clone()),
        recipient.public_key(),
        &routing,
        consent.offer().room_id(),
        consent.key_package().clone(),
    )
    .await;
    let carrier = codec
        .prepare_delivery(
            &fixture.server,
            &consent,
            &welcome.payload,
            fixture.now,
            fixture.now + 360,
        )
        .unwrap();
    let delivery = codec
        .open_delivery(&recipient, carrier.frame(), &consent, fixture.now)
        .unwrap();
    assert_eq!(
        owner
            .receive_custody_record(record(carrier.frame()))
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::Held)
    );
    assert_eq!(
        owner
            .advance_custody(selector, generation, |_| Ok(()))
            .await
            .unwrap(),
        CustodyProgress::Pending
    );
    let pending = custody(
        owner,
        selector,
        generation,
        fixture.now,
        CustodyState::Pending,
    );
    let account = MemberId::new(recipient.public_key().to_bytes());
    let endpoints = vec![TransportEndpoint(ORIGIN.into())];
    let refused = gate
        .prepare_key_package(KeyPackagePublication {
            account_id: account.clone(),
            key_package: consent.key_package().clone(),
            slot_id: owner.state.paths.provisional_label(pending.stage().0),
            created_at: NativeTimestamp(fixture.now),
            endpoints: endpoints.clone(),
        })
        .await
        .unwrap_err();
    assert!(!refused.externally_exposed);
    assert!(
        gate.publish(TransportPublishRequest {
            account_id: account,
            target: TransportPublishTarget::Group {
                group_id: group.clone(),
                transport_group_id: routing.nostr_group_id.to_vec(),
                endpoints,
            },
            message: welcome.clone(),
            required_acks: 1,
        })
        .await
        .is_err()
    );
    assert!(gate.take_candidate().is_err());
    let observed = gate.observe().unwrap();
    assert_eq!(
        (observed.denied, observed.commit_calls, observed.deliveries),
        (2, 0, 0)
    );
    assert_eq!(
        owner
            .activate_custody(selector, generation, None, |cut| {
                assert_eq!(cut, JoinCut::ActivatingCommitted);
                Ok(())
            })
            .unwrap(),
        CustodyProgress::Activating
    );
    let row = custody(
        owner,
        selector,
        generation,
        fixture.now,
        CustodyState::Activating,
    );
    assert_eq!(row.stage(), pending.stage());
    let readback = owner
        .inspect_joined(row.stage(), &delivery, row.package_digest())
        .unwrap();
    assert!(row.matches_pending_readback(&readback));
    assert_eq!(readback.coordinates().0, group.as_slice());
    let (binding, witness) = row.stage();
    let original = owner
        .state
        .paths
        .reopen_provisional(binding, witness)
        .unwrap();
    let image = original.path().to_owned();
    let initial_digest = original.digest().unwrap();
    let stage = ActivatingStage {
        row,
        selector,
        generation,
        group,
        routing,
        welcome: welcome.id,
        image,
        initial_digest,
    };
    (stage, founder)
}

struct NativeWall(Arc<AtomicU64>);

impl WallClock for NativeWall {
    fn now(&self) -> NativeTimestamp {
        NativeTimestamp(self.0.load(Ordering::Relaxed))
    }

    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::Relaxed) * 1_000
    }
}

fn open_native(
    owner: &PreparedConsentOwner<'_>,
    stage: &ActivatingStage,
    gate: &Gate,
    wall: &Arc<AtomicU64>,
) -> Result<Runtime> {
    let (binding, witness) = stage.row.stage();
    let original = owner.state.paths.reopen_provisional(binding, witness)?;
    assert_eq!(original.path(), stage.image);
    let key = owner.state.paths.provisional_key(binding);
    let storage = SqliteAccountStorage::open_encrypted_with_options(
        original.path(),
        &SqlCipherKey::raw(*key),
        SqliteStorageOptions {
            journal_mode: SqliteJournalMode::Delete,
            ..SqliteStorageOptions::default()
        },
    )
    .map_err(|_| Error::Path)?;
    assert_eq!(storage.migration_summary().0, 0);
    let config = postjoin_session_config(storage, owner.state.signer.clone());
    let endpoints = vec![TransportEndpoint(ORIGIN.into())];
    let routing = StaticTransportRouting::new(endpoints.clone()).with_group_route(
        stage.group.clone(),
        stage.routing.nostr_group_id.to_vec(),
        endpoints,
    );
    Ok(AccountDeviceRuntime::open(config, || {
        Ok::<_, marmot_session::SessionError>((gate.clone(), routing, gate.clone()))
    })
    .unwrap()
    .with_maintenance_sources(
        Arc::new(NativeWall(wall.clone())),
        Arc::new(SystemMonotonicClock::default()),
        Arc::new(OsMaintenanceRandom),
    )
    .with_maintenance_timing(MaintenanceTiming::immediate()))
}

fn prepare_history(recipient: &Runtime, founder: &PeerFounderRuntime, group: &GroupId) {
    recipient
        .mark_post_join_subscription_installed(group)
        .unwrap();
    let fetch = recipient
        .start_post_join_history_fetch(group)
        .unwrap()
        .unwrap();
    // This founder authored only the initial Welcome. Its same joined epoch
    // verifies that the controlled fetch has no later commit to deliver.
    assert_eq!(
        recipient.epoch_pin(group).unwrap(),
        founder.runtime().epoch_pin(group).unwrap()
    );
    let completion = recipient
        .finish_post_join_history_fetch(group, &fetch)
        .unwrap()
        .unwrap();
    assert_eq!(completion.outcome, PostJoinHistoryFetchOutcome::Fetched);
    recipient.mark_post_join_eose(group).unwrap();
}

fn advance_to_retry(recipient: &Runtime, group: &GroupId, wall: &AtomicU64, deadline: u64) {
    let delay = recipient
        .outbound_fanout_retry_delay_ms(group)
        .unwrap()
        .unwrap();
    assert!(
        delay > 0,
        "native pre-send refusal retains a bounded retry delay"
    );
    let next = wall.load(Ordering::Relaxed) + delay.div_ceil(1_000);
    assert!(next < deadline);
    wall.store(next, Ordering::Relaxed);
}

async fn prove_exact_postjoin() {
    let fixture = Fixture::new();
    let founder_dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    drop(fixture.start(Policy::default()));
    let route = Route::configured(ORIGIN).unwrap();
    drop(
        PreparedConsentOwner::open(
            &fixture.lease,
            &fixture.identity,
            Role::Both,
            Policy::default(),
            &route,
            Wall::Fixture(fixture.wall.clone()),
        )
        .unwrap(),
    );
    let mut owner = PreparedConsentOwner::open_custody(
        &fixture.lease,
        &fixture.identity,
        Role::Both,
        Policy {
            mode: Mode::Automatic,
            ..Policy::default()
        },
        &route,
        Wall::Fixture(fixture.wall.clone()),
    )
    .unwrap();
    let gate = Gate::unscoped_closed(fixture.wall.clone());
    let (stage, founder) = reach_activating(&fixture, &mut owner, founder_dir.path(), &gate).await;
    let deadline = stage.row.pending_deadline().unwrap();
    let (_, file) = stage.row.stage();
    let inode = (
        u64::from_be_bytes(file.device),
        u64::from_be_bytes(file.inode),
    );
    let original = stage.image.with_extension("held-original");
    fs::rename(&stage.image, &original).unwrap();
    fs::copy(&original, &stage.image).unwrap();
    assert!(matches!(
        open_native(&owner, &stage, &gate, &fixture.wall),
        Err(Error::StageIdentityMismatch)
    ));
    fs::rename(&original, &stage.image).unwrap();
    let (binding, witness) = stage.row.stage();
    assert_eq!(
        owner
            .state
            .paths
            .reopen_provisional(binding, witness)
            .unwrap()
            .digest()
            .unwrap(),
        stage.initial_digest
    );
    let mut recipient = open_native(&owner, &stage, &gate, &fixture.wall).unwrap();
    prepare_history(&recipient, &founder, &stage.group);
    for _ in 0..2 {
        recipient.run_due_maintenance().await.unwrap();
    }
    assert!(
        matches!(recipient.maintenance_obligations_for_group(&stage.group).unwrap().as_slice(),
        [obligation] if obligation.phase == MaintenancePhase::Jitter)
    );
    assert_eq!(gate.observe().unwrap().commit_calls, 0);
    let baseline = postjoin_verify::pre_capture(
        &recipient,
        founder.runtime(),
        postjoin_verify::Scope {
            image: &stage.image,
            inode,
            group: &stage.group,
            account: fixture.identity.public_key(),
            founder: fixture.server.public_key(),
            route: &stage.routing,
            generation: stage.generation,
            deadline,
            welcome: &stage.welcome,
        },
        fixture.now,
    )
    .unwrap();
    let scope = postjoin_gate::Scope::new(
        MemberId::new(fixture.identity.public_key().to_bytes().to_vec()),
        stage.group.clone(),
        stage.routing.nostr_group_id,
        TransportEndpoint(ORIGIN.into()),
        stage.generation,
        deadline,
        baseline.obligation_id().clone(),
    )
    .unwrap();
    gate.begin_capture(scope.clone()).unwrap();
    let capture_started = Instant::now();
    recipient.run_due_maintenance().await.unwrap();
    let candidate = gate
        .take_candidate()
        .unwrap()
        .expect("one captured native commit");
    let observed = gate.observe().unwrap();
    assert_eq!(
        (
            observed.commit_calls,
            observed.deliveries,
            observed.violation
        ),
        (1, 0, false)
    );
    let capture = postjoin_verify::post_capture(
        &recipient,
        founder.runtime(),
        &baseline,
        candidate.request(),
        1,
        fixture.now,
    )
    .unwrap();
    let capture_ms = capture_started.elapsed().as_millis();
    let reopen_started = Instant::now();
    drop(recipient);
    let gate = Gate::closed(scope.clone(), fixture.wall.clone());
    let mut recipient = open_native(&owner, &stage, &gate, &fixture.wall).unwrap();
    // Stay within native backoff so this unsolicited attempt cannot consume
    // the single refusal reserved for the verified retry below.
    recipient.resume_outbound_fanouts().await.unwrap();
    assert_eq!(gate.observe().unwrap().deliveries, 0);
    let fresh = postjoin_verify::reopen(
        &recipient,
        founder.runtime(),
        &baseline,
        &capture,
        fixture.now,
    )
    .unwrap();
    assert_eq!(gate.observe().unwrap().commit_calls, 0);
    assert_eq!(fresh.attempts(), 1);
    let reopen_ms = reopen_started.elapsed().as_millis();
    gate.arm(
        Permit::verify(&scope, &candidate, &fresh).unwrap(),
        &fresh,
        None,
    )
    .unwrap();
    advance_to_retry(&recipient, &stage.group, &fixture.wall, deadline);
    recipient.run_due_maintenance().await.unwrap();
    assert_eq!(gate.observe().unwrap().commit_calls, 1);
    assert_eq!(gate.observe().unwrap().deliveries, 0);
    let now = fixture.wall.load(Ordering::Relaxed);
    let fresh =
        postjoin_verify::reopen(&recipient, founder.runtime(), &baseline, &capture, now).unwrap();
    assert_eq!(fresh.attempts(), 2);
    let founder = Arc::new(AsyncMutex::new(founder));
    let sink = founder_delivery(&founder, &stage.group, &fixture);
    gate.arm(
        Permit::verify(&scope, &candidate, &fresh).unwrap(),
        &fresh,
        Some(sink),
    )
    .unwrap();
    advance_to_retry(&recipient, &stage.group, &fixture.wall, deadline);
    let confirm_started = Instant::now();
    recipient.run_due_maintenance().await.unwrap();
    let observed = gate.observe().unwrap();
    assert_eq!(
        (
            observed.commit_calls,
            observed.deliveries,
            observed.violation
        ),
        (2, 1, false)
    );
    let founder = founder.lock().await;
    let now = fixture.wall.load(Ordering::Relaxed);
    let confirm = |recipient: &Runtime| {
        postjoin_verify::post_confirm(recipient, founder.runtime(), &baseline, &fresh, now)
    };
    if confirm(&recipient).is_err() {
        recipient.run_due_maintenance().await.unwrap();
    }
    confirm(&recipient).unwrap();
    recipient.resume_outbound_fanouts().await.unwrap();
    let observed = gate.observe().unwrap();
    assert_eq!(
        (
            observed.commit_calls,
            observed.deliveries,
            observed.violation
        ),
        (2, 1, false)
    );
    drop(recipient);
    let closed = Gate::closed(scope, fixture.wall.clone());
    let mut recipient = open_native(&owner, &stage, &closed, &fixture.wall).unwrap();
    recipient.resume_outbound_fanouts().await.unwrap();
    confirm(&recipient).unwrap();
    let observed = closed.observe().unwrap();
    assert_eq!(
        (
            observed.commit_calls,
            observed.deliveries,
            observed.violation
        ),
        (0, 0, false)
    );
    let final_row = custody(
        &mut owner,
        stage.selector,
        stage.generation,
        now,
        CustodyState::Activating,
    );
    assert_eq!(final_row.stage(), stage.row.stage());
    eprintln!(
        "peer_postjoin capture_ms={capture_ms} reopen_ms={reopen_ms} confirm_and_reopen_ms={}",
        confirm_started.elapsed().as_millis()
    );
}

fn founder_delivery(
    founder: &Arc<AsyncMutex<PeerFounderRuntime>>,
    group: &GroupId,
    fixture: &Fixture,
) -> postjoin_gate::DeliverySink {
    let founder = founder.clone();
    let group = group.clone();
    let member = MemberId::new(fixture.server.public_key().to_bytes().to_vec());
    let wall = fixture.wall.clone();
    Box::new(move |message: TransportMessage| {
        Box::pin(async move {
            let mut founder = founder.lock().await;
            let prior = founder.runtime().epoch_pin(&group).unwrap();
            let delivery = TransportDelivery {
                account_id: member,
                group_id_hint: Some(group.clone()),
                received_at: NativeTimestamp(wall.load(Ordering::Relaxed)),
                source: TransportDeliverySource {
                    transport: message.source.clone(),
                    plane: TransportDeliveryPlane::Group,
                    endpoint: Some(TransportEndpoint(ORIGIN.into())),
                    subscription_id: Some("postjoin-exact-fixture".into()),
                    wire: None,
                },
                message,
            };
            let applied = founder.ingest_delivery(&delivery).await.is_ok();
            let current = founder.runtime().epoch_pin(&group).unwrap();
            if applied && current.epoch.0 == prior.epoch.0 + 1 {
                DeliveryResult::Accepted
            } else {
                DeliveryResult::PossiblyExposed
            }
        })
    })
}
