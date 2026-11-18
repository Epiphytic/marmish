//! Verify recovery of a joined Pending SelfRemove committed before route rejection.

use super::{
    Arc, DeniedTransport, FixedWall, Keys, NoIo, PURPOSE, RELAY, Store, endpoints, found_room,
    peer_self_remove_proof_tests::{MAX_STORED_SIGNED_WIRE_BYTES, check_positive},
    prepare_private_package, random,
};
use crate::mdk_adapter::Publisher;
use cgka_traits::engine::{EpochPin, SendIntent};
use cgka_traits::group::Group;
use cgka_traits::message::{MessageRecord, MessageState, StoredMessagePayload};
use cgka_traits::storage::{
    GroupStorage, LeaveRequest, LeaveRequestStorage, MemberValidationCacheStorage, MessageStorage,
    OutboundIntentStorage, StorageProvider,
};
use cgka_traits::{
    EpochId, FanoutTargetStatus, GroupId, GroupLifecycleState, OutboundFanout, TransportEnvelope,
    TransportMessage, TransportPublishReport, TransportPublishRequest, TransportPublishTarget,
};
use marmot_session::{
    AccountDeviceRuntime, GroupAuthoritySnapshot, OsMaintenanceRandom, PublishWork, SessionEffects,
    SessionError, StaticTransportRouting, SystemMonotonicClock,
};
use storage_sqlite::SqliteAccountStorage;
use zeroize::Zeroizing;

type Runtime = AccountDeviceRuntime<DeniedTransport, StaticTransportRouting, Publisher>;

struct RetainedLeave {
    authority: GroupAuthoritySnapshot,
    pin: EpochPin,
    group: Group,
    leave: LeaveRequest,
    fingerprint: [u8; 32],
    validation_marker: Option<Vec<u8>>,
    sent: Vec<MessageRecord>,
}

impl RetainedLeave {
    fn assert_same_leave_and_room(&self, prior: &Self) {
        assert_eq!(
            self.authority, prior.authority,
            "authenticated room authority"
        );
        assert!(self.pin == prior.pin, "authenticated branch pin");
        assert!(self.group == prior.group, "stored group record");
        assert!(self.leave == prior.leave, "stored LeaveRequest");
        assert!(self.sent == prior.sent, "stored Sent records");
    }

    fn assert_same(&self, prior: &Self) {
        self.assert_same_leave_and_room(prior);
        assert!(self.validation_marker == prior.validation_marker);
        assert!(
            self.fingerprint == prior.fingerprint,
            "replay state fingerprint"
        );
    }
}

fn retained_leave(runtime: &Runtime, group: &GroupId) -> RetainedLeave {
    let pin = runtime.epoch_pin(group).unwrap();
    assert!(runtime.pending_commit(group).unwrap().is_none());
    runtime
        .group_authority_view()
        .with_group_authority_snapshot(group, |storage, authority| {
            let leave = storage
                .leave_request(group)?
                .expect("persisted native LeaveRequest");
            assert!(leave.group_id == *group && leave.last_proposed_epoch == Some(pin.epoch));
            assert!(leave.blocked.is_none());
            assert_eq!(authority.lifecycle, GroupLifecycleState::Stable);
            assert_eq!(authority.facts.epoch, pin.epoch);
            assert!(
                authority.facts.is_member && !authority.facts.is_admin && !authority.disbanding
            );
            assert!(storage.list_queued_outbound_intents(group)?.is_empty());
            Ok::<_, SessionError>(RetainedLeave {
                authority,
                pin,
                group: storage.get_group(group)?,
                leave,
                fingerprint: storage
                    .group_replay_state_fingerprint(group)?
                    .expect("installed group state"),
                validation_marker: storage.validated_tree_marker(group)?,
                sent: storage.list_messages_in_states(group, &[MessageState::Sent], pin.epoch)?,
            })
        })
        .unwrap()
}

