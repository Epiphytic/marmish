//! Opt-in native shared account inbox and per-store admission proof.
use super::terminal_relay_fixture::{Relay, STEP};
use super::*;
use cgka_traits::{TransportDeliveryPlane, TransportSource};
use nostr::event::{Event, Kind, Tag, UnsignedEvent};
use nostr::types::{Timestamp as NostrTimestamp, Url};
use serde_json::json;
use std::path::Path;
use tokio::time::timeout;
use transport_nostr_adapter::NostrSubscription;
use transport_nostr_peeler::{NOSTR_SOURCE, SdkSigner, gift_wrap_rumor};

const TOTAL: Duration = Duration::from_secs(180);
const FIXTURE_DELIVERY: Kind = Kind::Custom(24860);

struct Receiver {
    client: Client,
    connector: RelayConnector,
    adapter: NostrTransportAdapter,
    account: MemberId,
    public: PublicKey,
    forwarder: tokio::task::JoinHandle<()>,
}

impl Receiver {
    async fn open(a: &Relay, b: &Relay, path: &Path, keys: &Arc<Keys>, split_inbox: bool) -> Self {
        let connector = a.paired_connector(b, path);
        connector.bootstrap(Instant::now() + STEP).await.unwrap();
        let client = connector.sdk_client(keys.clone()).unwrap();
        let public = keys.public_key();
        let account = MemberId::new(public.to_bytes().to_vec());
        let relay_client = Arc::new(NostrSdkRelayClient::with_account_signer(
            client.clone(),
            account.clone(),
            keys.clone(),
        ));
        let adapter = NostrTransportAdapter::new(relay_client.clone());
        let forwarder = relay_client.spawn_notification_forwarder(adapter.clone());
        timeout(
            STEP,
            adapter.activate_account(TransportAccountActivation {
                account_id: account.clone(),
                inbox_endpoints: [Some(a), split_inbox.then_some(b)]
                    .into_iter()
                    .flatten()
                    .map(|relay| endpoint(&relay.url))
                    .collect(),
                group_subscriptions: vec![],
                since: None,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        Self {
            client,
            connector,
            adapter,
            account,
            public,
            forwarder,
        }
    }

    async fn inbox_subscription(&self, endpoints: &[&Relay]) -> String {
        NostrSubscription::AccountInbox {
            account_id: self.account.clone(),
            endpoints: endpoints.iter().map(|relay| endpoint(&relay.url)).collect(),
            since: None,
            attempt: self
                .adapter
                .account_subscription_attempt(&self.account)
                .await
                .unwrap(),
        }
        .subscription_id()
    }

    async fn next(&self) -> TransportDelivery {
        timeout(STEP, self.adapter.receive())
            .await
            .expect("bounded canonical delivery")
            .unwrap()
            .unwrap()
    }

    async fn ack(&self, id: &MessageId) {
        timeout(STEP, self.adapter.ack_replay_delivery(&self.account, id))
            .await
            .unwrap();
    }

    async fn close(self) {
        timeout(STEP, self.adapter.deactivate_account(&self.account))
            .await
            .unwrap()
            .unwrap();
        timeout(STEP, self.client.shutdown()).await.unwrap();
        self.connector.cancel();
        self.forwarder.abort();
    }
}

fn endpoint(url: &Url) -> TransportEndpoint {
    TransportEndpoint(url.to_string())
}

fn exact_event(
    delivery: &TransportDelivery,
    account: &MemberId,
    plane: TransportDeliveryPlane,
    allowed: &Url,
    issued: &str,
) -> Option<Event> {
    let source = &delivery.source;
    if &delivery.account_id != account
        || source.transport != TransportSource(NOSTR_SOURCE.into())
        || delivery.message.source != source.transport
        || source.plane != plane
        || source
            .endpoint
            .as_ref()
            .and_then(|endpoint| Url::parse(&endpoint.0).ok())
            .as_ref()
            != Some(allowed)
        || issued == "local-publish"
        || source.subscription_id.as_deref() != Some(issued)
    {
        return None;
    }
    let raw = Event::from_json(&delivery.message.payload).ok()?;
    let wire = source.wire.as_ref()?;
    if raw.verify().is_err()
        || raw.id.as_bytes() != delivery.message.id.as_slice()
        || wire.wire_id.as_deref() != Some(raw.id.to_hex().as_str())
        || wire.wire_kind != Some(u16::from(raw.kind).into())
        || wire.wire_pubkey_hex.as_deref() != Some(raw.pubkey.to_hex().as_str())
        || match plane {
            TransportDeliveryPlane::AccountInbox => {
                raw.kind != Kind::GiftWrap
                    || wire.gift_wrap_event_id.as_deref() != Some(raw.id.to_hex().as_str())
            }
            TransportDeliveryPlane::Group => u16::from(raw.kind) != 445,
            _ => true,
        }
    {
        return None;
    }
    Some(raw)
}

async fn marker(founder: &Arc<Keys>, recipient: PublicKey, label: &str) -> Event {
    timeout(
        STEP,
        gift_wrap_rumor(
            founder.clone(),
            recipient,
            UnsignedEvent::new(
                founder.public_key(),
                NostrTimestamp::now(),
                FIXTURE_DELIVERY,
                [Tag::parse(["p", &recipient.to_hex()]).unwrap()],
                label,
            ),
        ),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
#[ignore = "opt-in native shared inbox origin qualification"]
async fn exact_inbox_origin_and_replay_disposition() {
    timeout(TOTAL, async {
        let dir = tempfile::tempdir().unwrap();
        let a = Relay::start().await;
        let b = Relay::start().await;
        let keys = Arc::new(Keys::generate());
        let founder = Arc::new(Keys::generate());
        let receiver = Receiver::open(&a, &b, dir.path(), &keys, true).await;
        let recipient = json!(receiver.public);
        let a_inbox = receiver.inbox_subscription(&[&a, &b]).await;
        let b_inbox = a_inbox.clone();
        assert_eq!(a.subscription(receiver.public, 1059, "#p", recipient.clone()).await, a_inbox);
        assert_eq!(b.subscription(receiver.public, 1059, "#p", recipient.clone()).await, b_inbox);
        let split = marker(&founder, receiver.public, "split-origin").await;
        b.inject(receiver.public, &b_inbox, &split);
        let denied = receiver.next().await;
        assert_eq!(denied.message.id.as_slice(), split.id.as_bytes());
        assert!(exact_event(&denied, &receiver.account, TransportDeliveryPlane::AccountInbox, &b.url, &b_inbox).is_some());
        assert!(exact_event(&denied, &receiver.account, TransportDeliveryPlane::AccountInbox, &a.url, &a_inbox).is_none());
        let receipts = Receipts::new(dir.path());
        assert!(receipts.denied(Tenant::Peer, 1, &denied));
        receiver.ack(&denied.message.id).await;

        a.inject(receiver.public, &a_inbox, &split);
        let control = marker(&founder, receiver.public, "independent-allowed-origin").await;
        a.inject(receiver.public, &a_inbox, &control);
        let delivered = receiver.next().await;
        // The independent event is a barrier on A's same socket and notification
        // stream. The earlier same-ID copy was not offered to the owner.
        assert_eq!(delivered.message.id.as_slice(), control.id.as_bytes());
        let signed = exact_event(&delivered, &receiver.account, TransportDeliveryPlane::AccountInbox, &a.url, &a_inbox)
            .expect("actual eligible inbox origin");
        let authenticated = nostr::nips::nip59::extract_rumor_async(&SdkSigner(keys.clone()), &signed).await.unwrap();
        assert_eq!(authenticated.sender, founder.public_key());
        assert_eq!(authenticated.rumor.kind, FIXTURE_DELIVERY);
        for source in [
            cgka_traits::TransportDeliverySource { endpoint: None, ..delivered.source.clone() },
            cgka_traits::TransportDeliverySource { subscription_id: None, ..delivered.source.clone() },
            cgka_traits::TransportDeliverySource { subscription_id: Some("local-publish".into()), ..delivered.source.clone() },
            cgka_traits::TransportDeliverySource { plane: TransportDeliveryPlane::Group, ..delivered.source.clone() },
            cgka_traits::TransportDeliverySource { transport: TransportSource("unknown".into()), ..delivered.source.clone() },
        ] {
            let invalid = TransportDelivery { source, ..delivered.clone() };
            assert!(exact_event(&invalid, &receiver.account, TransportDeliveryPlane::AccountInbox, &a.url, &a_inbox).is_none());
        }
        assert!(receipts.placed(Tenant::Peer, 1, &delivered));
        receiver.ack(&delivered.message.id).await;
        let a_counts = a.subscription_counts(receiver.public, 1059, "#p", &recipient);
        let b_counts = b.subscription_counts(receiver.public, 1059, "#p", &recipient);
        assert_eq!(a_counts, (1, 0));
        assert_eq!(b_counts, (1, 0));
        assert_ne!(split.id, signed.id);
        absent_frame(&receiver).await;
        eprintln!("peer_aggregate split_origin_owner_delivery=absent authorized_control=received inbox_req={} inbox_close={}", a_counts.0 + b_counts.0, a_counts.1 + b_counts.1);
        receiver.close().await;
    }).await.unwrap();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Denial {
    Activate,
    Synchronize,
    Deactivate,
    Receive,
    Publish,
    PublishPackage,
    WithdrawPackage,
    DeletePackage,
}

#[derive(Default)]
struct ChildState {
    allowed: Option<TransportGroupSubscription>,
    desired: Option<TransportGroupSubscription>,
    denied: Vec<Denial>,
}

#[derive(Clone)]
struct ChildTransport {
    account: MemberId,
    state: Arc<std::sync::Mutex<ChildState>>,
}

impl ChildTransport {
    fn new(account: MemberId) -> Self {
        Self {
            account,
            state: Arc::default(),
        }
    }

    fn allow(&self, route: TransportGroupSubscription) {
        let mut state = self.state.lock().unwrap();
        assert!(state.allowed.is_none());
        state.allowed = Some(route);
    }

    fn refuse<T>(&self, operation: Denial, error: T) -> T {
        let mut state = self.state.lock().unwrap();
        assert!(state.denied.len() < 32);
        state.denied.push(operation);
        error
    }

    fn desired(&self) -> Option<TransportGroupSubscription> {
        self.state.lock().unwrap().desired.clone()
    }

    fn denials(&self) -> Vec<Denial> {
        self.state.lock().unwrap().denied.clone()
    }
}

#[async_trait]
impl TransportAdapter for ChildTransport {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(self.refuse(Denial::Activate, TransportAdapterError::Closed))
    }

    async fn sync_account_groups(
        &self,
        sync: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        let mut state = self.state.lock().unwrap();
        let mut proposed = sync.group_subscriptions.into_iter();
        let first = proposed.next();
        if sync.account_id != self.account
            || proposed.next().is_some()
            || first
                .as_ref()
                .is_some_and(|route| state.allowed.as_ref() != Some(route))
        {
            drop(state);
            return Err(self.refuse(Denial::Synchronize, TransportAdapterError::Closed));
        }
        state.desired = first;
        Ok(())
    }

    async fn deactivate_account(
        &self,
        _: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(self.refuse(Denial::Deactivate, TransportAdapterError::Closed))
    }

    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        Err(self.refuse(Denial::Receive, TransportAdapterError::Closed))
    }

    async fn publish(
        &self,
        _: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        Err(self.refuse(Denial::Publish, TransportAdapterError::Closed))
    }
}

struct FixturePackageSigner {
    keys: Arc<Keys>,
    slot: String,
    scope: ChildTransport,
}

#[async_trait]
impl KeyPackagePublisher for FixturePackageSigner {
    fn legacy_slot_id(
        &self,
        _: &MemberId,
    ) -> std::result::Result<Option<String>, KeyPackagePublishError> {
        Ok(Some(self.slot.clone()))
    }

    async fn prepare_key_package(
        &self,
        request: KeyPackagePublication,
    ) -> std::result::Result<SignedPublicationArtifact, KeyPackagePublishError> {
        let dto = Publisher::publication(&request)?
            .to_event_at(request.created_at.0)
            .unwrap();
        let event = EventBuilder::new(Kind::Custom(30443), dto.content)
            .tags(dto.tags.iter().map(|tag| Tag::parse(tag).unwrap()))
            .custom_created_at(NostrTimestamp::from_secs(request.created_at.0))
            .finalize(self.keys.as_ref())
            .unwrap();
        Ok(SignedPublicationArtifact {
            id: MessageId::new(event.id.to_bytes().to_vec()),
            bytes: event.as_json().into_bytes(),
            created_at: request.created_at,
        })
    }

    async fn publish_prepared_key_package(
        &self,
        _: &KeyPackagePublication,
        _: &SignedPublicationArtifact,
    ) -> std::result::Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.scope.refuse(
            Denial::PublishPackage,
            KeyPackagePublishError::unexposed("scope"),
        ))
    }

    async fn withdraw_key_package_slot(
        &self,
        _: &marmot_session::KeyPackageSlotWithdrawal,
    ) -> std::result::Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.scope.refuse(
            Denial::WithdrawPackage,
            KeyPackagePublishError::unexposed("scope"),
        ))
    }

