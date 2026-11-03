//! One actor owns the MDK handle, application journal, signer and relay plane.
#[path = "command_adapter.rs"]
mod command_adapter;
#[path = "command_check.rs"]
pub(crate) mod command_check;
#[path = "command_local.rs"]
pub(crate) mod command_local;
#[path = "command_runtime.rs"]
mod command_runtime;
#[cfg(feature = "installed-checks")]
#[path = "installed_check.rs"]
pub(crate) mod installed_check;
use crate::foundation::{self, Body, Envelope};
use crate::identity::{IdentityMaterial, ProfileLease};
pub use crate::journal::{Conversation, ReceivedMessage, SendStatus};
use crate::journal::{Journal, Prepared};
use crate::transport::RelayConnector;
use crate::{Error, Result};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use cgka_traits::app_components::*;
use cgka_traits::engine::{
    CreateGroupRequest, GroupEvent, KeyPackage, KeyPackageSource, SendIntent,
};
use cgka_traits::group::ProtocolProfile;
use cgka_traits::ingest::IngestOutcome;
use cgka_traits::maintenance::{KeyPackageLifecycleState, SignedPublicationArtifact};
use cgka_traits::storage::{
    MaintenanceStorage, MessageStorage, OutboundFanoutStorage, OutboundIntentStorage,
};
use cgka_traits::transport::{TransportEnvelope, TransportMessage};
use cgka_traits::{
    GroupId, MemberId, MessageId, Timestamp, TransportAccountActivation, TransportAdapter,
    TransportAdapterError, TransportDelivery, TransportEndpoint, TransportEndpointFailure,
    TransportEndpointFailureKind, TransportGroupSubscription, TransportGroupSync,
    TransportPublishFailure, TransportPublishReport, TransportPublishRequest,
    TransportPublishTarget,
};
use hkdf::Hkdf;
use marmot_session::{
    AccountDeviceEffects, AccountDeviceRuntime, AccountIdentityProofRequest,
    AccountIdentityProofSigner, KeyPackagePublication, KeyPackagePublishError,
    KeyPackagePublishReceipt, KeyPackagePublisher, SessionConfig, TransportRoutingError,
    TransportRoutingPolicy, key_package_metadata,
};
use nostr::event::FinalizeEvent;
use nostr::event::{EventBuilder, Kind, Tag};
use nostr::key::{Keys, PublicKey};
use nostr::types::Timestamp as NostrTimestamp;
use nostr_sdk::client::Client;
use serde::Serialize;
use sha2::Sha256;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use storage_sqlite::{SqlCipherKey, SqliteAccountStorage};
use tokio::time::Instant;
use transport_nostr_adapter::{
    NostrKeyPackagePublication, NostrKeyPackagePublisher, NostrNotificationLoss, NostrRelayClient,
    NostrSdkRelayClient, NostrTransportAdapter,
};
use transport_nostr_peeler::{MarmotNostrSigner, NostrMlsPeeler, NostrTransportEvent};
use zeroize::Zeroizing;

type Runtime = AccountDeviceRuntime<PublicationGate, ExplicitRouting, Publisher>;
const PURPOSE: &str = "marmish/v2 identity transport";
const PREFIX: &str = "marmish/v2/";

#[derive(Clone, Copy, PartialEq, Eq)]
enum RoomPurpose {
    Foundation,
    Command,
}
impl RoomPurpose {
    fn prefix(self) -> &'static str {
        match self {
            Self::Foundation => PREFIX,
            Self::Command => "marmish/command/1/",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Foundation => PURPOSE,
            Self::Command => "marmish-command/1 command transport",
        }
    }
}

/// Authority supplied by the accepted MLS room, never by wire fields or IPC.
/// Only this adapter can construct production contexts after native validation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AuthenticatedCommandContext {
    local_account: crate::command_wire::Account,
    peer_account: crate::command_wire::Account,
    mls_group: Vec<u8>,
    transport_group: Vec<u8>,
    session: crate::command_wire::Id,
    direction: CommandDirection,
    target_uid: u32,
    authority_digest: crate::command_wire::Digest,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandDirection {
    Client,
    Server,
}

impl AuthenticatedCommandContext {
    pub(crate) fn local_account(&self) -> crate::command_wire::Account {
        self.local_account
    }
    pub(crate) fn peer_account(&self) -> crate::command_wire::Account {
        self.peer_account
    }
    pub(crate) fn mls_group(&self) -> &[u8] {
        &self.mls_group
    }
    pub(crate) fn transport_group(&self) -> &[u8] {
        &self.transport_group
    }
    pub(crate) fn session(&self) -> crate::command_wire::Id {
        self.session
    }
    pub(crate) fn direction(&self) -> CommandDirection {
        self.direction
    }
    pub(crate) fn target_uid(&self) -> u32 {
        self.target_uid
    }
    pub(crate) fn authority_digest(&self) -> crate::command_wire::Digest {
        self.authority_digest
    }

    #[cfg(test)]
    pub(crate) fn test_fixture(
        local_account: crate::command_wire::Account,
        peer_account: crate::command_wire::Account,
        mls_group: Vec<u8>,
        session: crate::command_wire::Id,
        direction: CommandDirection,
    ) -> Self {
        Self {
            local_account,
            peer_account,
            transport_group: mls_group.clone(),
            mls_group,
            session,
            direction,
            target_uid: unsafe { libc::geteuid() },
            authority_digest: [7; 32],
        }
    }
}

/// Check frozen publication bytes before the journal transaction. This does no
/// network work and does not grant authority; the store checks its sealed Owner.
pub(crate) fn validate_command_publication(
    publication: &crate::journal::PreparedCommandPublication,
    local: crate::command_wire::Account,
) -> Result<()> {
    use crate::command_wire as wire;
    use cgka_traits::app_event::MarmotAppEvent;
    let refuse = || Error::Invalid("command_publication");
    if publication.key.group.is_empty()
        || publication.key.group.len() > 1024
        || publication.encoded_wire.len() > wire::MAX_CONTENT
        || publication.encoded_app_event.len() > wire::MAX_CONTENT + 4096
    {
        return Err(refuse());
    }
    let event = MarmotAppEvent::decode(&publication.encoded_app_event).map_err(|_| refuse())?;
    if event.encode().map_err(|_| refuse())? != publication.encoded_app_event
        || event.id != publication.key.app_event_id
        || event.pubkey != hex::encode(local)
        || event.kind != wire::KIND
        || event.tags != vec![wire::TAG.map(str::to_owned).to_vec()]
        || event.content.as_bytes() != publication.encoded_wire
    {
        return Err(refuse());
    }
    if let Ok(request) = wire::decode_request(&publication.encoded_wire) {
        if request.binding != publication.binding
            || request.id != publication.request_id
            || request.binding.client != local
            || wire::request_digest(&publication.encoded_wire) != publication.request_sha256
        {
            return Err(refuse());
        }
    } else {
        let response = wire::decode_response(&publication.encoded_wire).map_err(|_| refuse())?;
        if response.binding != publication.binding
            || response.id != publication.request_id
            || response.binding.server != local
            || response.request_sha256 != publication.request_sha256
        {
            return Err(refuse());
        }
    }
    Ok(())
}

pub(crate) fn prepare_command_publication(
    group: &[u8],
    binding: crate::command_wire::Binding,
    request_id: crate::command_wire::Id,
    request_sha256: crate::command_wire::Digest,
    encoded_wire: Vec<u8>,
    local: crate::command_wire::Account,
) -> Result<crate::journal::PreparedCommandPublication> {
    use crate::command_wire as wire;
    let event = cgka_traits::app_event::MarmotAppEvent::new(
        hex::encode(local),
        wire::id_timestamp(request_id),
        wire::KIND,
        vec![wire::TAG.map(str::to_owned).to_vec()],
        String::from_utf8(encoded_wire.clone())
            .map_err(|_| Error::Invalid("command_publication"))?,
    );
    let publication = crate::journal::PreparedCommandPublication {
        key: crate::journal::PublicationKey {
            group: group.to_vec(),
            app_event_id: event.id.clone(),
        },
        binding,
        request_id,
        request_sha256,
        encoded_wire,
        encoded_app_event: event
            .encode()
            .map_err(|_| Error::Invalid("command_publication"))?,
    };
    validate_command_publication(&publication, local)?;
    Ok(publication)
}

#[cfg(test)]
mod command_publication_tests {
    use super::*;
    use crate::command_wire::{self as wire, Binding, Request, RequestOp};

    fn request() -> (Request, crate::journal::PreparedCommandPublication) {
        let request = Request {
            binding: Binding {
                session: [1; 16],
                client: [2; 32],
                server: [3; 32],
            },
            id: [4; 16],
            op: RequestOp::Status {
                invocation: [5; 16],
            },
        };
        let bytes = wire::encode_request(&request).unwrap();
        let publication = prepare_command_publication(
            b"authenticated-mls-group",
            request.binding.clone(),
            request.id,
            wire::request_digest(&bytes),
            bytes,
            request.binding.client,
        )
        .unwrap();
        (request, publication)
    }

    #[test]
    fn frozen_publication_requires_native_and_wire_binding() {
        let (request, publication) = request();
        publication.validate(request.binding.client).unwrap();
        assert!(publication.validate(request.binding.server).is_err());
        let mut changed = publication.clone();
        changed.request_sha256[0] ^= 1;
        assert!(changed.validate(request.binding.client).is_err());
        changed = publication.clone();
        changed.request_id[0] ^= 1;
        assert!(changed.validate(request.binding.client).is_err());
        changed = publication.clone();
        changed.binding.session[0] ^= 1;
        assert!(changed.validate(request.binding.client).is_err());
        changed = publication.clone();
        changed.key.app_event_id.push('0');
        assert!(changed.validate(request.binding.client).is_err());
        changed = publication;
        changed.encoded_app_event.push(b' ');
        assert!(changed.validate(request.binding.client).is_err());
    }

    #[test]
    fn valid_native_id_does_not_allow_a_different_purpose_or_wire_content() {
        let (request, publication) = request();
        let mut event =
            cgka_traits::app_event::MarmotAppEvent::decode(&publication.encoded_app_event).unwrap();
        event = cgka_traits::app_event::MarmotAppEvent::new(
            event.pubkey,
            event.created_at,
            event.kind,
            vec![vec!["marmish".into(), "v2-foundation".into()]],
            event.content,
        );
        let mut changed = publication.clone();
        changed.key.app_event_id = event.id.clone();
        changed.encoded_app_event = event.encode().unwrap();
        assert!(changed.validate(request.binding.client).is_err());
        let mut changed_request = request.clone();
        changed_request.id[15] ^= 1;
        event = cgka_traits::app_event::MarmotAppEvent::new(
            hex::encode(request.binding.client),
            event.created_at,
            wire::KIND,
            vec![wire::TAG.map(str::to_owned).to_vec()],
            String::from_utf8(wire::encode_request(&changed_request).unwrap()).unwrap(),
        );
        changed.key.app_event_id = event.id.clone();
        changed.encoded_app_event = event.encode().unwrap();
        assert!(changed.validate(request.binding.client).is_err());
    }
}

// Startup owns only dialing/status observation; native publication begins after
// this helper has observed Connected for every configured endpoint.
#[async_trait]
trait StartupRelay: Send {
    fn status(&self) -> nostr_sdk::relay::RelayStatus;
    async fn dial(
        &mut self,
        remaining: Duration,
    ) -> std::result::Result<(), nostr_sdk::error::ErrorKind>;
    async fn wait(&mut self, remaining: Duration);
}
struct SdkStartupRelay(nostr_sdk::relay::Relay);
#[async_trait]
impl StartupRelay for SdkStartupRelay {
    fn status(&self) -> nostr_sdk::relay::RelayStatus {
        self.0.status()
    }
    async fn dial(
        &mut self,
        remaining: Duration,
    ) -> std::result::Result<(), nostr_sdk::error::ErrorKind> {
        self.0
            .try_connect()
            .timeout(remaining)
            .await
            .map_err(|error| error.kind())
    }
    async fn wait(&mut self, remaining: Duration) {
        self.0.wait_for_connection(remaining).await;
    }
}
fn startup_status_label(status: nostr_sdk::relay::RelayStatus) -> &'static str {
    use nostr_sdk::relay::RelayStatus::*;
    match status {
        Initialized => "initialized",
        Pending => "pending",
        Connecting => "connecting",
        Connected => "connected",
        Disconnected => "disconnected",
        Terminated => "terminated",
        Banned => "banned",
        Sleeping => "sleeping",
        Shutdown => "shutdown",
    }
}
fn startup_error_label(kind: nostr_sdk::error::ErrorKind) -> &'static str {
    use nostr_sdk::error::ErrorKind::*;
    match kind {
        Protocol => "protocol",
        IO => "io",
        Transport => "transport",
        Database => "database",
        Gossip => "gossip",
        Policy => "policy",
        Timeout => "timeout",
        NotFound => "not_found",
        Invalid => "invalid",
        State => "state",
        Rejected => "rejected",
        Unsupported => "unsupported",
        LimitExceeded => "limit_exceeded",
        _ => "other",
    }
}
fn startup_connected(relay: &impl StartupRelay, deadline: Instant) -> Result<bool> {
    use nostr_sdk::relay::RelayStatus;
    if Instant::now() >= deadline {
        return Err(Error::Timeout);
    }
    let status = relay.status();
    crate::diagnostics::event("relay_startup_status", startup_status_label(status));
    if matches!(status, RelayStatus::Banned | RelayStatus::Shutdown) {
        return Err(Error::Transport("relay_connect"));
    }
    Ok(status == RelayStatus::Connected)
}
async fn wait_startup_turn(
    relay: &mut impl StartupRelay,
    turn_end: Instant,
    deadline: Instant,
) -> Result<bool> {
    if startup_connected(relay, deadline)? {
        return Ok(true);
    }
    while matches!(
        relay.status(),
        nostr_sdk::relay::RelayStatus::Pending
            | nostr_sdk::relay::RelayStatus::Connecting
            | nostr_sdk::relay::RelayStatus::Disconnected
            | nostr_sdk::relay::RelayStatus::Initialized
    ) {
        let remaining = turn_end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        relay.wait(remaining).await;
        if startup_connected(relay, deadline)? {
            return Ok(true);
        }
        if matches!(
            relay.status(),
            nostr_sdk::relay::RelayStatus::Terminated | nostr_sdk::relay::RelayStatus::Sleeping
        ) {
            break;
        }
        // A lagged/closed SDK notification receiver can return early. Keep
        // that pending state in its original turn without a busy loop.
        tokio::time::sleep_until(turn_end.min(Instant::now() + Duration::from_millis(50))).await;
    }
    Ok(false)
}
async fn connect_startup_relay(
    relay: &mut impl StartupRelay,
    deadline: Instant,
    connection_timeout: Duration,
) -> Result<()> {
    for attempt in ["one", "two", "three"] {
        if startup_connected(relay, deadline)? {
            return Ok(());
        }
        let turn_end = deadline.min(Instant::now() + connection_timeout);
        crate::diagnostics::event("relay_startup_attempt", attempt);
        match relay
            .dial(turn_end.saturating_duration_since(Instant::now()))
            .await
        {
            Ok(()) => crate::diagnostics::event("relay_startup_result", "ok"),
            Err(kind) => {
                crate::diagnostics::event("relay_startup_result", "error");
                crate::diagnostics::event("relay_startup_error", startup_error_label(kind));
            }
        }
        if wait_startup_turn(relay, turn_end, deadline).await? {
            return Ok(());
        }
    }
    if Instant::now() >= deadline {
        Err(Error::Timeout)
    } else {
        Err(Error::Transport("relay_connect"))
    }
}

