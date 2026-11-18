//! Loopback-free probe of exact native leave retry and durable fanout behavior.
//!
//! The sibling probe authenticates the exact event as the local MLS leaf's SelfRemove.
//! Every adapter observation stays before physical publication.
use super::{
    Arc, Keys, NoIo, PURPOSE, RELAY, Store, TransportAdapter, TransportAdapterError,
    TransportEndpoint, TransportPublishReport, TransportPublishRequest, endpoints, found_room,
    prepare_private_package, random,
};
use cgka_traits::engine::SendIntent;
use cgka_traits::message::{MessageState, StoredMessagePayload};
use cgka_traits::storage::{LeaveRequestStorage, MessageStorage};
use cgka_traits::{
    EngineError, FanoutTargetStatus, GroupLifecycleState, Timestamp, TransportAccountActivation,
    TransportDelivery, TransportEndpointFailure, TransportEndpointFailureKind, TransportGroupSync,
    TransportPublishFailure,
};
use marmot_session::{
    OsMaintenanceRandom, StaticTransportRouting, SystemMonotonicClock, WallClock,
};
use std::sync::Mutex;
use transport_nostr_peeler::NostrTransportEvent;

#[derive(Clone, Default)]
struct DeniedTransport {
    captured: Arc<Mutex<Vec<TransportPublishRequest>>>,
}

impl DeniedTransport {
    fn take(&self) -> Vec<TransportPublishRequest> {
        std::mem::take(&mut *self.captured.lock().unwrap())
    }
}

#[async_trait::async_trait]
impl TransportAdapter for DeniedTransport {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        panic!("the leave probe cannot activate transport")
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        panic!("the leave probe cannot subscribe")
    }

    async fn deactivate_account(
        &self,
        _: &cgka_traits::MemberId,
    ) -> Result<(), TransportAdapterError> {
        panic!("the leave probe cannot deactivate transport")
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        panic!("the leave probe uses explicit authenticated inbox delivery")
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        let [endpoint] = request.target.endpoints() else {
            panic!("one isolated room destination")
        };
        assert_eq!(endpoint.0, RELAY);
        let failure = TransportEndpointFailure {
            endpoint: endpoint.clone(),
            reason: "probe refuses before transport".into(),
            kind: TransportEndpointFailureKind::RetryableUnavailable,
            pre_send: true,
            size_class: None,
            rejection_category: None,
        };
        let mut captured = self.captured.lock().unwrap();
        assert!(captured.is_empty(), "one exact native proposal expected");
        captured.push(request);
        Err(TransportAdapterError::PublishEndpoints(
            TransportPublishFailure::with_endpoint_failures(
                "probe refuses before transport",
                vec![failure],
            ),
        ))
    }
}

struct FixedWall(u64);

impl WallClock for FixedWall {
    fn now(&self) -> Timestamp {
        Timestamp(self.0)
    }

    fn now_ms(&self) -> u64 {
        self.0 * 1_000
    }
}

