//! Opt-in in-vitro experiment against the pinned native MDK.
use super::{ProofSigner, Publisher};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use cgka_traits::app_components::{
    AppComponentData, DeviceRosterV1, DeviceTagClaim, GROUP_MESSAGE_RETENTION_COMPONENT_ID,
    NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1, default_group_components, encode_nostr_routing_v1,
};
use cgka_traits::app_event::{
    AppMessageRetentionDecision, MARMOT_APP_EVENT_KIND_CHAT, MarmotAppEvent,
};
use cgka_traits::engine::{CreateGroupRequest, GroupEvent, KeyPackageSource, SendIntent};
use cgka_traits::group::ProtocolProfile;
use cgka_traits::maintenance::{KeyPackageLifecycleState, SignedPublicationArtifact};
use cgka_traits::storage::MaintenanceStorage;
use cgka_traits::transport::{TransportEnvelope, TransportMessage};
use cgka_traits::{
    GroupId, MemberId, MessageId, Timestamp, TransportAccountActivation, TransportAdapter,
    TransportAdapterError, TransportDelivery, TransportDeliveryPlane, TransportDeliverySource,
    TransportEndpoint, TransportEndpointReceipt, TransportGroupSync, TransportPublishReport,
    TransportPublishRequest,
};
use marmot_session::{
    AccountDeviceRuntime, KeyPackagePublication, KeyPackagePublishError, KeyPackagePublishReceipt,
    KeyPackagePublisher, SessionConfig, StaticTransportRouting, key_package_metadata,
};
use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use nostr::key::Keys;
use nostr::types::Timestamp as NostrTimestamp;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use storage_sqlite::{SqliteAccountStorage, SqliteStorageOptions};
use transport_nostr_peeler::NostrMlsPeeler;

const ENDPOINT: &str = "wss://terminal-proof.invalid/";
const TERMINAL_PURPOSE: &str = "marmish in-vitro terminal proof";
const RETENTION_SECS: u64 = 60;

type Runtime = AccountDeviceRuntime<OfflineAdapter, StaticTransportRouting, PendingPackageSigner>;

#[derive(Clone, Default)]
struct OfflineAdapter(Arc<Mutex<Option<TransportMessage>>>);

impl OfflineAdapter {
    fn take(&self) -> TransportMessage {
        self.0.lock().unwrap().take().expect("one in-vitro event")
    }
}

#[async_trait]
impl TransportAdapter for OfflineAdapter {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        panic!("the in-vitro proof never starts transport")
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        panic!("the in-vitro proof never subscribes")
    }

    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        panic!("the in-vitro proof never starts transport")
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        panic!("the in-vitro proof delivers only explicitly captured events")
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        let [endpoint] = request.target.endpoints() else {
            panic!("one in-vitro destination")
        };
        assert_eq!(endpoint.0, ENDPOINT);
        let message_id = request.message.id.clone();
        assert!(self.0.lock().unwrap().replace(request.message).is_none());
        Ok(TransportPublishReport {
            message_id,
            accepted: vec![TransportEndpointReceipt {
                endpoint: endpoint.clone(),
                accepted_at: None,
                ack_kind: None,
            }],
            failed: vec![],
            required_acks: request.required_acks,
        })
    }
}

struct PendingPackageSigner(Arc<Keys>);

#[async_trait]
impl KeyPackagePublisher for PendingPackageSigner {
    async fn prepare_key_package(
        &self,
        publication: KeyPackagePublication,
    ) -> Result<SignedPublicationArtifact, KeyPackagePublishError> {
        let dto = Publisher::publication(&publication)?
            .to_event_at(publication.created_at.0)
            .expect("native publication shape");
        let tags = dto.tags.into_iter().map(|tag| Tag::parse(tag).unwrap());
        let signed = EventBuilder::new(Kind::Custom(30443), dto.content)
            .tags(tags)
            .custom_created_at(NostrTimestamp::from_secs(publication.created_at.0))
            .finalize(self.0.as_ref())
            .expect("local signature");
        Ok(SignedPublicationArtifact {
            id: MessageId::new(signed.id.to_bytes().to_vec()),
            created_at: publication.created_at,
            bytes: serde_json::to_vec(&signed).unwrap(),
        })
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        panic!("a KeyPackage must never reach publication in this proof")
    }
}