// The SDK owns one bounded dial across actor turns. Observing Pending or
// Connecting must never cancel it or reset its per-relay connection deadline.
fn recover_relay(relay: &nostr_sdk::relay::Relay) -> Result<bool> {
    use nostr_sdk::relay::RelayStatus;
    match relay.status() {
        RelayStatus::Connected => Ok(true),
        RelayStatus::Initialized | RelayStatus::Terminated => {
            relay.connect();
            Ok(false)
        }
        RelayStatus::Shutdown => Err(Error::Transport("shutdown")),
        RelayStatus::Banned => Err(Error::Transport("relay_reconnect")),
        RelayStatus::Pending
        | RelayStatus::Connecting
        | RelayStatus::Disconnected
        | RelayStatus::Sleeping => Ok(false),
    }
}

/// Fence every native publication, including replay-driven frozen fanouts.
/// Returning a typed retryable failure preserves the exact native obligation.
#[derive(Clone)]
struct PublicationGate {
    inner: NostrTransportAdapter,
    paused: Arc<AtomicBool>,
    loss: tokio::sync::watch::Receiver<Option<NostrNotificationLoss>>,
    baseline: Arc<RwLock<Option<NostrNotificationLoss>>>,
}
impl PublicationGate {
    fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }
    fn resume(&self, baseline: Option<NostrNotificationLoss>) {
        *self.baseline.write().unwrap_or_else(|p| p.into_inner()) = baseline;
        self.paused.store(false, Ordering::Release);
    }
    fn blocked(&self) -> bool {
        self.paused.load(Ordering::Acquire)
            || *self.loss.borrow() != *self.baseline.read().unwrap_or_else(|p| p.into_inner())
    }
}
#[async_trait]
impl TransportAdapter for PublicationGate {
    async fn activate_account(
        &self,
        value: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.inner.activate_account(value).await
    }
    async fn sync_account_groups(
        &self,
        value: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.inner.sync_account_groups(value).await
    }
    async fn deactivate_account(
        &self,
        account: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.inner.deactivate_account(account).await
    }
    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        self.inner.receive().await
    }
    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        if self.blocked() {
            let failed = request
                .target
                .endpoints()
                .iter()
                .cloned()
                .map(|endpoint| TransportEndpointFailure {
                    endpoint,
                    reason: "recovery_paused".into(),
                    kind: TransportEndpointFailureKind::RetryableUnavailable,
                    rejection_category: None,
                })
                .collect();
            return Err(TransportAdapterError::PublishEndpoints(
                TransportPublishFailure::with_endpoint_failures("recovery_paused", failed),
            ));
        }
        self.inner.publish(request).await
    }
}

pub struct OpenedAccountBinding {
    profile: String,
    public: PublicKey,
}
impl OpenedAccountBinding {
    pub(crate) fn profile_id(&self) -> &str {
        &self.profile
    }
    pub(crate) fn public_key(&self) -> PublicKey {
        self.public
    }
}
#[derive(Serialize)]
pub struct RuntimeStatus {
    pub identity: String,
    pub conversations: usize,
    pub received: usize,
    pub pending: usize,
}

#[derive(Clone)]
struct ExplicitRouting {
    endpoints: Vec<TransportEndpoint>,
    state: Arc<RwLock<Routes>>,
}
#[derive(Default)]
struct Routes {
    inbox: HashMap<MemberId, Vec<TransportEndpoint>>,
    groups: Vec<TransportGroupSubscription>,
}
impl ExplicitRouting {
    fn new(endpoints: Vec<TransportEndpoint>) -> Self {
        Self {
            endpoints,
            state: Arc::new(RwLock::new(Routes::default())),
        }
    }
    fn inbox(&self, peer: MemberId) {
        self.state
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .inbox
            .insert(peer, self.endpoints.clone());
    }
    fn remove(&self, group: &GroupId) {
        self.state
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .groups
            .retain(|v| &v.group_id != group);
    }
    fn validate_group(&self, group: &GroupId, routing: &NostrRoutingV1) -> Result<()> {
        if !self.endpoints.iter().any(|e| routing.relays.contains(&e.0)) {
            return Err(Error::Mdk("group_relay_intersection"));
        }
        let state = self.state.read().unwrap_or_else(|p| p.into_inner());
        if state.groups.iter().any(|other| {
            &other.group_id != group && other.transport_group_id == routing.nostr_group_id
        }) {
            return Err(Error::Mdk("duplicate_transport_group"));
        }
        Ok(())
    }
    fn group(&self, group: GroupId, routing: NostrRoutingV1) -> Result<()> {
        self.validate_group(&group, &routing)?;
        let endpoints: Vec<_> = self
            .endpoints
            .iter()
            .filter(|e| routing.relays.contains(&e.0))
            .cloned()
            .collect();
        if endpoints.is_empty() {
            return Err(Error::Transport("group_relay_intersection"));
        }
        let mut state = self.state.write().unwrap_or_else(|p| p.into_inner());
        state.groups.retain(|v| v.group_id != group);
        state.groups.push(TransportGroupSubscription {
            group_id: group,
            transport_group_id: routing.nostr_group_id.to_vec(),
            endpoints,
            retained_since: None,
        });
        Ok(())
    }
}
impl TransportRoutingPolicy for ExplicitRouting {
    fn local_inbox_endpoints(&self) -> Vec<TransportEndpoint> {
        self.endpoints.clone()
    }
    fn key_package_endpoints(&self) -> Vec<TransportEndpoint> {
        self.endpoints.clone()
    }
    fn group_subscriptions(&self) -> Vec<TransportGroupSubscription> {
        self.state
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .groups
            .clone()
    }
    fn publish_target(
        &self,
        message: &TransportMessage,
    ) -> std::result::Result<TransportPublishTarget, TransportRoutingError> {
        let state = self.state.read().unwrap_or_else(|p| p.into_inner());
        match &message.envelope {
            TransportEnvelope::Welcome { recipient } => Ok(TransportPublishTarget::Inbox {
                recipient: recipient.clone(),
                endpoints: state
                    .inbox
                    .get(recipient)
                    .cloned()
                    .ok_or(TransportRoutingError::MissingInboxRoute)?,
            }),
            TransportEnvelope::GroupMessage { transport_group_id } => {
                let r = state
                    .groups
                    .iter()
                    .find(|r| &r.transport_group_id == transport_group_id)
                    .ok_or(TransportRoutingError::MissingGroupRoute)?;
                Ok(TransportPublishTarget::Group {
                    group_id: r.group_id.clone(),
                    transport_group_id: r.transport_group_id.clone(),
                    endpoints: r.endpoints.clone(),
                })
            }
        }
    }
    fn required_acks(&self, _: &TransportPublishTarget) -> usize {
        1
    }
}
struct ProofSigner(Arc<Keys>);
impl AccountIdentityProofSigner for ProofSigner {
    fn sign_account_identity_proof(
        &self,
        r: &AccountIdentityProofRequest,
    ) -> std::result::Result<[u8; 64], String> {
        if r.account_identity.as_slice() != self.0.public_key().to_bytes() {
            return Err("proof_account".into());
        }
        let event = r
            .proof_event()?
            .finalize(self.0.as_ref())
            .map_err(|_| "proof_sign".to_owned())?;
        r.signature_from_signed_event(event)
    }
}
#[derive(Clone)]
struct Publisher {
    keys: Arc<Keys>,
    relay: Arc<dyn NostrRelayClient>,
}
impl Publisher {
    fn publication(
        p: &KeyPackagePublication,
    ) -> std::result::Result<NostrKeyPackagePublication, KeyPackagePublishError> {
        let m = key_package_metadata(&p.key_package)
            .map_err(|_| KeyPackagePublishError::unexposed("metadata"))?;
        if m.protocol_profile != ProtocolProfile::Current
            || m.credential_identity_hex != hex::encode(p.account_id.as_slice())
        {
            return Err(KeyPackagePublishError::unexposed("account"));
        }
        Ok(NostrKeyPackagePublication {
            client_name: None,
            account_id: p.account_id.clone(),
            key_package: p.key_package.clone(),
            key_package_slot_id: p.slot_id.clone(),
            key_package_ref: m.key_package_ref_hex,
            mls_ciphersuite: format!("0x{:04x}", m.ciphersuite),
            mls_extensions: m
                .mls_extensions
                .iter()
                .map(|v| format!("0x{v:04x}"))
                .collect(),
            mls_proposals: m
                .mls_proposals
                .iter()
                .map(|v| format!("0x{v:04x}"))
                .collect(),
            app_components: m
                .app_components
                .iter()
                .filter(|v| **v >= PRIVATE_USE_APP_COMPONENT_ID_START)
                .map(|v| format!("0x{v:04x}"))
                .collect(),
            publish_endpoints: p.endpoints.clone(),
        })
    }
}
#[async_trait]
impl KeyPackagePublisher for Publisher {
    async fn prepare_key_package(
        &self,
        p: KeyPackagePublication,
    ) -> std::result::Result<SignedPublicationArtifact, KeyPackagePublishError> {
        let dto = Self::publication(&p)?
            .to_event_at(p.created_at.0)
            .map_err(|_| KeyPackagePublishError::unexposed("event"))?;
        let tags = dto
            .tags
            .into_iter()
            .map(Tag::parse)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| KeyPackagePublishError::unexposed("tags"))?;
        let signed = EventBuilder::new(Kind::Custom(30443), dto.content)
            .tags(tags)
            .custom_created_at(NostrTimestamp::from_secs(p.created_at.0))
            .finalize(self.keys.as_ref())
            .map_err(|_| KeyPackagePublishError::unexposed("sign"))?;
        let event = NostrTransportEvent::from_nostr_event(&signed)
            .map_err(|_| KeyPackagePublishError::unexposed("event"))?;
        Ok(SignedPublicationArtifact {
            id: MessageId::new(signed.id.to_bytes().to_vec()),
            created_at: p.created_at,
            bytes: serde_json::to_vec(&event)
                .map_err(|_| KeyPackagePublishError::unexposed("encode"))?,
        })
    }
    async fn publish_prepared_key_package(
        &self,
        p: &KeyPackagePublication,
        a: &SignedPublicationArtifact,
    ) -> std::result::Result<KeyPackagePublishReceipt, KeyPackagePublishError> {
        let np = Self::publication(p)?;
        let event: NostrTransportEvent = serde_json::from_slice(&a.bytes)
            .map_err(|_| KeyPackagePublishError::unusable_artifact("artifact"))?;
        if event.id != hex::encode(a.id.as_slice())
            || event.created_at != a.created_at.0
            || !np
                .matches_prepared_event(&event)
                .map_err(|_| KeyPackagePublishError::unusable_artifact("artifact"))?
        {
            return Err(KeyPackagePublishError::unusable_artifact(
                "artifact_binding",
            ));
        }
        let result = NostrKeyPackagePublisher::new(self.relay.clone())
            .publish_prepared_key_package(&np, &event)
            .await
            .map_err(|_| {
                crate::diagnostics::event("mdk_key_package_publish", "transport_error");
                KeyPackagePublishError::exposed("publish")
            })?;
        crate::diagnostics::event(
            "mdk_key_package_publish",
            if result.accepted.is_empty() {
                "no_relay_accepted"
            } else if result.failed.is_empty() {
                "accepted"
            } else {
                "partially_accepted"
            },
        );
        Ok(KeyPackagePublishReceipt {
            accepted: result.accepted.into_iter().map(|r| r.endpoint).collect(),
            failed: result.failed.into_iter().map(|r| r.endpoint).collect(),
        })
    }
}