#[tokio::test]
#[ignore = "private native leave boundary qualification"]
async fn peer_pending_leave_public_runtime_redrives_exact_event() {
    let dir = tempfile::tempdir().unwrap();
    let server = Store::new(dir.path(), "inviter.sqlite", Arc::new(Keys::generate()));
    let client = Store::new(dir.path(), "pending.sqlite", Arc::new(Keys::generate()));
    let (founder_storage, mut founder) = server.open(&client.account());
    let (storage, mut staging) = client.open(&server.account());
    let (package, _) = prepare_private_package(&storage, &mut staging, &client.account()).await;
    drop(staging);
    drop(storage);

    let route = super::NostrRoutingV1::new(random().unwrap(), vec![RELAY.into()]).unwrap();
    let (group, mut welcomes) =
        found_room(&mut founder, vec![package], &route, PURPOSE, vec![]).await;
    let welcome = welcomes.pop().unwrap();
    assert!(welcomes.is_empty());

    let transport = DeniedTransport::default();
    let routing = StaticTransportRouting::new(endpoints()).with_group_route(
        group.clone(),
        route.nostr_group_id.to_vec(),
        endpoints(),
    );
    let at = super::now();
    let open = |at| {
        let (storage, runtime) =
            client.open_with_routing(routing.clone(), transport.clone(), Arc::new(NoIo));
        (
            storage,
            runtime.with_maintenance_sources(
                Arc::new(FixedWall(at)),
                Arc::new(SystemMonotonicClock::default()),
                Arc::new(OsMaintenanceRandom),
            ),
        )
    };
    let (storage, mut runtime) = open(at);
    let joined = runtime
        .ingest_delivery(&client.delivery(welcome))
        .await
        .unwrap();
    assert!(joined.effects.events.iter().any(|event| matches!(
        event,
        cgka_traits::GroupEvent::GroupJoined { group_id, welcomer: Some(welcomer), .. }
            if group_id == &group && welcomer == &server.account()
    )));
    assert!(transport.take().is_empty());
    assert!(
        !runtime
            .maintenance_obligations_for_group(&group)
            .unwrap()
            .is_empty()
    );

    let current = runtime
        .group_authority_view()
        .with_group_authority_snapshot(&group, |_, authority| {
            Ok::<_, marmot_session::SessionError>(authority)
        })
        .unwrap();
    assert_eq!(current.lifecycle, GroupLifecycleState::Stable);
    assert!(current.facts.is_member && !current.facts.is_admin);

    peer_self_remove_proof_tests::check_authenticated_negatives(
        &runtime,
        &storage,
        &founder_storage,
        &group,
        &client.account(),
        &server.account(),
        &route.nostr_group_id,
    );

    let send = SendIntent::Leave {
        group_id: group.clone(),
    };
    runtime
        .send_host_change_with_audit_context(send.clone(), Default::default())
        .await
        .unwrap();
    let publication = transport
        .take()
        .pop()
        .expect("captured before any wire call");

    peer_self_remove_proof_tests::check_positive(
        &mut runtime,
        &group,
        &client.account(),
        &route.nostr_group_id,
        &publication,
        false,
    )
    .await;
    let fingerprint =
        cgka_traits::storage::StorageProvider::group_replay_state_fingerprint(&storage, &group)
            .unwrap()
            .unwrap();

    let event = NostrTransportEvent::from_transport_message(&publication.message)
        .unwrap()
        .to_verified_nostr_event()
        .unwrap();
    assert_eq!(
        event.id.to_bytes().as_slice(),
        publication.message.id.as_slice()
    );
    drop(runtime);
    drop(storage);

    let persisted = super::SqliteAccountStorage::open_encrypted(
        &client.path,
        &super::SqlCipherKey::raw(client.key),
    )
    .unwrap();
    assert!(
        cgka_traits::storage::StorageProvider::group_replay_state_fingerprint(&persisted, &group)
            .unwrap()
            == Some(fingerprint),
        "the probe must preserve persisted group state across storage reopen"
    );
    drop(persisted);

    let (storage, mut runtime) = open(at + 4);
    peer_self_remove_proof_tests::check_positive(
        &mut runtime,
        &group,
        &client.account(),
        &route.nostr_group_id,
        &publication,
        true,
    )
    .await;
    let request = storage
        .leave_request(&group)
        .unwrap()
        .expect("native request survived");
    let record = storage.get_message(&publication.message.id).unwrap();
    assert_eq!(request.last_proposed_epoch, Some(record.epoch));
    assert_eq!(record.group_id, group);
    assert_eq!(record.state, MessageState::Sent);
    let retained = StoredMessagePayload::decode(&record.payload).unwrap();
    assert_eq!(retained.as_exact_transport(), Some(&publication.message));

    let stored = runtime.outbound_fanouts_for_group(&group).unwrap();
    let [frozen] = stored.as_slice() else {
        panic!("exactly one native fanout expected")
    };
    assert_eq!(frozen.request().message, publication.message);
    assert_eq!(
        frozen.request().target.endpoints(),
        &[TransportEndpoint(RELAY.into())]
    );
    assert_eq!(frozen.request().required_acks, publication.required_acks);
    assert_eq!(
        frozen.target_status(0),
        Some(FanoutTargetStatus::RetryableUnavailable)
    );
    runtime.resume_outbound_fanouts().await.unwrap();
    assert!(transport.take().is_empty());

    let repeated = runtime
        .send_host_change_with_audit_context(send, Default::default())
        .await
        .expect_err("same-epoch Leave is already durably requested");
    assert!(matches!(
        repeated.as_engine_error(),
        Some(EngineError::LeaveAlreadyRequested { group_id }) if group_id == &group
    ));
    assert!(transport.take().is_empty());
    drop(runtime);
    drop(storage);

    let (storage, mut runtime) = open(at + 5);
    peer_self_remove_proof_tests::check_positive(
        &mut runtime,
        &group,
        &client.account(),
        &route.nostr_group_id,
        &publication,
        false,
    )
    .await;
    runtime.resume_outbound_fanouts().await.unwrap();
    let retry = transport.take().pop().expect("original fanout due");
    assert_eq!(retry.message, publication.message);
    assert_eq!(retry.target, publication.target);
    assert_eq!(retry.account_id, publication.account_id);
    assert_eq!(retry.required_acks, publication.required_acks);
    peer_self_remove_proof_tests::check_authoritative_denials(
        &mut runtime,
        &storage,
        &group,
        &route.nostr_group_id,
        &retry,
    )
    .await;
}

#[path = "peer_self_remove_proof_tests.rs"]
mod peer_self_remove_proof_tests;