    async fn delete_key_package_event(
        &self,
        _: &marmot_session::KeyPackageEventDeletion,
    ) -> std::result::Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        Err(self.scope.refuse(
            Denial::DeletePackage,
            KeyPackagePublishError::unexposed("scope"),
        ))
    }
}

#[derive(Clone)]
struct Capture {
    scope: ChildTransport,
    pending: Arc<std::sync::Mutex<Option<TransportMessage>>>,
}

#[async_trait]
impl TransportAdapter for Capture {
    async fn activate_account(
        &self,
        activation: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.scope.activate_account(activation).await
    }

    async fn sync_account_groups(
        &self,
        sync: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.scope.sync_account_groups(sync).await
    }

    async fn deactivate_account(
        &self,
        account: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.scope.deactivate_account(account).await
    }

    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        self.scope.receive().await
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        let [endpoint] = request.target.endpoints() else {
            panic!("single fixture destination");
        };
        let message_id = request.message.id.clone();
        assert!(
            self.pending
                .lock()
                .unwrap()
                .replace(request.message)
                .is_none()
        );
        Ok(TransportPublishReport {
            message_id,
            accepted: vec![cgka_traits::TransportEndpointReceipt {
                endpoint: endpoint.clone(),
                accepted_at: None,
                ack_kind: None,
            }],
            failed: vec![],
            required_acks: request.required_acks,
        })
    }
}

type ChildRuntime<A = ChildTransport> =
    AccountDeviceRuntime<A, ExplicitRouting, FixturePackageSigner>;

#[derive(Clone)]
struct Provider {
    path: std::path::PathBuf,
    cipher: [u8; 32],
    slot: String,
    keys: Arc<Keys>,
    routing: ExplicitRouting,
    scope: ChildTransport,
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

impl Provider {
    fn new(directory: &Path, name: &str, keys: Arc<Keys>, relay: &Url) -> Self {
        let account = MemberId::new(keys.public_key().to_bytes().to_vec());
        Self {
            path: directory.join(name),
            cipher: random(),
            slot: hex::encode(random::<16>()),
            keys,
            routing: ExplicitRouting::new(vec![endpoint(relay)]),
            scope: ChildTransport::new(account),
        }
    }

    fn open(&self) -> (SqliteAccountStorage, ChildRuntime) {
        self.open_with(self.scope.clone())
    }

    fn open_existing(&self) -> Option<(SqliteAccountStorage, ChildRuntime)> {
        if !self.path.is_file() {
            return None;
        }
        let storage =
            SqliteAccountStorage::open_encrypted(&self.path, &SqlCipherKey::raw(self.cipher))
                .ok()?;
        Some(self.runtime(storage, self.scope.clone()))
    }

    fn open_with<A: TransportAdapter>(
        &self,
        adapter: A,
    ) -> (SqliteAccountStorage, ChildRuntime<A>) {
        marmish_engine::initialize_storage().unwrap();
        let storage =
            SqliteAccountStorage::open_encrypted(&self.path, &SqlCipherKey::raw(self.cipher))
                .unwrap();
        self.runtime(storage, adapter)
    }

