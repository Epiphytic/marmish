//! Opt-in test of isolated first-Welcome custody using temporary native stores.
use super::{ProofSigner, Publisher, random};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cgka_traits::app_components::{
    AppComponentData, NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1, decode_nostr_routing_v1,
    default_group_components, encode_nostr_routing_v1,
};
use cgka_traits::engine::{
    CreateGroupRequest, GroupEvent, KeyPackage, KeyPackageSource, SendIntent,
};
use cgka_traits::group::ProtocolProfile;
use cgka_traits::maintenance::KeyPackageLifecycleState;
use cgka_traits::storage::{GroupStorage, MaintenanceStorage, MessageStorage};
use cgka_traits::transport::TransportMessage;
use cgka_traits::{
    GroupId, MemberId, MessageId, Timestamp, TransportAccountActivation, TransportAdapter,
    TransportAdapterError, TransportDelivery, TransportDeliveryPlane, TransportDeliverySource,
    TransportEndpoint, TransportGroupSync, TransportPublishReport, TransportPublishRequest,
};
use futures_util::{SinkExt as _, StreamExt as _};
use hkdf::Hkdf;
use marmot_session::{
    AccountDeviceRuntime, PublishWork, SessionConfig, StaticTransportRouting, key_package_metadata,
};
use nostr::event::{Event, Kind, Tag, UnsignedEvent};
use nostr::key::Keys;
use nostr::types::Timestamp as NostrTimestamp;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use storage_sqlite::{SqlCipherKey, SqliteAccountStorage};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use transport_nostr_adapter::{
    NostrPublishOutcome, NostrRelayClient, NostrSdkRelayClient, NostrSubscription,
    NostrTransportAdapter,
};
use transport_nostr_peeler::{NostrMlsPeeler, NostrTransportEvent, SdkSigner, gift_wrap_rumor};

const RELAY: &str = "wss://127.0.0.1:9/";
const PURPOSE: &str = "marmish/peer/1/00112233445566778899aabbccddeeff";
const DELIVERY: Kind = Kind::Custom(24860); // Fixture envelope only.
const DESCRIPTION: &str = "marmish-peer/1 peer transport";
const COMMAND_SUFFIX: [u8; 16] = [0x18; 16];

type StageRuntime = AccountDeviceRuntime<NoIo, StaticTransportRouting, Publisher>;

struct NoIo;

#[async_trait]
impl TransportAdapter for NoIo {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        panic!("quarantine must not activate transport")
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        panic!("quarantine must not subscribe")
    }

    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        panic!("quarantine must not deactivate the account")
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        panic!("quarantine must ingest only the exact candidate")
    }

    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        panic!("quarantine must not publish")
    }
}

#[async_trait]
impl NostrRelayClient for NoIo {
    async fn subscribe(&self, _: NostrSubscription) -> Result<(), TransportAdapterError> {
        panic!("quarantine must not subscribe")
    }

    async fn unsubscribe(&self, _: NostrSubscription) -> Result<(), TransportAdapterError> {
        panic!("quarantine must not unsubscribe")
    }

    async fn unsubscribe_account(
        &self,
        _: &MemberId,
        _: &[NostrSubscription],
    ) -> Result<(), TransportAdapterError> {
        panic!("quarantine must not unsubscribe the account")
    }

    async fn publish_event(
        &self,
        _: &[TransportEndpoint],
        _: &NostrTransportEvent,
        _: usize,
    ) -> Result<NostrPublishOutcome, TransportAdapterError> {
        panic!("quarantine must not publish a KeyPackage")
    }

    async fn publish_event_for_account(
        &self,
        _: &MemberId,
        _: &[TransportEndpoint],
        _: &NostrTransportEvent,
        _: usize,
    ) -> Result<NostrPublishOutcome, TransportAdapterError> {
        panic!("quarantine must not publish a KeyPackage")
    }
}

struct Store {
    path: PathBuf,
    key: [u8; 32],
    signer: Arc<Keys>,
}

impl Store {
    fn new(dir: &Path, name: &str, signer: Arc<Keys>) -> Self {
        Self {
            path: dir.join(name),
            key: random().unwrap(),
            signer,
        }
    }

    fn account(&self) -> MemberId {
        MemberId::new(self.signer.public_key().to_bytes().to_vec())
    }

    fn open(&self, peer: &MemberId) -> (SqliteAccountStorage, StageRuntime) {
        self.open_with(peer, endpoints(), NoIo, Arc::new(NoIo))
    }