// This selection is for the fresh fixture's bounded inventory. Recheck epoch
// because the storage API returns matching states at or above its epoch floor.
fn exact_candidate(
    records: &[MessageRecord],
    group: &GroupId,
    epoch: EpochId,
    room_tag: &[u8],
) -> Option<TransportMessage> {
    let mut candidate = None;
    for record in records.iter().filter(|record| record.epoch == epoch) {
        if record.group_id != *group
            || record.state != MessageState::Sent
            || !(1..=MAX_STORED_SIGNED_WIRE_BYTES).contains(&record.payload.len())
        {
            return None;
        }
        match StoredMessagePayload::decode(&record.payload).ok()? {
            StoredMessagePayload::RawTransport(marker) => {
                // The native own-echo marker carries inner MLS bytes.
                let _inner = Zeroizing::new(marker.payload);
                continue;
            }
            StoredMessagePayload::SignedOpenMlsWire {
                exact_message,
                openmls_message,
                ..
            } => {
                let _inner = Zeroizing::new(openmls_message.payload);
                if candidate.is_some()
                    || exact_message.id != record.id
                    || openmls_message.id != record.id
                    || !matches!(
                        &exact_message.envelope,
                        TransportEnvelope::GroupMessage { transport_group_id }
                            if transport_group_id.as_slice() == room_tag
                    )
                {
                    return None;
                }
                candidate = Some(exact_message);
            }
            _ => return None,
        }
    }
    candidate
}

fn check_selection_refusals(retained: &RetainedLeave, group: &GroupId, room_tag: &[u8]) {
    let epoch = retained.pin.epoch;
    let refuses =
        |records: &[MessageRecord]| exact_candidate(records, group, epoch, room_tag).is_none();
    assert!(refuses(&[]));
    assert!(exact_candidate(&retained.sent, group, EpochId(epoch.0 + 1), room_tag).is_none());
    assert!(exact_candidate(&retained.sent, group, epoch, &[0; 32]).is_none());

    let mut ambiguous = retained.sent.clone();
    ambiguous.extend_from_slice(&retained.sent);
    assert!(refuses(&ambiguous));

    let mut corrupt = retained.sent.clone();
    corrupt[0].payload = vec![0xff];
    assert!(refuses(&corrupt));
    corrupt[0].payload = vec![0xff; MAX_STORED_SIGNED_WIRE_BYTES + 1];
    assert!(refuses(&corrupt));
}

fn exact_fanout(
    runtime: &Runtime,
    group: &GroupId,
    request: &TransportPublishRequest,
) -> OutboundFanout {
    // Global inventory makes it safe to invoke the native whole-account retry.
    let mut entries = runtime.outbound_fanouts().unwrap();
    assert_eq!(entries.len(), 1);
    let fanout = entries.pop().unwrap();
    assert!(fanout.group_id() == Some(group));
    assert!(fanout.request() == request);
    assert!(fanout.queued_intent_id().is_none());
    assert!(fanout.application_message().is_none());
    assert!(fanout.first_acceptance_observation().is_none());
    assert_eq!(
        fanout.target_status(0),
        Some(FanoutTargetStatus::RetryableUnavailable)
    );
    assert!(
        fanout
            .target_failure(0)
            .is_some_and(|failure| failure.pre_send)
    );
    fanout
}

fn assert_denied_report(reports: &[TransportPublishReport], request: &TransportPublishRequest) {
    let [report] = reports else {
        panic!("exactly one native aggregate denial expected");
    };
    assert!(report.message_id == request.message.id);
    assert_eq!(report.required_acks, request.required_acks);
    assert!(report.accepted.is_empty());
    let [failure] = report.failed.as_slice() else {
        panic!("exactly one denied endpoint expected");
    };
    assert!(failure.pre_send && failure.endpoint == request.target.endpoints()[0]);
}