pub struct MdkOwner {
    runtime: Runtime,
    storage: SqliteAccountStorage,
    journal: Journal,
    adapter: NostrTransportAdapter,
    publication: PublicationGate,
    routing: ExplicitRouting,
    connector: RelayConnector,
    client: Client,
    relay: Arc<NostrSdkRelayClient>,
    forwarder: Option<tokio::task::JoinHandle<()>>,
    notification_loss: tokio::sync::watch::Receiver<Option<NostrNotificationLoss>>,
    account: MemberId,
    identity: String,
    allowed: BTreeSet<String>,
    binding: OpenedAccountBinding,
    active: bool,
    #[cfg(test)]
    command_creation_test: Option<command_runtime::CreationTest>,
    retention: u64,
    welcome_retry: HashMap<Vec<u8>, (Instant, u8)>,
    reconnect_after: Instant,
    recovery: bool,
    recovery_subscribed: bool,
    loss_baseline: Option<NostrNotificationLoss>,
    pending_cohort: VecDeque<(u8, Vec<u8>)>,
    pending_budget: usize,
    commands: Option<crate::command_service::CommandService>,
    command_probe: Option<command_adapter::CommandProbe>,
    command_source_cursor: Option<crate::journal::SourceCursor>,
    command_invalidation_cursor: Option<crate::journal::SourceKey>,
}
type PendingKey = (u8, Vec<u8>);
fn select_pending(
    cohort: &mut VecDeque<PendingKey>,
    budget: &mut usize,
    keys: impl Iterator<Item = PendingKey>,
) -> Vec<PendingKey> {
    if cohort.is_empty() {
        cohort.extend(keys);
    }
    let count = (*budget).min(cohort.len());
    *budget -= count;
    cohort.drain(..count).collect()
}
fn mdk<T, E>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(|_| Error::Mdk("runtime"))
}
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::Invalid("clock"))
}
fn random<const N: usize>() -> Result<[u8; N]> {
    let mut v = [0; N];
    getrandom::fill(&mut v).map_err(|_| Error::Identity("random"))?;
    Ok(v)
}
impl MdkOwner {
    pub fn open(
        profile: &ProfileLease,
        material: &IdentityMaterial,
        connector: RelayConnector,
        allowed_peers: Vec<String>,
    ) -> Result<Self> {
        let identity = material.public_key().to_hex();
        let mut allowed = BTreeSet::new();
        for p in allowed_peers {
            if !foundation::valid_hex(&p, 64) || p == identity {
                return Err(Error::Invalid("peer"));
            }
            allowed.insert(p);
        }
        if allowed.len() > 8192 {
            return Err(Error::Invalid("peer_count"));
        }
        let hk = Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), material.sqlcipher_root());
        let mut mk = Zeroizing::new([0; 32]);
        let mut jk = Zeroizing::new([0; 32]);
        hk.expand(b"mdk", mk.as_mut())
            .map_err(|_| Error::Storage("derive"))?;
        hk.expand(b"journal", jk.as_mut())
            .map_err(|_| Error::Storage("derive"))?;
        marmish_engine::initialize_storage().map_err(|_| Error::Storage("sqlcipher_init"))?;
        // Close/reopen proof precedes constructing any networking object. The
        // retained clone below shares one connection rather than reopening it.
        drop(mdk(SqliteAccountStorage::open_encrypted(
            profile.mdk_database(),
            &SqlCipherKey::raw(*mk),
        ))?);
        let storage = mdk(SqliteAccountStorage::open_encrypted(
            profile.mdk_database(),
            &SqlCipherKey::raw(*mk),
        ))?;
        drop(Journal::open(profile.journal_database(), &jk, &identity)?);
        let journal = Journal::open(profile.journal_database(), &jk, &identity)?;
        if mdk(storage.key_package_lifecycle())?.is_none() {
            mdk(
                storage.put_key_package_lifecycle(&KeyPackageLifecycleState::slot_only(
                    hex::encode(material.device_slot()),
                )),
            )?;
        }
        let keys = material.signer();
        let client = connector.sdk_client(keys.clone())?;
        let account = MemberId::new(material.public_key().to_bytes().to_vec());
        let signer: Arc<dyn MarmotNostrSigner> = keys.clone();
        let relay = Arc::new(NostrSdkRelayClient::with_account_signer(
            client.clone(),
            account.clone(),
            signer.clone(),
        ));
        let adapter = NostrTransportAdapter::new(relay.clone());
        let routing = ExplicitRouting::new(
            connector
                .policy()
                .urls()
                .iter()
                .map(|u| TransportEndpoint(u.to_string()))
                .collect(),
        );
        for p in &allowed {
            routing.inbox(MemberId::new(
                hex::decode(p).map_err(|_| Error::Invalid("peer"))?,
            ));
        }
        let config = SessionConfig::with_storage(
            storage.clone(),
            Duration::ZERO,
            account.as_slice().to_vec(),
            Box::new(NostrMlsPeeler::new().with_welcome_signer_arc(signer)),
        )
        .account_identity_proof_signer(Arc::new(ProofSigner(keys.clone())))
        .supported_app_components(
            default_group_components()
                .into_iter()
                .chain([NOSTR_ROUTING_COMPONENT_ID]),
        );
        let notification_loss = relay
            .notification_loss()
            .map_err(|_| Error::Transport("notification_watch"))?;
        let publication = PublicationGate {
            inner: adapter.clone(),
            paused: Arc::new(AtomicBool::new(false)),
            loss: notification_loss.clone(),
            baseline: Arc::new(RwLock::new(notification_loss.borrow().clone())),
        };
        let runtime = mdk(AccountDeviceRuntime::open(config, || {
            Ok::<_, marmot_session::SessionError>((
                publication.clone(),
                routing.clone(),
                Publisher {
                    keys,
                    relay: relay.clone(),
                },
            ))
        }))?;
        let mut owner = Self {
            runtime,
            storage,
            journal,
            adapter,
            publication,
            routing,
            connector,
            client,
            relay,
            forwarder: None,
            notification_loss,
            account,
            identity,
            allowed,
            binding: OpenedAccountBinding {
                profile: profile.profile_id().into(),
                public: material.public_key(),
            },
            active: false,
            #[cfg(test)]
            command_creation_test: None,
            retention: 86400,
            welcome_retry: HashMap::new(),
            reconnect_after: Instant::now(),
            recovery: false,
            recovery_subscribed: false,
            loss_baseline: None,
            pending_cohort: VecDeque::new(),
            pending_budget: 64,
            commands: None,
            command_probe: None,
            command_source_cursor: None,
            command_invalidation_cursor: None,
        };
        for c in owner
            .journal
            .conversations()?
            .into_iter()
            .filter(|c| c.revoked)
        {
            let g = GroupId::new(hex::decode(c.group_id).map_err(|_| Error::Storage("group_id"))?);
            mdk(owner.runtime.forget_group_local(&g))?;
        }
        owner.reconcile_orphans()?;
        owner.maintain_journal()?;
        owner.rebuild_routes()?;
        owner.reconcile_invalidations()?;
        Ok(owner)
    }
    fn native_group_union(&self) -> Result<Vec<GroupId>> {
        let mut groups: BTreeMap<Vec<u8>, GroupId> = BTreeMap::new();
        let live = mdk(self.runtime.live_group_ids())?;
        let unhydrated = self.runtime.unhydrated_group_ids();
        let quarantined = self.runtime.quarantined_groups();
        let fanouts = mdk(self.storage.list_outbound_fanouts())?;
        for group in live
            .into_iter()
            .chain(unhydrated)
            .chain(quarantined.into_iter().map(|(g, _)| g))
            .chain(fanouts.into_iter().filter_map(|f| f.group_id().cloned()))
        {
            groups.insert(group.as_slice().to_vec(), group);
        }
        for c in self.journal.conversations()? {
            let bytes = hex::decode(c.group_id).map_err(|_| Error::Storage("group_id"))?;
            groups.insert(bytes.clone(), GroupId::new(bytes));
        }
        Ok(groups.into_values().collect())
    }
    fn reconcile_orphans(&mut self) -> Result<()> {
        let pending = mdk(self.storage.list_pending_application_events())?;
        let joins: BTreeSet<_> = pending
            .iter()
            .filter_map(|e| {
                if let GroupEvent::GroupJoined { group_id, .. } = e {
                    Some(group_id.as_slice().to_vec())
                } else {
                    None
                }
            })
            .collect();
        for group in self.native_group_union()? {
            if self
                .journal
                .conversation_by_group(group.as_slice())?
                .is_some()
                || self.journal.command_room(group.as_slice())?.is_some()
                || joins.contains(group.as_slice())
            {
                continue;
            }
            // Valid locally-created orphans retain a durable admission refusal;
            // unsolicited policy-invalid groups cannot consume that ledger.
            match self.check_group(&group, None) {
                Ok((session, _, _, _)) => {
                    let current = now()?;
                    if foundation::classify_new_id(&session, current, self.journal.replay_floor()?)?
                        == foundation::Freshness::Eligible
                    {
                        self.journal.reject_group_at(
                            group.as_slice(),
                            Some(foundation::id_timestamp(&session)?),
                            current,
                        )?;
                    }
                }
                Err(Error::Mdk(_) | Error::Invalid(_)) => {}
                // Quarantined/unhydrated orphans have no accepted authority.
                Err(Error::Transport("group_state_unavailable")) => {}
                Err(e) => return Err(e),
            }
            self.routing.remove(&group);
            self.forget_and_confirm(&group)?;
        }
        for group in self.journal.pending_group_cleanup()? {
            self.forget_and_confirm(&GroupId::new(group))?;
        }
        Ok(())
    }
    fn maintain_journal(&mut self) -> Result<()> {
        let mut protected: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut command_protection = crate::journal::CommandProtection::default();
        // Defensive compatibility with storage that retains invalidations. At
        // pinned MDK 8abc51b only receives/joins are pending events; invalidations
        // arrive in ephemeral effect sets. The terminal-state source sweep is
        // their durable recovery backstop, rather than this normally empty gate.
        for event in mdk(self.storage.list_pending_application_events())? {
            if let GroupEvent::AppMessageInvalidated { group_id, .. } = event {
                command_protection
                    .invalidation_groups
                    .insert(group_id.as_slice().to_vec());
            }
        }
        let fanouts = mdk(self.storage.list_outbound_fanouts())?;
        protected.extend(
            fanouts
                .iter()
                .filter_map(|f| f.group_id().map(|g| g.as_slice().to_vec())),
        );
        for fanout in &fanouts {
            if let (Some(group), Some(application)) =
                (fanout.group_id(), fanout.application_message())
            {
                command_protection
                    .publications
                    .insert(crate::journal::PublicationKey {
                        group: group.as_slice().to_vec(),
                        app_event_id: application.app_event_id.clone(),
                    });
            }
        }
        protected.extend(
            self.runtime
                .unhydrated_group_ids()
                .iter()
                .map(|g| g.as_slice().to_vec()),
        );
        protected.extend(
            self.runtime
                .quarantined_groups()
                .iter()
                .map(|(g, _)| g.as_slice().to_vec()),
        );
        for group in self.native_group_union()? {
            let queued = mdk(self.storage.list_queued_outbound_intents(&group))?;
            for intent in &queued {
                command_protection
                    .queued
                    .insert((group.as_slice().to_vec(), intent.id.as_slice().to_vec()));
            }
            if !queued.is_empty() {
                protected.insert(group.as_slice().to_vec());
            }
        }
        command_protection.groups = protected.clone();
        self.journal.command_maintain(now()?, &command_protection)?;
        if command_protection.invalidation_groups.is_empty() {
            self.journal
                .maintain(now()?, self.retention_secs(), &protected)?;
        }
        if !protected.is_empty() {
            crate::diagnostics::event("mdk_retention", "native_obligations_protected");
        }
        Ok(())
    }
    fn reconcile_invalidations(&mut self) -> Result<()> {
        for (group, native) in self.journal.native_bindings()? {
            match self.storage.get_message(&MessageId::new(native.clone())) {
                Ok(row)
                    if row.group_id.as_slice() == group
                        && matches!(
                            row.state,
                            cgka_traits::MessageState::Sent | cgka_traits::MessageState::Processed
                        ) => {}
                Ok(_) => self.journal.invalidate(&group, &native)?,
                Err(cgka_traits::StorageError::NotFound) => {
                    self.journal.invalidate(&group, &native)?
                }
                Err(_) => return Err(Error::Storage("native_reconcile")),
            }
        }
        Ok(())
    }
    pub fn opened_binding(&self) -> &OpenedAccountBinding {
        &self.binding
    }
    pub fn conversations(&self) -> Result<Vec<Conversation>> {
        self.journal.conversations()
    }
    pub fn received(&self) -> Result<Vec<ReceivedMessage>> {
        self.journal.received()
    }
    pub fn received_page(&self, after: Option<u64>, limit: usize) -> Result<Vec<ReceivedMessage>> {
        self.journal.received_page(after, limit)
    }
    pub fn status(&self) -> Result<RuntimeStatus> {
        let (conversations, received, pending) = self.journal.status_counts()?;
        Ok(RuntimeStatus {
            identity: self.identity.clone(),
            conversations,
            received,
            pending,
        })
    }
    pub fn sent_status(&self, session: &str, id: &str) -> Result<SendStatus> {
        let (_, g) = self
            .journal
            .conversation(session)?
            .ok_or(Error::Invalid("session"))?;
        self.journal
            .outgoing(&g, id)?
            .ok_or(Error::Invalid("application_id"))
    }
}
impl Drop for MdkOwner {
    fn drop(&mut self) {
        self.connector.cancel();
        if let Some(forwarder) = self.forwarder.take() {
            forwarder.abort();
        }
    }
}