    fn runtime<A: TransportAdapter>(
        &self,
        storage: SqliteAccountStorage,
        adapter: A,
    ) -> (SqliteAccountStorage, ChildRuntime<A>) {
        let config = SessionConfig::with_storage(
            storage.clone(),
            Duration::ZERO,
            self.scope.account.as_slice().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(self.keys.clone())),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(self.keys.clone())))
        .supported_app_components(default_group_components().into_iter().chain([
            NOSTR_ROUTING_COMPONENT_ID,
            GROUP_MESSAGE_RETENTION_COMPONENT_ID,
        ]));
        let runtime = AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((
                adapter,
                self.routing.clone(),
                FixturePackageSigner {
                    keys: self.keys.clone(),
                    slot: self.slot.clone(),
                    scope: self.scope.clone(),
                },
            ))
        })
        .unwrap();
        (storage, runtime)
    }

    async fn package(
        &self,
        storage: &SqliteAccountStorage,
        runtime: &mut ChildRuntime,
        relay: &Url,
    ) -> KeyPackage {
        let mut package = timeout(
            STEP,
            runtime.prepare_fresh_key_package(vec![endpoint(relay)]),
        )
        .await
        .unwrap()
        .unwrap();
        let state = storage.key_package_lifecycle().unwrap().unwrap();
        let pending = state.pending_replacement.unwrap();
        assert!(state.current_key_package.is_none() && state.authored_event_id.is_none());
        assert!(pending.consumed_at.is_none());
        assert_eq!(pending.key_package, package);
        let signed = pending.signed_event.unwrap();
        let event = Event::from_json(&signed.bytes).unwrap();
        event.verify().unwrap();
        assert_eq!(event.pubkey, self.keys.public_key());
        assert_eq!(event.id.as_bytes().as_slice(), signed.id.as_slice());
        package.source = Some(KeyPackageSource {
            event_id: signed.id,
        });
        package
    }
}

struct Room {
    group: GroupId,
    route: NostrRoutingV1,
    name: &'static str,
    welcome: TransportMessage,
}