struct Endpoint {
    account: MemberId,
    keys: Arc<Keys>,
    storage: SqliteAccountStorage,
    slot: String,
    runtime: Runtime,
    adapter: OfflineAdapter,
}

impl Endpoint {
    fn open(
        keys: Arc<Keys>,
        account: MemberId,
        peer: MemberId,
        group: GroupId,
        route: &[u8; 32],
    ) -> Self {
        let storage = SqliteAccountStorage::in_memory_with_options(SqliteStorageOptions {
            temp_store_memory: true,
            cipher_memory_security: true,
            ..SqliteStorageOptions::default()
        })
        .unwrap();
        let slot = hex::encode(random::<16>());
        storage
            .put_key_package_lifecycle(&KeyPackageLifecycleState::slot_only(slot.clone()))
            .unwrap();
        let config = SessionConfig::with_storage(
            storage.clone(),
            Duration::ZERO,
            account.as_slice().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(keys.clone())),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(keys.clone())))
        .supported_app_components(default_group_components().into_iter().chain([
            NOSTR_ROUTING_COMPONENT_ID,
            GROUP_MESSAGE_RETENTION_COMPONENT_ID,
        ]));
        let transport = vec![TransportEndpoint(ENDPOINT.into())];
        let routing = StaticTransportRouting::new(transport.clone())
            .with_inbox_route(peer, transport.clone())
            .with_group_route(group, route.to_vec(), transport);
        let adapter = OfflineAdapter::default();
        let runtime = AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((
                adapter.clone(),
                routing,
                PendingPackageSigner(keys.clone()),
            ))
        })
        .unwrap();
        Self {
            account,
            keys,
            storage,
            slot,
            runtime,
            adapter,
        }
    }

    fn delivery(
        &self,
        message: TransportMessage,
        plane: TransportDeliveryPlane,
    ) -> TransportDelivery {
        TransportDelivery {
            account_id: self.account.clone(),
            group_id_hint: None,
            received_at: Timestamp(now()),
            source: TransportDeliverySource {
                transport: message.source.clone(),
                plane,
                endpoint: Some(TransportEndpoint(ENDPOINT.into())),
                subscription_id: None,
                wire: None,
            },
            message,
        }
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn verify_event(message: &TransportMessage, kind: u16) -> Event {
    let signed: Event = serde_json::from_slice(&message.payload).unwrap();
    signed.verify().expect("valid Nostr ID and outer signature");
    assert_eq!(u16::from(signed.kind), kind);
    assert_eq!(signed.id.to_bytes().as_slice(), message.id.as_slice());
    signed
}

async fn round_trip(
    sender: &mut Endpoint,
    receiver: &mut Endpoint,
    group: &GroupId,
    route: &[u8; 32],
    sample: &[u8],
) -> Duration {
    let started = Instant::now();
    let created_at = now();
    let expected_sender = &sender.account;
    let payload = MarmotAppEvent::new(
        hex::encode(sender.account.as_slice()),
        created_at,
        MARMOT_APP_EVENT_KIND_CHAT,
        vec![],
        STANDARD.encode(sample),
    )
    .encode()
    .unwrap();
    sender
        .runtime
        .send(SendIntent::AppMessage {
            group_id: group.clone(),
            payload: payload.clone(),
            expected_epoch: None,
        })
        .await
        .unwrap();
    let message = sender.adapter.take();
    let event = verify_event(&message, 445);
    let expected_expiration = (created_at + RETENTION_SECS).to_string();
    assert!(
        event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["expiration", expected_expiration.as_str()])
    );
    assert_eq!(
        message.envelope,
        TransportEnvelope::GroupMessage {
            transport_group_id: route.to_vec()
        }
    );
    let incoming = receiver
        .runtime
        .ingest_delivery(&receiver.delivery(message, TransportDeliveryPlane::Group))
        .await
        .unwrap();
    let received = incoming
        .effects
        .events
        .iter()
        .find_map(|event| match event {
            GroupEvent::MessageReceived {
                group_id,
                sender,
                sender_leaf_index,
                payload,
                retention,
                ..
            } => {
                assert!(sender_leaf_index.is_some());
                assert!(group_id == group && sender == expected_sender);
                assert_eq!(
                    *retention,
                    Some(AppMessageRetentionDecision::new(created_at, RETENTION_SECS))
                );
                Some(payload)
            }
            _ => None,
        })
        .expect("authenticated MLS application delivery");
    assert!(
        received == &payload,
        "account and room authenticated the exact app event"
    );
    let app = MarmotAppEvent::decode(received).unwrap();
    assert!(
        STANDARD.decode(app.content).unwrap() == sample,
        "frame bytes match"
    );
    started.elapsed()
}

