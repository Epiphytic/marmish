//! Opt-in staged custody proof against the public MDK account runtime.
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cgka_traits::app_components::{
    AppComponentData, NOSTR_ROUTING_COMPONENT_ID, NostrRoutingV1,
    PRIVATE_USE_APP_COMPONENT_ID_START, decode_nostr_routing_v1, default_group_components,
    encode_nostr_routing_v1,
};
use cgka_traits::engine::{CreateGroupRequest, EpochPin, GroupEvent, KeyPackage, KeyPackageSource};
use cgka_traits::group::ProtocolProfile;
use cgka_traits::ingest::IngestOutcome;
use cgka_traits::maintenance::SignedPublicationArtifact;
use cgka_traits::message::MessageRecord;
use cgka_traits::storage::{MaintenanceStorage, MessageStorage, StorageError};
use cgka_traits::transport::TransportMessage;
use cgka_traits::{
    GroupId, MemberId, MessageId, Timestamp, TransportAccountActivation, TransportAdapter,
    TransportAdapterError, TransportDelivery, TransportDeliveryPlane, TransportDeliverySource,
    TransportEndpoint, TransportGroupSync, TransportPublishReport, TransportPublishRequest,
};
use futures_util::{SinkExt as _, StreamExt as _};
use hkdf::Hkdf;
use marmot_session::{
    AccountDeviceEffects, AccountDeviceRuntime, AccountError, AccountIdentityProofRequest,
    AccountIdentityProofSigner, KeyPackageEventDeletion, KeyPackagePublication,
    KeyPackagePublishError, KeyPackagePublishReceipt, KeyPackagePublisher,
    KeyPackageSlotWithdrawal, PublishWork, SessionConfig, SessionEffects, StaticTransportRouting,
    WelcomeWithdrawal, key_package_metadata,
};
use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag, UnsignedEvent};
use nostr::key::Keys;
use nostr::types::Timestamp as NostrTimestamp;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use storage_sqlite::{SqlCipherKey, SqliteAccountStorage};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use transport_nostr_adapter::{
    NostrKeyPackagePublication, NostrRelayClient, NostrSdkRelayClient, NostrTransportAdapter,
};
use transport_nostr_peeler::{NostrMlsPeeler, NostrTransportEvent, SdkSigner, gift_wrap_rumor};

const RELAY: &str = "wss://127.0.0.1:9/";
const PURPOSE: &str = "marmish/peer/1/00112233445566778899aabbccddeeff";
const DESCRIPTION: &str = "marmish-peer/1 peer transport";
const DELIVERY: Kind = Kind::Custom(24860); // Fixture envelope only.

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockedIo {
    Activate,
    Sync,
    Deactivate,
    Receive,
    Publish,
    PublishKeyPackage,
    WithdrawKeyPackage,
    DeleteKeyPackage,
}

#[derive(Clone, Default)]
struct Quarantine(Arc<Mutex<Vec<BlockedIo>>>);

impl Quarantine {
    fn attempts(&self) -> Vec<BlockedIo> {
        self.0.lock().unwrap().clone()
    }

    fn deny<T>(&self, method: BlockedIo, error: T) -> T {
        self.0.lock().unwrap().push(method);
        error
    }

    fn deny_package(&self, method: BlockedIo) -> KeyPackagePublishError {
        self.deny(method, KeyPackagePublishError::unexposed("quarantine"))
    }
}

#[async_trait]
impl TransportAdapter for Quarantine {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> Result<(), TransportAdapterError> {
        Err(self.deny(BlockedIo::Activate, TransportAdapterError::Closed))
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> Result<(), TransportAdapterError> {
        Err(self.deny(BlockedIo::Sync, TransportAdapterError::Closed))
    }

    async fn deactivate_account(&self, _: &MemberId) -> Result<(), TransportAdapterError> {
        Err(self.deny(BlockedIo::Deactivate, TransportAdapterError::Closed))
    }

    async fn receive(&self) -> Result<Option<TransportDelivery>, TransportAdapterError> {
        Err(self.deny(BlockedIo::Receive, TransportAdapterError::Closed))
    }

    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> Result<TransportPublishReport, TransportAdapterError> {
        Err(self.deny(BlockedIo::Publish, TransportAdapterError::Closed))
    }
}

struct Signer(Arc<Keys>);

impl AccountIdentityProofSigner for Signer {
    fn sign_account_identity_proof(
        &self,
        request: &AccountIdentityProofRequest,
    ) -> Result<[u8; 64], String> {
        if request.account_identity.as_slice() != self.0.public_key().to_bytes() {
            return Err("proof account mismatch".into());
        }
        let event = request
            .proof_event()?
            .finalize(self.0.as_ref())
            .map_err(|e| e.to_string())?;
        request.signature_from_signed_event(event)
    }
}

struct PackageSigner {
    keys: Arc<Keys>,
    slot: String,
    guard: Quarantine,
}

#[async_trait]
impl KeyPackagePublisher for PackageSigner {
    fn legacy_slot_id(&self, account: &MemberId) -> Result<Option<String>, KeyPackagePublishError> {
        assert_eq!(account.as_slice(), self.keys.public_key().to_bytes());
        Ok(Some(self.slot.clone()))
    }

