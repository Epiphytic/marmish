use super::{Admission, Error, Fatal, IdentityMaterial, MdkOwner, ProfileLease, RelayConnector};
use super::{evidence, socket_count};
use crate::client_inventory::{InventoryError, InventoryRequest};
use crate::mdk_adapter::{
    ActivePublisher, AppComponentData, CreateGroupRequest, DISPATCHER_LOST, KeyPackage,
    KeyPackagePublication, KeyPackagePublisher, MessageId, NOSTR_ROUTING_COMPONENT_ID,
    NostrRoutingV1, Observation, PREFIX, PURPOSE, Publisher, Timestamp, TransportAdapter,
    TransportAdapterError, TransportEndpoint, TransportEndpointFailureKind, TransportEnvelope,
    TransportMessage, TransportPublishRequest, TransportPublishTarget, encode_nostr_routing_v1,
};
use cgka_traits::{GroupId, TransportSource};
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use tokio::time::{Duration, Instant};

#[derive(Debug, Eq, PartialEq)]
struct Evidence {
    account: [Option<[u8; 32]>; 3],
    journal: [Option<[u8; 32]>; 3],
    sockets: usize,
}

impl Evidence {
    fn read(lease: &ProfileLease) -> Self {
        Self {
            account: evidence(lease.mdk_database()),
            journal: evidence(lease.journal_database()),
            sockets: socket_count(),
        }
    }
}

#[derive(Default)]
struct Visits {
    order: Vec<Observation>,
    evidence: Option<Evidence>,
}

pub(super) fn qualify(
    case: &str,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    connector: RelayConnector,
    admission: &Admission,
    fatal: &Fatal,
    displace: fn(),
) {
    use Observation::*;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let mut owner =
        MdkOwner::open_candidate(lease, material, connector, vec![], admission.clone()).unwrap();
    let inventory = || InventoryRequest::List {
        after: None,
        limit: 1,
    };
    assert!(matches!(
        owner.checked_client_inventory(inventory()),
        Ok(Err(InventoryError::ServerUnavailable))
    ));

    let (target, occurrence) = match case {
        "active_effect_inventory_before" => (InventoryBeforeQuery, 1),
        "active_effect_inventory_after" => (InventoryAfterQuery, 1),
        "active_effect_receive_pending" => (ReceiveBeforePoll, 2),
        "active_effect_receive_ready" => (ReceiveReady, 1),
        "active_effect_orphan_second" => (OrphanBeforeRecord, 2),
        "active_effect_publication" => (Publish, 1),
        _ => panic!("unrecognized effect cut"),
    };
    let initial_orphans = (target == OrphanBeforeRecord).then(|| {
        runtime.block_on(seed_orphans(&mut owner));
        Evidence::read(lease)
    });
    let visits = Arc::new(Mutex::new(Visits::default()));
    let observe = Arc::clone(&visits);
    let retained = Arc::clone(lease);
    owner.active_mode.set_observer(move |location| {
        let mut visits = observe.lock().unwrap();
        assert!(visits.order.len() < 8);
        visits.order.push(location);
        if location == target
            && visits
                .order
                .iter()
                .filter(|&&visit| visit == location)
                .count()
                == occurrence
        {
            assert!(visits.evidence.is_none());
            visits.evidence = Some(Evidence::read(&retained));
            displace();
        }
    });

    let expected: &[Observation] = match target {
        InventoryBeforeQuery | InventoryAfterQuery => {
            assert_dispatcher_lost(owner.checked_client_inventory(inventory()));
            if target == InventoryBeforeQuery {
                &[InventoryBeforeQuery]
            } else {
                &[InventoryBeforeQuery, InventoryAfterQuery]
            }
        }
        ReceiveBeforePoll => {
            let mut receive = pin!(owner.receive_one(Instant::now() + Duration::from_secs(1)));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(receive.as_mut().poll(&mut cx).is_pending());
            assert!(!fatal.is_tripped());
            let Poll::Ready(result) = receive.as_mut().poll(&mut cx) else {
                panic!("displaced receiver must fail on the next poll");
            };
            assert_dispatcher_lost(result);
            &[ReceiveBeforePoll, ReceiveBeforePoll]
        }
        ReceiveReady => {
            let mut receive = pin!(owner.receive_one(Instant::now() + Duration::from_secs(1)));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(receive.as_mut().poll(&mut cx).is_pending());
            let result = runtime.block_on(receive);
            assert_dispatcher_lost(result);
            &[]
        }
        OrphanBeforeRecord => {
            assert_dispatcher_lost(owner.reconcile_orphans());
            &[OrphanBeforeRecord, OrphanAfterForget, OrphanBeforeRecord]
        }
        Publish => {
            runtime.block_on(publication_refusals(&owner, material));
            &[Publish, PreparePackage]
        }
        _ => unreachable!(),
    };

    assert!(fatal.is_tripped());
    let visits = visits.lock().unwrap();
    if target == ReceiveReady {
        let (last, preceding) = visits.order.split_last().unwrap();
        assert_eq!(*last, ReceiveReady);
        // Timers and spurious wakes may poll more than once before Ready.
        assert!(preceding.len() >= 2);
        assert!(preceding.iter().all(|visit| *visit == ReceiveBeforePoll));
    } else {
        assert_eq!(visits.order, expected);
    }
    let cut_evidence = visits.evidence.as_ref().unwrap();
    if let Some(before) = initial_orphans {
        assert_ne!(
            cut_evidence.account, before.account,
            "first forget changed native storage"
        );
        assert_eq!(cut_evidence.journal, before.journal);
    }
    assert_eq!(&Evidence::read(lease), cut_evidence);
    // Park SQLCipher without closing/checkpointing. The bounded child exits immediately.
    std::mem::forget(owner);
}