    fn open_with<A: TransportAdapter>(
        &self,
        peer: &MemberId,
        relays: Vec<TransportEndpoint>,
        adapter: A,
        relay: Arc<dyn NostrRelayClient>,
    ) -> (
        SqliteAccountStorage,
        AccountDeviceRuntime<A, StaticTransportRouting, Publisher>,
    ) {
        marmish_engine::initialize_storage().unwrap();
        let storage =
            SqliteAccountStorage::open_encrypted(&self.path, &SqlCipherKey::raw(self.key)).unwrap();
        if storage.key_package_lifecycle().unwrap().is_none() {
            storage
                .put_key_package_lifecycle(&KeyPackageLifecycleState::slot_only(hex::encode(
                    random::<16>().unwrap(),
                )))
                .unwrap();
        }
        let config = SessionConfig::with_storage(
            storage.clone(),
            Duration::ZERO,
            self.account().as_slice().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(self.signer.clone())),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(self.signer.clone())))
        .supported_app_components(
            default_group_components()
                .into_iter()
                .chain([NOSTR_ROUTING_COMPONENT_ID]),
        );
        let routing =
            StaticTransportRouting::new(relays.clone()).with_inbox_route(peer.clone(), relays);
        let runtime = AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((
                adapter,
                routing,
                Publisher {
                    keys: self.signer.clone(),
                    relay,
                },
            ))
        })
        .unwrap();
        (storage, runtime)
    }

    fn delivery(&self, message: TransportMessage) -> TransportDelivery {
        TransportDelivery {
            account_id: self.account(),
            group_id_hint: None,
            received_at: Timestamp(now()),
            source: TransportDeliverySource {
                transport: message.source.clone(),
                plane: TransportDeliveryPlane::AccountInbox,
                endpoint: Some(TransportEndpoint(RELAY.into())),
                subscription_id: None,
                wire: None,
            },
            message,
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn endpoints() -> Vec<TransportEndpoint> {
    vec![TransportEndpoint(RELAY.into())]
}

async fn prepare_private_package(
    storage: &SqliteAccountStorage,
    runtime: &mut StageRuntime,
    expected: &MemberId,
) -> (KeyPackage, Vec<u8>) {
    let prepared = runtime
        .prepare_fresh_key_package(endpoints())
        .await
        .unwrap();
    let lifecycle = storage.key_package_lifecycle().unwrap().unwrap();
    assert!(lifecycle.current_key_package.is_none() && lifecycle.authored_event_id.is_none());
    let pending = lifecycle.pending_replacement.as_ref().unwrap();
    let artifact = pending.signed_event.as_ref().unwrap();
    assert_eq!(pending.key_package, prepared);
    assert_eq!(pending.attempt_count, 0);
    assert!(pending.consumed_at.is_none());
    assert_eq!(
        lifecycle.held_publications().collect::<Vec<_>>(),
        vec![(&artifact.id, pending.key_package_ref.as_slice())]
    );
    let signed = Event::from_json(&artifact.bytes).unwrap();
    signed.verify().unwrap();
    assert_eq!(signed.kind, Kind::Custom(30443));
    assert_eq!(signed.pubkey.to_bytes().as_slice(), expected.as_slice());
    assert_eq!(signed.id.to_bytes().as_slice(), artifact.id.as_slice());
    let package = KeyPackage {
        bytes: STANDARD.decode(signed.content).unwrap(),
        source: Some(KeyPackageSource {
            event_id: artifact.id.clone(),
        }),
        protocol_profile: ProtocolProfile::Current,
    };
    let metadata = key_package_metadata(&package).unwrap();
    assert_eq!(
        metadata.credential_identity_hex,
        hex::encode(expected.as_slice())
    );
    assert_eq!(metadata.protocol_profile, ProtocolProfile::Current);
    assert_eq!(
        hex::decode(metadata.key_package_ref_hex).unwrap(),
        pending.key_package_ref
    );
    (package, pending.key_package_ref.clone())
}

async fn found_room(
    runtime: &mut StageRuntime,
    members: Vec<KeyPackage>,
    route: &NostrRoutingV1,
    name: &str,
    admins: Vec<MemberId>,
) -> (GroupId, Vec<TransportMessage>) {
    let prepared = runtime
        .prepare_create_group_with_audit_context(
            CreateGroupRequest {
                name: name.into(),
                description: if name.starts_with(super::RoomPurpose::Command.prefix()) {
                    super::RoomPurpose::Command.description().into()
                } else {
                    DESCRIPTION.into()
                },
                members,
                required_features: vec![],
                initial_admins: admins,
                app_components: vec![AppComponentData {
                    component_id: NOSTR_ROUTING_COMPONENT_ID,
                    data: encode_nostr_routing_v1(route).unwrap(),
                }],
            },
            Default::default(),
        )
        .await
        .unwrap();
    let mut publications = prepared.effects.publish.into_iter();
    let Some(PublishWork::FoundingGroupCreated { welcomes }) = publications.next() else {
        panic!("Current profile must produce an independent Welcome obligation");
    };
    assert!(publications.next().is_none());
    (prepared.group_id, welcomes)
}

// The ordinary provider exercises actual Command wire payload authentication.
// Process admission/authorization is outside this fixture.
async fn ordinary_command_traffic(
    client: &Store,
    sender: &mut StageRuntime,
    receiver: &mut StageRuntime,
    server: &Store,
    group: &GroupId,
) {
    use crate::command_wire::{self as wire, Binding, Request, RequestOp};

    let mut id = random::<16>().unwrap();
    id[..6].copy_from_slice(&now().to_be_bytes()[2..]);
    let request = Request {
        binding: Binding {
            session: COMMAND_SUFFIX,
            client: client.signer.public_key().to_bytes(),
            server: server.signer.public_key().to_bytes(),
        },
        id,
        op: RequestOp::Status {
            invocation: random().unwrap(),
        },
    };
    let encoded = wire::encode_request(&request).unwrap();
    let payload = super::prepare_command_publication(
        group.as_slice(),
        request.binding.clone(),
        request.id,
        wire::request_digest(&encoded),
        encoded,
        request.binding.client,
    )
    .unwrap()
    .encoded_app_event;
    let mut effects = sender
        .session_mut()
        .send(SendIntent::AppMessage {
            group_id: group.clone(),
            payload: payload.clone(),
            expected_epoch: None,
        })
        .await
        .unwrap();
    let Some(PublishWork::ApplicationMessage { msg, .. }) = effects.publish.pop() else {
        panic!("ordinary command wire must produce a native MLS application message");
    };
    let mut delivery = server.delivery(msg);
    delivery.source.plane = TransportDeliveryPlane::Group;
    let receiver_effects = receiver
        .session_mut()
        .ingest_delivery(&delivery)
        .await
        .unwrap();
    assert!(receiver_effects.effects.events.iter().any(|event| {
        matches!(
            event,
            GroupEvent::MessageReceived { group_id, sender, payload: received, .. }
                if group_id == group && sender == &client.account() && received == &payload
        )
    }));
}

async fn verify_hints(
    message: &TransportMessage,
    recipient: &Store,
    server: &Store,
    package_id: &MessageId,
    expected: &NostrRoutingV1,
) -> Result<(), &'static str> {
    let peeler = NostrMlsPeeler::new().with_welcome_signer_arc(recipient.signer.clone());
    let (peeled, proof) = peeler
        .peel_welcome_with_provenance(message)
        .await
        .map_err(|_| "native_peel")?;
    // Native peeler first on the same bytes enforces the pinned tag/URL bounds.
    // The SDK extraction only recovers fields the public peeler omits.
    let signed = Event::from_json(&message.payload).map_err(|_| "wrapper")?;
    let unwrapped =
        nostr::nips::nip59::extract_rumor_async(&SdkSigner(recipient.signer.clone()), &signed)
            .await
            .map_err(|_| "rumor")?;
    if unwrapped.rumor.compute_id().to_bytes() != proof.rumor_event_id
        || unwrapped.sender != server.signer.public_key()
        || peeled.sender.as_ref() != Some(&server.account())
        || proof.key_package_event_id.as_slice() != package_id.as_slice()
    {
        return Err("provenance");
    }
    let hints: Vec<_> = unwrapped
        .rumor
        .tags
        .iter()
        .find(|tag| tag.as_slice().first().is_some_and(|name| name == "relays"))
        .ok_or("hints")?
        .as_slice()
        .iter()
        .skip(1)
        .cloned()
        .collect();
    if hints != expected.relays {
        return Err("hints");
    }
    Ok(())
}

async fn rewrap(
    welcome: &TransportMessage,
    recipient: &Store,
    server: &Store,
    mutate: impl FnOnce(&mut UnsignedEvent),
) -> TransportMessage {
    let signed = Event::from_json(&welcome.payload).unwrap();
    let mut inner =
        nostr::nips::nip59::extract_rumor_async(&SdkSigner(recipient.signer.clone()), &signed)
            .await
            .unwrap()
            .rumor;
    inner.pubkey = server.signer.public_key();
    inner.id = None;
    mutate(&mut inner);
    let wrapped = gift_wrap_rumor(server.signer.clone(), recipient.signer.public_key(), inner)
        .await
        .unwrap();
    NostrTransportEvent::from_nostr_event(&wrapped)
        .unwrap()
        .to_transport_message()
        .unwrap()
}

fn replace_tag(rumor: &mut UnsignedEvent, replacement: &[&str]) {
    let tags: Vec<_> = rumor
        .tags
        .iter()
        .filter(|tag| tag.as_slice()[0] != replacement[0])
        .cloned()
        .chain([Tag::parse(replacement.iter().copied()).unwrap()])
        .collect();
    rumor.tags = tags.into_iter().collect();
}

async fn delivery_carrier(
    server: &Store,
    client: &Store,
    welcome: &TransportMessage,
    offer: &[u8; 16],
) -> Event {
    gift_wrap_rumor(
        server.signer.clone(),
        client.signer.public_key(),
        UnsignedEvent::new(
            server.signer.public_key(),
            NostrTimestamp::from_secs(now()),
            DELIVERY,
            [
                Tag::parse(["p", &client.signer.public_key().to_hex()]).unwrap(),
                Tag::parse(["d", &hex::encode(offer)]).unwrap(),
            ],
            STANDARD.encode(&welcome.payload),
        ),
    )
    .await
    .unwrap()
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct NativeAuthority {
    epoch: cgka_traits::engine::EpochPin,
    leaves: Vec<(String, u32)>,
    own_leaf_hash: Vec<u8>,
}

struct ExpectedJoin<'a> {
    group: &'a GroupId,
    welcome: &'a MessageId,
    server: &'a Store,
    client: &'a Store,
    package_ref: &'a [u8],
    route: &'a NostrRoutingV1,
}

fn validate_join(
    storage: &SqliteAccountStorage,
    runtime: &StageRuntime,
    expected: &ExpectedJoin<'_>,
    require_join_receipt: bool,
) -> Result<NativeAuthority, &'static str> {
    let ExpectedJoin {
        group,
        welcome,
        server,
        client,
        package_ref,
        route,
    } = *expected;
    if require_join_receipt && !storage
        .list_pending_application_events()
        .unwrap()
        .iter()
        .any(|event| {
            matches!(
                event, GroupEvent::GroupJoined { group_id, via_welcome, welcomer: Some(author), .. }
                    if group_id == group && via_welcome == welcome && author == &server.account()
            )
        })
    {
        return Err("native_welcomer");
    }
    let session = runtime.session();
    let record = session.group_record(group).map_err(|_| "group")?;
    if record.protocol_profile != ProtocolProfile::Current
        || record.name != PURPOSE
        || record.description != DESCRIPTION
        || record.removed
    {
        return Err("purpose");
    }
    let roster = session
        .canonical_group_membership(group)
        .map_err(|_| "roster")?;
    let mut leaves: Vec<_> = roster
        .member_leaves
        .iter()
        .map(|leaf| (hex::encode(leaf.member.as_slice()), leaf.leaf_index))
        .collect();
    leaves.sort();
    let mut accounts = [
        server.signer.public_key().to_hex(),
        client.signer.public_key().to_hex(),
    ];
    accounts.sort();
    if !roster.local_leaf_active || !leaves.iter().map(|leaf| &leaf.0).eq(accounts.iter()) {
        return Err("roster");
    }
    if session.admin_pubkeys(group).unwrap() != [server.signer.public_key().to_bytes()] {
        return Err("admin");
    }
    let raw = session
        .app_component(group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap()
        .ok_or("route")?;
    if decode_nostr_routing_v1(&raw).unwrap() != *route {
        return Err("route");
    }
    let lifecycle = storage.key_package_lifecycle().unwrap().unwrap();
    let pending = lifecycle.pending_replacement.as_ref().ok_or("pending")?;
    if pending.consumed_at.is_none() || pending.key_package_ref != package_ref {
        return Err("consumed_ref");
    }
    Ok(NativeAuthority {
        epoch: session.epoch_pin(group).map_err(|_| "epoch")?,
        leaves,
        own_leaf_hash: session.own_leaf_hash(group).map_err(|_| "own_leaf")?,
    })
}

#[derive(Serialize, Deserialize)]
struct StageRecord {
    offer: [u8; 16],
    consent: String,
    expires: u64,
    stage_name: String,
    stage_key: [u8; 32],
    client: String,
    server: String,
    room: String,
    route: Vec<String>,
    package_id: String,
    package_ref: Vec<u8>,
    wrapper: String,
    wrapper_digest: [u8; 32],
    group: String,
    authority: Option<NativeAuthority>,
}

struct RegistryFile {
    path: PathBuf,
    key: [u8; 32],
    mac_key: [u8; 32],
}

impl RegistryFile {
    fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("registry.sqlite"),
            key: random().unwrap(),
            mac_key: random().unwrap(),
        }
    }

    fn open(&self) -> Connection {
        let conn = Connection::open(&self.path).unwrap();
        conn.pragma_update(None, "key", format!("x'{}'", hex::encode(self.key)))
            .unwrap();
        let version = conn
            .pragma_query_value(None, "cipher_version", |row| row.get::<_, String>(0))
            .unwrap();
        assert!(!version.is_empty());
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS proof(offer BLOB PRIMARY KEY, data BLOB NOT NULL, mac BLOB NOT NULL)",
        ).unwrap();
        conn
    }

    fn authenticate(&self, data: &[u8]) -> [u8; 32] {
        let domain = b"marmish staged Peer test registry v1";
        let mut tagged = domain.to_vec();
        tagged.extend_from_slice(data);
        Hkdf::<Sha256>::extract(Some(&self.mac_key), &tagged)
            .0
            .into()
    }

    fn save(&self, conn: &Connection, stage: &StageRecord) {
        let data = serde_json::to_vec(stage).unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO proof VALUES (?1, ?2, ?3)",
            params![stage.offer, &data, self.authenticate(&data)],
        )
        .unwrap();
    }

    fn load(&self, conn: &Connection, offer: &[u8; 16]) -> Option<StageRecord> {
        let (data, mac): (Vec<u8>, Vec<u8>) = conn
            .query_row(
                "SELECT data, mac FROM proof WHERE offer=?1",
                [offer],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap()?;
        if self.authenticate(&data).as_slice() != mac {
            return None;
        }
        let record: StageRecord = serde_json::from_slice(&data).ok()?;
        (record.offer == *offer && record.expires > now()).then_some(record)
    }
}