    async fn prepare_key_package(
        &self,
        publication: KeyPackagePublication,
    ) -> Result<SignedPublicationArtifact, KeyPackagePublishError> {
        let metadata = key_package_metadata(&publication.key_package).unwrap();
        assert_eq!(
            metadata.credential_identity_hex,
            hex::encode(publication.account_id.as_slice())
        );
        assert_eq!(publication.slot_id, self.slot);
        let event = NostrKeyPackagePublication {
            client_name: None,
            account_id: publication.account_id,
            key_package: publication.key_package,
            key_package_slot_id: publication.slot_id,
            key_package_ref: metadata.key_package_ref_hex,
            mls_ciphersuite: format!("0x{:04x}", metadata.ciphersuite),
            mls_extensions: metadata
                .mls_extensions
                .iter()
                .map(|v| format!("0x{v:04x}"))
                .collect(),
            mls_proposals: metadata
                .mls_proposals
                .iter()
                .map(|v| format!("0x{v:04x}"))
                .collect(),
            app_components: metadata
                .app_components
                .iter()
                .filter(|id| **id >= PRIVATE_USE_APP_COMPONENT_ID_START)
                .map(|id| format!("0x{id:04x}"))
                .collect(),
            publish_endpoints: publication.endpoints,
        }
        .to_event_at(publication.created_at.0)
        .unwrap();
        let signed = EventBuilder::new(Kind::Custom(30443), event.content)
            .tags(event.tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
            .custom_created_at(NostrTimestamp::from_secs(publication.created_at.0))
            .finalize(self.keys.as_ref())
            .unwrap();
        Ok(SignedPublicationArtifact {
            id: MessageId::new(signed.id.to_bytes().to_vec()),
            created_at: publication.created_at,
            bytes: serde_json::to_vec(&NostrTransportEvent::from_nostr_event(&signed).unwrap())
                .unwrap(),
        })
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.guard.deny_package(BlockedIo::PublishKeyPackage))
    }

    async fn withdraw_key_package_slot(
        &self,
        _: &KeyPackageSlotWithdrawal,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.guard.deny_package(BlockedIo::WithdrawKeyPackage))
    }

    async fn delete_key_package_event(
        &self,
        _: &KeyPackageEventDeletion,
    ) -> Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.guard.deny_package(BlockedIo::DeleteKeyPackage))
    }
}

type Runtime<A = Quarantine> = AccountDeviceRuntime<A, StaticTransportRouting, PackageSigner>;

struct Store {
    path: PathBuf,
    key: [u8; 32],
    keys: Arc<Keys>,
    slot: String,
}

impl Store {
    fn new(dir: &Path, name: &str, keys: Arc<Keys>) -> Self {
        Self {
            path: dir.join(name),
            key: random(),
            keys,
            slot: hex::encode(random::<16>()),
        }
    }

    fn account(&self) -> MemberId {
        MemberId::new(self.keys.public_key().to_bytes().to_vec())
    }

    fn storage(&self) -> SqliteAccountStorage {
        marmish_engine::initialize_storage().unwrap();
        SqliteAccountStorage::open_encrypted(&self.path, &SqlCipherKey::raw(self.key)).unwrap()
    }

    fn open(
        &self,
        route: &StaticTransportRouting,
        guard: &Quarantine,
    ) -> (SqliteAccountStorage, Runtime) {
        self.open_with(route, guard, guard.clone())
    }

