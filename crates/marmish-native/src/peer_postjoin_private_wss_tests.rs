//! State-bearing native PostJoin history over the qualified private relay.

use super::*;
use crate::{
    mdk_adapter::{
        peer_staged_proof_tests::{
            PeerFounderRuntime, peer_enrollment_founder_with_group, postjoin_session_config,
        },
        terminal_relay_fixture::{GroupFault, GroupHistoryCut, GroupWrite},
    },
    peer_invitation::{postjoin_gate::Gate, socket::owner::GroupPage},
};
use async_trait::async_trait;
use cgka_traits::{
    EpochPin, EpochState, GroupId, MemberId, MessageId, MessageStorage, Timestamp,
    TransportAccountActivation, TransportAdapter, TransportAdapterError, TransportDelivery,
    TransportEndpoint, TransportEndpointAckKind, TransportEndpointReceipt, TransportGroupSync,
    TransportPublishReport, TransportPublishRequest, TransportPublishTarget,
    app_components::{NOSTR_ROUTING_COMPONENT_ID, decode_nostr_routing_v1},
    ingest::IngestOutcome,
    maintenance::PostJoinHistoryFetchOutcome,
};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublisher, MaintenanceTiming, OsMaintenanceRandom,
    StaticTransportRouting, SystemMonotonicClock, TransportRoutingPolicy, WallClock,
};
use sha2::{Digest, Sha256};
use storage_sqlite::{SqlCipherKey, SqliteAccountStorage, SqliteJournalMode, SqliteStorageOptions};
use tokio::sync::Mutex;

const EOSE_TIMEOUT: Duration = Duration::from_secs(2);
const TOTAL: Duration = Duration::from_secs(100);
type Recipient = AccountDeviceRuntime<Gate, StaticTransportRouting, Gate>;

#[derive(Clone)]
struct FounderTransport {
    turn: Arc<Mutex<Option<FounderTurn>>>,
    sent: Arc<Mutex<Option<CommitDigest>>>,
}

#[derive(Debug, PartialEq, Eq)]
struct CommitDigest {
    id: [u8; 32],
    bytes_sha256: [u8; 32],
}

struct FounderTurn {
    account: MemberId,
    group: GroupId,
    route: [u8; 32],
    endpoint: TransportEndpoint,
    socket: Owner,
    deadline: Instant,
}

#[async_trait]
impl TransportAdapter for FounderTransport {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn deactivate_account(
        &self,
        _: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn publish_commit(
        &self,
        request: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        let closed = TransportAdapterError::Closed;
        let Some(mut turn) = self.turn.lock().await.take() else {
            return Err(closed);
        };
        let expected = TransportPublishTarget::Group {
            group_id: turn.group,
            transport_group_id: turn.route.to_vec(),
            endpoints: vec![turn.endpoint.clone()],
        };
        if request.account_id != turn.account
            || request.target != expected
            || request.required_acks != 1
        {
            return Err(closed);
        }
        let id = request
            .message
            .id
            .as_slice()
            .try_into()
            .map_err(|_| TransportAdapterError::Closed)?;
        let ack = turn
            .socket
            .push_group_exact(turn.route, &request.message.payload, id, turn.deadline)
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        if !turn.socket.accepted(&ack, id) {
            return Err(closed);
        }
        *self.sent.lock().await = Some(CommitDigest {
            id,
            bytes_sha256: Sha256::digest(&request.message.payload).into(),
        });
        turn.socket
            .stop(turn.deadline)
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        Ok(TransportPublishReport {
            message_id: request.message.id,
            accepted: vec![TransportEndpointReceipt {
                endpoint: turn.endpoint,
                accepted_at: None,
                ack_kind: Some(TransportEndpointAckKind::Affirmative),
            }],
            failed: Vec::new(),
            required_acks: 1,
        })
    }
}

struct FixtureWall(Arc<AtomicU64>);

impl WallClock for FixtureWall {
    fn now(&self) -> Timestamp {
        Timestamp(self.0.load(Ordering::Relaxed))
    }