#[tokio::test]
#[ignore = "offline staged Peer proof"]
async fn pending_native_package_and_join_survive_provider_reopen() {
    let started = Instant::now();
    let dir = tempfile::tempdir().unwrap();
    let server = Store::new(dir.path(), "server.sqlite", Arc::new(Keys::generate()));
    let client = Store::new(dir.path(), "quarantine.sqlite", Arc::new(Keys::generate()));
    let ordinary = Store::new(dir.path(), "ordinary.sqlite", client.signer.clone());
    let (ordinary_storage, mut ordinary_runtime) = ordinary.open(&server.account());
    let (ordinary_package, _) =
        prepare_private_package(&ordinary_storage, &mut ordinary_runtime, &client.account()).await;
    let (server_storage, mut founder) = server.open(&client.account());
    let ordinary_route = NostrRoutingV1::new(random().unwrap(), vec![RELAY.into()]).unwrap();
    let name = format!(
        "{}{}",
        super::RoomPurpose::Command.prefix(),
        hex::encode(COMMAND_SUFFIX)
    );
    let (ordinary_group, mut ordinary_welcomes) = found_room(
        &mut founder,
        vec![ordinary_package],
        &ordinary_route,
        &name,
        vec![],
    )
    .await;
    let ordinary_welcome = ordinary_welcomes.pop().unwrap();
    founder
        .session()
        .mark_sent_welcome_delivered(&ordinary_welcome.id)
        .unwrap();
    ordinary_runtime
        .session_mut()
        .ingest_delivery(&ordinary.delivery(ordinary_welcome))
        .await
        .unwrap();
    ordinary_command_traffic(
        &ordinary,
        &mut ordinary_runtime,
        &mut founder,
        &server,
        &ordinary_group,
    )
    .await;
    let ordinary_package = ordinary_storage.key_package_lifecycle().unwrap();
    let ordinary_events = ordinary_storage.list_pending_application_events().unwrap();
    let ordinary_epoch = ordinary_runtime
        .session()
        .epoch_pin(&ordinary_group)
        .unwrap();
    let ordinary_routing = ordinary_runtime
        .session()
        .app_component(&ordinary_group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap();
    let offer_started = Instant::now();
    let (client_storage, mut quarantine) = client.open(&server.account());
    let (package, reference) =
        prepare_private_package(&client_storage, &mut quarantine, &client.account()).await;
    let package_id = package.source.as_ref().unwrap().event_id.clone();
    let route = NostrRoutingV1::new(random().unwrap(), vec![RELAY.into()]).unwrap();
    let (group, mut welcomes) =
        found_room(&mut founder, vec![package], &route, PURPOSE, vec![]).await;
    let welcome = welcomes.pop().unwrap();
    assert!(welcomes.is_empty());
    assert_eq!(founder.outstanding_welcome_deliveries().unwrap().len(), 1);
    verify_hints(&welcome, &client, &server, &package_id, &route)
        .await
        .unwrap();

    let wrong = rewrap(&welcome, &client, &server, |rumor| {
        replace_tag(rumor, &["e", &"31".repeat(32)]);
    })
    .await;
    let malformed_route = rewrap(&welcome, &client, &server, |rumor| {
        replace_tag(rumor, &["relays", "invalid"]);
    })
    .await;
    assert_eq!(
        verify_hints(&malformed_route, &client, &server, &package_id, &route).await,
        Err("native_peel")
    );
    let duplicate_route_tag = rewrap(&welcome, &client, &server, |rumor| {
        let duplicate = Tag::parse(["relays", "wss://127.0.0.1:10/"]).unwrap();
        let tags: Vec<_> = rumor.tags.iter().cloned().chain([duplicate]).collect();
        rumor.tags = tags.into_iter().collect();
    })
    .await;
    assert_eq!(
        verify_hints(&duplicate_route_tag, &client, &server, &package_id, &route).await,
        Err("native_peel")
    );
    assert_eq!(
        verify_hints(&welcome, &server, &server, &package_id, &route).await,
        Err("native_peel")
    );
    let unexpected_route = rewrap(&welcome, &client, &server, |rumor| {
        replace_tag(rumor, &["relays", "wss://127.0.0.1:10/"]);
    })
    .await;
    assert_eq!(
        verify_hints(&unexpected_route, &client, &server, &package_id, &route).await,
        Err("hints")
    );
    assert_eq!(
        verify_hints(&wrong, &client, &server, &package_id, &route).await,
        Err("provenance")
    );
    let receipt = quarantine
        .session_mut()
        .ingest_delivery(&client.delivery(wrong))
        .await
        .unwrap();
    assert!(receipt.effects.events.is_empty());
    assert!(client_storage.get_group(&group).is_err());
    assert_eq!(
        client_storage
            .key_package_lifecycle()
            .unwrap()
            .unwrap()
            .pending_replacement
            .unwrap()
            .consumed_at,
        None
    );

    let id = welcome.id.clone();
    // A normal intake recognizing exact pending-stage package ownership rejects the
    // raw artifact before forwarding it to the ordinary provider. No production
    // intake is installed by this test.
    let normal_lifecycle = ordinary_package.as_ref().unwrap();
    assert!(
        !normal_lifecycle
            .held_publications()
            .any(|(known, _)| known == &package_id)
    );
    assert!(!ordinary_storage.has_ingress_dedup_marker(&id).unwrap());
    let offer = random::<16>().unwrap();
    let registry = RegistryFile::new(dir.path());
    let carrier = delivery_carrier(&server, &client, &welcome, &offer).await;
    let extracted =
        nostr::nips::nip59::extract_rumor_async(&SdkSigner(client.signer.clone()), &carrier)
            .await
            .unwrap();
    assert_eq!(extracted.sender, server.signer.public_key());
    assert_eq!(extracted.rumor.kind, DELIVERY);
    assert!(
        extracted
            .rumor
            .tags
            .iter()
            .any(|tag| { tag.as_slice() == ["d", &hex::encode(offer)] })
    );
    assert!(
        extracted
            .rumor
            .tags
            .iter()
            .any(|tag| { tag.as_slice() == ["p", &client.signer.public_key().to_hex()] })
    );
    assert_eq!(
        STANDARD.decode(extracted.rumor.content).unwrap(),
        welcome.payload
    );
    let mut stage = StageRecord {
        offer,
        consent: "manual".into(),
        expires: now() + 24 * 60 * 60,
        stage_name: client.path.file_name().unwrap().to_str().unwrap().into(),
        stage_key: client.key,
        client: client.signer.public_key().to_hex(),
        server: server.signer.public_key().to_hex(),
        room: PURPOSE.into(),
        route: route.relays.clone(),
        package_id: hex::encode(package_id.as_slice()),
        package_ref: reference.clone(),
        wrapper: hex::encode(id.as_slice()),
        wrapper_digest: Sha256::digest(&welcome.payload).into(),
        group: hex::encode(group.as_slice()),
        authority: None,
    };
    registry.save(&registry.open(), &stage);
    let receipt = quarantine
        .session_mut()
        .ingest_delivery(&client.delivery(welcome))
        .await
        .unwrap();
    assert!(receipt.effects.publish.is_empty());
    let expected = ExpectedJoin {
        group: &group,
        welcome: &id,
        server: &server,
        client: &client,
        package_ref: &reference,
        route: &route,
    };
    let authority = validate_join(&client_storage, &quarantine, &expected, true).unwrap();
    assert_eq!(authority.own_leaf_hash.len(), 32);
    drop((quarantine, client_storage));

    // Simulated crash after the join transaction and before registry Ready.
    let reopen_started = Instant::now();
    let (reopened, quarantine) = client.open(&server.account());
    let connection = registry.open();
    let reopen_elapsed = reopen_started.elapsed();
    let pending = registry.load(&connection, &offer).unwrap();
    assert!(pending.authority.is_none());
    assert_eq!(pending.stage_key, client.key);
    assert!(validate_join(&ordinary_storage, &ordinary_runtime, &expected, true).is_err());
    assert_eq!(
        validate_join(&reopened, &quarantine, &expected, true).unwrap(),
        authority
    );
    stage.authority = Some(authority);
    registry.save(&connection, &stage);
    let offer_to_ready = offer_started.elapsed();
    drop((connection, reopened, quarantine));

    // Crash after registry Ready and before deleting the stage outbox receipt.
    let connection = registry.open();
    let (reopened, quarantine) = client.open(&server.account());
    let ready = registry.load(&connection, &offer).unwrap();
    assert_eq!(
        Some(validate_join(&reopened, &quarantine, &expected, true).unwrap()),
        ready.authority
    );
    reopened
        .delete_pending_application_events(std::slice::from_ref(&id))
        .unwrap();
    drop((connection, reopened, quarantine));
    let connection = registry.open();
    let (reopened, quarantine) = client.open(&server.account());
    assert!(
        reopened
            .list_pending_application_events()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        Some(validate_join(&reopened, &quarantine, &expected, false).unwrap()),
        registry.load(&connection, &offer).unwrap().authority
    );
    connection
        .execute("UPDATE proof SET mac=zeroblob(32)", [])
        .unwrap();
    assert!(registry.load(&connection, &offer).is_none());
    assert!(ordinary_storage.get_group(&group).is_err());
    assert_eq!(
        ordinary_storage.key_package_lifecycle().unwrap(),
        ordinary_package
    );
    assert_eq!(
        ordinary_storage.list_pending_application_events().unwrap(),
        ordinary_events
    );
    assert_eq!(
        ordinary_runtime.live_group_ids().unwrap(),
        vec![ordinary_group.clone()]
    );
    assert_eq!(
        ordinary_runtime
            .session()
            .epoch_pin(&ordinary_group)
            .unwrap(),
        ordinary_epoch
    );
    assert_eq!(
        ordinary_runtime
            .session()
            .app_component(&ordinary_group, NOSTR_ROUTING_COMPONENT_ID)
            .unwrap(),
        ordinary_routing
    );
    assert!(!ordinary_storage.has_ingress_dedup_marker(&id).unwrap());
    ordinary_command_traffic(
        &ordinary,
        &mut ordinary_runtime,
        &mut founder,
        &server,
        &ordinary_group,
    )
    .await;
    assert!(server_storage.get_group(&group).is_ok());
    let all_fixture_files_bytes: u64 = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    eprintln!(
        "peer_staged_fixture offer_to_ready_ms={} stage_and_registry_reopen_ms={} total_ms={} encrypted_fixture_files_bytes={all_fixture_files_bytes}",
        offer_to_ready.as_millis(),
        reopen_elapsed.as_millis(),
        started.elapsed().as_millis(),
    );
}

#[tokio::test]
#[ignore = "offline staged Peer proof"]
async fn authenticated_outer_sender_and_invalid_roster_do_not_authorize_native_join() {
    let dir = tempfile::tempdir().unwrap();
    let server = Store::new(dir.path(), "server.sqlite", Arc::new(Keys::generate()));
    let issuer = Store::new(
        dir.path(),
        "not-the-founder.sqlite",
        Arc::new(Keys::generate()),
    );
    let client = Store::new(
        dir.path(),
        "wrong-signer.sqlite",
        Arc::new(Keys::generate()),
    );
    let expanded = Store::new(dir.path(), "wrong-roster.sqlite", client.signer.clone());
    let (store, mut stage) = client.open(&server.account());
    let (extra_store, mut extra) = expanded.open(&server.account());
    let (_, mut founder) = server.open(&client.account());
    let route = NostrRoutingV1::new(random().unwrap(), vec![RELAY.into()]).unwrap();

    let (package, reference) = prepare_private_package(&store, &mut stage, &client.account()).await;
    let package_id = package.source.as_ref().unwrap().event_id.clone();
    let (group, mut welcomes) =
        found_room(&mut founder, vec![package], &route, PURPOSE, vec![]).await;
    let falsely_attributed = rewrap(&welcomes.pop().unwrap(), &client, &issuer, |_| {}).await;
    verify_hints(&falsely_attributed, &client, &issuer, &package_id, &route)
        .await
        .unwrap();
    let expected = ExpectedJoin {
        group: &group,
        welcome: &falsely_attributed.id,
        server: &issuer,
        client: &client,
        package_ref: &reference,
        route: &route,
    };
    stage
        .session_mut()
        .ingest_delivery(&client.delivery(falsely_attributed.clone()))
        .await
        .unwrap();
    assert!(store.get_group(&group).is_ok());
    assert!(store.list_pending_application_events().unwrap().iter().any(|event| {
        matches!(event, GroupEvent::GroupJoined { welcomer: Some(member), .. } if member == &server.account())
    }));
    assert_eq!(
        validate_join(&store, &stage, &expected, true),
        Err("native_welcomer")
    );

    let outsider = Store::new(dir.path(), "outsider.sqlite", Arc::new(Keys::generate()));
    let (outsider_store, mut outsider_stage) = outsider.open(&server.account());
    let (package, reference) =
        prepare_private_package(&extra_store, &mut extra, &expanded.account()).await;
    let (outsider_package, _) =
        prepare_private_package(&outsider_store, &mut outsider_stage, &outsider.account()).await;
    let package_id = package.source.as_ref().unwrap().event_id.clone();
    let (group, welcomes) = found_room(
        &mut founder,
        vec![package, outsider_package],
        &route,
        PURPOSE,
        vec![],
    )
    .await;
    let welcome = welcomes
        .into_iter()
        .find(|message| {
            message.envelope
                == cgka_traits::TransportEnvelope::Welcome {
                    recipient: expanded.account(),
                }
        })
        .unwrap();
    verify_hints(&welcome, &expanded, &server, &package_id, &route)
        .await
        .unwrap();
    let id = welcome.id.clone();
    extra
        .session_mut()
        .ingest_delivery(&expanded.delivery(welcome))
        .await
        .unwrap();
    assert!(extra_store.get_group(&group).is_ok());
    let expected = ExpectedJoin {
        group: &group,
        welcome: &id,
        server: &server,
        client: &expanded,
        package_ref: &reference,
        route: &route,
    };
    assert_eq!(
        validate_join(&extra_store, &extra, &expected, true),
        Err("roster")
    );
}

struct Loopback {
    endpoint: TransportEndpoint,
    observed: Arc<std::sync::Mutex<Vec<nostr::event::EventId>>>,
    task: JoinHandle<()>,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Loopback {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = TransportEndpoint(format!("ws://{}/", listener.local_addr().unwrap()));
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = observed.clone();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    stream = listener.accept() => {
                        let (stream, _) = stream.unwrap();
                        let received = received.clone();
                        connections.spawn(async move {
                            let mut ws = accept_async(stream).await.unwrap();
                            while let Some(Ok(message)) = ws.next().await {
                                let Message::Text(text) = message else {
                                    continue;
                                };
                                let frame: serde_json::Value = serde_json::from_str(&text).unwrap();
                                let reply = match frame[0].as_str() {
                                    Some("EVENT") => {
                                        let event: Event = serde_json::from_value(frame[1].clone()).unwrap();
                                        event.verify().unwrap();
                                        received.lock().unwrap().push(event.id);
                                        serde_json::json!(["OK", event.id, true, "fixture"])
                                    }
                                    Some("REQ") => serde_json::json!(["EOSE", frame[1]]),
                                    Some("CLOSE") => continue,
                                    unexpected => panic!("unexpected relay frame: {unexpected:?}"),
                                };
                                ws.send(Message::Text(reply.to_string().into())).await.unwrap();
                            }
                        });
                    }
                    next = connections.join_next(), if !connections.is_empty() => {
                        next.unwrap().unwrap();
                    }
                }
            }
        });
        Self {
            endpoint,
            observed,
            task,
        }
    }

    fn observed(&self) -> Vec<nostr::event::EventId> {
        self.observed.lock().unwrap().clone()
    }
}