impl Room {
    async fn found<A: TransportAdapter>(
        runtime: &mut ChildRuntime<A>,
        members: Vec<KeyPackage>,
        relay: &Url,
        name: &'static str,
        include_retention: bool,
        exact_recipient: Option<&MemberId>,
    ) -> Self {
        let route = NostrRoutingV1::new(random(), vec![relay.to_string()]).unwrap();
        let mut components = vec![AppComponentData {
            component_id: NOSTR_ROUTING_COMPONENT_ID,
            data: encode_nostr_routing_v1(&route).unwrap(),
        }];
        if include_retention {
            components.push(AppComponentData {
                component_id: GROUP_MESSAGE_RETENTION_COMPONENT_ID,
                data: 60u64.to_be_bytes().to_vec(),
            });
        }
        let prepared = timeout(
            STEP,
            runtime.prepare_create_group_with_audit_context(
                CreateGroupRequest {
                    name: name.into(),
                    description: name.into(),
                    members,
                    required_features: vec![],
                    initial_admins: vec![],
                    app_components: components,
                },
                Default::default(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let [marmot_session::PublishWork::FoundingGroupCreated { welcomes }] =
            prepared.effects.publish.as_slice()
        else {
            panic!("expected native Welcome obligations");
        };
        Self {
            group: prepared.group_id,
            route,
            name,
            welcome: match exact_recipient {
                Some(recipient) => welcomes.iter().find(|item| {
                    matches!(&item.envelope, TransportEnvelope::Welcome { recipient: actual } if actual == recipient)
                }).unwrap(),
                None => {
                    assert_eq!(welcomes.len(), 1);
                    &welcomes[0]
                }
            }.clone(),
        }
    }

    fn subscription(&self, relay: &Url) -> TransportGroupSubscription {
        TransportGroupSubscription {
            group_id: self.group.clone(),
            transport_group_id: self.route.nostr_group_id.to_vec(),
            endpoints: vec![endpoint(relay)],
            retained_since: None,
        }
    }

    async fn frame(
        &self,
        founder: &mut ChildRuntime<Capture>,
        capture: &Capture,
        label: &str,
    ) -> (Event, Vec<u8>) {
        let payload = cgka_traits::app_event::MarmotAppEvent::new(
            hex::encode(capture.scope.account.as_slice()),
            NostrTimestamp::now().as_secs(),
            cgka_traits::app_event::MARMOT_APP_EVENT_KIND_CHAT,
            vec![],
            label,
        )
        .encode()
        .unwrap();
        let effects = timeout(
            STEP,
            founder.send(SendIntent::AppMessage {
                group_id: self.group.clone(),
                payload: payload.clone(),
                expected_epoch: None,
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(effects.failures.is_empty());
        let msg = capture
            .pending
            .lock()
            .unwrap()
            .take()
            .expect("native application publication");
        let event = Event::from_json(&msg.payload).unwrap();
        event.verify().unwrap();
        (event, payload)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tenant {
    Ordinary,
    Peer,
}

#[derive(Clone, Debug)]
struct RegisteredRoute {
    tenant: Tenant,
    generation: u64,
    spec: TransportGroupSubscription,
}

#[derive(Default)]
struct Registry {
    live: Vec<RegisteredRoute>,
    retired: Vec<TransportGroupSubscription>,
    revision: u64,
}

impl Registry {
    fn register(&mut self, tenant: Tenant, spec: TransportGroupSubscription) -> bool {
        let conflicts = |other: &TransportGroupSubscription| {
            other.transport_group_id == spec.transport_group_id || other.group_id == spec.group_id
        };
        if self.live.len() + self.retired.len() >= 4
            || self.live.iter().any(|entry| conflicts(&entry.spec))
            || self.retired.iter().any(conflicts)
        {
            return false;
        }
        self.revision += 1;
        self.live.push(RegisteredRoute {
            tenant,
            generation: self.revision,
            spec,
        });
        true
    }

    fn revoke(&mut self, tenant: Tenant) {
        let index = self
            .live
            .iter()
            .position(|entry| entry.tenant == tenant)
            .unwrap();
        self.revision += 1;
        self.retired.push(self.live.remove(index).spec);
    }

    async fn sync(&self, owner: &Receiver, children: &[&ChildTransport]) {
        let group_subscriptions = self
            .live
            .iter()
            .filter(|entry| {
                children
                    .iter()
                    .any(|child| child.desired().as_ref() == Some(&entry.spec))
            })
            .map(|entry| entry.spec.clone())
            .collect();
        timeout(
            STEP,
            owner.adapter.sync_account_groups(TransportGroupSync {
                account_id: owner.account.clone(),
                group_subscriptions,
                since: None,
            }),
        )
        .await
        .unwrap()
        .unwrap();
    }

    async fn lease(
        &self,
        delivery: &TransportDelivery,
        owner: &Receiver,
        relay: &Url,
    ) -> Option<RegisteredRoute> {
        let TransportEnvelope::GroupMessage { transport_group_id } = &delivery.message.envelope
        else {
            return None;
        };
        let route = self.live.iter().find(|entry| {
            entry.spec.transport_group_id == *transport_group_id
                && Some(&entry.spec.group_id) == delivery.group_id_hint.as_ref()
                && entry.spec.endpoints.contains(&endpoint(relay))
        })?;
        let issued = owner
            .adapter
            .group_route_subscription_id(&owner.account, &route.spec.transport_group_id)
            .await?;
        exact_event(
            delivery,
            &owner.account,
            TransportDeliveryPlane::Group,
            relay,
            &issued,
        )?;
        let wire = delivery.source.wire.as_ref()?;
        (wire.transport_group_id.as_deref() == Some(hex::encode(transport_group_id).as_str()))
            .then(|| route.clone())
    }

    fn still_current(&self, lease: &RegisteredRoute) -> bool {
        self.live.iter().any(|route| {
            route.generation == lease.generation
                && route.spec == lease.spec
                && route.tenant == lease.tenant
        })
    }
}

struct Receipts {
    path: std::path::PathBuf,
    key: [u8; 32],
}

impl Receipts {
    fn new(path: &Path) -> Self {
        Self {
            path: path.join("receipts.sqlite"),
            key: random(),
        }
    }

    fn open(&self) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(&self.path).unwrap();
        conn.pragma_update(None, "key", format!("x'{}'", hex::encode(self.key)))
            .unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS receipts (tenant TEXT, generation INTEGER, plane TEXT, endpoint TEXT, event BLOB, accepted INTEGER NOT NULL, PRIMARY KEY (tenant, generation, plane, endpoint, event));
                            CREATE TABLE IF NOT EXISTS ready (generation INTEGER PRIMARY KEY, carrier BLOB, raw BLOB, group_id BLOB);").unwrap();
        conn
    }

    fn placed(&self, tenant: Tenant, generation: u64, delivery: &TransportDelivery) -> bool {
        self.record(tenant, generation, delivery, true)
    }

    fn denied(&self, tenant: Tenant, generation: u64, delivery: &TransportDelivery) -> bool {
        self.record(tenant, generation, delivery, false)
    }

    fn record(
        &self,
        tenant: Tenant,
        generation: u64,
        delivery: &TransportDelivery,
        accepted: bool,
    ) -> bool {
        let conn = self.open();
        let changes = conn.execute(
            "INSERT OR IGNORE INTO receipts SELECT ?1,?2,?3,?4,?5,?6 WHERE (SELECT count(*) FROM receipts) < 32 AND
             (SELECT count(*) FROM receipts WHERE tenant=?1 AND generation=?2 AND plane=?3 AND endpoint=?4) < 8",
            rusqlite::params![format!("{tenant:?}"), i64::try_from(generation).unwrap(), format!("{:?}", delivery.source.plane), delivery.source.endpoint.as_ref().unwrap().0, delivery.message.id.as_slice(), accepted],
        ).unwrap();
        changes == 1
    }

    fn ready(&self, generation: u64, carrier: &TransportDelivery, room: &Room) {
        self.open()
            .execute(
                "INSERT INTO ready VALUES (?1,?2,?3,?4)",
                rusqlite::params![
                    i64::try_from(generation).unwrap(),
                    carrier.message.id.as_slice(),
                    room.welcome.id.as_slice(),
                    room.group.as_slice(),
                ],
            )
            .unwrap();
    }

    fn has_ready(&self, generation: u64, carrier: &TransportDelivery, room: &Room) -> bool {
        self.open().query_row(
            "SELECT EXISTS(SELECT 1 FROM ready WHERE generation=?1 AND carrier=?2 AND raw=?3 AND group_id=?4)",
            rusqlite::params![
                i64::try_from(generation).unwrap(), carrier.message.id.as_slice(),
                room.welcome.id.as_slice(), room.group.as_slice(),
            ],
            |row| row.get(0),
        ).unwrap()
    }

    fn placements(&self, tenant: Tenant, plane: TransportDeliveryPlane) -> i64 {
        self.open()
            .query_row(
                "SELECT count(*) FROM receipts WHERE tenant=?1 AND plane=?2",
                rusqlite::params![format!("{tenant:?}"), format!("{plane:?}")],
                |row| row.get(0),
            )
            .unwrap()
    }
}

const ORDINARY: &str = "marmish/command/1/aggregate-proof";
const PEER: &str = "marmish/peer/1/aggregate-proof";

async fn authorize_group(
    provider: &Provider,
    runtime: &mut ChildRuntime,
    room: &Room,
    relay: &Url,
) {
    provider
        .routing
        .group(room.group.clone(), room.route.clone())
        .unwrap();
    provider.scope.allow(room.subscription(relay));
    timeout(STEP, runtime.sync_transport_groups(None))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(provider.scope.desired(), Some(room.subscription(relay)));
    assert!(provider.scope.denials().is_empty());
}

fn validate_group(runtime: &ChildRuntime, room: &Room, founder: &MemberId, account: &MemberId) {
    let group = runtime.group_record(&room.group).unwrap();
    assert_eq!(group.name, room.name);
    assert_eq!(group.description, room.name);
    assert_eq!(group.protocol_profile, ProtocolProfile::Current);
    assert!(!group.removed);
    let roster = runtime.canonical_group_membership(&room.group).unwrap();
    assert!(roster.local_leaf_active);
    assert_eq!(roster.member_leaves.len(), 2);
    assert!(
        roster
            .member_leaves
            .iter()
            .any(|leaf| &leaf.member == founder)
    );
    assert!(
        roster
            .member_leaves
            .iter()
            .any(|leaf| &leaf.member == account)
    );
    assert_eq!(
        runtime.admin_pubkeys(&room.group).unwrap(),
        [<[u8; 32]>::try_from(founder.as_slice()).unwrap()]
    );
    let raw = runtime
        .app_component(&room.group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap()
        .unwrap();
    assert_eq!(decode_nostr_routing_v1(&raw).unwrap(), room.route);
    runtime.epoch_pin(&room.group).unwrap();
}

async fn ingest_frame(
    runtime: &mut ChildRuntime,
    lease: &RegisteredRoute,
    delivery: &TransportDelivery,
    sender: &MemberId,
    expected: &[u8],
) -> Duration {
    let pin = runtime.epoch_pin(&lease.spec.group_id).unwrap();
    let roster = runtime
        .canonical_group_membership(&lease.spec.group_id)
        .unwrap();
    assert!(roster.local_leaf_active);
    let peer = roster
        .member_leaves
        .iter()
        .find(|leaf| &leaf.member == sender)
        .unwrap();
    let started = Instant::now();
    let result = timeout(STEP, runtime.ingest_delivery(delivery))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, IngestOutcome::Processed);
    assert!(!result.left_object_unpersisted);
    assert!(
        matches!(
            result.effects.events.as_slice(),
            [GroupEvent::MessageReceived { group_id, sender: actual, sender_leaf_index: Some(leaf), epoch, payload, .. }]
            if group_id == &lease.spec.group_id && actual == sender && *leaf == peer.leaf_index
                && *epoch == pin.epoch && payload == expected
        ),
        "unexpected native effects for {:?}",
        lease.tenant
    );
    let duration = started.elapsed();
    assert_eq!(runtime.epoch_pin(&lease.spec.group_id).unwrap(), pin);
    duration
}

async fn peer_wrapper(
    founder: &Arc<Keys>,
    account: PublicKey,
    token: &[u8; 16],
    generation: u64,
    message: &TransportMessage,
) -> Event {
    peer_rumor(
        founder,
        account,
        account,
        token,
        generation,
        STANDARD.encode(&message.payload),
    )
    .await
}

async fn peer_rumor(
    founder: &Arc<Keys>,
    outer_recipient: PublicKey,
    inner_recipient: PublicKey,
    token: &[u8; 16],
    generation: u64,
    content: String,
) -> Event {
    timeout(
        STEP,
        gift_wrap_rumor(
            founder.clone(),
            outer_recipient,
            UnsignedEvent::new(
                founder.public_key(),
                NostrTimestamp::now(),
                FIXTURE_DELIVERY,
                [
                    Tag::parse(["p", &inner_recipient.to_hex()]).unwrap(),
                    Tag::parse(["d", &hex::encode(token)]).unwrap(),
                    Tag::parse(["g", &generation.to_string()]).unwrap(),
                ],
                content,
            ),
        ),
    )
    .await
    .unwrap()
    .unwrap()
}

struct Candidate<'a> {
    stage: &'a Provider,
    founder: PublicKey,
    room: &'a Room,
    held_package: &'a MessageId,
    token: [u8; 16],
    generation: u64,
    carrier: nostr::event::EventId,
}

impl Candidate<'_> {
    async fn resolve(
        &self,
        receiver: &Receiver,
        delivery: &TransportDelivery,
        origin: &Url,
        issued: &str,
    ) -> Option<TransportDelivery> {
        let outer = exact_event(
            delivery,
            &receiver.account,
            TransportDeliveryPlane::AccountInbox,
            origin,
            issued,
        )?;
        if outer.kind != Kind::GiftWrap
            || outer.id != self.carrier
            || self.room.route.relays != [origin.to_string()]
        {
            return None;
        }
        let signed = timeout(
            STEP,
            nostr::nips::nip59::extract_rumor_async(&SdkSigner(self.stage.keys.clone()), &outer),
        )
        .await
        .ok()?
        .ok()?;
        let tags: Vec<_> = signed.rumor.tags.iter().map(|tag| tag.as_slice()).collect();
        if signed.sender != self.founder
            || signed.rumor.kind != FIXTURE_DELIVERY
            || tags
                != [
                    &["p", self.stage.keys.public_key().to_hex().as_str()][..],
                    &["d", hex::encode(self.token).as_str()][..],
                    &["g", self.generation.to_string().as_str()][..],
                ]
        {
            return None;
        }
        let raw = STANDARD.decode(signed.rumor.content).ok()?;
        let event = Event::from_json(&raw).ok()?;
        event.verify().ok()?;
        let message = NostrTransportEvent::from_nostr_event(&event)
            .ok()?
            .to_transport_message()
            .ok()?;
        if raw != self.room.welcome.payload
            || message != self.room.welcome
            || message.envelope
                != (TransportEnvelope::Welcome {
                    recipient: receiver.account.clone(),
                })
        {
            return None;
        }
        let peeler = NostrMlsPeeler::new().with_welcome_signer_arc(self.stage.keys.clone());
        let (peeled, provenance) = timeout(STEP, peeler.peel_welcome_with_provenance(&message))
            .await
            .ok()?
            .ok()?;
        let provenance = provenance?;
        if peeled.sender.as_ref().map(MemberId::as_slice) != Some(self.founder.as_bytes())
            || provenance.key_package_event_id.as_slice() != self.held_package.as_slice()
        {
            return None;
        }
        // The native peeler validates the signed tag layout first. Recover
        // the exact authenticated relay hints from the same nested event.
        let nested = timeout(
            STEP,
            nostr::nips::nip59::extract_rumor_async(&SdkSigner(self.stage.keys.clone()), &event),
        )
        .await
        .ok()?
        .ok()?;
        let hints = nested.rumor.tags.iter().find_map(|tag| {
            let (first, hints) = tag.as_slice().split_first()?;
            (first == "relays").then_some(hints)
        })?;
        if nested.sender != self.founder
            || nested.rumor.compute_id().to_bytes() != provenance.rumor_event_id
            || hints != self.room.route.relays
        {
            return None;
        }
        Some(TransportDelivery {
            message,
            source: cgka_traits::TransportDeliverySource {
                wire: None,
                ..delivery.source.clone()
            },
            ..delivery.clone()
        })
    }
}

async fn stage_join(
    candidate: &Candidate<'_>,
    account: &MemberId,
    raw: &TransportDelivery,
    runtime: &mut ChildRuntime,
    storage: &SqliteAccountStorage,
) -> (bool, AccountDeviceEffects) {
    let outcome = timeout(STEP, runtime.ingest_delivery(raw))
        .await
        .unwrap()
        .unwrap();
    let expected = GroupEvent::GroupJoined {
        group_id: candidate.room.group.clone(),
        via_welcome: raw.message.id.clone(),
        welcomer: Some(MemberId::new(candidate.founder.to_bytes().to_vec())),
        explicitly_confirmed: false,
    };
    if outcome.outcome != IngestOutcome::Processed
        || outcome.left_object_unpersisted
        || outcome.effects
            != (AccountDeviceEffects {
                events: vec![expected.clone()],
                ..Default::default()
            })
        || !candidate.stage.scope.denials().is_empty()
        || storage.list_pending_application_events().unwrap() != [expected]
    {
        return (false, outcome.effects);
    }
    let roster = runtime
        .canonical_group_membership(&candidate.room.group)
        .unwrap();
    if roster.member_leaves.len() != 2 {
        return (false, outcome.effects);
    }
    validate_group(
        runtime,
        candidate.room,
        &MemberId::new(candidate.founder.to_bytes().to_vec()),
        account,
    );
    let state = storage.key_package_lifecycle().unwrap().unwrap();
    let pending = state.pending_replacement.unwrap();
    (
        pending.consumed_at.is_some()
            && pending.signed_event.unwrap().id == *candidate.held_package,
        outcome.effects,
    )
}

fn assert_absent(storage: &SqliteAccountStorage, id: &MessageId) {
    assert!(!storage.has_ingress_dedup_marker(id).unwrap());
    assert!(matches!(
        storage.get_message(id),
        Err(cgka_traits::storage::StorageError::NotFound)
    ));
}

async fn absent_frame(owner: &Receiver) {
    assert!(
        timeout(Duration::from_millis(125), owner.adapter.receive())
            .await
            .is_err()
    );
}

async fn ordinary_held_source(
    delivery: &TransportDelivery,
    keys: &Arc<Keys>,
    storage: &SqliteAccountStorage,
) -> Option<MessageId> {
    let peeler = NostrMlsPeeler::new().with_welcome_signer_arc(keys.clone());
    let (_, provenance) = timeout(STEP, peeler.peel_welcome_with_provenance(&delivery.message))
        .await
        .ok()?
        .ok()?;
    let provenance = provenance?;
    storage
        .key_package_lifecycle()
        .unwrap()?
        .held_publications()
        .find(|(id, _)| id.as_slice() == provenance.key_package_event_id.as_slice())
        .map(|(id, _)| id.clone())
}

struct World {
    _temp: tempfile::TempDir,
    a: Relay,
    b: Relay,
    receiver: Receiver,
    founder: Provider,
    capture: Capture,
    sender: ChildRuntime<Capture>,
    ordinary: Provider,
    storage: SqliteAccountStorage,
    command: ChildRuntime,
    room: Room,
    registry: Registry,
    receipts: Receipts,
    inbox: String,
}

struct HeldPeer {
    stage: Provider,
    room: Room,
    token: [u8; 16],
    carrier: Event,
    incoming: TransportDelivery,
    package_id: MessageId,
}

impl World {
    async fn setup() -> Self {
        Self::setup_shared(false).await.0
    }

    async fn setup_shared(hold_peer: bool) -> (Self, Option<HeldPeer>) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();
        let a = Relay::start().await;
        let b = Relay::start().await;
        let keys = Arc::new(Keys::generate());
        let founder = Provider::new(path, "founder.sqlite", Arc::new(Keys::generate()), &a.url);
        let receiver = Receiver::open(&a, &b, path, &keys, false).await;
        let inbox = receiver.inbox_subscription(&[&a]).await;
        assert_eq!(
            a.subscription(receiver.public, 1059, "#p", json!(receiver.public))
                .await,
            inbox
        );
        let capture = Capture {
            scope: founder.scope.clone(),
            pending: Arc::default(),
        };
        let (_, mut sender) = founder.open_with(capture.clone());
        founder.routing.inbox(receiver.account.clone());
        let ordinary = Provider::new(path, "ordinary.sqlite", keys, &a.url);
        let (storage, mut command) = ordinary.open();
        let package = ordinary.package(&storage, &mut command, &a.url).await;
        let ordinary_package = package.source.as_ref().unwrap().event_id.clone();
        let room = Room::found(&mut sender, vec![package], &a.url, ORDINARY, false, None).await;
        let registry = Registry::default();
        let receipts = Receipts::new(path);
        let mut world = Self {
            _temp: temp,
            a,
            b,
            receiver,
            founder,
            capture,
            sender,
            ordinary,
            storage,
            command,
            room,
            registry,
            receipts,
            inbox,
        };
        let held = if hold_peer {
            Some(world.hold_new_peer().await)
        } else {
            None
        };
        let candidate = held.as_ref().map(|held| Candidate {
            stage: &held.stage,
            founder: world.founder.keys.public_key(),
            room: &held.room,
            held_package: &held.package_id,
            generation: 2,
            token: held.token,
            carrier: held.carrier.id,
        });
        let event = Event::from_json(&world.room.welcome.payload).unwrap();
        let incoming = world.send_inbox(&event).await;
        assert_eq!(
            inbox_target(&world, &incoming, candidate.as_ref(), 2).await,
            InboxTarget::Ordinary,
        );
        assert_eq!(
            ordinary_held_source(&incoming, &world.ordinary.keys, &world.storage).await,
            Some(ordinary_package)
        );
        let result = timeout(STEP, world.command.ingest_delivery(&incoming))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.outcome, IngestOutcome::Processed);
        assert_eq!(result.effects.events.len(), 1);
        assert!(!result.left_object_unpersisted);
        validate_group(
            &world.command,
            &world.room,
            &world.founder.scope.account,
            &world.receiver.account,
        );
        assert!(world.receipts.placed(Tenant::Ordinary, 0, &incoming));
        world.receiver.ack(&incoming.message.id).await;
        authorize_group(
            &world.ordinary,
            &mut world.command,
            &world.room,
            &world.a.url,
        )
        .await;
        world
            .founder
            .routing
            .group(world.room.group.clone(), world.room.route.clone())
            .unwrap();
        assert!(
            world
                .registry
                .register(Tenant::Ordinary, world.room.subscription(&world.a.url))
        );
        world
            .registry
            .sync(&world.receiver, &[&world.ordinary.scope])
            .await;
        (world, held)
    }

    async fn hold_new_peer(&mut self) -> HeldPeer {
        let stage = Provider::new(
            self._temp.path(),
            "stage.sqlite",
            self.ordinary.keys.clone(),
            &self.a.url,
        );
        let (storage, mut runtime) = stage.open();
        let package = stage.package(&storage, &mut runtime, &self.a.url).await;
        let package_id = package.source.as_ref().unwrap().event_id.clone();
        let room = Room::found(
            &mut self.sender,
            vec![package],
            &self.a.url,
            PEER,
            false,
            None,
        )
        .await;
        let token = random();
        let carrier = peer_wrapper(
            &self.founder.keys,
            self.receiver.public,
            &token,
            2,
            &room.welcome,
        )
        .await;
        let incoming = self.send_inbox(&carrier).await;
        HeldPeer {
            stage,
            room,
            token,
            carrier,
            incoming,
            package_id,
        }
    }

    async fn ordinary_round_trip(&mut self, label: &str) -> [Duration; 5] {
        let start = Instant::now();
        let (event, expected) = self
            .room
            .frame(&mut self.sender, &self.capture, label)
            .await;
        let issued = self
            .a
            .subscription(
                self.receiver.public,
                445,
                "#h",
                json!(hex::encode(self.room.route.nostr_group_id)),
            )
            .await;
        self.a.inject(self.receiver.public, &issued, &event);
        let delivery = self.receiver.next().await;
        assert_eq!(delivery.message.id.as_slice(), event.id.as_bytes());
        let dispatch_started = Instant::now();
        let lease = self
            .registry
            .lease(&delivery, &self.receiver, &self.a.url)
            .await
            .unwrap();
        assert_eq!(lease.tenant, Tenant::Ordinary);
        assert!(self.registry.still_current(&lease));
        let dispatch = dispatch_started.elapsed();
        let ingest = ingest_frame(
            &mut self.command,
            &lease,
            &delivery,
            &self.founder.scope.account,
            &expected,
        )
        .await;
        let placement_started = Instant::now();
        assert!(
            self.receipts
                .placed(lease.tenant, lease.generation, &delivery)
        );
        let placement = placement_started.elapsed();
        let release_started = Instant::now();
        self.receiver.ack(&delivery.message.id).await;
        [
            start.elapsed(),
            dispatch,
            ingest,
            placement,
            release_started.elapsed(),
        ]
    }

    async fn send_inbox(&self, event: &Event) -> TransportDelivery {
        self.a.inject(self.receiver.public, &self.inbox, event);
        let delivery = self.receiver.next().await;
        assert_eq!(delivery.message.id.as_slice(), event.id.as_bytes());
        delivery
    }

    fn assert_inbox_stable(&self) {
        assert_eq!(
            self.a.subscription_counts(
                self.receiver.public,
                1059,
                "#p",
                &json!(self.receiver.public)
            ),
            (1, 0)
        );
        assert_eq!(
            self.b.subscription_counts(
                self.receiver.public,
                1059,
                "#p",
                &json!(self.receiver.public)
            ),
            (0, 0)
        );
    }
}

#[tokio::test]
#[ignore = "opt-in native same-account disjoint ingress and revoke proof"]
async fn ordinary_and_peer_routes_are_disjoint() {
    timeout(TOTAL, async {
        let (mut world, held) = World::setup_shared(true).await;
        world.ordinary_round_trip("warmup").await;
        let HeldPeer { stage, room, token, carrier, incoming, package_id } = held.unwrap();
        let generation = 2;
        let candidate = Candidate { stage: &stage, founder: world.founder.keys.public_key(), room: &room,
            held_package: &package_id, token, generation, carrier: carrier.id };
        let hidden = stage.path.with_extension("unavailable");
        std::fs::rename(&stage.path, &hidden).unwrap();
        // Native delivery remains owned and unacknowledged across this pause.
        let guarded = world.receiver.adapter.take_replay_status(&world.receiver.account).await;
        assert!(guarded.pending);
        drop(guarded);
        assert!(stage.open_existing().is_none());
        std::fs::copy(&world.ordinary.path, &stage.path).unwrap();
        assert!(stage.open_existing().is_none());
        std::fs::remove_file(&stage.path).unwrap();
        assert!(!world.receipts.has_ready(generation, &incoming, &room));
        world.a.inject(world.receiver.public, &world.inbox, &carrier);
        // The next ordinary positive is a barrier for this exact wire copy.
        let backlog = world.ordinary_round_trip("ordinary during held Peer inbox").await;
        assert!(matches!(inbox_target(&world, &incoming, Some(&candidate), generation + 1).await, InboxTarget::Denied));
        assert!(stage.open_existing().is_none());
        std::fs::rename(hidden, &stage.path).unwrap();
        let (storage, mut runtime) = stage.open_existing().expect("same encrypted stage");
        let peer_filter = json!(hex::encode(room.route.nostr_group_id));
        assert_eq!(world.a.subscription_counts(world.receiver.public, 445, "#h", &peer_filter), (0, 0));
        let InboxTarget::Stage(raw) = inbox_target(&world, &incoming, Some(&candidate), generation).await else {
            panic!("expected exact authorized stage target");
        };
        assert!(stage_join(&candidate, &world.receiver.account, &raw, &mut runtime, &storage).await.0);
        world.receipts.ready(generation, &incoming, &room);
        assert!(world.receipts.placed(Tenant::Peer, generation, &incoming));
        assert!(!world.receipts.placed(Tenant::Peer, generation, &incoming));
        drop((storage, runtime));
        let (storage, runtime) = stage.open();
        assert!(world.receipts.has_ready(generation, &incoming, &room));
        validate_group(&runtime, &room, &world.founder.scope.account, &world.receiver.account);
        assert!(storage.has_ingress_dedup_marker(&room.welcome.id).unwrap());
        assert!(matches!(
            storage.list_pending_application_events().unwrap().as_slice(),
            [GroupEvent::GroupJoined { group_id, via_welcome, .. }]
                if group_id == &room.group && via_welcome == &room.welcome.id
        ));
        storage.delete_pending_application_events(std::slice::from_ref(&room.welcome.id)).unwrap();
        assert!(storage.list_pending_application_events().unwrap().is_empty());
        // A session open loads durable, unconsumed events into the engine.
        // After the application acknowledges the Join, load the clean queue.
        drop((storage, runtime));
        let (storage, mut runtime) = stage.open();
        // Both SQLite files were reopened while the adapter still holds the work.
        assert!(!world.receipts.placed(Tenant::Peer, generation, &incoming));
        world.receiver.ack(&room.welcome.id).await;
        let status = world.receiver.adapter.take_replay_status(&world.receiver.account).await;
        assert!(status.pending);
        drop(status);
        let mut changes = world.receiver.adapter.replay_changes(&world.receiver.account).await.unwrap();
        world.receiver.ack(&incoming.message.id).await;
        timeout(STEP, async {
            loop {
                let status = world.receiver.adapter.take_replay_status(&world.receiver.account).await;
                if !status.pending { break; }
                drop(status);
                changes.changed().await.unwrap();
            }
        }).await.unwrap();
        authorize_group(&stage, &mut runtime, &room, &world.a.url).await;
        assert!(world.registry.register(Tenant::Peer, room.subscription(&world.a.url)));
        let mut duplicate = room.subscription(&world.b.url);
        duplicate.group_id = GroupId::new(random::<32>().to_vec());
        assert!(!world.registry.register(Tenant::Peer, duplicate));
        let mut duplicate = room.subscription(&world.b.url);
        duplicate.transport_group_id = random::<32>().to_vec();
        assert!(!world.registry.register(Tenant::Peer, duplicate));
        world.founder.routing.group(room.group.clone(), room.route.clone()).unwrap();
        let (event, payload) = room.frame(&mut world.sender, &world.capture, "only Peer on replay").await;
        world.a.retain_for_replay(event.clone());
        world.registry.sync(&world.receiver, &[&world.ordinary.scope, &stage.scope]).await;
        let group_req = world.a.subscription(world.receiver.public, 445, "#h", peer_filter.clone()).await;
        let delivery = world.receiver.next().await;
        assert_eq!(delivery.message.id.as_slice(), event.id.as_bytes());
        let lease = world.registry.lease(&delivery, &world.receiver, &world.a.url).await.unwrap();
        assert_eq!(lease.tenant, Tenant::Peer);
        assert!(world.registry.still_current(&lease));
        ingest_frame(&mut runtime, &lease, &delivery, &world.founder.scope.account, &payload).await;
        assert!(world.receipts.placed(Tenant::Peer, lease.generation, &delivery));
        world.receiver.ack(&delivery.message.id).await;
        assert_absent(&world.storage, &delivery.message.id);
        assert_absent(&world.storage, &incoming.message.id);
        assert_absent(&world.storage, &room.welcome.id);

        let (late, _) = room.frame(&mut world.sender, &world.capture, "queued before revoke").await;
        let wrong_group = EventBuilder::new(Kind::Custom(445), &late.content)
            .tags([Tag::parse(["h", &hex::encode(random::<32>())]).unwrap()])
            .finalize(&Keys::generate()).unwrap();
        world.a.inject(world.receiver.public, &group_req, &wrong_group);
        world.a.inject(world.receiver.public, &group_req, &late);
        let held = world.receiver.next().await;
        assert_eq!(held.message.id.as_slice(), late.id.as_bytes());
        let wrong_id = MessageId::new(wrong_group.id.to_bytes().to_vec());
        assert_absent(&storage, &wrong_id);
        assert_absent(&world.storage, &wrong_id);
        let mut wrong_req = held.clone();
        let ordinary_req = world.receiver.adapter.group_route_subscription_id(
            &world.receiver.account,
            &world.room.route.nostr_group_id,
        ).await.unwrap();
        if ordinary_req != group_req {
            wrong_req.source.subscription_id = Some(ordinary_req);
            assert!(world.registry.lease(&wrong_req, &world.receiver, &world.a.url).await.is_none());
        }
        wrong_req.group_id_hint = Some(world.room.group.clone());
        assert!(world.registry.lease(&wrong_req, &world.receiver, &world.a.url).await.is_none());
        let old = world.registry.lease(&held, &world.receiver, &world.a.url).await.unwrap();
        world.registry.revoke(Tenant::Peer);
        assert!(!world.registry.still_current(&old));
        assert_absent(&storage, &held.message.id);
        assert_absent(&world.storage, &held.message.id);
        assert!(world.receipts.denied(Tenant::Peer, old.generation, &held));
        world.receiver.ack(&held.message.id).await;
        world.registry.sync(&world.receiver, &[&world.ordinary.scope, &stage.scope]).await;
        timeout(STEP, async {
            while world.a.subscription_counts(world.receiver.public, 445, "#h", &peer_filter).1 == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.expect("relay observes the group CLOSE");
        assert!(!world.registry.register(Tenant::Peer, room.subscription(&world.a.url)));
        let alone = world.ordinary_round_trip("ordinary after revoke").await;
        world.assert_inbox_stable();
        assert!(stage.scope.denials().is_empty());
        assert!(world.ordinary.scope.denials().is_empty());
        let convert = |set: [Duration; 5]| set.map(|d| d.as_secs_f64() * 1000.0);
        eprintln!("peer_aggregate ordinary_ms=[round_trip,dispatch,ingest,receipt,pin] alone={:?} paused={:?}", convert(alone), convert(backlog));
        world.receiver.close().await;
    }).await.unwrap();
}

#[derive(Debug, PartialEq, Eq)]
enum InboxTarget {
    Ordinary,
    Stage(Box<TransportDelivery>),
    Denied,
}

async fn inbox_target(
    world: &World,
    delivery: &TransportDelivery,
    candidate: Option<&Candidate<'_>>,
    current_generation: u64,
) -> InboxTarget {
    let Some(outer) = exact_event(
        delivery,
        &world.receiver.account,
        TransportDeliveryPlane::AccountInbox,
        &world.a.url,
        &world.inbox,
    ) else {
        return InboxTarget::Denied;
    };
    let signer = SdkSigner(world.ordinary.keys.clone());
    let Ok(Ok(decrypted)) = timeout(
        STEP,
        nostr::nips::nip59::extract_rumor_async(&signer, &outer),
    )
    .await
    else {
        return InboxTarget::Denied;
    };
    if decrypted.rumor.kind == FIXTURE_DELIVERY {
        let Some(candidate) = candidate else {
            return InboxTarget::Denied;
        };
        if current_generation != candidate.generation {
            return InboxTarget::Denied;
        }
        return match candidate
            .resolve(&world.receiver, delivery, &world.a.url, &world.inbox)
            .await
        {
            Some(stage) => InboxTarget::Stage(Box::new(stage)),
            None => InboxTarget::Denied,
        };
    }
    if ordinary_held_source(delivery, &world.ordinary.keys, &world.storage)
        .await
        .is_some()
    {
        return InboxTarget::Ordinary;
    }
    InboxTarget::Denied
}

#[tokio::test]
#[ignore = "opt-in native shared inbox isolation negatives"]
async fn invalid_stage_inputs_never_reach_ordinary() {
    timeout(TOTAL, async {
        let mut world = World::setup().await;
        let ordinary_lifecycle = world.storage.key_package_lifecycle().unwrap();
        let ordinary_events = world.storage.list_pending_application_events().unwrap();
        let stage = Provider::new(
            world._temp.path(),
            "stage.sqlite",
            world.ordinary.keys.clone(),
            &world.a.url,
        );
        let (storage, mut runtime) = stage.open();
        let package = stage.package(&storage, &mut runtime, &world.a.url).await;
        let id = package.source.as_ref().unwrap().event_id.clone();
        let room = Room::found(
            &mut world.sender,
            vec![package],
            &world.a.url,
            PEER,
            false,
            None,
        )
        .await;
        let token = random();
        let outsider = Arc::new(Keys::generate());
        let generation = 7;
        let malformed = peer_rumor(
            &world.founder.keys,
            world.receiver.public,
            world.receiver.public,
            &token,
            generation,
            "!not base64!".to_owned(),
        )
        .await;
        let wrong_inner = peer_rumor(
            &world.founder.keys,
            world.receiver.public,
            outsider.public_key(),
            &token,
            generation,
            STANDARD.encode(&room.welcome.payload),
        )
        .await;
        let wrong_token = peer_wrapper(
            &world.founder.keys,
            world.receiver.public,
            &random(),
            generation,
            &room.welcome,
        )
        .await;
        let wrong_founder = peer_wrapper(
            &outsider,
            world.receiver.public,
            &token,
            generation,
            &room.welcome,
        )
        .await;
        let bad_reference = MessageId::new(random::<32>().to_vec());
        let wrong_ref = peer_wrapper(
            &world.founder.keys,
            world.receiver.public,
            &token,
            generation,
            &room.welcome,
        )
        .await;
        let stale = peer_wrapper(
            &world.founder.keys,
            world.receiver.public,
            &token,
            generation,
            &room.welcome,
        )
        .await;
        let raw = Event::from_json(&room.welcome.payload).unwrap();
        let mut rejected = Vec::new();
        for (event, current, package) in [
            (malformed, generation, &id),
            (wrong_inner, generation, &id),
            (wrong_token, generation, &id),
            (wrong_founder, generation, &id),
            (wrong_ref, generation, &bad_reference),
            (stale, generation + 1, &id),
            (raw, generation, &id),
        ] {
            let candidate = Candidate {
                stage: &stage,
                founder: world.founder.keys.public_key(),
                room: &room,
                held_package: package,
                token,
                generation,
                carrier: event.id,
            };
            let delivery = world.send_inbox(&event).await;
            assert_eq!(
                inbox_target(&world, &delivery, Some(&candidate), current).await,
                InboxTarget::Denied
            );
            assert_absent(&storage, &delivery.message.id);
            assert_absent(&world.storage, &delivery.message.id);
            assert!(world.receipts.denied(Tenant::Peer, generation, &delivery));
            world.receiver.ack(&delivery.message.id).await;
            rejected.push(delivery.message.id);
        }
        let wrong_destination = peer_wrapper(
            &world.founder.keys,
            outsider.public_key(),
            &token,
            generation,
            &room.welcome,
        )
        .await;
        world
            .a
            .inject(world.receiver.public, &world.inbox, &wrong_destination);
        absent_frame(&world.receiver).await;
        rejected.push(MessageId::new(wrong_destination.id.to_bytes().to_vec()));
        assert!(runtime.live_group_ids().unwrap().is_empty());

        let extra = Provider::new(
            world._temp.path(),
            "extra-leaf.sqlite",
            outsider,
            &world.a.url,
        );
        let (extra_storage, mut extra_runtime) = extra.open();
        let stage_package = stage.package(&storage, &mut runtime, &world.a.url).await;
        let stage_id = stage_package.source.as_ref().unwrap().event_id.clone();
        let extra_package = extra
            .package(&extra_storage, &mut extra_runtime, &world.a.url)
            .await;
        let extra_room = Room::found(
            &mut world.sender,
            vec![stage_package, extra_package],
            &world.a.url,
            PEER,
            false,
            Some(&world.receiver.account),
        )
        .await;
        let extra_carrier = peer_wrapper(
            &world.founder.keys,
            world.receiver.public,
            &token,
            generation,
            &extra_room.welcome,
        )
        .await;
        let extra_candidate = Candidate {
            stage: &stage,
            founder: world.founder.keys.public_key(),
            room: &extra_room,
            held_package: &stage_id,
            token,
            generation,
            carrier: extra_carrier.id,
        };
        let extra_delivery = world.send_inbox(&extra_carrier).await;
        let InboxTarget::Stage(raw) =
            inbox_target(&world, &extra_delivery, Some(&extra_candidate), generation).await
        else {
            panic!("outer authority valid for isolated extra-leaf inspection");
        };
        assert!(
            !stage_join(
                &extra_candidate,
                &world.receiver.account,
                &raw,
                &mut runtime,
                &storage,
            )
            .await
            .0
        );
        assert!(
            !world
                .receipts
                .has_ready(generation, &extra_delivery, &extra_room)
        );
        let extra_roster = runtime
            .canonical_group_membership(&extra_room.group)
            .unwrap();
        assert_eq!(extra_roster.member_leaves.len(), 3);
        assert!(stage.scope.desired().is_none());
        assert_eq!(
            world.a.subscription_counts(
                world.receiver.public,
                445,
                "#h",
                &json!(hex::encode(extra_room.route.nostr_group_id))
            ),
            (0, 0)
        );
        assert!(
            world
                .receipts
                .denied(Tenant::Peer, generation, &extra_delivery)
        );
        world.receiver.ack(&extra_delivery.message.id).await;
        rejected.push(extra_delivery.message.id);
        rejected.push(extra_room.welcome.id);
        assert_eq!(
            world
                .receipts
                .placements(Tenant::Peer, TransportDeliveryPlane::AccountInbox),
            8
        );
        assert_eq!(
            world
                .receipts
                .placements(Tenant::Ordinary, TransportDeliveryPlane::AccountInbox),
            1
        );
        for id in &rejected {
            assert_absent(&world.storage, id);
        }
        assert_eq!(
            world.storage.key_package_lifecycle().unwrap(),
            ordinary_lifecycle
        );
        assert_eq!(
            world.storage.list_pending_application_events().unwrap(),
            ordinary_events
        );
        assert_eq!(
            world.command.live_group_ids().unwrap(),
            [world.room.group.clone()]
        );
        drop((world.storage, world.command));
        let (storage, _) = world.ordinary.open();
        for id in rejected {
            assert_absent(&storage, &id);
        }
        assert_absent(&storage, &room.welcome.id);
        world.a.assert_zero_package_writes();
        world.receiver.close().await;
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "opt-in native signed Welcome effect and scoped ownership checks"]
async fn child_transport_refuses_ownership() {
    timeout(TOTAL, async {
        let mut world = World::setup().await;
        let stage = Provider::new(
            world._temp.path(),
            "stage.sqlite",
            world.ordinary.keys.clone(),
            &world.a.url,
        );
        let (storage, mut runtime) = stage.open();
        let package = stage.package(&storage, &mut runtime, &world.a.url).await;
        let id = package.source.as_ref().unwrap().event_id.clone();
        let room = Room::found(
            &mut world.sender,
            vec![package.clone()],
            &world.a.url,
            PEER,
            true,
            None,
        )
        .await;
        let token = random();
        let generation = 2;
        let carrier = peer_wrapper(
            &world.founder.keys,
            world.receiver.public,
            &token,
            generation,
            &room.welcome,
        )
        .await;
        let candidate = Candidate {
            stage: &stage,
            founder: world.founder.keys.public_key(),
            room: &room,
            held_package: &id,
            generation,
            token,
            carrier: carrier.id,
        };
        let incoming = world.send_inbox(&carrier).await;
        let raw = candidate
            .resolve(&world.receiver, &incoming, &world.a.url, &world.inbox)
            .await
            .unwrap();
        let (admitted, effects) = stage_join(
            &candidate,
            &world.receiver.account,
            &raw,
            &mut runtime,
            &storage,
        )
        .await;
        assert!(!admitted);
        assert!(effects.reports.is_empty() && effects.failures.is_empty());
        assert!(matches!(
            effects.events.as_slice(),
            [
                GroupEvent::GroupJoined { .. },
                GroupEvent::GroupStateChanged {
                    change: cgka_traits::engine::GroupStateChange::MessageRetentionChanged {
                        old_seconds: 0,
                        new_seconds: 60
                    },
                    ..
                }
            ]
        ));
        assert!(stage.scope.denials().is_empty());
        assert!(stage.scope.desired().is_none());
        assert_absent(&world.storage, &raw.message.id);
        assert!(!world.receipts.has_ready(generation, &incoming, &room));
        assert!(world.receipts.denied(Tenant::Peer, generation, &incoming));
        world.receiver.ack(&incoming.message.id).await;
        let account = &stage.scope.account;
        let endpoints = vec![endpoint(&world.a.url)];
        let activation = TransportAccountActivation {
            account_id: account.clone(),
            inbox_endpoints: endpoints.clone(),
            group_subscriptions: vec![],
            since: None,
        };
        assert!(matches!(
            stage.scope.activate_account(activation).await,
            Err(TransportAdapterError::Closed)
        ));
        let invalid_sync = TransportGroupSync {
            account_id: account.clone(),
            group_subscriptions: vec![room.subscription(&world.a.url)],
            since: None,
        };
        assert!(matches!(
            stage.scope.sync_account_groups(invalid_sync).await,
            Err(TransportAdapterError::Closed)
        ));
        assert!(matches!(
            stage.scope.deactivate_account(account).await,
            Err(TransportAdapterError::Closed)
        ));
        assert!(matches!(
            stage.scope.receive().await,
            Err(TransportAdapterError::Closed)
        ));
        let target = TransportPublishTarget::Inbox {
            recipient: account.clone(),
            endpoints: endpoints.clone(),
        };
        assert!(matches!(
            stage
                .scope
                .publish(TransportPublishRequest {
                    account_id: account.clone(),
                    target,
                    message: raw.message,
                    required_acks: 1,
                })
                .await,
            Err(TransportAdapterError::Closed)
        ));
        let signer = FixturePackageSigner {
            keys: stage.keys.clone(),
            slot: stage.slot.clone(),
            scope: stage.scope.clone(),
        };
        let publication = KeyPackagePublication {
            account_id: account.clone(),
            key_package: package,
            slot_id: stage.slot.clone(),
            created_at: Timestamp(NostrTimestamp::now().as_secs()),
            endpoints: endpoints.clone(),
        };
        let made = signer
            .prepare_key_package(publication.clone())
            .await
            .unwrap();
        assert!(
            signer
                .publish_prepared_key_package(&publication, &made)
                .await
                .is_err()
        );
        assert!(
            signer
                .withdraw_key_package_slot(&marmot_session::KeyPackageSlotWithdrawal {
                    account_id: account.clone(),
                    slot_id: stage.slot.clone(),
                    created_at: publication.created_at,
                    endpoints: endpoints.clone(),
                })
                .await
                .is_err()
        );
        assert!(
            signer
                .delete_key_package_event(&marmot_session::KeyPackageEventDeletion {
                    account_id: account.clone(),
                    event_id: id,
                    created_at: publication.created_at,
                    endpoints,
                })
                .await
                .is_err()
        );
        assert_eq!(
            stage.scope.denials(),
            [
                Denial::Activate,
                Denial::Synchronize,
                Denial::Deactivate,
                Denial::Receive,
                Denial::Publish,
                Denial::PublishPackage,
                Denial::WithdrawPackage,
                Denial::DeletePackage
            ]
        );
        assert!(stage.scope.desired().is_none());
        world.assert_inbox_stable();
        world.receiver.close().await;
    })
    .await
    .unwrap();
}