impl MdkOwner {
    fn check_group(
        &self,
        g: &GroupId,
        expected_creator: Option<&str>,
    ) -> Result<(String, String, NostrRoutingV1, String)> {
        self.check_group_for(g, expected_creator, RoomPurpose::Foundation)
    }
    fn check_group_for(
        &self,
        g: &GroupId,
        expected_creator: Option<&str>,
        purpose: RoomPurpose,
    ) -> Result<(String, String, NostrRoutingV1, String)> {
        if purpose == RoomPurpose::Foundation && self.journal.group_rejected(g.as_slice())? {
            return Err(Error::Mdk("group_rejected"));
        }
        let record = self
            .runtime
            .group_record(g)
            .map_err(|_| Error::Transport("group_state_unavailable"))?;
        let session = record
            .name
            .strip_prefix(purpose.prefix())
            .ok_or(Error::Mdk("conversation_purpose"))?;
        if !foundation::valid_hex(session, 32)
            || record.description != purpose.description()
            || record.protocol_profile != ProtocolProfile::Current
            || record.removed
        {
            return Err(Error::Mdk("conversation_purpose"));
        }
        let roster = self
            .runtime
            .canonical_group_membership(g)
            .map_err(|_| Error::Transport("group_state_unavailable"))?;
        if !roster.local_leaf_active || roster.member_leaves.len() != 2 {
            return Err(Error::Mdk("conversation_membership"));
        }
        let mut peers: BTreeSet<_> = roster
            .member_leaves
            .iter()
            .map(|v| hex::encode(v.member.as_slice()))
            .collect();
        if peers.len() != 2 || !peers.remove(&self.identity) {
            return Err(Error::Mdk("conversation_membership"));
        }
        let peer = peers
            .into_iter()
            .next()
            .ok_or(Error::Mdk("conversation_peer"))?;
        if !self.allowed.contains(&peer) {
            return Err(Error::Mdk("untrusted_peer"));
        }
        let admins = self
            .runtime
            .admin_pubkeys(g)
            .map_err(|_| Error::Transport("group_state_unavailable"))?;
        if admins.len() != 1 {
            return Err(Error::Mdk("conversation_admin"));
        }
        let admin = hex::encode(admins[0]);
        if admin != self.identity && admin != peer || expected_creator.is_some_and(|v| v != admin) {
            return Err(Error::Mdk("conversation_admin"));
        }
        let ids: Vec<_> = default_group_components()
            .into_iter()
            .chain([
                NOSTR_ROUTING_COMPONENT_ID,
                ACCOUNT_DEVICES_COMPONENT_ID,
                GROUP_MESSAGE_RETENTION_COMPONENT_ID,
            ])
            .collect();
        let values = self
            .runtime
            .app_components(g, &ids)
            .map_err(|_| Error::Transport("group_state_unavailable"))?;
        let mut digest = Vec::new();
        for (id, value) in ids.into_iter().zip(values) {
            if id == ACCOUNT_DEVICES_COMPONENT_ID || id == GROUP_MESSAGE_RETENTION_COMPONENT_ID {
                if value.is_some() {
                    return Err(Error::Mdk("forbidden_component"));
                }
                continue;
            }
            let value = value.ok_or(Error::Mdk("required_component"))?;
            digest.extend(id.to_be_bytes());
            digest.extend((value.len() as u32).to_be_bytes());
            digest.extend(value);
        }
        let routing = self
            .runtime
            .app_component(g, NOSTR_ROUTING_COMPONENT_ID)
            .map_err(|_| Error::Transport("group_state_unavailable"))?
            .ok_or(Error::Mdk("routing_component"))?;
        let routing =
            decode_nostr_routing_v1(&routing).map_err(|_| Error::Mdk("routing_component"))?;
        // Route policy is an authenticated group-admission condition, not an
        // operational transport outage. Reject it before journal acceptance.
        self.routing.validate_group(g, &routing)?;
        // Freeze the authenticated initial components, admin and leaf set.
        digest.extend(admin.as_bytes());
        let mut leaves: Vec<_> = roster
            .member_leaves
            .iter()
            .map(|v| (hex::encode(v.member.as_slice()), v.leaf_index))
            .collect();
        leaves.sort();
        digest.extend(serde_json::to_vec(&leaves).map_err(|_| Error::Mdk("authority"))?);
        Ok((
            session.to_owned(),
            peer,
            routing,
            foundation::payload_digest(&digest),
        ))
    }
    fn rebuild_routes(&mut self) -> Result<()> {
        for conversation in self.journal.conversations()? {
            let g = GroupId::new(
                hex::decode(&conversation.group_id).map_err(|_| Error::Storage("group_id"))?,
            );
            if conversation.revoked {
                self.routing.remove(&g);
                continue;
            }
            match self.ensure_trusted(&g) {
                Ok(()) => {}
                Err(Error::Mdk("conversation_revoked")) => continue,
                Err(error) => return Err(error),
            }
            let (_, _, route, _) = self.check_group(&g, None)?;
            self.routing.group(g, route)?;
        }
        self.rebuild_command_routes()?;
        Ok(())
    }
    fn ensure_ingress(&self) -> Result<()> {
        if self.recovery {
            return Err(Error::Transport("transport_recovery_pending"));
        }
        if *self.notification_loss.borrow() != self.loss_baseline {
            return Err(Error::Transport("notification_loss"));
        }
        if self
            .forwarder
            .as_ref()
            .is_none_or(|task| task.is_finished())
        {
            return Err(Error::Transport("notification_forwarder_stopped"));
        }
        Ok(())
    }
    pub async fn activate(&mut self, deadline: Instant) -> Result<()> {
        self.pending_budget = 64;
        if self.active {
            return Ok(());
        }
        self.connector.bootstrap(deadline).await?;
        if self.forwarder.is_none() {
            self.forwarder = Some(
                self.relay
                    .spawn_notification_forwarder(self.adapter.clone()),
            );
        }
        self.ensure_ingress()?;
        for endpoint in self.connector.policy().urls() {
            let connection_timeout = self.connector.connection_timeout(endpoint)?;
            tokio::time::timeout_at(
                deadline,
                self.client
                    .add_relay(endpoint.as_str())
                    .connect_timeout(connection_timeout)
                    .limits(self.connector.relay_limits())
                    .reconnect(false)
                    .notification_channel_size(64),
            )
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("add_relay"))?;
            let relay = self
                .client
                .relay(endpoint.as_str())
                .await
                .map_err(|_| Error::Transport("relay_connect"))?
                .ok_or(Error::Transport("relay_connect"))?;
            connect_startup_relay(&mut SdkStartupRelay(relay), deadline, connection_timeout)
                .await?;
        }
        // Ordinary drain does not drive KeyPackage maintenance. Readiness
        // requires an actual acknowledged publication of this device's stable
        // slot; restart republishing reuses its durable signed artifact.
        tokio::time::timeout_at(deadline, self.runtime.republish_key_package())
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| {
                crate::diagnostics::event("mdk_key_package_publish", "failed");
                Error::Mdk("key_package_publish")
            })?;
        tokio::time::timeout_at(deadline, self.runtime.activate_transport(None))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("activate"))?;
        // Drain may publish. All transport admission and Arti bootstrap above
        // must therefore finish first. Reconcile retained pending work before
        // accepting a fresh command from IPC.
        let effects = tokio::time::timeout_at(deadline, self.runtime.drain())
            .await
            .map_err(|_| Error::Timeout)?;
        self.apply_effects(mdk(effects)?, deadline).await?;
        self.replay_pending(deadline).await?;
        self.submit_unsubmitted(deadline).await?;
        self.ensure_ingress()?;
        self.active = true;
        Ok(())
    }
    async fn discover(&self, peer: &str, deadline: Instant) -> Result<KeyPackage> {
        use nostr::filter::Filter;
        use nostr_sdk::client::ReqTarget;
        use nostr_sdk::relay::{AcquisitionEnd, AcquisitionLimits};
        let peerkey = PublicKey::from_hex(peer).map_err(|_| Error::Invalid("peer"))?;
        let operation = deadline.min(Instant::now() + Duration::from_secs(15));
        let mut candidates = Vec::new();
        let mut total = 0usize;
        let mut seen = BTreeSet::new();
        for endpoint in self.connector.policy().urls() {
            let left = operation.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let h = self
                .client
                .acquire_events(
                    ReqTarget::single(
                        endpoint.as_str(),
                        [Filter::new().author(peerkey).kind(Kind::Custom(30443))],
                    ),
                    AcquisitionLimits::new(1, 17, 4 * 1024 * 1024 - total, left),
                )
                .await
                .map_err(|_| Error::Transport("key_package_query"))?;
            let report = tokio::time::timeout_at(operation, h.finish())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("key_package_query"))?;
            for result in report.relays.into_values() {
                if !matches!(result.end, AcquisitionEnd::Completed) {
                    return Err(Error::Transport("key_package_incomplete"));
                }
                total = total
                    .checked_add(result.received_event_bytes)
                    .ok_or(Error::Transport("key_package_budget"))?;
                if total >= 4 * 1024 * 1024 {
                    return Err(Error::Transport("key_package_budget"));
                }
                for event in result.events {
                    if seen.insert(event.id) {
                        candidates.push(event);
                    }
                    if candidates.len() > 16 {
                        return Err(Error::Transport("key_package_candidates"));
                    }
                }
            }
        }
        candidates.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        for event in candidates {
            if event.pubkey != peerkey
                || event.kind != Kind::Custom(30443)
                || event.verify().is_err()
            {
                continue;
            }
            let bytes = match STANDARD.decode(event.content.as_bytes()) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let kp = KeyPackage {
                bytes,
                source: Some(KeyPackageSource {
                    event_id: MessageId::new(event.id.to_bytes().to_vec()),
                }),
                protocol_profile: ProtocolProfile::Current,
            };
            let metadata = match key_package_metadata(&kp) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let time = now()?;
            if metadata.protocol_profile != ProtocolProfile::Current
                || metadata.credential_identity_hex != peer
                || metadata.ciphersuite != u16::from(cgka_engine::DEFAULT_CIPHERSUITE)
                || metadata.not_before > time
                || metadata.not_after <= time
                || !default_group_components()
                    .into_iter()
                    .chain([NOSTR_ROUTING_COMPONENT_ID])
                    .all(|id| metadata.app_components.contains(&id))
            {
                continue;
            }
            let dto = NostrTransportEvent::from_nostr_event(&event)
                .map_err(|_| Error::Mdk("key_package_event"))?;
            let slots: Vec<_> = dto
                .tags
                .iter()
                .filter(|t| t.first().is_some_and(|v| v == "d"))
                .collect();
            if slots.len() != 1 || slots[0].len() != 2 {
                continue;
            }
            let publication = KeyPackagePublication {
                account_id: MemberId::new(peerkey.to_bytes().to_vec()),
                key_package: kp.clone(),
                slot_id: slots[0][1].clone(),
                created_at: Timestamp(event.created_at.as_secs()),
                endpoints: self.routing.endpoints.clone(),
            };
            let mut np = Publisher::publication(&publication)
                .map_err(|_| Error::Mdk("key_package_metadata"))?;
            np.client_name =
                transport_nostr_adapter::key_package_client_name(&dto).map(str::to_owned);
            if np.matches_prepared_event(&dto).unwrap_or(false) {
                return Ok(kp);
            }
        }
        Err(Error::Mdk("key_package_unavailable"))
    }
    pub async fn open_conversation(
        &mut self,
        peer: &str,
        deadline: Instant,
    ) -> Result<Conversation> {
        self.pending_budget = 64;
        self.maintain_journal()?;
        match self.open_conversation_inner(peer, deadline).await {
            Ok(conversation) => Ok(conversation),
            Err(original) => {
                let cleanup = self.reconcile_orphans();
                match (original, cleanup) {
                    (original @ (Error::Storage(_) | Error::Io(_)), _) => Err(original),
                    (_, Err(cleanup)) => Err(cleanup),
                    (original, Ok(())) => Err(original),
                }
            }
        }
    }
    async fn open_conversation_inner(
        &mut self,
        peer: &str,
        deadline: Instant,
    ) -> Result<Conversation> {
        if !self.active {
            return Err(Error::Mdk("inactive"));
        }
        self.ensure_ingress()?;
        if !self.allowed.contains(peer) {
            return Err(Error::Invalid("untrusted_peer"));
        }
        if let Some(c) = self
            .journal
            .conversations()?
            .into_iter()
            .find(|c| c.peer == peer && !c.revoked)
        {
            return Ok(c);
        }
        if self
            .journal
            .conversations()?
            .iter()
            .filter(|c| !c.revoked)
            .count()
            >= 64
        {
            return Err(Error::Invalid("conversation_limit"));
        }
        let kp = self.discover(peer, deadline).await?;
        let current = now()?;
        self.check_local_admission(current)?;
        let session = foundation::fresh_id(current)?;
        let routing = NostrRoutingV1::new(
            random::<32>()?,
            self.routing.endpoints.iter().map(|v| v.0.clone()).collect(),
        )
        .map_err(|_| Error::Invalid("routing"))?;
        let request = CreateGroupRequest {
            name: format!("{PREFIX}{session}"),
            description: PURPOSE.into(),
            members: vec![kp],
            required_features: vec![],
            app_components: vec![AppComponentData {
                component_id: NOSTR_ROUTING_COMPONENT_ID,
                data: encode_nostr_routing_v1(&routing).map_err(|_| Error::Invalid("routing"))?,
            }],
            initial_admins: vec![],
        };
        let (group, effects) =
            tokio::time::timeout_at(deadline, self.runtime.create_group(request))
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Mdk("create_group"))?;
        let (_, actual_peer, route, digest) = self.check_group(&group, Some(&self.identity))?;
        if actual_peer != peer {
            return Err(Error::Mdk("conversation_peer"));
        }
        self.journal
            .accept_conversation(group.as_slice(), &session, peer)?;
        self.journal.bind_authority(group.as_slice(), &digest)?;
        self.routing.group(group, route)?;
        self.apply_effects(effects, deadline).await?;
        mdk(self.runtime.sync_transport_groups(None).await)?;
        self.journal
            .conversation(&session)?
            .map(|v| v.0)
            .ok_or(Error::Storage("conversation"))
    }
    pub async fn send(
        &mut self,
        session: &str,
        id: &str,
        data: &[u8],
        deadline: Instant,
    ) -> Result<SendStatus> {
        self.pending_budget = 64;
        self.maintain_journal()?;
        if !self.active {
            return Err(Error::Mdk("inactive"));
        }
        self.ensure_ingress()?;
        let (c, g) = self
            .journal
            .conversation(session)?
            .ok_or(Error::Invalid("session"))?;
        if c.revoked {
            return Err(Error::Mdk("conversation_revoked"));
        }
        let group = GroupId::new(g.clone());
        self.ensure_trusted(&group)?;
        let envelope = Envelope::message(
            session.into(),
            id.into(),
            self.identity.clone(),
            c.peer,
            data.to_vec(),
        )?;
        if let Some(status) = self.journal.verify_outgoing(&g, &envelope)? {
            return Ok(status);
        }
        if now()? < self.journal.replay_floor()? {
            return Err(Error::Invalid("clock_behind_floor"));
        }
        self.submit(group, envelope, deadline).await?;
        self.sent_status(session, id)
    }
    async fn submit(
        &mut self,
        group: GroupId,
        envelope: Envelope,
        deadline: Instant,
    ) -> Result<()> {
        let event = cgka_traits::app_event::MarmotAppEvent::new(
            self.identity.clone(),
            now()?,
            foundation::KIND,
            vec![vec!["marmish".into(), "v2-foundation".into()]],
            String::from_utf8(envelope.encode()?).map_err(|_| Error::Invalid("foundation_utf8"))?,
        );
        let encoded = event.encode().map_err(|_| Error::Invalid("app_event"))?;
        self.journal.prepare_send(
            &Prepared {
                group: group.as_slice().to_vec(),
                envelope,
                event_id: event.id.clone(),
                encoded: encoded.clone(),
            },
            now()?,
        )?;
        self.journal.mark(group.as_slice(), &event.id, "prepared")?;
        let effects = tokio::time::timeout_at(
            deadline,
            self.runtime.send(SendIntent::AppMessage {
                group_id: group,
                payload: encoded,
                expected_epoch: None,
            }),
        )
        .await
        .map_err(|_| Error::Timeout)?;
        let effects = mdk(effects)?;
        self.bind_queued(&effects, &event.id)?;
        self.apply_effects(effects, deadline).await
    }
    fn bind_queued(&self, effects: &AccountDeviceEffects, event: &str) -> Result<()> {
        if let [queued] = effects.queued.as_slice() {
            self.journal.bind_queued(
                queued.group_id.as_slice(),
                queued.intent_id.as_slice(),
                event,
            )?;
        }
        Ok(())
    }
    fn ensure_trusted(&mut self, g: &GroupId) -> Result<()> {
        let c = self
            .journal
            .conversation_by_group(g.as_slice())?
            .ok_or(Error::Mdk("unaccepted_group"))?;
        if c.revoked {
            return Err(Error::Mdk("conversation_revoked"));
        }
        let validation = self
            .check_group(g, None)
            .and_then(|(session, peer, _, digest)| {
                if session != c.session || peer != c.peer {
                    return Err(Error::Mdk("authority_changed"));
                }
                self.journal.bind_authority(g.as_slice(), &digest)
            });
        match validation {
            Ok(()) => Ok(()),
            Err(Error::Mdk(_)) => {
                self.journal.reject_group_at(
                    g.as_slice(),
                    Some(foundation::id_timestamp(&c.session)?),
                    now()?,
                )?;
                self.routing.remove(g);
                self.forget_and_confirm(g)?;
                Err(Error::Mdk("conversation_revoked"))
            }
            Err(error) => Err(error),
        }
    }
    async fn apply_effects(
        &mut self,
        effects: AccountDeviceEffects,
        _deadline: Instant,
    ) -> Result<()> {
        for event in &effects.events {
            if let GroupEvent::QueuedIntentRefused {
                group_id,
                intent_id,
                ..
            } = event
            {
                self.journal
                    .refuse_queued(group_id.as_slice(), intent_id.as_slice())?;
            }
            if let GroupEvent::AppMessageInvalidated {
                group_id,
                message_id,
                ..
            } = event
            {
                self.journal
                    .invalidate(group_id.as_slice(), message_id.as_slice())?;
            }
        }
        self.project_application_publications(&effects)?;
        // Always process the durable queue: it survives a crash between MDK
        // acceptance and the application's transaction. Never acknowledge a
        // message before its projection and deterministic receipt intent exist.
        let pending = mdk(self.storage.list_pending_application_events())?;
        let pending_joins: BTreeSet<Vec<u8>> = pending
            .iter()
            .filter_map(|event| {
                if let GroupEvent::GroupJoined { group_id, .. } = event {
                    Some(group_id.as_slice().to_vec())
                } else {
                    None
                }
            })
            .collect();
        let mut events: BTreeMap<_, _> = pending
            .into_iter()
            .filter_map(|event| Self::pending_key(&event).map(|key| (key, event)))
            .collect();
        let keys = select_pending(
            &mut self.pending_cohort,
            &mut self.pending_budget,
            events.keys().cloned(),
        );
        let mut selected = Vec::new();
        for key in keys {
            if let Some(event) = events.remove(&key) {
                selected.push(event);
            }
        }
        selected.sort_by_key(|event| !matches!(event, GroupEvent::GroupJoined { .. }));
        let command_proofs = self.command_publication_proofs(&[])?;
        for event in selected {
            let result = match event {
                event @ GroupEvent::GroupJoined { .. } if self.is_command_join(&event)? => {
                    self.project_command_join(event)
                }
                event @ GroupEvent::GroupJoined { .. } => self.project_join(event),
                GroupEvent::MessageReceived { ref group_id, .. }
                    if pending_joins.contains(group_id.as_slice())
                        && self
                            .journal
                            .conversation_by_group(group_id.as_slice())?
                            .is_none()
                        && self.journal.command_room(group_id.as_slice())?.is_none() =>
                {
                    Ok(())
                }
                ref event @ GroupEvent::MessageReceived { ref group_id, .. }
                    if self.journal.command_room(group_id.as_slice())?.is_some() =>
                {
                    self.project_command_message(event.clone())
                }
                event @ GroupEvent::MessageReceived { .. } => self.project_message(event),
                GroupEvent::AppMessageInvalidated {
                    group_id,
                    message_id,
                    ..
                } => (|| {
                    self.journal
                        .invalidate(group_id.as_slice(), message_id.as_slice())?;
                    self.journal.command_apply_native(&[
                        crate::journal::CommandNativeUpdate::Invalidated {
                            group: group_id.as_slice().to_vec(),
                            native_id: message_id.as_slice().to_vec(),
                            possible_publication: command_proofs
                                .get(&(
                                    group_id.as_slice().to_vec(),
                                    message_id.as_slice().to_vec(),
                                ))
                                .cloned(),
                        },
                    ])?;
                    mdk(self
                        .storage
                        .delete_pending_application_events(&[message_id]))
                })(),
                _ => Ok(()),
            };
            match result {
                Err(Error::Storage(
                    "journal_capacity" | "source_capacity" | "command_invalidation_capacity",
                ))
                | Err(Error::Transport("foundation_future")) => {
                    crate::diagnostics::event("mdk_projection", "deferred");
                }
                other => other?,
            }
        }
        // Frozen canonical metadata makes any trust-relevant modification a
        // terminal revocation, including membership changes between deliveries.
        self.rebuild_routes()?;
        if !effects.failures.is_empty() || !effects.welcome_failures.is_empty() {
            crate::diagnostics::event("mdk_publish", "degraded_retained");
        }
        Ok(())
    }
    fn pending_key(event: &GroupEvent) -> Option<(u8, Vec<u8>)> {
        match event {
            GroupEvent::GroupJoined { via_welcome, .. } => {
                Some((0, via_welcome.as_slice().to_vec()))
            }
            GroupEvent::MessageReceived { message_id, .. } => {
                Some((1, message_id.as_slice().to_vec()))
            }
            GroupEvent::AppMessageInvalidated { message_id, .. } => {
                Some((2, message_id.as_slice().to_vec()))
            }
            _ => None,
        }
    }
    fn check_local_admission(&self, current: u64) -> Result<()> {
        if current < self.journal.replay_floor()? {
            return Err(Error::Invalid("clock_behind_floor"));
        }
        if self
            .journal
            .admission_reject_through()?
            .is_some_and(|cutoff| current <= cutoff)
        {
            return Err(Error::Invalid("admission_backpressure"));
        }
        Ok(())
    }
    fn forget_and_confirm(&mut self, group: &GroupId) -> Result<()> {
        mdk(self.runtime.forget_group_local(group))?;
        let pending = mdk(self.storage.list_pending_application_events())?;
        let remains = pending.iter().any(|e| match e {
            GroupEvent::GroupJoined { group_id, .. }
            | GroupEvent::MessageReceived { group_id, .. }
            | GroupEvent::AppMessageInvalidated { group_id, .. } => group_id == group,
            _ => false,
        });
        if remains {
            return Err(Error::Storage("group_cleanup_pending"));
        }
        self.journal.mark_group_cleanup_complete(group.as_slice())
    }
    fn project_join(&mut self, event: GroupEvent) -> Result<()> {
        let GroupEvent::GroupJoined {
            group_id,
            via_welcome,
            welcomer,
            ..
        } = event
        else {
            return Err(Error::Mdk("join_event"));
        };
        let existing = self.journal.conversation_by_group(group_id.as_slice())?;
        let creator = welcomer.map(|v| hex::encode(v.as_slice()));
        // Validate authenticated policy before allocating rejection metadata.
        let validated = creator
            .as_deref()
            .ok_or(Error::Mdk("welcome_author"))
            .and_then(|author| self.check_group(&group_id, Some(author)));
        let (session, peer, route, digest) = match validated {
            Ok(v) => v,
            Err(Error::Mdk(_) | Error::Invalid(_)) => {
                if let Some(c) = existing {
                    self.journal.reject_group_at(
                        group_id.as_slice(),
                        Some(foundation::id_timestamp(&c.session)?),
                        now()?,
                    )?;
                }
                self.routing.remove(&group_id);
                self.forget_and_confirm(&group_id)?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if existing.is_none() {
            let current = now()?;
            match foundation::classify_new_id(&session, current, self.journal.replay_floor()?)? {
                foundation::Freshness::Deferred => return Ok(()),
                foundation::Freshness::Expired | foundation::Freshness::ExcessiveFuture => {
                    self.forget_and_confirm(&group_id)?;
                    return Ok(());
                }
                foundation::Freshness::Eligible => {}
            }
            if current < self.journal.replay_floor()? {
                return Ok(());
            }
            if self
                .journal
                .admission_reject_through()?
                .is_some_and(|floor| foundation::id_timestamp(&session).is_ok_and(|ts| ts <= floor))
            {
                self.forget_and_confirm(&group_id)?;
                return Ok(());
            }
        }
        match self
            .journal
            .accept_conversation(group_id.as_slice(), &session, &peer)
        {
            Ok(()) => {}
            Err(Error::Mdk("conversation_capacity")) => {
                self.journal.reject_group_at(
                    group_id.as_slice(),
                    Some(foundation::id_timestamp(&session)?),
                    now()?,
                )?;
                self.forget_and_confirm(&group_id)?;
                return Ok(());
            }
            Err(Error::Mdk(_) | Error::Invalid(_)) => {
                // A valid peer may still propose an already-bound session ID.
                // This new group never acquired authority: refuse it without
                // consuming rejection metadata or poisoning other sessions.
                self.routing.remove(&group_id);
                self.forget_and_confirm(&group_id)?;
                return Ok(());
            }
            Err(e) => return Err(e),
        }
        self.journal.bind_authority(group_id.as_slice(), &digest)?;
        self.routing.group(group_id, route)?;
        mdk(self
            .storage
            .delete_pending_application_events(&[via_welcome]))?;
        Ok(())
    }
    fn project_message(&mut self, event: GroupEvent) -> Result<()> {
        let GroupEvent::MessageReceived {
            group_id,
            message_id,
            sender,
            sender_leaf_index,
            epoch,
            payload,
            authority,
            retention,
            ..
        } = event
        else {
            return Err(Error::Mdk("message_event"));
        };

        if self.journal.conversation_by_group(group_id.as_slice())?.is_none()
            && mdk(self.storage.list_pending_application_events())?.iter().any(|e|matches!(e,GroupEvent::GroupJoined{group_id:pending,..} if pending==&group_id)) {
            return Ok(());
        }
        if let Err(error) = self.ensure_trusted(&group_id) {
            match error {
                Error::Storage(_) | Error::Io(_) | Error::Transport(_) => return Err(error),
                _ => self
                    .journal
                    .reject(group_id.as_slice(), message_id.as_slice())?,
            }
            mdk(self
                .storage
                .delete_pending_application_events(&[message_id]))?;
            return Ok(());
        }
        let c = self
            .journal
            .conversation_by_group(group_id.as_slice())?
            .ok_or(Error::Mdk("conversation"))?;
        // Reject malformed/purpose-mismatched data durably without
        // converting it into a foundation message or receipt.
        let decode = (|| -> Result<Envelope> {
            if payload.len() > foundation::MAX_CONTENT + 4096 {
                return Err(Error::Invalid("app_event_size"));
            }
            let app = cgka_traits::app_event::MarmotAppEvent::decode(&payload)
                .map_err(|_| Error::Invalid("app_event"))?;
            let author = hex::encode(sender.as_slice());
            if sender_leaf_index.is_none()
                || author != c.peer
                || app.pubkey != author
                || app.kind != foundation::KIND
                || app.tags != vec![vec!["marmish".to_owned(), "v2-foundation".to_owned()]]
            {
                return Err(Error::Invalid("app_event_binding"));
            }
            let envelope = Envelope::decode(app.content.as_bytes())?;
            if envelope.sender() != author
                || envelope.recipient() != self.identity
                || envelope.session() != c.session
            {
                return Err(Error::Invalid("foundation_binding"));
            }
            if let Body::Receipt {
                message_id,
                payload_sha256,
            } = envelope.body()
                && envelope.id()
                    != foundation::receipt_id_for(
                        group_id.as_slice(),
                        envelope.session(),
                        envelope.recipient(),
                        envelope.sender(),
                        message_id,
                        payload_sha256,
                    )?
            {
                return Err(Error::Invalid("receipt_identity"));
            }
            Ok(envelope)
        })();
        match decode {
            Ok(envelope) => {
                let metadata =
                    serde_json::to_vec(&(sender_leaf_index, epoch, authority, retention))
                        .map_err(|_| Error::Storage("message_authority"))?;
                let receipt = self.prepare_receipt(&group_id, &c, &envelope)?;
                match self.journal.project_with_receipt(
                    group_id.as_slice(),
                    message_id.as_slice(),
                    &envelope,
                    now()?,
                    &metadata,
                    receipt.as_ref(),
                ) {
                    Ok(_) => {}
                    Err(Error::Invalid(_)) => {
                        self.journal
                            .reject(group_id.as_slice(), message_id.as_slice())?;
                        mdk(self
                            .storage
                            .delete_pending_application_events(&[message_id]))?;
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(_) => self
                .journal
                .reject(group_id.as_slice(), message_id.as_slice())?,
        }
        mdk(self
            .storage
            .delete_pending_application_events(&[message_id]))?;
        Ok(())
    }
    fn prepare_receipt(
        &self,
        group: &GroupId,
        c: &Conversation,
        envelope: &Envelope,
    ) -> Result<Option<Prepared>> {
        let Body::Message(bytes) = envelope.body() else {
            return Ok(None);
        };
        let receipt = Envelope::receipt(
            c.session.clone(),
            foundation::receipt_id(group.as_slice(), envelope)?,
            self.identity.clone(),
            c.peer.clone(),
            envelope.id().into(),
            foundation::payload_digest(bytes),
        )?;
        let event = cgka_traits::app_event::MarmotAppEvent::new(
            self.identity.clone(),
            foundation::id_timestamp(envelope.id())?,
            foundation::KIND,
            vec![vec!["marmish".into(), "v2-foundation".into()]],
            String::from_utf8(receipt.encode()?).map_err(|_| Error::Invalid("receipt"))?,
        );
        Ok(Some(Prepared {
            group: group.as_slice().to_vec(),
            envelope: receipt,
            event_id: event.id.clone(),
            encoded: event.encode().map_err(|_| Error::Invalid("receipt"))?,
        }))
    }
    async fn submit_unsubmitted(&mut self, deadline: Instant) -> Result<()> {
        for p in self.journal.unsubmitted()?.into_iter().take(64) {
            if foundation::id_timestamp(p.envelope.id())? < self.journal.replay_floor()? {
                self.journal.mark(&p.group, &p.event_id, "expired")?;
                continue;
            }
            let group = GroupId::new(p.group.clone());
            self.ensure_trusted(&group)?;
            self.ensure_ingress()?;
            // Commit the ambiguity boundary BEFORE crossing into MDK. A crash
            // after this point must never regenerate ciphertext without proof.
            self.journal.mark(&p.group, &p.event_id, "prepared")?;
            let effects = tokio::time::timeout_at(
                deadline,
                self.runtime.send(SendIntent::AppMessage {
                    group_id: group,
                    payload: p.encoded,
                    expected_epoch: None,
                }),
            )
            .await
            .map_err(|_| Error::Timeout)?;
            let effects = mdk(effects)?;
            self.bind_queued(&effects, &p.event_id)?;
            self.apply_effects(effects, deadline).await?;
        }
        Ok(())
    }
    async fn replay_pending(&mut self, _deadline: Instant) -> Result<()> {
        // A crash before or during send may leave a prepared journal record
        // without enough MDK evidence to classify exposure. Do not manufacture
        // fresh ciphertext or a second application event in that case.
        for p in self.journal.pending()? {
            let group = GroupId::new(p.group.clone());
            let fanouts = mdk(self.runtime.outbound_fanouts_for_group(&group))?;
            if !fanouts.iter().any(|f| {
                f.application_message()
                    .is_some_and(|a| a.app_event_id == p.event_id)
            }) {
                self.journal.mark(&p.group, &p.event_id, "uncertain")?;
            }
        }
        Ok(())
    }
    async fn drive_retained(&mut self, deadline: Instant) -> Result<()> {
        // These native obligations survive cross-store crashes. Re-delivery
        // reuses the stored Welcome and its original target, never a new group.
        let owed = mdk(self.runtime.outstanding_welcome_deliveries())?;
        if owed.len() > 4096 {
            return Err(Error::Storage("welcome_capacity"));
        }
        let live: BTreeSet<_> = owed.iter().map(|(_, m)| m.id.as_slice().to_vec()).collect();
        self.welcome_retry.retain(|id, _| live.contains(id));
        for (group, message) in owed.into_iter().take(64) {
            match self.ensure_application_group(&group) {
                Ok(()) => {}
                Err(Error::Mdk(_)) => {
                    // A cross-store crash can retain a native Welcome without
                    // an application acceptance. Never publish that orphan.
                    self.reconcile_orphans()?;
                    self.routing.remove(&group);
                    if self.journal.command_room(group.as_slice())?.is_some() {
                        self.command_forget_and_confirm(&group)?;
                    } else {
                        self.forget_and_confirm(&group)?;
                    }
                    continue;
                }
                Err(error) => return Err(error),
            }
            let id = message.id.as_slice().to_vec();
            if self
                .welcome_retry
                .get(&id)
                .is_some_and(|(due, _)| *due > Instant::now())
            {
                continue;
            }
            let attempt = self.welcome_retry.get(&id).map_or(0, |(_, n)| *n);
            let delay = if attempt < 2 { 1u64 << attempt } else { 60 };
            self.welcome_retry.insert(
                id,
                (
                    Instant::now() + Duration::from_secs(delay),
                    (attempt + 1) % 3,
                ),
            );
            let effects =
                tokio::time::timeout_at(deadline, self.runtime.redeliver_welcome(&message.id))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(|_| Error::Transport("welcome_retry"))?;
            self.apply_effects(effects, deadline).await?;
        }
        for c in self
            .journal
            .conversations()?
            .into_iter()
            .filter(|c| !c.revoked)
            .take(64)
        {
            let group =
                GroupId::new(hex::decode(c.group_id).map_err(|_| Error::Storage("group_id"))?);
            self.advance_application_convergence(&group, deadline)
                .await?;
        }
        for room in self.journal.command_rooms()? {
            self.advance_application_convergence(&GroupId::new(room.group), deadline)
                .await?;
        }
        Ok(())
    }
    async fn network_connected(&self) -> bool {
        for endpoint in self.connector.policy().urls() {
            match self.client.relay(endpoint.as_str()).await {
                Ok(Some(relay)) if relay.status().is_connected() => {}
                _ => return false,
            }
        }
        true
    }
    async fn recover_transport(&mut self, deadline: Instant) -> Result<bool> {
        let loss = self.notification_loss.borrow().clone();
        let stopped = self
            .forwarder
            .as_ref()
            .is_none_or(|task| task.is_finished());
        let connected = self.network_connected().await;
        if !self.recovery && (stopped || loss != self.loss_baseline || !connected) {
            self.recovery = true;
            self.publication.pause();
            self.recovery_subscribed = false;
            crate::diagnostics::event("mdk_transport", "recovery_paused");
        }
        if !self.recovery {
            return Ok(true);
        }
        if loss != self.loss_baseline || stopped || !connected {
            self.recovery_subscribed = false;
        }
        if !self.recovery_subscribed && Instant::now() >= self.reconnect_after {
            self.reconnect_after = Instant::now() + Duration::from_secs(2);
            // A healthy forwarder continues to observe loss and network events
            // during a long SDK-owned dial. Do not restart it on every tick.
            if stopped {
                if let Some(task) = self.forwarder.take() {
                    let _ = tokio::time::timeout_at(deadline, task).await;
                }
                self.forwarder = Some(
                    self.relay
                        .spawn_notification_forwarder(self.adapter.clone()),
                );
            }
            let mut all_connected = true;
            for endpoint in self.connector.policy().urls() {
                if Instant::now() >= deadline {
                    return Err(Error::Timeout);
                }
                let relay = tokio::time::timeout_at(deadline, self.client.relay(endpoint.as_str()))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(|_| Error::Transport("relay_reconnect"))?
                    .ok_or(Error::Transport("relay_reconnect"))?;
                all_connected &= recover_relay(&relay)?;
            }
            if !all_connected || !self.network_connected().await {
                return Err(Error::Transport("relay_reconnect"));
            }
            self.loss_baseline = self.notification_loss.borrow().clone();
            tokio::time::timeout_at(deadline, self.runtime.activate_transport(None))
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("recovery_subscription"))?;
            self.recovery_subscribed = true;
        }
        Ok(false)
    }
    async fn receive_one(&mut self, deadline: Instant) -> Result<()> {
        let receive_deadline = deadline.min(Instant::now() + Duration::from_millis(20));
        match tokio::time::timeout_at(receive_deadline, self.adapter.receive()).await {
            Ok(Ok(Some(delivery))) => {
                crate::diagnostics::event(
                    "mdk_delivery",
                    match delivery.message.envelope {
                        TransportEnvelope::Welcome { .. } => "welcome",
                        TransportEnvelope::GroupMessage { .. } => "group_message",
                    },
                );
                let effects =
                    tokio::time::timeout_at(deadline, self.runtime.ingest_delivery(&delivery))
                        .await
                        .map_err(|_| Error::Timeout)?;
                let effects = effects.map_err(|_| Error::Transport("ingest_deferred"))?;
                crate::diagnostics::event(
                    "mdk_ingest",
                    match &effects.outcome {
                        IngestOutcome::Processed => "processed",
                        IngestOutcome::Buffered { .. } => "buffered",
                        IngestOutcome::Ignored { .. } => "ignored",
                        IngestOutcome::LocalState { .. } => "local_state",
                        IngestOutcome::TransportDeferred { .. } => "transport_deferred",
                        IngestOutcome::ResourceRefused { .. } => "resource_refused",
                        IngestOutcome::Stale { .. } => "stale",
                        IngestOutcome::Rejected { .. } => "rejected",
                    },
                );
                let durable = !effects.left_object_unpersisted;
                if effects.effects.events.is_empty() {
                    crate::diagnostics::event("mdk_delivery", "no_application_events");
                }
                self.apply_effects(effects.effects, deadline).await?;
                if durable {
                    self.adapter
                        .ack_replay_delivery(&self.account, &delivery.message.id)
                        .await;
                } else {
                    crate::diagnostics::event("mdk_delivery", "unpersisted_not_acknowledged");
                }
                mdk(self.runtime.sync_transport_groups(None).await)?;
            }
            Ok(Ok(None)) => return Err(Error::Transport("delivery_closed")),
            Ok(Err(_)) => return Err(Error::Transport("delivery")),
            Err(_) => {}
        }
        Ok(())
    }
    pub async fn tick(&mut self, deadline: Instant) -> Result<()> {
        self.pending_budget = 64;
        self.tick_inner(deadline)
            .await
            .map_err(Self::classify_backpressure)
    }
    fn classify_backpressure(error: Error) -> Error {
        match error {
            Error::Storage(
                "journal_capacity"
                | "source_capacity"
                | "rejected_capacity"
                | "rejected_group_capacity"
                | "pending_event_limit"
                | "welcome_capacity",
            ) => {
                // Capacity is expected bounded backpressure, not disk corruption
                // or proof of an authority change. Leave native work unacked.
                Error::Transport("journal_backpressure")
            }
            error => error,
        }
    }
    async fn tick_inner(&mut self, deadline: Instant) -> Result<()> {
        if !self.active {
            return Err(Error::Mdk("inactive"));
        }
        self.progress_commands()?;
        self.reconcile_command_sources()?;
        self.maintain_journal()?;
        self.rebuild_routes()?;
        if !self.recover_transport(deadline).await? {
            self.receive_one(deadline).await?;
            let unchanged = *self.notification_loss.borrow() == self.loss_baseline;
            if self.recovery_subscribed
                && unchanged
                && self.network_connected().await
                && self
                    .adapter
                    .account_subscription_eose(&self.account)
                    .await
                    .complete()
            {
                self.publication.resume(self.loss_baseline.clone());
                self.recovery = false;
                crate::diagnostics::event("mdk_transport", "recovery_caught_up");
            } else {
                return Err(Error::Transport("transport_recovery_pending"));
            }
        }
        self.ensure_ingress()?;
        tokio::time::timeout_at(deadline, self.runtime.sync_transport_groups(None))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("subscription_sync"))?;
        let effects = tokio::time::timeout_at(deadline, self.runtime.drain())
            .await
            .map_err(|_| Error::Timeout)?;
        self.apply_effects(mdk(effects)?, deadline).await?;
        self.receive_one(deadline).await?;
        self.drive_retained(deadline).await?;
        self.submit_unsubmitted(deadline).await?;
        self.submit_command_publications(deadline).await?;
        self.progress_commands()?;
        self.maintain_journal()?;
        Ok(())
    }
    pub async fn shutdown(&mut self, deadline: Instant) -> Result<()> {
        let mut result =
            match tokio::time::timeout_at(deadline, self.adapter.deactivate_account(&self.account))
                .await
            {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err(Error::Transport("deactivate")),
                Err(_) => Err(Error::Timeout),
            };
        if tokio::time::timeout_at(deadline, self.client.shutdown())
            .await
            .is_err()
            && result.is_ok()
        {
            result = Err(Error::Timeout);
        }
        if let Some(forwarder) = self.forwarder.take() {
            forwarder.abort();
            if tokio::time::timeout_at(deadline, forwarder).await.is_err() && result.is_ok() {
                result = Err(Error::Timeout);
            }
        }
        if tokio::time::timeout_at(deadline, self.relay.shutdown())
            .await
            .is_err()
            && result.is_ok()
        {
            result = Err(Error::Timeout);
        }
        self.active = false;
        self.connector.cancel();
        let shutdown = self.connector.shutdown(deadline).await;
        if result.is_ok() { shutdown } else { result }
    }
    pub fn set_retention(&mut self, duration: Duration) -> Result<()> {
        if duration.as_secs() == 0 || duration.as_secs() > 365 * 86400 {
            return Err(Error::Invalid("retention"));
        }
        self.retention = duration.as_secs();
        Ok(())
    }
    fn retention_secs(&self) -> u64 {
        self.retention
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::BackendChoice;
    use crate::transport::{ArtiDirs, RelayPolicy, RouteMode, TransportLimits};
    use std::os::unix::fs::PermissionsExt;
    fn connector(profile: &ProfileLease) -> RelayConnector {
        RelayConnector::new(
            RelayPolicy::parse(
                &["wss://127.0.0.1:9".into()],
                RouteMode::Off,
                TransportLimits::default(),
            )
            .unwrap(),
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            },
        )
        .unwrap()
    }
    #[tokio::test]
    async fn encrypted_production_open_and_signed_keypackage_survive_restart_without_network() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(temp.path(), "account").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let peer = Keys::generate().public_key().to_hex();
        let mut owner =
            MdkOwner::open(&profile, &material, connector(&profile), vec![peer.clone()]).unwrap();
        profile.mark_ready(owner.opened_binding()).unwrap();
        let kp = owner
            .runtime
            .prepare_fresh_key_package(owner.routing.endpoints.clone())
            .await
            .unwrap();
        let metadata = key_package_metadata(&kp).unwrap();
        assert_eq!(metadata.protocol_profile, ProtocolProfile::Current);
        assert_eq!(
            metadata.credential_identity_hex,
            material.public_key().to_hex()
        );
        assert!(
            metadata
                .app_components
                .contains(&NOSTR_ROUTING_COMPONENT_ID)
        );
        assert!(
            !metadata
                .app_components
                .contains(&ACCOUNT_DEVICES_COMPONENT_ID)
        );
        let pending = owner
            .storage
            .key_package_lifecycle()
            .unwrap()
            .unwrap()
            .pending_replacement
            .unwrap();
        let artifact = pending.signed_event.unwrap();
        let signed: NostrTransportEvent = serde_json::from_slice(&artifact.bytes).unwrap();
        assert_eq!(signed.pubkey, material.public_key().to_hex());
        assert_eq!(signed.id, hex::encode(artifact.id.as_slice()));
        assert_eq!(signed.kind, 30443);
        // Pinned deaddrop accepts the seven standard KP tags; the upstream
        // optional advisory client tag must not change that wire contract.
        let names: BTreeSet<_> = signed.tags.iter().map(|t| t[0].as_str()).collect();
        assert_eq!(signed.tags.len(), 7);
        assert_eq!(
            names,
            BTreeSet::from([
                "d",
                "mls_protocol_version",
                "i",
                "mls_ciphersuite",
                "mls_extensions",
                "mls_proposals",
                "app_components"
            ])
        );
        assert!(
            !std::fs::read(profile.mdk_database())
                .unwrap()
                .starts_with(b"SQLite format 3")
        );
        assert!(
            !std::fs::read(profile.journal_database())
                .unwrap()
                .starts_with(b"SQLite format 3")
        );
        assert!(matches!(
            owner
                .send(
                    &"01".repeat(16),
                    &"02".repeat(16),
                    b"notactive",
                    Instant::now() + Duration::from_secs(1)
                )
                .await,
            Err(Error::Mdk("inactive"))
        ));
        drop(owner);
        let mut owner =
            MdkOwner::open(&profile, &material, connector(&profile), vec![peer]).unwrap();
        let kp2 = owner
            .runtime
            .prepare_fresh_key_package(owner.routing.endpoints.clone())
            .await
            .unwrap();
        assert_eq!(kp.bytes, kp2.bytes);
        assert_eq!(
            owner
                .storage
                .key_package_lifecycle()
                .unwrap()
                .unwrap()
                .pending_replacement
                .unwrap()
                .signed_event
                .unwrap()
                .bytes,
            artifact.bytes
        );
        drop(owner);
        profile.cleanup_runtime().unwrap();
    }
    #[tokio::test]
    async fn ingress_forwarder_is_owned_supervised_and_aborted_without_network() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(temp.path(), "account").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let mut owner = MdkOwner::open(&profile, &material, connector(&profile), vec![]).unwrap();
        assert!(matches!(
            owner.ensure_ingress(),
            Err(Error::Transport("notification_forwarder_stopped"))
        ));
        owner.forwarder = Some(
            owner
                .relay
                .spawn_notification_forwarder(owner.adapter.clone()),
        );
        owner.ensure_ingress().unwrap();
        let forwarder = owner.forwarder.take().unwrap();
        forwarder.abort();
        let _ = forwarder.await;
        assert!(matches!(
            owner.ensure_ingress(),
            Err(Error::Transport("notification_forwarder_stopped"))
        ));
        owner.forwarder = Some(
            owner
                .relay
                .spawn_notification_forwarder(owner.adapter.clone()),
        );
        owner.relay.record_notification_gap(1);
        assert!(matches!(
            owner.ensure_ingress(),
            Err(Error::Transport("notification_loss"))
        ));
        // An expired recovery turn cannot connect, but must enter the
        // publication pause and retain the observed loss until real catchup.
        assert!(owner.recover_transport(Instant::now()).await.is_err());
        assert!(owner.recovery);
        assert!(matches!(
            owner.ensure_ingress(),
            Err(Error::Transport("transport_recovery_pending"))
        ));
        assert_ne!(*owner.notification_loss.borrow(), owner.loss_baseline);
        assert!(owner.publication.blocked());
        let request = TransportPublishRequest {
            account_id: owner.account.clone(),
            message: TransportMessage {
                id: MessageId::new(vec![7; 32]),
                payload: vec![1, 2, 3],
                timestamp: Timestamp(1),
                causal_deps: vec![],
                source: cgka_traits::transport::TransportSource("marmot.transport.nostr".into()),
                envelope: TransportEnvelope::Welcome {
                    recipient: owner.account.clone(),
                },
            },
            target: TransportPublishTarget::Inbox {
                recipient: owner.account.clone(),
                endpoints: vec![
                    TransportEndpoint("wss://127.0.0.1:9/".into()),
                    TransportEndpoint("wss://another.invalid/".into()),
                ],
            },
            required_acks: 1,
        };
        let error = owner.publication.publish(request).await.unwrap_err();
        let failures = error.publish_endpoint_failures();
        assert_eq!(failures.len(), 2);
        assert!(
            failures
                .iter()
                .all(|v| v.kind == TransportEndpointFailureKind::RetryableUnavailable)
        );
        // Re-subscribing/resetting a baseline alone never releases the pause.
        owner.loss_baseline = owner.notification_loss.borrow().clone();
        assert!(owner.publication.blocked());
        let abort = owner.forwarder.as_ref().unwrap().abort_handle();
        drop(owner);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !abort.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        profile.cleanup_runtime().unwrap();
    }
    #[tokio::test]
    async fn changed_membership_is_revoked_before_any_drain_can_publish() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(temp.path(), "account").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let peer = Keys::generate().public_key().to_hex();
        let mut owner =
            MdkOwner::open(&profile, &material, connector(&profile), vec![peer.clone()]).unwrap();
        let unavailable = GroupId::new(vec![99; 32]);
        let unavailable_session = "09".repeat(16);
        owner
            .journal
            .accept_conversation(unavailable.as_slice(), &unavailable_session, &peer)
            .unwrap();
        // Failure to hydrate/query the native group is not evidence that an
        // authenticated roster changed. Preserve trust for a later retry.
        assert!(matches!(
            owner.ensure_trusted(&unavailable),
            Err(Error::Transport("group_state_unavailable"))
        ));
        assert!(
            !owner
                .journal
                .conversation(&unavailable_session)
                .unwrap()
                .unwrap()
                .0
                .revoked
        );
        assert!(
            !owner
                .journal
                .group_rejected(unavailable.as_slice())
                .unwrap()
        );
        owner
            .journal
            .revoke(unavailable.as_slice(), &unavailable_session, &peer)
            .unwrap();
        let session = "01".repeat(16);
        let route = NostrRoutingV1::new([2; 32], vec!["wss://127.0.0.1:9/".into()]).unwrap();
        // Real production Current-profile engine group with only the local
        // member: unlike an accepted two-party room, its roster is invalid.
        // No invitee means no Welcome or network publication is attempted.
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
        owner
            .journal
            .accept_conversation(group.as_slice(), &session, &peer)
            .unwrap();
        owner.routing.group(group.clone(), route).unwrap();
        assert_eq!(owner.routing.group_subscriptions().len(), 1);
        owner.forwarder = Some(
            owner
                .relay
                .spawn_notification_forwarder(owner.adapter.clone()),
        );
        owner.active = true;
        // Tick must revoke the incompatible persisted room before reaching
        // runtime.drain, even when no runtime work budget remains.
        let _ = owner.tick(Instant::now()).await;
        assert!(
            owner
                .journal
                .conversation(&session)
                .unwrap()
                .unwrap()
                .0
                .revoked
        );
        assert!(owner.journal.group_rejected(group.as_slice()).unwrap());
        assert!(owner.routing.group_subscriptions().is_empty());
        assert!(owner.runtime.group_record(&group).is_err());
        drop(owner);
        profile.cleanup_runtime().unwrap();
    }
    #[test]
    fn bounded_capacity_is_recoverable_but_operational_storage_errors_are_preserved() {
        for category in [
            "journal_capacity",
            "source_capacity",
            "rejected_capacity",
            "rejected_group_capacity",
            "pending_event_limit",
            "welcome_capacity",
        ] {
            assert!(matches!(
                MdkOwner::classify_backpressure(Error::Storage(category)),
                Error::Transport("journal_backpressure")
            ));
        }
        assert!(matches!(
            MdkOwner::classify_backpressure(Error::Storage("journal")),
            Error::Storage("journal")
        ));
        assert!(matches!(
            MdkOwner::classify_backpressure(Error::Io(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied
            ))),
            Error::Io(_)
        ));
    }
    #[test]
    fn shared_routes_intersect_the_allowset_and_reject_unknown_routes() {
        let routing =
            ExplicitRouting::new(vec![TransportEndpoint("wss://allowed.example/".into())]);
        let clone = routing.clone();
        let group = GroupId::new(vec![1; 16]);
        let transport = [2; 32];
        assert!(
            routing
                .group(
                    group.clone(),
                    NostrRoutingV1::new(transport, vec!["wss://evil.example/".into()]).unwrap()
                )
                .is_err()
        );
        routing
            .group(
                group.clone(),
                NostrRoutingV1::new(
                    transport,
                    vec![
                        "wss://evil.example/".into(),
                        "wss://allowed.example/".into(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(clone.group_subscriptions().len(), 1);
        assert_eq!(
            clone.group_subscriptions()[0].endpoints,
            vec![TransportEndpoint("wss://allowed.example/".into())]
        );
        routing.remove(&group);
        assert!(clone.group_subscriptions().is_empty());
    }
}

#[cfg(test)]
#[path = "mdk_adapter_tests.rs"]
mod trust_tests;

#[cfg(test)]
mod scheduler_tests {
    use super::*;
    #[test]
    fn frozen_cohort_is_fair_with_removed_blocked_and_new_keys() {
        let original: Vec<_> = (0u16..150).map(|n| (1, n.to_be_bytes().to_vec())).collect();
        let mut cohort = VecDeque::new();
        let mut budget = 64;
        let first = select_pending(&mut cohort, &mut budget, original.clone().into_iter());
        assert_eq!(first, original[..64]);
        assert_eq!(budget, 0);
        // New arrivals (even with earlier sort keys) cannot restart the sweep.
        let changed = std::iter::once((0, vec![0])).chain(original.clone());
        budget = 64;
        let second = select_pending(&mut cohort, &mut budget, changed);
        assert_eq!(second, original[64..128]);
        budget = 64;
        let third = select_pending(&mut cohort, &mut budget, std::iter::empty());
        assert_eq!(third, original[128..]);
        assert_eq!(budget, 42);
        // Every original key was attempted in ceil(150/64) operations; removed
        // keys still consumed budget, without requiring a surviving event.
        assert!(cohort.is_empty());
        let next = select_pending(&mut cohort, &mut budget, std::iter::once((0, vec![0])));
        assert_eq!(next, vec![(0, vec![0])]);
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use nostr_sdk::{error::ErrorKind, relay::RelayStatus};

    struct Step {
        dial_time: Duration,
        result: std::result::Result<(), ErrorKind>,
        after_dial: RelayStatus,
        after_wait: RelayStatus,
        wait_time: Option<Duration>,
    }
    struct Script {
        status: RelayStatus,
        steps: VecDeque<Step>,
        current: Option<Step>,
        calls: usize,
        waits: Vec<Duration>,
    }
    impl Script {
        fn new(status: RelayStatus, steps: Vec<Step>) -> Self {
            Self {
                status,
                steps: steps.into(),
                current: None,
                calls: 0,
                waits: Vec::new(),
            }
        }
    }
    #[async_trait]
    impl StartupRelay for Script {
        fn status(&self) -> RelayStatus {
            self.status
        }
        async fn dial(&mut self, remaining: Duration) -> std::result::Result<(), ErrorKind> {
            let step = self.steps.pop_front().expect("unexpected additional dial");
            assert!(step.dial_time <= remaining);
            self.calls += 1;
            tokio::time::sleep(step.dial_time).await;
            self.status = step.after_dial;
            let result = step.result;
            self.current = Some(step);
            result
        }
        async fn wait(&mut self, remaining: Duration) {
            self.waits.push(remaining);
            let step = self.current.as_ref().unwrap();
            let duration = step.wait_time.unwrap_or(remaining);
            assert!(duration <= remaining);
            tokio::time::sleep(duration).await;
            self.status = step.after_wait;
        }
    }
    fn step(
        result: std::result::Result<(), ErrorKind>,
        after_dial: RelayStatus,
        after_wait: RelayStatus,
        wait_time: Option<Duration>,
    ) -> Step {
        Step {
            dial_time: Duration::ZERO,
            result,
            after_dial,
            after_wait,
            wait_time,
        }
    }
    #[tokio::test(start_paused = true)]
    async fn startup_pending_ok_and_queued_error_wait_before_admission() {
        for result in [Ok(()), Err(ErrorKind::State)] {
            let start = Instant::now();
            let mut relay = Script::new(
                RelayStatus::Initialized,
                vec![step(
                    result,
                    RelayStatus::Pending,
                    RelayStatus::Connected,
                    Some(Duration::from_secs(4)),
                )],
            );
            // Successful return is the publication/subscription admission gate.
            connect_startup_relay(
                &mut relay,
                start + Duration::from_secs(120),
                Duration::from_secs(15),
            )
            .await
            .unwrap();
            assert_eq!(relay.status, RelayStatus::Connected);
            assert_eq!(relay.calls, 1);
            assert_eq!(Instant::now() - start, Duration::from_secs(4));
            assert_eq!(relay.waits, vec![Duration::from_secs(15)]);
        }
    }
    #[tokio::test(start_paused = true)]
    async fn startup_noop_states_do_not_spin_or_admit() {
        for status in [
            RelayStatus::Pending,
            RelayStatus::Connecting,
            RelayStatus::Disconnected,
        ] {
            let start = Instant::now();
            let mut relay = Script::new(
                status,
                (0..3).map(|_| step(Ok(()), status, status, None)).collect(),
            );
            assert!(matches!(
                connect_startup_relay(
                    &mut relay,
                    start + Duration::from_secs(120),
                    Duration::from_secs(15)
                )
                .await,
                Err(Error::Transport("relay_connect"))
            ));
            assert_eq!(relay.calls, 3);
            assert_eq!(relay.waits.len(), 3);
            assert_eq!(Instant::now() - start, Duration::from_secs(45));
        }
    }
    #[tokio::test(start_paused = true)]
    async fn startup_early_pending_wait_stays_in_its_turn() {
        let start = Instant::now();
        let mut relay = Script::new(
            RelayStatus::Pending,
            (0..3)
                .map(|_| {
                    step(
                        Ok(()),
                        RelayStatus::Pending,
                        RelayStatus::Pending,
                        Some(Duration::ZERO),
                    )
                })
                .collect(),
        );
        assert!(matches!(
            connect_startup_relay(
                &mut relay,
                start + Duration::from_secs(120),
                Duration::from_secs(15)
            )
            .await,
            Err(Error::Transport("relay_connect"))
        ));
        assert_eq!(relay.calls, 3);
        assert_eq!(Instant::now() - start, Duration::from_secs(45));
    }
    #[tokio::test(start_paused = true)]
    async fn startup_original_deadline_is_never_extended() {
        let start = Instant::now();
        let mut relay = Script::new(
            RelayStatus::Pending,
            (0..2)
                .map(|_| step(Ok(()), RelayStatus::Pending, RelayStatus::Pending, None))
                .collect(),
        );
        assert!(matches!(
            connect_startup_relay(
                &mut relay,
                start + Duration::from_secs(20),
                Duration::from_secs(15)
            )
            .await,
            Err(Error::Timeout)
        ));
        assert_eq!(Instant::now() - start, Duration::from_secs(20));
        assert_eq!(
            relay.waits,
            vec![Duration::from_secs(15), Duration::from_secs(5)]
        );
    }
    #[tokio::test(start_paused = true)]
    async fn startup_connected_needs_no_dial_and_terminal_states_refuse() {
        for status in [
            RelayStatus::Connected,
            RelayStatus::Banned,
            RelayStatus::Shutdown,
        ] {
            let mut relay = Script::new(status, vec![]);
            let result = connect_startup_relay(
                &mut relay,
                Instant::now() + Duration::from_secs(120),
                Duration::from_secs(15),
            )
            .await;
            assert_eq!(result.is_ok(), status == RelayStatus::Connected);
            assert_eq!(relay.calls, 0);
            assert!(relay.waits.is_empty());
        }
    }
    #[tokio::test(start_paused = true)]
    async fn startup_early_termination_allows_next_dial() {
        let start = Instant::now();
        let mut relay = Script::new(
            RelayStatus::Initialized,
            vec![
                step(
                    Err(ErrorKind::Transport),
                    RelayStatus::Terminated,
                    RelayStatus::Terminated,
                    Some(Duration::ZERO),
                ),
                step(Ok(()), RelayStatus::Connected, RelayStatus::Connected, None),
            ],
        );
        connect_startup_relay(
            &mut relay,
            start + Duration::from_secs(120),
            Duration::from_secs(15),
        )
        .await
        .unwrap();
        assert_eq!(relay.calls, 2);
        assert_eq!(Instant::now(), start);
    }
    #[tokio::test(start_paused = true)]
    async fn startup_three_timed_dials_and_late_connected_are_bounded() {
        let start = Instant::now();
        let mut relay = Script::new(
            RelayStatus::Initialized,
            (0..3)
                .map(|_| Step {
                    dial_time: Duration::from_secs(15),
                    ..step(
                        Err(ErrorKind::Timeout),
                        RelayStatus::Terminated,
                        RelayStatus::Terminated,
                        None,
                    )
                })
                .collect(),
        );
        assert!(matches!(
            connect_startup_relay(
                &mut relay,
                start + Duration::from_secs(120),
                Duration::from_secs(15)
            )
            .await,
            Err(Error::Transport("relay_connect"))
        ));
        assert_eq!(Instant::now() - start, Duration::from_secs(45));
        assert!(relay.waits.is_empty());
        let mut late = Script::new(
            RelayStatus::Pending,
            vec![step(
                Ok(()),
                RelayStatus::Pending,
                RelayStatus::Connected,
                None,
            )],
        );
        assert!(matches!(
            connect_startup_relay(
                &mut late,
                Instant::now() + Duration::from_secs(5),
                Duration::from_secs(15)
            )
            .await,
            Err(Error::Timeout)
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn startup_tor_budget_preserves_bootstrap_and_shared_endpoint_deadline() {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(150);
        // Bootstrap has already used part of the one activation budget.
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut first = Script::new(
            RelayStatus::Initialized,
            vec![Step {
                dial_time: Duration::from_secs(20),
                ..step(Ok(()), RelayStatus::Connected, RelayStatus::Connected, None)
            }],
        );
        connect_startup_relay(&mut first, deadline, Duration::from_secs(135))
            .await
            .unwrap();
        assert_eq!(Instant::now() - start, Duration::from_secs(50));
        let mut second = Script::new(
            RelayStatus::Initialized,
            vec![step(
                Ok(()),
                RelayStatus::Pending,
                RelayStatus::Pending,
                None,
            )],
        );
        assert!(matches!(
            connect_startup_relay(&mut second, deadline, Duration::from_secs(135)).await,
            Err(Error::Timeout)
        ));
        assert_eq!(second.calls, 1);
        assert_eq!(second.waits, vec![Duration::from_secs(100)]);
        assert_eq!(Instant::now(), deadline);
    }
    #[test]
    fn startup_labels_are_finite() {
        let errors = [
            ErrorKind::Protocol,
            ErrorKind::IO,
            ErrorKind::Transport,
            ErrorKind::Database,
            ErrorKind::Gossip,
            ErrorKind::Policy,
            ErrorKind::Timeout,
            ErrorKind::NotFound,
            ErrorKind::Invalid,
            ErrorKind::State,
            ErrorKind::Rejected,
            ErrorKind::Unsupported,
            ErrorKind::LimitExceeded,
            ErrorKind::Other,
        ];
        assert_eq!(
            errors.map(startup_error_label),
            [
                "protocol",
                "io",
                "transport",
                "database",
                "gossip",
                "policy",
                "timeout",
                "not_found",
                "invalid",
                "state",
                "rejected",
                "unsupported",
                "limit_exceeded",
                "other"
            ]
        );
        let statuses = [
            RelayStatus::Initialized,
            RelayStatus::Pending,
            RelayStatus::Connecting,
            RelayStatus::Connected,
            RelayStatus::Disconnected,
            RelayStatus::Terminated,
            RelayStatus::Banned,
            RelayStatus::Sleeping,
            RelayStatus::Shutdown,
        ];
        assert_eq!(
            statuses.map(startup_status_label),
            [
                "initialized",
                "pending",
                "connecting",
                "connected",
                "disconnected",
                "terminated",
                "banned",
                "sleeping",
                "shutdown"
            ]
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use futures_util::SinkExt;
    use nostr_sdk::relay::{Relay, RelayOptions, RelayStatus};
    use nostr_sdk::transport::websocket::{WebSocketSink, WebSocketStream, WebSocketTransport};
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::Semaphore;

    #[derive(Debug, Clone)]
    struct DelayedTransport {
        delay: Duration,
        calls: Arc<AtomicUsize>,
        permit: Arc<Semaphore>,
    }
    impl WebSocketTransport for DelayedTransport {
        fn support_ping(&self) -> bool {
            false
        }
        fn connect<'a>(
            &'a self,
            _url: &'a nostr::types::Url,
            _proxy: Option<std::net::SocketAddr>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<
                            (WebSocketSink, WebSocketStream),
                            nostr_sdk::error::Error,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let _permit = self.permit.clone().acquire_owned().await.unwrap();
                tokio::time::sleep(self.delay).await;
                let sink = futures_util::sink::drain().sink_map_err(
                    |never: std::convert::Infallible| -> nostr_sdk::error::Error { match never {} },
                );
                Ok((
                    Box::pin(sink) as WebSocketSink,
                    Box::pin(futures_util::stream::pending()) as WebSocketStream,
                ))
            })
        }
    }
    fn transport(delay: Duration) -> DelayedTransport {
        DelayedTransport {
            delay,
            calls: Arc::new(AtomicUsize::new(0)),
            permit: Arc::new(Semaphore::new(1)),
        }
    }
    fn relay(transport: &DelayedTransport) -> Relay {
        Relay::builder(nostr::types::RelayUrl::parse("wss://relay.invalid/").unwrap())
            .websocket_transport(transport.clone())
            .opts(
                RelayOptions::new()
                    .reconnect(false)
                    .ping(false)
                    .connect_timeout(Duration::from_secs(135)),
            )
            .build()
    }
    async fn settle() {
        // Give the actual SDK's spawned task and status notifications turns;
        // yielding does not advance paused time or hide an elapsed deadline.
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }
    #[tokio::test(start_paused = true)]
    async fn recovery_sdk_dial_survives_multiple_actor_turns_without_reset() {
        let transport = transport(Duration::from_secs(50));
        let relay = relay(&transport);
        assert!(!recover_relay(&relay).unwrap());
        settle().await;
        for _ in 0..3 {
            tokio::time::advance(Duration::from_secs(15)).await;
            settle().await;
            assert!(!recover_relay(&relay).unwrap());
            assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        }
        tokio::time::advance(Duration::from_secs(5)).await;
        settle().await;
        assert!(recover_relay(&relay).unwrap());
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        relay.shutdown();
        settle().await;
    }
    #[tokio::test(start_paused = true)]
    async fn recovery_sdk_timeout_is_fixed_and_shutdown_releases_pending_dial() {
        let transport = transport(Duration::from_secs(200));
        let relay = relay(&transport);
        assert!(!recover_relay(&relay).unwrap());
        settle().await;
        for _ in 0..8 {
            tokio::time::advance(Duration::from_secs(15)).await;
            settle().await;
            assert!(!recover_relay(&relay).unwrap());
            assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        }
        tokio::time::advance(Duration::from_secs(15)).await;
        settle().await;
        assert_eq!(relay.status(), RelayStatus::Terminated);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert_eq!(transport.permit.available_permits(), 1);
        // A later paced turn can retry the terminated attempt explicitly.
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(!recover_relay(&relay).unwrap());
        settle().await;
        assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
        assert_eq!(transport.permit.available_permits(), 0);
        relay.shutdown();
        settle().await;
        assert_eq!(transport.permit.available_permits(), 1);
        assert!(matches!(
            recover_relay(&relay),
            Err(Error::Transport("shutdown"))
        ));
        tokio::time::advance(Duration::from_secs(200)).await;
        settle().await;
        assert_eq!(relay.status(), RelayStatus::Shutdown);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    }
    #[tokio::test(start_paused = true)]
    async fn pending_owner_recovery_keeps_publication_paused_and_forwarder_alive() {
        use crate::identity::BackendChoice;
        use crate::transport::{ArtiDirs, RelayPolicy, RouteMode, TransportLimits};
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut profile = ProfileLease::acquire(temp.path(), "pending").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let connector = RelayConnector::new(
            RelayPolicy::parse(
                &["wss://relay.invalid/".into()],
                RouteMode::Off,
                TransportLimits::default(),
            )
            .unwrap(),
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            },
        )
        .unwrap();
        let mut owner = MdkOwner::open(&profile, &material, connector, vec![]).unwrap();
        let transport = transport(Duration::from_secs(100));
        let client = Client::builder()
            .websocket_transport(transport.clone())
            .build();
        client
            .add_relay("wss://relay.invalid/")
            .reconnect(false)
            .connect_timeout(Duration::from_secs(135))
            .await
            .unwrap();
        // The original relay plane remains owned by owner.relay/adapter; keep
        // its healthy notification forwarder while testing fake SDK dialing.
        owner.client = client;
        owner.forwarder = Some(
            owner
                .relay
                .spawn_notification_forwarder(owner.adapter.clone()),
        );
        let forwarder_id = owner.forwarder.as_ref().unwrap().id();
        for turn in 0..4 {
            if turn == 2 {
                // A lost connection after a prior resubscription must enter
                // dialing again, rather than waiting forever for old EOSE.
                owner.recovery_subscribed = true;
            }
            assert!(matches!(
                owner
                    .recover_transport(Instant::now() + Duration::from_secs(15))
                    .await,
                Err(Error::Transport("relay_reconnect"))
            ));
            settle().await;
            assert!(owner.recovery && owner.publication.blocked());
            assert!(!owner.recovery_subscribed);
            assert_eq!(owner.forwarder.as_ref().unwrap().id(), forwarder_id);
            assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
            tokio::time::advance(Duration::from_secs(15)).await;
        }
        drop(owner);
        settle().await;
        assert_eq!(transport.permit.available_permits(), 1);
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        profile.cleanup_runtime().unwrap();
    }
}