#[tokio::test]
#[ignore = "loopback staged Peer proof"]
async fn disposition_precedes_native_loopback_startup_retry() {
    let dir = tempfile::tempdir().unwrap();
    let relay = Loopback::start().await;
    let relays = vec![relay.endpoint.clone()];
    let route = NostrRoutingV1::new(random().unwrap(), vec![relay.endpoint.0.clone()]).unwrap();
    let server = Store::new(dir.path(), "server.sqlite", Arc::new(Keys::generate()));
    let client = Store::new(dir.path(), "client.sqlite", Arc::new(Keys::generate()));
    let (store, mut stage) = client.open(&server.account());
    let (_, mut founder) =
        server.open_with(&client.account(), relays.clone(), NoIo, Arc::new(NoIo));
    let (package, _) = prepare_private_package(&store, &mut stage, &client.account()).await;

    let (confirmed_group, mut confirmed) =
        found_room(&mut founder, vec![package.clone()], &route, PURPOSE, vec![]).await;
    let (cancelled_group, mut cancelled) =
        found_room(&mut founder, vec![package.clone()], &route, PURPOSE, vec![]).await;
    let (control_group, mut control) =
        found_room(&mut founder, vec![package], &route, PURPOSE, vec![]).await;
    let confirmed = confirmed.pop().unwrap();
    let cancelled = cancelled.pop().unwrap();
    let control = control.pop().unwrap();

    let sdk = nostr_sdk::client::Client::new();
    let endpoint = nostr::types::RelayUrl::parse(&relay.endpoint.0).unwrap();
    sdk.add_relay(&endpoint).await.unwrap();
    sdk.try_connect_relay(&endpoint, Duration::from_secs(2))
        .await
        .unwrap();
    let network = Arc::new(NostrSdkRelayClient::with_account_signer(
        sdk.clone(),
        server.account(),
        server.signer.clone(),
    ));
    let carrier = delivery_carrier(&server, &client, &confirmed, &random().unwrap()).await;
    let wrapped = NostrTransportEvent::from_nostr_event(&carrier).unwrap();
    let ack = network
        .publish_event_for_account(&server.account(), &relays, &wrapped, 1)
        .await
        .unwrap();
    assert_eq!(ack.accepted.len(), 1);

    let registry = RegistryFile::new(dir.path());
    let conn = registry.open();
    conn.execute_batch("CREATE TABLE fixture_ack(carrier TEXT NOT NULL, welcome BLOB NOT NULL)")
        .unwrap();
    conn.execute(
        "INSERT INTO fixture_ack VALUES (?1,?2)",
        params![carrier.id.to_hex(), confirmed.id.as_slice()],
    )
    .unwrap();
    assert_eq!(founder.outstanding_welcome_deliveries().unwrap().len(), 3);
    drop((conn, founder));

    // Crash after the relay acknowledgement but before native disposition.
    let conn = registry.open();
    let (carrier_id, welcome_id): (String, Vec<u8>) = conn
        .query_row("SELECT carrier,welcome FROM fixture_ack", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(carrier_id, carrier.id.to_hex());
    assert_eq!(welcome_id, confirmed.id.as_slice());
    let (_, quarantine) = server.open_with(&client.account(), relays.clone(), NoIo, Arc::new(NoIo));
    assert_eq!(
        quarantine.outstanding_welcome_deliveries().unwrap().len(),
        3
    );
    quarantine
        .session()
        .mark_sent_welcome_delivered(&MessageId::new(welcome_id))
        .unwrap();
    quarantine
        .session()
        .withdraw_sent_welcome(&cancelled.id)
        .unwrap();
    drop((conn, quarantine));

    let (_, mut live) = server.open_with(
        &client.account(),
        relays,
        NostrTransportAdapter::new(network.clone()),
        network,
    );
    live.activate_transport(None).await.unwrap();
    live.sync_transport_groups(None).await.unwrap();
    live.deliver_owed_welcomes_with_audit_context(
        std::slice::from_ref(&confirmed.id),
        confirmed_group,
        Default::default(),
    )
    .await
    .unwrap();
    live.deliver_owed_welcomes_with_audit_context(
        std::slice::from_ref(&cancelled.id),
        cancelled_group,
        Default::default(),
    )
    .await
    .unwrap();
    live.retry_welcome_messages_with_audit_context(vec![cancelled], Default::default())
        .await
        .unwrap();
    let owed = live.outstanding_welcome_deliveries().unwrap();
    assert_eq!(owed, vec![(control_group, control.clone())]);
    live.retry_welcome_messages_with_audit_context(
        owed.into_iter().map(|(_, message)| message).collect(),
        Default::default(),
    )
    .await
    .unwrap();
    let raw_control = Event::from_json(&control.payload).unwrap();
    assert_eq!(relay.observed(), vec![carrier.id, raw_control.id]);
    assert!(live.outstanding_welcome_deliveries().unwrap().is_empty());
    sdk.disconnect().await;
}