async fn prove(founder_index: usize) {
    let started = Instant::now();
    let keys = [Arc::new(Keys::generate()), Arc::new(Keys::generate())];
    let accounts = keys
        .each_ref()
        .map(|key| MemberId::new(key.public_key().to_bytes().to_vec()));
    assert_ne!(accounts[0], accounts[1]);
    let group = GroupId::new(random::<32>().to_vec());
    let route = random::<32>();
    let endpoint = |index: usize| {
        Endpoint::open(
            keys[index].clone(),
            accounts[index].clone(),
            accounts[1 - index].clone(),
            group.clone(),
            &route,
        )
    };
    let mut founder = endpoint(founder_index);
    let mut joiner = endpoint(1 - founder_index);
    assert_ne!(founder.slot, joiner.slot);
    assert!(founder.runtime.live_group_ids().unwrap().is_empty());
    assert!(joiner.runtime.live_group_ids().unwrap().is_empty());
    let construction = started.elapsed();
    let migrations = founder.storage.migration_summary().1 + joiner.storage.migration_summary().1;

    let mut package = joiner
        .runtime
        .prepare_fresh_key_package(vec![TransportEndpoint(ENDPOINT.into())])
        .await
        .unwrap();
    let lifecycle = joiner.storage.key_package_lifecycle().unwrap().unwrap();
    assert!(lifecycle.authored_event_id.is_none());
    assert!(lifecycle.current_key_package.is_none());
    let pending = lifecycle.pending_replacement.as_ref().unwrap();
    assert_eq!(pending.attempt_count, 0);
    assert_eq!(pending.key_package, package);
    let artifact = pending.signed_event.as_ref().unwrap();
    assert_eq!(
        lifecycle.held_publications().collect::<Vec<_>>(),
        vec![(&artifact.id, pending.key_package_ref.as_slice())]
    );
    let signed: Event = serde_json::from_slice(&artifact.bytes).unwrap();
    signed.verify().expect("real signed pending event");
    assert_eq!(signed.kind, Kind::Custom(30443));
    assert_eq!(
        signed.pubkey.to_bytes().as_slice(),
        joiner.account.as_slice()
    );
    assert_eq!(signed.id.to_bytes().as_slice(), artifact.id.as_slice());
    let metadata = key_package_metadata(&package).unwrap();
    assert_eq!(metadata.protocol_profile, ProtocolProfile::Current);
    assert_eq!(
        metadata.credential_identity_hex,
        hex::encode(joiner.account.as_slice())
    );
    assert_eq!(
        hex::decode(&metadata.key_package_ref_hex).unwrap(),
        pending.key_package_ref
    );
    package.source = Some(KeyPackageSource {
        event_id: artifact.id.clone(),
    });

    let room_start = Instant::now();
    let prepared = founder
        .runtime
        .prepare_create_group_with_chosen_id_and_audit_context(
            CreateGroupRequest {
                name: TERMINAL_PURPOSE.into(),
                description: TERMINAL_PURPOSE.into(),
                members: vec![package],
                required_features: vec![],
                initial_admins: vec![],
                app_components: vec![
                    AppComponentData {
                        component_id: NOSTR_ROUTING_COMPONENT_ID,
                        data: encode_nostr_routing_v1(
                            &NostrRoutingV1::new(route, vec![ENDPOINT.into()]).unwrap(),
                        )
                        .unwrap(),
                    },
                    AppComponentData {
                        component_id: GROUP_MESSAGE_RETENTION_COMPONENT_ID,
                        data: RETENTION_SECS.to_be_bytes().to_vec(),
                    },
                ],
            },
            group.clone(),
            None,
            Timestamp(now() + RETENTION_SECS),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(prepared.group_id, group);
    founder
        .runtime
        .publish_prepared_session_effects_with_audit_context(prepared.effects, Default::default())
        .await
        .unwrap();
    let welcome = founder.adapter.take();
    let welcome_event = verify_event(&welcome, 1059);
    assert_ne!(
        welcome_event.pubkey.to_bytes().as_slice(),
        founder.account.as_slice()
    );
    assert_eq!(
        welcome.envelope,
        TransportEnvelope::Welcome {
            recipient: joiner.account.clone()
        }
    );
    let peeler = NostrMlsPeeler::new().with_welcome_signer_arc(joiner.keys.clone());
    let (peeled, provenance) = peeler.peel_welcome_with_provenance(&welcome).await.unwrap();
    assert_eq!(peeled.sender.as_ref(), Some(&founder.account));
    assert_eq!(
        provenance.key_package_event_id.as_slice(),
        artifact.id.as_slice()
    );
    let joined = joiner
        .runtime
        .ingest_delivery(&joiner.delivery(welcome, TransportDeliveryPlane::AccountInbox))
        .await
        .unwrap();
    assert!(joined.effects.events.iter().any(|event| matches!(event,
        GroupEvent::GroupJoined { group_id, via_welcome, welcomer: Some(author), .. }
            if group_id == &group && via_welcome == &peeled.id && author == &founder.account
    )));
    let after = joiner.storage.key_package_lifecycle().unwrap().unwrap();
    let consumed = after.pending_replacement.as_ref().unwrap();
    assert_eq!(consumed.key_package_ref, pending.key_package_ref);
    assert_eq!(
        consumed.signed_event.as_ref().map(|event| &event.id),
        Some(&artifact.id)
    );
    assert!(consumed.consumed_at.is_some());
    assert_eq!(consumed.attempt_count, 0);
    assert!(after.authored_event_id.is_none());
    assert!(after.current_key_package.is_none());
    let expected = HashSet::from([founder.account.clone(), joiner.account.clone()]);
    for member in [&founder, &joiner] {
        assert_eq!(
            member.runtime.live_group_ids().unwrap().as_slice(),
            std::slice::from_ref(&group)
        );
        let record = member.runtime.group_record(&group).unwrap();
        assert_eq!(record.description, TERMINAL_PURPOSE);
        assert_eq!(record.protocol_profile, ProtocolProfile::Current);
        let membership = member.runtime.canonical_group_membership(&group).unwrap();
        assert!(membership.local_leaf_active);
        assert_eq!(membership.member_leaves.len(), 2);
        for leaf in &membership.member_leaves {
            assert_eq!(
                member
                    .runtime
                    .member_device_tag_claim(
                        &group,
                        record.epoch,
                        leaf.leaf_index,
                        &DeviceRosterV1::default(),
                    )
                    .unwrap(),
                Some(DeviceTagClaim::Unclaimed),
            );
        }
        assert_eq!(
            membership
                .member_leaves
                .into_iter()
                .map(|leaf| leaf.member)
                .collect::<HashSet<_>>(),
            expected
        );
    }
    let join_time = room_start.elapsed();
    let forward = round_trip(
        &mut founder,
        &mut joiner,
        &group,
        &route,
        b"\0\xff\x1bterminal output\n",
    )
    .await;
    let reverse = round_trip(
        &mut joiner,
        &mut founder,
        &group,
        &route,
        b"\x03\0\x7fkeystrokes\r",
    )
    .await;
    println!(
        "terminal_memory_probe founder={founder_index} construction_us={} migrations_us={} create_join_us={} founder_send_receive_us={} joiner_send_receive_us={} total_us={}",
        construction.as_micros(),
        migrations.as_micros(),
        join_time.as_micros(),
        forward.as_micros(),
        reverse.as_micros(),
        started.elapsed().as_micros()
    );
}

#[tokio::test]
#[ignore = "opt-in offline proof with the production SQLCipher memory-security setting"]
async fn unpublished_pending_package_joins_and_carries_terminal_frames() {
    tokio::time::timeout(Duration::from_secs(20), async {
        prove(0).await;
        prove(1).await;
    })
    .await
    .expect("bounded offline experiment");
}