fn open_pending(
    store: &Store,
    routing: StaticTransportRouting,
    at: u64,
) -> (SqliteAccountStorage, Runtime, DeniedTransport) {
    let transport = DeniedTransport::default();
    let (storage, runtime) = store.open_with_routing(routing, transport.clone(), Arc::new(NoIo));
    let runtime = runtime.with_maintenance_sources(
        Arc::new(FixedWall(at)),
        Arc::new(SystemMonotonicClock::default()),
        Arc::new(OsMaintenanceRandom),
    );
    (storage, runtime, transport)
}

#[tokio::test]
#[ignore = "private Pending decline route-rejection qualification"]
async fn peer_pending_unrouted_leave_recovers_exact_event() {
    let dir = tempfile::tempdir().unwrap();
    let inviter = Store::new(dir.path(), "inviter.sqlite", Arc::new(Keys::generate()));
    let pending = Store::new(dir.path(), "pending.sqlite", Arc::new(Keys::generate()));
    let (author_storage, mut author) = inviter.open(&pending.account());
    let (package_storage, mut staging) = pending.open(&inviter.account());
    let (package, _) =
        prepare_private_package(&package_storage, &mut staging, &pending.account()).await;
    drop(staging);
    drop(package_storage);

    let route = super::super::NostrRoutingV1::new(random().unwrap(), vec![RELAY.into()]).unwrap();
    let (group, welcomes) = found_room(&mut author, vec![package], &route, PURPOSE, vec![]).await;
    drop(author);
    drop(author_storage);
    let [welcome]: [_; 1] = welcomes.try_into().expect("one authenticated Welcome");

    let room_tag = route.nostr_group_id.as_slice();
    let at = super::super::now();
    let unrouted = StaticTransportRouting::new(endpoints());
    let (storage, mut runtime, denied) = open_pending(&pending, unrouted, at);
    let joined = runtime
        .ingest_delivery(&pending.delivery(welcome))
        .await
        .unwrap();
    assert!(joined.effects.events.iter().any(|event| matches!(
        event, cgka_traits::GroupEvent::GroupJoined { group_id, welcomer: Some(welcomer), .. }
            if group_id == &group && welcomer == &inviter.account()
    )));
    assert!(denied.take().is_empty());
    let before = runtime.epoch_pin(&group).unwrap();
    let joined_authority = runtime
        .group_authority_view()
        .with_group_authority_snapshot(&group, |_, authority| Ok::<_, SessionError>(authority))
        .unwrap();
    assert_eq!(joined_authority.lifecycle, GroupLifecycleState::Stable);
    assert!(joined_authority.facts.is_member && !joined_authority.facts.is_admin);
    assert!(!joined_authority.disbanding);
    assert!(runtime.pending_commit(&group).unwrap().is_none());
    assert!(storage.leave_request(&group).unwrap().is_none());
    assert!(
        storage
            .list_queued_outbound_intents(&group)
            .unwrap()
            .is_empty()
    );
    assert!(runtime.outbound_fanouts().unwrap().is_empty());

    // This is the only Leave call. The resulting event ID is an oracle, never
    // an input to discovery after reopening.
    let effects = runtime
        .send_host_change_with_audit_context(
            SendIntent::Leave {
                group_id: group.clone(),
            },
            Default::default(),
        )
        .await
        .expect("native Leave commits despite missing static route");
    let [failure] = effects.failures.as_slice() else {
        panic!("one route failure expected")
    };
    assert_eq!(failure.reason, "missing group route for transport group id");
    assert!(effects.queued.is_empty() && effects.reports.is_empty() && effects.fanout.is_empty());
    assert!(denied.take().is_empty());
    assert!(runtime.outbound_fanouts().unwrap().is_empty());
    let original = retained_leave(&runtime, &group);
    assert!(original.pin == before && original.authority == joined_authority);
    let original_row = storage
        .get_message(&failure.message_id)
        .expect("event durably committed");
    assert!(original_row.group_id == group && original_row.epoch == before.epoch);
    assert_eq!(original_row.state, MessageState::Sent);
    let provenance = StoredMessagePayload::decode(&original_row.payload).unwrap();
    let StoredMessagePayload::SignedOpenMlsWire {
        exact_message: oracle,
        openmls_message,
        ..
    } = provenance
    else {
        panic!("expected exact native signed event before reopen");
    };
    let _inner = Zeroizing::new(openmls_message.payload);
    assert!(oracle.id == failure.message_id);
    drop(runtime);
    drop(storage);

    // Match the original protocol-state fingerprint on a SQLCipher-only
    // reopen. Runtime hydration can populate MDK's validated-tree cache.
    let persisted = SqliteAccountStorage::open_encrypted(
        &pending.path,
        &super::super::SqlCipherKey::raw(pending.key),
    )
    .unwrap();
    assert!(
        persisted.group_replay_state_fingerprint(&group).unwrap() == Some(original.fingerprint),
        "original group replay state survives an encrypted reopen"
    );
    drop(persisted);

    let routed = StaticTransportRouting::new(endpoints()).with_group_route(
        group.clone(),
        room_tag.to_vec(),
        endpoints(),
    );
    let (storage, mut runtime, denied) = open_pending(&pending, routed.clone(), at);
    let recovered = retained_leave(&runtime, &group);
    assert!(runtime.outbound_fanouts().unwrap().is_empty());
    check_selection_refusals(&recovered, &group, room_tag);
    assert!(denied.take().is_empty());
    let msg = exact_candidate(&recovered.sent, &group, recovered.pin.epoch, room_tag)
        .expect("unique exact persisted proposal");
    let request = TransportPublishRequest {
        account_id: pending.account(),
        message: msg.clone(),
        target: TransportPublishTarget::Group {
            group_id: group.clone(),
            transport_group_id: room_tag.to_vec(),
            endpoints: endpoints(),
        },
        required_acks: 1,
    };
    check_positive(
        &mut runtime,
        &group,
        &pending.account(),
        room_tag,
        &request,
        false,
    )
    .await;
    assert!(msg == oracle, "exact event from the routing-failure oracle");
    recovered.assert_same_leave_and_room(&original);
    assert!(original.validation_marker.is_none() && recovered.validation_marker.is_some());
    retained_leave(&runtime, &group).assert_same(&recovered);
    assert!(runtime.outbound_fanouts().unwrap().is_empty());
    assert!(denied.take().is_empty());

    let staged = runtime
        .publish_session_effects(SessionEffects {
            events: vec![],
            publish: vec![PublishWork::Proposal {
                msg,
                queued_intent: None,
            }],
            queued: vec![],
            pending_convergence: vec![],
        })
        .await
        .unwrap();
    assert!(staged.queued.is_empty() && staged.events.is_empty());
    assert_denied_report(&staged.reports, &request);
    assert!(denied.take().as_slice() == std::slice::from_ref(&request));
    let frozen = exact_fanout(&runtime, &group, &request);
    retained_leave(&runtime, &group).assert_same(&recovered);
    drop(runtime);
    drop(storage);

    let (storage, mut runtime, denied) = open_pending(&pending, routed.clone(), at + 4);
    retained_leave(&runtime, &group).assert_same(&recovered);
    assert!(exact_fanout(&runtime, &group, &request) == frozen);
    let early = runtime.resume_outbound_fanouts().await.unwrap();
    assert!(early.reports.is_empty() && denied.take().is_empty());
    assert!(exact_fanout(&runtime, &group, &request) == frozen);
    drop(runtime);
    drop(storage);

    let (_storage, mut runtime, denied) = open_pending(&pending, routed, at + 5);
    assert!(exact_fanout(&runtime, &group, &request) == frozen);
    let retry = runtime.resume_outbound_fanouts().await.unwrap();
    assert_denied_report(&retry.reports, &request);
    assert!(denied.take().as_slice() == std::slice::from_ref(&request));
    exact_fanout(&runtime, &group, &request);
    retained_leave(&runtime, &group).assert_same(&recovered);
}

#[path = "peer_native_leave_proof_tests.rs"]
mod native_leave;