fn assert_dispatcher_lost<T>(result: crate::Result<T>) {
    assert!(matches!(result, Err(Error::Storage(DISPATCHER_LOST))));
}

async fn seed_orphans(owner: &mut MdkOwner) {
    let mut created = Vec::new();
    for n in [1, 2] {
        let session = hex::encode([n; 16]);
        let route = NostrRoutingV1::new([n; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap();
        let (group, _) = owner
            .runtime
            .create_group(CreateGroupRequest {
                name: format!("{PREFIX}{session}"),
                description: PURPOSE.into(),
                members: vec![],
                required_features: vec![],
                initial_admins: vec![],
                app_components: vec![AppComponentData {
                    component_id: NOSTR_ROUTING_COMPONENT_ID,
                    data: encode_nostr_routing_v1(&route).unwrap(),
                }],
            })
            .await
            .unwrap();
        created.push(group);
    }
    created.sort_by(|a, b| a.as_slice().cmp(b.as_slice()));
    assert_eq!(owner.native_group_union().unwrap(), created);
    let mut live = owner.runtime.live_group_ids().unwrap();
    live.sort_by(|a, b| a.as_slice().cmp(b.as_slice()));
    assert_eq!(live, created);
    for group in &created {
        assert!(
            owner
                .journal
                .conversation_by_group(group.as_slice())
                .unwrap()
                .is_none()
        );
        assert!(
            owner
                .journal
                .command_room(group.as_slice())
                .unwrap()
                .is_none()
        );
    }
}

async fn publication_refusals(owner: &MdkOwner, material: &IdentityMaterial) {
    let endpoint = TransportEndpoint::from("wss://127.0.0.1:9/");
    let group = GroupId::new([1; 32]);
    let transport_group_id = vec![2; 32];
    let request = TransportPublishRequest {
        account_id: owner.account.clone(),
        message: TransportMessage {
            id: MessageId::new([3; 32]),
            payload: vec![0],
            timestamp: Timestamp(1),
            causal_deps: vec![],
            source: TransportSource("marmot.transport.nostr".into()),
            envelope: TransportEnvelope::GroupMessage {
                transport_group_id: transport_group_id.clone(),
            },
        },
        target: TransportPublishTarget::Group {
            group_id: group,
            transport_group_id,
            endpoints: vec![endpoint.clone()],
        },
        required_acks: 1,
    };
    let Err(TransportAdapterError::PublishEndpoints(failure)) =
        owner.publication.publish(request).await
    else {
        panic!("candidate gate must refuse before transport");
    };
    assert_eq!(failure.summary, DISPATCHER_LOST);
    assert!(failure.message_id.is_none());
    let [item] = failure.endpoint_failures.as_slice() else {
        panic!("exactly one endpoint");
    };
    assert_eq!(item.endpoint, endpoint);
    assert_eq!(item.reason, DISPATCHER_LOST);
    assert_eq!(
        item.kind,
        TransportEndpointFailureKind::RetryableUnavailable
    );
    assert!(item.pre_send);

    let publisher = ActivePublisher {
        inner: Publisher {
            keys: material.signer(),
            relay: owner.relay.clone(),
        },
        mode: owner.active_mode.clone(),
    };
    // Typed local DTO with sentinel bytes that would fail parsing if the signer ran.
    let Err(failure) = publisher
        .prepare_key_package(KeyPackagePublication {
            account_id: owner.account.clone(),
            key_package: KeyPackage::new(vec![0]),
            slot_id: "local-cut-proof".into(),
            created_at: Timestamp(1),
            endpoints: vec![endpoint],
        })
        .await
    else {
        panic!("candidate publisher must refuse before signing");
    };
    assert_eq!(failure.message, DISPATCHER_LOST);
    assert!(!failure.externally_exposed && !failure.artifact_unusable);
}