    fn open_with<A: TransportAdapter>(
        &self,
        route: &StaticTransportRouting,
        guard: &Quarantine,
        adapter: A,
    ) -> (SqliteAccountStorage, Runtime<A>) {
        let storage = self.storage();
        let config = SessionConfig::with_storage(
            storage.clone(),
            Duration::ZERO,
            self.account().as_slice().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(self.keys.clone())),
        )
        .account_identity_proof_signer(Arc::new(Signer(self.keys.clone())))
        .supported_app_components(
            default_group_components()
                .into_iter()
                .chain([NOSTR_ROUTING_COMPONENT_ID]),
        );
        let publisher = PackageSigner {
            keys: self.keys.clone(),
            slot: self.slot.clone(),
            guard: guard.clone(),
        };
        let runtime = AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((adapter, route.clone(), publisher))
        })
        .unwrap();
        (storage, runtime)
    }

    fn delivery(
        &self,
        message: TransportMessage,
        endpoint: &TransportEndpoint,
    ) -> TransportDelivery {
        TransportDelivery {
            account_id: self.account(),
            group_id_hint: None,
            received_at: Timestamp(now()),
            source: TransportDeliverySource {
                transport: message.source.clone(),
                plane: TransportDeliveryPlane::AccountInbox,
                endpoint: Some(endpoint.clone()),
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

async fn prepare_private_package(
    storage: &SqliteAccountStorage,
    runtime: &mut Runtime,
    expected: &Store,
    endpoint: &TransportEndpoint,
) -> (KeyPackage, Vec<u8>) {
    let prepared = runtime
        .prepare_fresh_key_package(vec![endpoint.clone()])
        .await
        .unwrap();
    let lifecycle = storage.key_package_lifecycle().unwrap().unwrap();
    assert!(lifecycle.current_key_package.is_none() && lifecycle.authored_event_id.is_none());
    let pending = lifecycle.pending_replacement.as_ref().unwrap();
    let artifact = pending.signed_event.as_ref().unwrap();
    assert_eq!(lifecycle.stable_slot_id, expected.slot);
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
    assert_eq!(
        signed.pubkey.to_bytes().as_slice(),
        expected.account().as_slice()
    );
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
        hex::encode(expected.account().as_slice())
    );
    assert_eq!(metadata.protocol_profile, ProtocolProfile::Current);
    assert_eq!(
        hex::decode(metadata.key_package_ref_hex).unwrap(),
        pending.key_package_ref
    );
    (package, pending.key_package_ref.clone())
}

async fn found_room(
    runtime: &mut Runtime,
    members: Vec<KeyPackage>,
    route: &NostrRoutingV1,
) -> (GroupId, Vec<TransportMessage>) {
    let prepared = runtime
        .prepare_create_group_with_audit_context(
            CreateGroupRequest {
                name: PURPOSE.into(),
                description: DESCRIPTION.into(),
                members,
                required_features: vec![],
                initial_admins: vec![],
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
    assert_eq!(
        runtime
            .group_record(&prepared.group_id)
            .unwrap()
            .protocol_profile,
        ProtocolProfile::Current
    );
    (prepared.group_id, welcomes)
}

async fn verify_hints(
    message: &TransportMessage,
    recipient: &Store,
    server: &Store,
    package_id: &MessageId,
    expected: &NostrRoutingV1,
) -> Result<(), &'static str> {
    let peeler = NostrMlsPeeler::new().with_welcome_signer_arc(recipient.keys.clone());
    let (peeled, proof) = peeler
        .peel_welcome_with_provenance(message)
        .await
        .map_err(|_| "native_peel")?;
    // Native peeler first on the same bytes enforces the pinned tag/URL bounds.
    // The SDK extraction only recovers fields the public peeler omits.
    let signed = Event::from_json(&message.payload).map_err(|_| "wrapper")?;
    let unwrapped =
        nostr::nips::nip59::extract_rumor_async(&SdkSigner(recipient.keys.clone()), &signed)
            .await
            .map_err(|_| "rumor")?;
    if unwrapped.rumor.compute_id().to_bytes() != proof.rumor_event_id
        || unwrapped.sender != server.keys.public_key()
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
        nostr::nips::nip59::extract_rumor_async(&SdkSigner(recipient.keys.clone()), &signed)
            .await
            .unwrap()
            .rumor;
    inner.pubkey = server.keys.public_key();
    inner.id = None;
    mutate(&mut inner);
    let wrapped = gift_wrap_rumor(server.keys.clone(), recipient.keys.public_key(), inner)
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
        server.keys.clone(),
        client.keys.public_key(),
        UnsignedEvent::new(
            server.keys.public_key(),
            NostrTimestamp::from_secs(now()),
            DELIVERY,
            [
                Tag::parse(["p", &client.keys.public_key().to_hex()]).unwrap(),
                Tag::parse(["d", &hex::encode(offer)]).unwrap(),
            ],
            STANDARD.encode(&welcome.payload),
        ),
    )
    .await
    .unwrap()
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Authority {
    epoch: EpochPin,
    local_leaf_index: u32,
}

#[derive(Clone)]
struct ExpectedJoin<'a> {
    group: &'a GroupId,
    welcome: &'a MessageId,
    founder: MemberId,
    client: MemberId,
    package_ref: &'a [u8],
    route: &'a NostrRoutingV1,
}

impl ExpectedJoin<'_> {
    fn event(&self) -> GroupEvent {
        GroupEvent::GroupJoined {
            group_id: self.group.clone(),
            via_welcome: self.welcome.clone(),
            welcomer: Some(self.founder.clone()),
            explicitly_confirmed: false,
        }
    }

    fn recover(
        &self,
        store: &Store,
        route: &StaticTransportRouting,
        guard: &Quarantine,
    ) -> Result<Authority, &'static str> {
        // The native open creates a database when its path is missing. An
        // existing Candidate may only recover the original stage.
        std::fs::File::open(&store.path).map_err(|_| "stage_unavailable")?;
        let (storage, runtime) = store.open(route, guard);
        self.verify(&storage, &runtime, guard, true)
    }

    fn verify(
        &self,
        storage: &SqliteAccountStorage,
        runtime: &Runtime,
        guard: &Quarantine,
        require_receipt: bool,
    ) -> Result<Authority, &'static str> {
        if !guard.attempts().is_empty() {
            return Err("transport");
        }
        if require_receipt
            && !storage
                .list_pending_application_events()
                .unwrap()
                .eq(&[self.event()])
        {
            return Err("receipt");
        }
        if runtime.live_group_ids().unwrap() != [self.group.clone()] {
            return Err("live_groups");
        }
        let group = runtime.group_record(self.group).map_err(|_| "group")?;
        if group.protocol_profile != ProtocolProfile::Current
            || group.name != PURPOSE
            || group.description != DESCRIPTION
            || group.removed
        {
            return Err("purpose");
        }
        let roster = runtime
            .canonical_group_membership(self.group)
            .map_err(|_| "roster")?;
        let local = roster
            .member_leaves
            .iter()
            .find(|leaf| leaf.member == self.client)
            .ok_or("local_leaf")?;
        if !roster.local_leaf_active
            || roster.member_leaves.len() != 2
            || !roster
                .member_leaves
                .iter()
                .any(|leaf| leaf.member == self.founder)
        {
            return Err("roster");
        }
        let admin: [u8; 32] = self.founder.as_slice().try_into().unwrap();
        if runtime.admin_pubkeys(self.group).unwrap() != [admin] {
            return Err("admin");
        }
        let raw = runtime
            .app_component(self.group, NOSTR_ROUTING_COMPONENT_ID)
            .unwrap()
            .ok_or("route")?;
        if decode_nostr_routing_v1(&raw).unwrap() != *self.route {
            return Err("route");
        }
        let state = storage.key_package_lifecycle().unwrap().unwrap();
        let pending = state.pending_replacement.ok_or("pending")?;
        if pending.consumed_at.is_none() || pending.key_package_ref != self.package_ref {
            return Err("consumed");
        }
        Ok(Authority {
            epoch: runtime.epoch_pin(self.group).unwrap(),
            local_leaf_index: local.leaf_index,
        })
    }
}

fn assert_no_ingress(storage: &SqliteAccountStorage, id: &MessageId) {
    assert!(!storage.has_ingress_dedup_marker(id).unwrap());
    assert!(matches!(
        storage.get_message(id),
        Err(StorageError::NotFound)
    ));
}