    fn now_ms(&self) -> u64 {
        self.now().0 * 1_000
    }
}

enum History {
    Exact,
    Omit,
    LostEose,
}

#[test]
#[ignore = "requires same-build native bins and a qualified loopback TLS relay"]
fn native_founder_update_is_required_in_recipient_history() {
    run_native(TOTAL, || history(History::Exact));
}

#[test]
#[ignore = "requires same-build native bins and a qualified loopback TLS relay"]
fn native_history_refuses_omitted_commit_even_with_eose() {
    run_native(TOTAL, || history(History::Omit));
}

#[test]
#[ignore = "requires same-build native bins and a qualified loopback TLS relay"]
fn native_history_expires_without_eose() {
    run_native(TOTAL, || history(History::LostEose));
}

struct HistorySetup {
    selector: Selector,
    generation: u64,
    routing: NostrRoutingV1,
    founder: PeerFounderRuntime<FounderTransport>,
    group: GroupId,
    welcome: MessageId,
    delivery: Delivery,
    initial_leaves: Vec<cgka_traits::engine::GroupMemberLeaf>,
    initial: EpochPin,
    next: EpochPin,
    writes: Vec<GroupWrite>,
    activation_ms: u128,
    update_ms: u128,
}

async fn prepare_history(round: &Round, group_route: [u8; 32]) -> HistorySetup {
    let socket = round.socket();
    let daemon = round
        .start(Admission::Automatic, super::LostReply::None)
        .await;
    let started = Instant::now();
    let mut enrollment = round.send_offer(group_route).await;
    let selector = Selector::from(enrollment.consent.offer());
    let routing = enrollment.consent.offer().routing().clone();
    let adapter = FounderTransport {
        turn: Arc::new(Mutex::new(None)),
        sent: Arc::new(Mutex::new(None)),
    };
    let (mut founder, group, welcome) = peer_enrollment_founder_with_group(
        round.founder_root.path(),
        round.founder.clone(),
        round.signer,
        &routing,
        enrollment.consent.offer().room_id(),
        enrollment.consent.key_package().clone(),
        adapter.clone(),
    )
    .await;
    let initial = stable_pin(founder.runtime(), &group);
    let initial_membership = founder
        .runtime()
        .canonical_group_membership(&group)
        .unwrap();
    assert!(initial_membership.local_leaf_active);
    let delivery = round
        .finish_delivery(&mut enrollment, &welcome.payload)
        .await;
    let selected = entry(&socket, selector, peer::State::Activating).await;
    assert_no_command(&socket).await;
    daemon.stop().await;
    round.assert_stopped(&socket);
    let activation_ms = started.elapsed().as_millis();

    let prior_welcome = founder
        .runtime()
        .tracked_outbound_welcome_ids()
        .unwrap()
        .contains(&welcome.id);
    let update_started = Instant::now();
    let origin = round.relay.url.as_str();
    *adapter.turn.lock().await = Some(FounderTurn {
        account: founder.runtime().self_id(),
        group: group.clone(),
        route: group_route,
        endpoint: TransportEndpoint(origin.into()),
        socket: enrollment.socket,
        deadline: Instant::now() + STEP,
    });
    founder
        .self_update(&group)
        .await
        .expect("native founder SelfUpdate after actual Peer Delivery");
    let next = stable_pin(founder.runtime(), &group);
    let tracked_after = founder
        .runtime()
        .tracked_outbound_welcome_ids()
        .unwrap()
        .contains(&welcome.id);
    assert!(prior_welcome && tracked_after);
    assert_eq!(Some(next.epoch.0), initial.epoch.0.checked_add(1));
    assert_ne!(initial.identity, next.identity);
    let writes = round.relay.group_writes();
    let [
        GroupWrite {
            sender,
            record,
            duplicate: false,
            acknowledged: true,
        },
    ] = writes.as_slice()
    else {
        panic!("one distinct affirmative Group relay write for founder update");
    };
    assert_eq!(*sender, round.founder.public_key());
    assert_eq!(
        *adapter.sent.lock().await,
        Some(CommitDigest {
            id: record.id,
            bytes_sha256: record.bytes_sha256,
        }),
    );
    let update_ms = update_started.elapsed().as_millis();

    HistorySetup {
        selector,
        generation: selected.selection.generation,
        routing,
        founder,
        group,
        welcome: welcome.id,
        delivery,
        initial_leaves: initial_membership.member_leaves,
        initial,
        next,
        writes,
        activation_ms,
        update_ms,
    }
}

async fn history(case: History) {
    let group_route = random();
    let mut round = Round::initialize_with_group(Some(group_route)).await;
    let setup = prepare_history(&round, group_route).await;
    let HistorySetup {
        selector,
        generation,
        routing,
        founder,
        group,
        welcome,
        delivery,
        initial_leaves,
        initial,
        next,
        writes,
        activation_ms,
        update_ms,
    } = setup;
    let [write] = writes.as_slice() else {
        panic!("founder history is established by one Group publication")
    };
    let record = &write.record;
    let origin = round.relay.url.as_str();
    let mut lease = ProfileLease::acquire(round.root.path(), PROFILE).unwrap();
    let identity = material(&mut lease, &round.daemon);
    let route = Route::configured(origin).unwrap();
    let mut owner = PreparedConsentOwner::open_custody(
        &lease,
        &identity,
        crate::command_config::Role::Client,
        Policy {
            mode: Mode::Automatic,
            ..Policy::default()
        },
        &route,
        Wall::Fixture(round.wall.clone()),
    )
    .unwrap();
    let wall = round.wall.load(Ordering::Relaxed);
    let row = read_joined(&mut owner, selector, generation, wall, &delivery, &group);
    let recipient = open_recipient(
        &owner,
        &row,
        &round.wall,
        &group,
        &routing,
        Gate::unscoped_closed(round.wall.clone()),
    );
    assert_eq!(stable_pin(&recipient, &group), initial);
    let joined_membership = recipient.canonical_group_membership(&group).unwrap();
    assert!(joined_membership.local_leaf_active);
    let mut founding_leaves = initial_leaves;
    let mut joined_leaves = joined_membership.member_leaves;
    founding_leaves.sort_by_key(|leaf| leaf.leaf_index);
    joined_leaves.sort_by_key(|leaf| leaf.leaf_index);
    assert_eq!(founding_leaves, joined_leaves);

    let qualified = QualifiedOrigin::for_daemon_peer_fixture(
        round.relay.peer_mode().unwrap(),
        lease.root(),
        round.signer,
        round.founder.public_key(),
    )
    .unwrap();
    let mut connection = Owner::open(
        &qualified.connector,
        origin,
        round.signer,
        &identity.signer(),
        Instant::now() + WAIT,
    )
    .await
    .unwrap();
    match case {
        History::Exact => {}
        History::Omit => round.relay.group_fault(GroupFault::Omit(record.id)),
        History::LostEose => round.relay.group_fault(GroupFault::LoseEose),
    }

    let history_started = Instant::now();
    let mut fetch = None;
    let mut request = None;
    let page = connection
        .group_history_after_install(group_route, Instant::now() + STEP, |id| {
            recipient
                .mark_post_join_subscription_installed(&group)
                .map_err(|_| Failure::Binding)?;
            fetch = recipient
                .start_post_join_history_fetch(&group)
                .map_err(|_| Failure::Binding)?;
            request = Some(id.to_owned());
            Ok(())
        })
        .await;
    let fetch = fetch.expect("native fetch reserved at real subscription installation");
    let request = request.expect("exact registered subscription identifier");
    let cut = round
        .relay
        .group_history_cut(round.signer, &request)
        .expect("the relay recorded its exact pre-fault cut");
    assert_eq!(cut.records, [*record]);
    match case {
        History::Exact => {
            let page = page.expect("actual bounded history through EOSE");
            assert_eq!(page.request(), request);
            let mut recipient = recipient;
            let observed =
                process_history(&connection, &page, &mut recipient, &group, &cut, wall, next).await;
            assert!(observed);
            match_stable_roster(&recipient, founder.runtime(), &group, &routing);
            let done = recipient
                .finish_post_join_history_fetch(&group, &fetch)
                .unwrap()
                .unwrap();
            assert_eq!(done.outcome, PostJoinHistoryFetchOutcome::Fetched);
            let stored = recipient.post_join_history_fetch(&group).unwrap().unwrap();
            assert_eq!(stored.completion.as_ref(), Some(&done));
            assert!(
                recipient
                    .finish_post_join_history_fetch(&group, &fetch)
                    .unwrap()
                    .is_none()
            );
            recipient.mark_post_join_eose(&group).unwrap();
        }
        History::Omit => {
            let page = page.expect("EOSE alone is not the native history");
            assert_eq!(page.request(), request);
            assert!(page.records().is_empty());
            let mut recipient = recipient;
            assert!(
                !process_history(&connection, &page, &mut recipient, &group, &cut, wall, next)
                    .await
            );
            assert_eq!(stable_pin(&recipient, &group), initial);
            assert!(
                recipient
                    .post_join_history_fetch(&group)
                    .unwrap()
                    .unwrap()
                    .completion
                    .is_none()
            );
        }
        History::LostEose => {
            assert!(matches!(page, Err(Failure::Deadline)));
            assert!(
                recipient
                    .expire_post_join_history_fetch(&group, &fetch)
                    .unwrap()
                    .is_none()
            );
            round
                .wall
                .fetch_add(EOSE_TIMEOUT.as_secs() - 1, Ordering::Relaxed);
            assert!(
                recipient
                    .expire_post_join_history_fetch(&group, &fetch)
                    .unwrap()
                    .is_none()
            );
            round.wall.fetch_add(1, Ordering::Relaxed);
            let done = recipient
                .expire_post_join_history_fetch(&group, &fetch)
                .unwrap()
                .unwrap();
            assert_eq!(done.outcome, PostJoinHistoryFetchOutcome::DeadlineExpired);
            let stored = recipient.post_join_history_fetch(&group).unwrap().unwrap();
            assert_eq!(stored.completion.as_ref(), Some(&done));
            assert!(
                recipient
                    .finish_post_join_history_fetch(&group, &fetch)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(stable_pin(&recipient, &group), initial);
        }
    }
    if !matches!(case, History::LostEose) {
        connection.stop(Instant::now() + STEP).await.unwrap();
    }
    assert_eq!(round.relay.group_writes(), writes);
    assert_eq!(round.relay.peer_transcript().len(), 3);
    assert_eq!(round.ordinary.publication_attempts(7435), 0);
    assert_eq!(round.ordinary.publication_attempts(445), 0);
    round.relay.stop().await;
    round.ordinary.stop().await;
    assert!(
        founder
            .runtime()
            .tracked_outbound_welcome_ids()
            .unwrap()
            .contains(&welcome)
    );
    eprintln!(
        "peer_postjoin_history activation_ms={activation_ms} update_ms={update_ms} history_ms={} tracked_welcome_before=true tracked_welcome_after=true",
        history_started.elapsed().as_millis(),
    );
}

fn read_joined(
    owner: &mut PreparedConsentOwner<'_>,
    selector: Selector,
    generation: u64,
    wall: u64,
    delivery: &Delivery,
    group: &GroupId,
) -> crate::peer_invitation::HeldEvidence {
    let row = owner
        .state
        .checkpoint
        .read_custody(&owner.state.source, selector, generation, wall)
        .unwrap()
        .expect("the exact Activating row");
    assert_eq!(row.state(), CustodyState::Activating);
    let joined = owner
        .inspect_joined(row.stage(), delivery, row.package_digest())
        .unwrap();
    assert!(row.matches_pending_readback(&joined));
    assert_eq!(joined.coordinates().0, group.as_slice());
    row
}

fn open_recipient<A: TransportAdapter + KeyPackagePublisher + Clone>(
    owner: &PreparedConsentOwner<'_>,
    row: &crate::peer_invitation::HeldEvidence,
    wall: &Arc<AtomicU64>,
    group: &GroupId,
    routing: &NostrRoutingV1,
    gate: A,
) -> AccountDeviceRuntime<A, StaticTransportRouting, A> {
    let (binding, file) = row.stage();
    let original = owner.state.paths.reopen_provisional(binding, file).unwrap();
    let key = owner.state.paths.provisional_key(binding);
    let storage = SqliteAccountStorage::open_encrypted_with_options(
        original.path(),
        &SqlCipherKey::raw(*key),
        SqliteStorageOptions {
            journal_mode: SqliteJournalMode::Delete,
            ..SqliteStorageOptions::default()
        },
    )
    .unwrap();
    assert_eq!(storage.migration_summary().0, 0);
    let config = postjoin_session_config(storage, owner.state.signer.clone());
    let endpoints = routing
        .relays
        .iter()
        .cloned()
        .map(TransportEndpoint)
        .collect::<Vec<_>>();
    let router = StaticTransportRouting::new(endpoints.clone()).with_group_route(
        group.clone(),
        routing.nostr_group_id.to_vec(),
        endpoints,
    );
    AccountDeviceRuntime::open(config, || {
        Ok::<_, marmot_session::SessionError>((gate.clone(), router, gate))
    })
    .unwrap()
    .with_maintenance_sources(
        Arc::new(FixtureWall(wall.clone())),
        Arc::new(SystemMonotonicClock::default()),
        Arc::new(OsMaintenanceRandom),
    )
    .with_maintenance_timing(MaintenanceTiming {
        eose_timeout: EOSE_TIMEOUT,
        ..MaintenanceTiming::immediate()
    })
}

fn stable_pin(
    runtime: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    group: &GroupId,
) -> EpochPin {
    let pin = runtime.epoch_pin(group).unwrap();
    assert!(matches!(
        runtime.epoch_state(group),
        Some(EpochState::Stable { epoch }) if epoch == pin.epoch
    ));
    pin
}

async fn process_history(
    connection: &Owner,
    page: &GroupPage,
    recipient: &mut Recipient,
    group: &GroupId,
    cut: &GroupHistoryCut,
    wall: u64,
    target: EpochPin,
) -> bool {
    if !connection.accepts_group_page(page)
        || !page
            .records()
            .iter()
            .map(|record| record.fingerprint())
            .eq(cut.records.iter().copied())
    {
        return false;
    }
    let [record] = page.records() else {
        return false;
    };
    if !connection.accepts_group_record(record) {
        return false;
    }
    let delivery = record.delivery(group, wall);
    let digest: [u8; 32] = Sha256::digest(&delivery.message.payload).into();
    assert_eq!(digest, record.fingerprint().bytes_sha256);
    assert_eq!(delivery.message.id.as_slice(), record.fingerprint().id);
    let ingested = recipient.ingest_delivery(&delivery).await.unwrap();
    let accepted = match ingested.outcome {
        IngestOutcome::Processed => true,
        IngestOutcome::Buffered { group_id, .. } => &group_id == group,
        _ => false,
    };
    assert!(accepted && !ingested.left_object_unpersisted);
    timeout(Duration::from_secs(8), async {
        while recipient.has_pending_convergence_inputs(group).unwrap() {
            recipient.advance_convergence(group).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("bounded exact native convergence");
    stable_pin(recipient, group) == target
        && recipient
            .group_authority_view()
            .with_group_authority_snapshot(group, |storage, _| {
                Ok::<_, marmot_session::SessionError>(
                    storage.has_processed_transport_id(&delivery.message.id)?,
                )
            })
            .unwrap()
}

fn match_stable_roster(
    recipient: &Recipient,
    founder: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    group: &GroupId,
    route: &NostrRoutingV1,
) {
    let recipient_membership = recipient.canonical_group_membership(group).unwrap();
    let founder_membership = founder.canonical_group_membership(group).unwrap();
    assert!(recipient_membership.local_leaf_active && founder_membership.local_leaf_active);
    let mut recipient_leaves = recipient_membership.member_leaves;
    let mut founder_leaves = founder_membership.member_leaves;
    recipient_leaves.sort_by_key(|leaf| leaf.leaf_index);
    founder_leaves.sort_by_key(|leaf| leaf.leaf_index);
    assert_eq!(recipient_leaves, founder_leaves);
    let [a, b] = recipient_leaves.as_slice() else {
        panic!("two exact native leaves");
    };
    let recipient_id = recipient.self_id();
    let founder_id = founder.self_id();
    assert_ne!(recipient_id, founder_id);
    assert_eq!(a.member, founder_id);
    assert_eq!(a.leaf_index, 0);
    assert_eq!(b.member, recipient_id);
    assert_eq!(b.leaf_index, 1);
    let recipient_route = recipient
        .app_component(group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap()
        .unwrap();
    let founder_route = founder
        .app_component(group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap()
        .unwrap();
    assert_eq!(decode_nostr_routing_v1(&recipient_route).unwrap(), *route);
    assert_eq!(decode_nostr_routing_v1(&founder_route).unwrap(), *route);
}

#[path = "peer_postjoin_durable_wss_tests.rs"]
mod durable;