fn admissible_effects(
    effects: &AccountDeviceEffects,
    expected: &GroupEvent,
    guard: &Quarantine,
) -> bool {
    guard.attempts().is_empty()
        && effects
            == &AccountDeviceEffects {
                events: vec![expected.clone()],
                ..AccountDeviceEffects::default()
            }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Candidate {
    stage: String,
    account: MemberId,
    founder: MemberId,
    group: GroupId,
    package: MessageId,
    package_ref: Vec<u8>,
    welcome: MessageId,
    wrapper_digest: [u8; 32],
    carrier: String,
    route: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Ready {
    candidate: Candidate,
    authority: Authority,
}

struct Registry {
    path: PathBuf,
    key: [u8; 32],
    mac_key: [u8; 32],
}

impl Registry {
    fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("registry.sqlite"),
            key: random(),
            mac_key: random(),
        }
    }

    fn open(&self) -> Connection {
        let conn = Connection::open(&self.path).unwrap();
        conn.pragma_update(None, "key", format!("x'{}'", hex::encode(self.key)))
            .unwrap();
        let version: String = conn
            .pragma_query_value(None, "cipher_version", |row| row.get(0))
            .unwrap();
        assert!(!version.is_empty());
        conn.execute_batch("CREATE TABLE IF NOT EXISTS proof (name TEXT PRIMARY KEY, data BLOB NOT NULL, mac BLOB NOT NULL)").unwrap();
        conn
    }

    fn mac(&self, name: &str, data: &[u8]) -> [u8; 32] {
        let mut tagged = b"marmish peer public runtime fixture v1\0".to_vec();
        tagged.extend_from_slice(name.as_bytes());
        tagged.push(0);
        tagged.extend_from_slice(data);
        Hkdf::<Sha256>::extract(Some(&self.mac_key), &tagged)
            .0
            .into()
    }

    fn save(&self, conn: &Connection, name: &str, value: &impl Serialize) {
        let data = serde_json::to_vec(value).unwrap();
        conn.execute(
            "INSERT INTO proof VALUES (?1, ?2, ?3)",
            params![name, &data, self.mac(name, &data)],
        )
        .unwrap();
    }

    fn read<T: DeserializeOwned>(&self, conn: &Connection, name: &str) -> Option<T> {
        let (data, mac): (Vec<u8>, Vec<u8>) = conn
            .query_row(
                "SELECT data, mac FROM proof WHERE name = ?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()?;
        (self.mac(name, &data).as_slice() == mac).then_some(())?;
        serde_json::from_slice(&data).ok()
    }
}

async fn unwrapped_delivery(
    server: &Store,
    client: &Store,
    welcome: &TransportMessage,
) -> (Event, TransportMessage) {
    let offer = random();
    let carrier = delivery_carrier(server, client, welcome, &offer).await;
    let signed = nostr::nips::nip59::extract_rumor_async(&SdkSigner(client.keys.clone()), &carrier)
        .await
        .unwrap();
    assert_eq!(signed.sender, server.keys.public_key());
    assert_eq!(signed.rumor.kind, DELIVERY);
    let tags: Vec<_> = signed.rumor.tags.iter().map(|tag| tag.as_slice()).collect();
    assert_eq!(
        tags,
        [
            ["p", &client.keys.public_key().to_hex()].as_slice(),
            ["d", &hex::encode(offer)].as_slice(),
        ]
    );
    let exact = STANDARD.decode(signed.rumor.content).unwrap();
    assert_eq!(exact, welcome.payload);
    let raw = Event::from_json(&exact).unwrap();
    raw.verify().unwrap();
    let message = NostrTransportEvent::from_nostr_event(&raw)
        .unwrap()
        .to_transport_message()
        .unwrap();
    assert_eq!(message, *welcome);
    (carrier, message)
}

#[tokio::test]
#[ignore = "opt-in public runtime Peer custody proof"]
async fn exact_welcome_and_ready_survive_public_runtime_recovery() {
    let started = Instant::now();
    let dir = tempfile::tempdir().unwrap();
    let server = Store::new(dir.path(), "founder.sqlite", Arc::new(Keys::generate()));
    let client = Store::new(dir.path(), "stage.sqlite", Arc::new(Keys::generate()));
    let ordinary = Store::new(dir.path(), "ordinary.sqlite", client.keys.clone());
    let endpoint = TransportEndpoint(RELAY.into());
    let routes = StaticTransportRouting::new(vec![endpoint.clone()]);
    let guard = Quarantine::default();
    let (ordinary_storage, mut ordinary_runtime) = ordinary.open(&routes, &guard);
    let (ordinary_package, _) = prepare_private_package(
        &ordinary_storage,
        &mut ordinary_runtime,
        &ordinary,
        &endpoint,
    )
    .await;
    let (_, mut founder) = server.open(&routes, &guard);
    let ordinary_route = NostrRoutingV1::new(random(), vec![endpoint.0.clone()]).unwrap();
    let (ordinary_group, mut ordinary_welcomes) =
        found_room(&mut founder, vec![ordinary_package], &ordinary_route).await;
    let ordinary_welcome = ordinary_welcomes.pop().unwrap();
    let baseline = ordinary_runtime
        .ingest_delivery(&ordinary.delivery(ordinary_welcome, &endpoint))
        .await
        .unwrap();
    assert_eq!(baseline.outcome, IngestOutcome::Processed);
    let ordinary_events = ordinary_storage.list_pending_application_events().unwrap();
    assert_eq!(ordinary_events.len(), 1);
    let ordinary_lifecycle = ordinary_storage.key_package_lifecycle().unwrap();
    let ordinary_pin = ordinary_runtime.epoch_pin(&ordinary_group).unwrap();
    let ordinary_baseline = ordinary_runtime
        .app_component(&ordinary_group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap();

    let (storage, mut runtime) = client.open(&routes, &guard);
    let (package, package_ref) =
        prepare_private_package(&storage, &mut runtime, &client, &endpoint).await;
    let package_id = package.source.as_ref().unwrap().event_id.clone();
    let route = NostrRoutingV1::new(random(), vec![endpoint.0.clone()]).unwrap();
    let (group, mut welcomes) = found_room(&mut founder, vec![package], &route).await;
    let (carrier, welcome) = unwrapped_delivery(&server, &client, &welcomes.pop().unwrap()).await;
    let exact_id = welcome.id.clone();
    verify_hints(&welcome, &client, &server, &package_id, &route)
        .await
        .unwrap();

    let changed_route = rewrap(&welcome, &client, &server, |rumor| {
        replace_tag(rumor, &["relays", "wss://127.0.0.1:10/"]);
    })
    .await;
    assert_eq!(
        verify_hints(&changed_route, &client, &server, &package_id, &route).await,
        Err("hints")
    );
    let wrong_package = rewrap(&welcome, &client, &server, |rumor| {
        replace_tag(rumor, &["e", &"31".repeat(32)]);
    })
    .await;
    assert_eq!(
        verify_hints(&wrong_package, &client, &server, &package_id, &route).await,
        Err("provenance")
    );
    let wrong_package_id = wrong_package.id.clone();
    // A diagnostic pass still exercises the public MDK rejection on the isolated store.
    let ignored = runtime
        .ingest_delivery(&client.delivery(wrong_package, &endpoint))
        .await
        .unwrap();
    assert!(ignored.effects.events.is_empty());
    assert!(runtime.live_group_ids().unwrap().is_empty());
    assert!(
        storage
            .key_package_lifecycle()
            .unwrap()
            .unwrap()
            .pending_replacement
            .unwrap()
            .consumed_at
            .is_none()
    );

    let delivery = client.delivery(welcome, &endpoint);
    let wrong_account = TransportDelivery {
        account_id: server.account(),
        ..delivery.clone()
    };
    assert!(matches!(
        runtime.ingest_delivery(&wrong_account).await,
        Err(AccountError::WrongAccountDelivery)
    ));
    assert!(runtime.live_group_ids().unwrap().is_empty());

    let registry = Registry::new(dir.path());
    let conn = registry.open();
    let candidate = Candidate {
        stage: client.path.file_name().unwrap().to_str().unwrap().into(),
        account: client.account(),
        founder: server.account(),
        group: group.clone(),
        package: package_id,
        package_ref: package_ref.clone(),
        welcome: exact_id.clone(),
        wrapper_digest: Sha256::digest(&delivery.message.payload).into(),
        carrier: carrier.id.to_hex(),
        route: route.relays.clone(),
    };
    registry.save(&conn, "candidate", &candidate);
    assert_eq!(registry.read(&conn, "candidate"), Some(candidate));
    let candidate: Candidate = registry.read(&conn, "candidate").unwrap();
    let expected = ExpectedJoin {
        group: &group,
        welcome: &exact_id,
        founder: server.account(),
        client: client.account(),
        package_ref: &package_ref,
        route: &route,
    };

    let ingest_started = Instant::now();
    let receipt = runtime.ingest_delivery(&delivery).await.unwrap();
    assert_eq!(receipt.outcome, IngestOutcome::Processed);
    assert!(!receipt.left_object_unpersisted);
    assert!(
        admissible_effects(&receipt.effects, &expected.event(), &guard),
        "{:?}",
        receipt.effects
    );
    assert_eq!(
        storage.list_pending_application_events().unwrap(),
        [expected.event()]
    );
    let authority = expected.verify(&storage, &runtime, &guard, true).unwrap();
    let unapproved_route = NostrRoutingV1::new(random(), route.relays.clone()).unwrap();
    let wrong = ExpectedJoin {
        route: &unapproved_route,
        ..expected.clone()
    };
    assert_eq!(wrong.verify(&storage, &runtime, &guard, true), Err("route"));
    let ingest_millis = ingest_started.elapsed().as_secs_f64() * 1000.;
    drop((conn, storage, runtime));

    // Reopen before Ready and reject a swapped provider key without opening the runtime.
    assert!(
        SqliteAccountStorage::open_encrypted(&client.path, &SqlCipherKey::raw(ordinary.key))
            .is_err()
    );
    let conn = registry.open();
    assert_eq!(
        registry.read::<Candidate>(&conn, "candidate").as_ref(),
        Some(&candidate)
    );
    assert!(registry.read::<Ready>(&conn, "ready").is_none());
    let unavailable = dir.path().join("temporarily-unavailable.sqlite");
    std::fs::rename(&client.path, &unavailable).unwrap();
    assert_eq!(
        expected.recover(&client, &routes, &guard),
        Err("stage_unavailable")
    );
    assert!(!client.path.exists());
    assert!(registry.read::<Ready>(&conn, "ready").is_none());
    std::fs::rename(&unavailable, &client.path).unwrap();
    let recovered_authority = expected.recover(&client, &routes, &guard).unwrap();
    assert_eq!(recovered_authority, authority);
    let ready = Ready {
        candidate,
        authority: recovered_authority,
    };
    registry.save(&conn, "ready", &ready);
    drop(conn);

    // Reopen after committed Ready but before exact outbox acknowledgement.
    let (storage, runtime) = client.open(&routes, &guard);
    let conn = registry.open();
    assert_eq!(
        registry.read::<Ready>(&conn, "ready").as_ref(),
        Some(&ready)
    );
    assert_eq!(
        expected.verify(&storage, &runtime, &guard, true).as_ref(),
        Ok(&ready.authority)
    );
    let sentinel_id = MessageId::new(random::<32>().to_vec());
    let sentinel = GroupEvent::GroupJoined {
        group_id: group.clone(),
        via_welcome: sentinel_id.clone(),
        welcomer: Some(server.account()),
        explicitly_confirmed: false,
    };
    // Synthetic sentinel tests the exact deletion selector only. The outbox
    // requires its own message row before it accepts any application event.
    storage
        .put_message(&MessageRecord {
            id: sentinel_id,
            payload: vec![],
            ..storage.get_message(&exact_id).unwrap()
        })
        .unwrap();
    storage.put_pending_application_event(&sentinel).unwrap();
    storage
        .delete_pending_application_events(std::slice::from_ref(&exact_id))
        .unwrap();
    drop((conn, storage, runtime));

    let (storage, runtime) = client.open(&routes, &guard);
    let conn = registry.open();
    assert_eq!(
        registry.read::<Ready>(&conn, "ready").as_ref(),
        Some(&ready)
    );
    assert_eq!(
        storage.list_pending_application_events().unwrap(),
        [sentinel]
    );
    assert_eq!(
        expected.verify(&storage, &runtime, &guard, false).as_ref(),
        Ok(&ready.authority)
    );
    conn.execute(
        "UPDATE proof SET mac = zeroblob(32) WHERE name = 'ready'",
        [],
    )
    .unwrap();
    assert!(registry.read::<Ready>(&conn, "ready").is_none());

    assert_eq!(
        ordinary_storage.key_package_lifecycle().unwrap(),
        ordinary_lifecycle
    );
    assert_eq!(
        ordinary_storage.list_pending_application_events().unwrap(),
        ordinary_events
    );
    assert_eq!(
        ordinary_runtime.live_group_ids().unwrap(),
        std::slice::from_ref(&ordinary_group)
    );
    assert_eq!(
        ordinary_runtime.epoch_pin(&ordinary_group).unwrap(),
        ordinary_pin
    );
    assert_eq!(
        ordinary_runtime
            .app_component(&ordinary_group, NOSTR_ROUTING_COMPONENT_ID)
            .unwrap(),
        ordinary_baseline
    );
    for id in [
        &exact_id,
        &changed_route.id,
        &wrong_package_id,
        &wrong_account.message.id,
    ] {
        assert_no_ingress(&ordinary_storage, id);
    }
    assert!(guard.attempts().is_empty());
    eprintln!(
        "public_peer_inbound ingest_to_verified_ms={ingest_millis:.2} total_ms={:.2}",
        started.elapsed().as_secs_f64() * 1000.
    );
}

#[tokio::test]
#[ignore = "opt-in public runtime Peer custody proof"]
async fn wrong_native_welcomer_and_extra_leaf_are_not_admitted() {
    let dir = tempfile::tempdir().unwrap();
    let server = Store::new(dir.path(), "server.sqlite", Arc::new(Keys::generate()));
    let issuer = Store::new(
        dir.path(),
        "outer-signer.sqlite",
        Arc::new(Keys::generate()),
    );
    let client = Store::new(
        dir.path(),
        "stage-signer.sqlite",
        Arc::new(Keys::generate()),
    );
    let expanded = Store::new(dir.path(), "stage-roster.sqlite", client.keys.clone());
    let ordinary = Store::new(dir.path(), "ordinary.sqlite", client.keys.clone());
    let outsider = Store::new(dir.path(), "outsider.sqlite", Arc::new(Keys::generate()));
    let endpoint = TransportEndpoint(RELAY.into());
    let routing = StaticTransportRouting::new(vec![endpoint.clone()]);
    let route = NostrRoutingV1::new(random(), vec![endpoint.0.clone()]).unwrap();
    let guard = Quarantine::default();
    let (_, mut founder) = server.open(&routing, &guard);
    let (ordinary_storage, ordinary_runtime) = ordinary.open(&routing, &guard);
    let ordinary_lifecycle = ordinary_storage.key_package_lifecycle().unwrap();

    let (storage, mut stage) = client.open(&routing, &guard);
    let (package, package_ref) =
        prepare_private_package(&storage, &mut stage, &client, &endpoint).await;
    let package_id = package.source.as_ref().unwrap().event_id.clone();
    let (group, mut welcomes) = found_room(&mut founder, vec![package], &route).await;
    let falsely_attributed = rewrap(&welcomes.pop().unwrap(), &client, &issuer, |_| {}).await;
    let (_, message) = unwrapped_delivery(&issuer, &client, &falsely_attributed).await;
    verify_hints(&message, &client, &issuer, &package_id, &route)
        .await
        .unwrap();
    let expected = ExpectedJoin {
        group: &group,
        welcome: &message.id,
        founder: issuer.account(),
        client: client.account(),
        package_ref: &package_ref,
        route: &route,
    };
    let receipt = stage
        .ingest_delivery(&client.delivery(message.clone(), &endpoint))
        .await
        .unwrap();
    assert_eq!(receipt.outcome, IngestOutcome::Processed);
    let actual = ExpectedJoin {
        founder: server.account(),
        ..expected.clone()
    };
    assert_eq!(
        storage.list_pending_application_events().unwrap(),
        [actual.event()]
    );
    assert!(!admissible_effects(
        &receipt.effects,
        &expected.event(),
        &guard
    ));
    assert_eq!(
        expected.verify(&storage, &stage, &guard, true),
        Err("receipt")
    );
    assert_no_ingress(&ordinary_storage, &message.id);

    let (storage, mut stage) = expanded.open(&routing, &guard);
    let (outsider_storage, mut outsider_runtime) = outsider.open(&routing, &guard);
    let (package, package_ref) =
        prepare_private_package(&storage, &mut stage, &expanded, &endpoint).await;
    let package_id = package.source.as_ref().unwrap().event_id.clone();
    let (extra_leaf, _) = prepare_private_package(
        &outsider_storage,
        &mut outsider_runtime,
        &outsider,
        &endpoint,
    )
    .await;
    let (group, welcomes) = found_room(&mut founder, vec![package, extra_leaf], &route).await;
    let welcome = welcomes
        .into_iter()
        .find(|message| {
            message.envelope
                == cgka_traits::TransportEnvelope::Welcome {
                    recipient: expanded.account(),
                }
        })
        .unwrap();
    let (_, message) = unwrapped_delivery(&server, &expanded, &welcome).await;
    verify_hints(&message, &expanded, &server, &package_id, &route)
        .await
        .unwrap();
    let expected = ExpectedJoin {
        group: &group,
        welcome: &message.id,
        founder: server.account(),
        client: expanded.account(),
        package_ref: &package_ref,
        route: &route,
    };
    let delivery = expanded.delivery(message.clone(), &endpoint);
    let receipt = stage.ingest_delivery(&delivery).await.unwrap();
    assert_eq!(receipt.outcome, IngestOutcome::Processed);
    assert!(!receipt.left_object_unpersisted);
    assert!(admissible_effects(
        &receipt.effects,
        &expected.event(),
        &guard
    ));
    assert_eq!(
        expected.verify(&storage, &stage, &guard, true),
        Err("roster")
    );
    assert_no_ingress(&ordinary_storage, &message.id);
    assert!(ordinary_runtime.live_group_ids().unwrap().is_empty());
    assert!(
        ordinary_storage
            .list_pending_application_events()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ordinary_storage.key_package_lifecycle().unwrap(),
        ordinary_lifecycle
    );
    assert!(guard.attempts().is_empty());
}

async fn dispatch_welcome(
    founder: &mut Runtime,
    group: &GroupId,
    welcome: &TransportMessage,
) -> AccountDeviceEffects {
    let signed = Event::from_json(&welcome.payload).unwrap();
    signed.verify().unwrap();
    founder
        .publish_session_effects(SessionEffects {
            events: vec![GroupEvent::GroupCreated {
                group_id: group.clone(),
            }],
            publish: vec![PublishWork::FoundingGroupCreated {
                welcomes: vec![welcome.clone()],
            }],
            queued: vec![],
            pending_convergence: vec![],
        })
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "opt-in public runtime Peer custody proof"]
async fn successful_dispatch_with_effect_failure_cannot_promote() {
    let dir = tempfile::tempdir().unwrap();
    let server = Store::new(dir.path(), "server.sqlite", Arc::new(Keys::generate()));
    let client = Store::new(dir.path(), "client.sqlite", Arc::new(Keys::generate()));
    let endpoint = TransportEndpoint(RELAY.into());
    let route = NostrRoutingV1::new(random(), vec![endpoint.0.clone()]).unwrap();
    let unrouted = StaticTransportRouting::new(vec![endpoint.clone()]);
    let routed = unrouted
        .clone()
        .with_inbox_route(client.account(), vec![endpoint.clone()]);
    let guard = Quarantine::default();
    let (storage, mut stage) = client.open(&unrouted, &guard);
    let (package, _) = prepare_private_package(&storage, &mut stage, &client, &endpoint).await;

    let (_, mut founder) = server.open(&unrouted, &guard);
    let (group, mut welcomes) = found_room(&mut founder, vec![package.clone()], &route).await;
    let welcome = welcomes.pop().unwrap();
    let effects = dispatch_welcome(&mut founder, &group, &welcome).await;
    assert_eq!(
        effects.failures,
        [marmot_session::PublishFailure {
            message_id: welcome.id,
            reason: marmot_session::TransportRoutingError::MissingInboxRoute.to_string(),
        }]
    );
    assert!(guard.attempts().is_empty());
    let expected = GroupEvent::GroupCreated { group_id: group };
    assert_eq!(effects.events, std::slice::from_ref(&expected));
    assert!(!admissible_effects(&effects, &expected, &guard));
    drop(founder);

    let (_, mut founder) = server.open(&routed, &guard);
    let (group, mut welcomes) = found_room(&mut founder, vec![package], &route).await;
    let welcome = welcomes.pop().unwrap();
    let effects = dispatch_welcome(&mut founder, &group, &welcome).await;
    assert_eq!(guard.attempts(), [BlockedIo::Publish]);
    let expected = GroupEvent::GroupCreated { group_id: group };
    assert_eq!(effects.events, std::slice::from_ref(&expected));
    assert_eq!(
        effects.failures,
        [marmot_session::PublishFailure {
            message_id: welcome.id,
            reason: TransportAdapterError::Closed.to_string(),
        }]
    );
    assert!(!admissible_effects(&effects, &expected, &guard));
    // Even the exact allowlisted event cannot override the latched typed denial.
    let otherwise_allowed = AccountDeviceEffects {
        events: vec![expected.clone()],
        ..AccountDeviceEffects::default()
    };
    assert!(!admissible_effects(&otherwise_allowed, &expected, &guard));
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
enum CarrierDisposition {
    Acknowledged { carrier: String },
    Cancelled { why: String },
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RawDisposition {
    stage: MemberId,
    stage_slot: String,
    raw: MessageId,
    route: String,
    disposition: CarrierDisposition,
}

#[tokio::test]
#[ignore = "opt-in public runtime Peer custody proof"]
async fn public_withdrawal_suppresses_raw_welcome_before_live_loopback_retry() {
    let started = Instant::now();
    let dir = tempfile::tempdir().unwrap();
    let relay = Loopback::start().await;
    let routes = vec![relay.endpoint.clone()];
    let route = NostrRoutingV1::new(random(), vec![relay.endpoint.0.clone()]).unwrap();
    let server = Store::new(dir.path(), "server.sqlite", Arc::new(Keys::generate()));
    let client = Store::new(dir.path(), "client.sqlite", Arc::new(Keys::generate()));
    let routed = StaticTransportRouting::new(routes.clone())
        .with_inbox_route(client.account(), routes.clone());
    let guard = Quarantine::default();
    let (storage, mut stage) = client.open(&routed, &guard);
    let (_, mut founder) = server.open(&routed, &guard);
    let (package, _) =
        prepare_private_package(&storage, &mut stage, &client, &relay.endpoint).await;
    let (_, mut confirmed) = found_room(&mut founder, vec![package.clone()], &route).await;
    let (_, mut cancelled) = found_room(&mut founder, vec![package.clone()], &route).await;
    let (control_group, mut control) = found_room(&mut founder, vec![package], &route).await;
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
        server.keys.clone(),
    ));
    let (carrier, _) = unwrapped_delivery(&server, &client, &confirmed).await;
    let wrapped = NostrTransportEvent::from_nostr_event(&carrier).unwrap();
    let ack_started = Instant::now();
    let ack = network
        .publish_event_for_account(&server.account(), &routes, &wrapped, 1)
        .await
        .unwrap();
    let ack_ms = ack_started.elapsed().as_secs_f64() * 1000.;
    assert_eq!(
        ack.message_id,
        Some(MessageId::new(carrier.id.to_bytes().to_vec()))
    );
    assert_eq!(
        ack.accepted
            .iter()
            .map(|receipt| &receipt.endpoint)
            .collect::<Vec<_>>(),
        [&relay.endpoint]
    );
    assert!(ack.failed.is_empty());
    assert_eq!(relay.observed(), [carrier.id]);
    let confirmed_disposition = RawDisposition {
        stage: client.account(),
        stage_slot: client.slot.clone(),
        raw: confirmed.id.clone(),
        route: relay.endpoint.0.clone(),
        disposition: CarrierDisposition::Acknowledged {
            carrier: carrier.id.to_hex(),
        },
    };
    let cancelled_disposition = RawDisposition {
        stage: client.account(),
        stage_slot: client.slot.clone(),
        raw: cancelled.id.clone(),
        route: relay.endpoint.0.clone(),
        disposition: CarrierDisposition::Cancelled {
            why: "fixture rejected".into(),
        },
    };
    let registry = Registry::new(dir.path());
    let conn = registry.open();
    registry.save(&conn, "confirmed", &confirmed_disposition);
    registry.save(&conn, "cancelled", &cancelled_disposition);
    assert_eq!(founder.outstanding_welcome_deliveries().unwrap().len(), 3);
    drop((conn, founder));

    // Crash after the carrier ACK and before native disposition.
    let (_, founder) = server.open(&routed, &guard);
    let conn = registry.open();
    let pending = founder.outstanding_welcome_deliveries().unwrap();
    assert_eq!(pending.len(), 3);
    for (name, expected) in [
        ("confirmed", &confirmed_disposition),
        ("cancelled", &cancelled_disposition),
    ] {
        let exact: RawDisposition = registry.read(&conn, name).unwrap();
        assert_eq!(&exact, expected);
        assert!(pending.iter().any(|(_, welcome)| welcome.id == exact.raw));
        assert_eq!(
            founder.withdraw_welcome(&exact.raw).unwrap(),
            WelcomeWithdrawal::Withdrawn
        );
    }
    assert_eq!(
        founder.outstanding_welcome_deliveries().unwrap(),
        [(control_group.clone(), control.clone())]
    );
    drop((conn, founder));

    // Crash after withdrawal and before the live owner is allowed to start.
    let (_, founder) = server.open(&routed, &guard);
    let conn = registry.open();
    for (name, expected) in [
        ("confirmed", &confirmed_disposition),
        ("cancelled", &cancelled_disposition),
    ] {
        let exact: RawDisposition = registry.read(&conn, name).unwrap();
        assert_eq!(&exact, expected);
        assert_eq!(
            founder.withdraw_welcome(&exact.raw).unwrap(),
            WelcomeWithdrawal::AlreadyWithdrawn
        );
    }
    assert_eq!(
        founder.outstanding_welcome_deliveries().unwrap(),
        [(control_group.clone(), control.clone())]
    );
    assert!(guard.attempts().is_empty());
    drop((conn, founder));

    let (_, mut live) = server.open_with(&routed, &guard, NostrTransportAdapter::new(network));
    live.activate_transport(None).await.unwrap();
    live.sync_transport_groups(None).await.unwrap();
    let owed = live.outstanding_welcome_deliveries().unwrap();
    assert_eq!(owed, [(control_group, control.clone())]);
    let startup = live
        .retry_welcome_messages_with_audit_context(
            owed.into_iter().map(|(_, message)| message).collect(),
            Default::default(),
        )
        .await
        .unwrap();
    assert!(startup.failures.is_empty());
    let stale = live
        .retry_welcome_messages_with_audit_context(vec![confirmed, cancelled], Default::default())
        .await
        .unwrap();
    assert!(stale.reports.is_empty() && stale.failures.is_empty());
    assert!(live.outstanding_welcome_deliveries().unwrap().is_empty());
    let control_event = Event::from_json(&control.payload).unwrap();
    assert_eq!(relay.observed(), [carrier.id, control_event.id]);
    assert!(guard.attempts().is_empty());
    sdk.disconnect().await;
    eprintln!(
        "public_peer_loopback carrier_ack_ms={ack_ms:.2} total_ms={:.2}",
        started.elapsed().as_secs_f64() * 1000.
    );
}
