//! Independent, generation-fenced relay lanes for one volatile enrollment.
#[path = "terminal_live_enrollment_readiness.rs"]
mod readiness;

use super::*;
use crate::terminal_fence::{AttachmentFence, FenceState};
use crate::terminal_frames;
use cgka_traits::TransportDeliveryPlane;
use futures_util::StreamExt as _;
use nostr::event::Event;
use nostr::message::RelayMessage;
use nostr_sdk::NotificationUpdate;
use nostr_sdk::client::ClientNotification;
use readiness::{Readiness, RecordAuth, SignedAuth};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock, atomic::AtomicU8};
use transport_nostr_adapter::{
    NostrRelayEvent, NostrSdkNotification, NostrSubscription, RelayConnection, SubscriptionAttempt,
    dispatch_sdk_replay, sdk_subscription_refusal,
};

const FRAME_QUEUE: usize = 4;
const MAX_STORED_EVENTS: usize = 256;
const MAX_STORED_BYTES: usize = 1024 * 1024;
const MAX_IDLE_LOSSES: usize = 8;
const IDLE_LOSS_WINDOW: Duration = Duration::from_secs(10);
const AUTH: Error = Error::Transport("terminal_enrollment_auth");
const UNAVAILABLE: Error = Error::Transport("terminal_transport_unavailable");
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const ACTIVE_BOOTSTRAP: u8 = 1;
const ACTIVE_ENROLLMENT: u8 = 2;
const REPLAY_WINDOW: usize = 32;
const AUTHORITY: Error = Error::Invalid("terminal_enrollment_authority");
const ROUTE: Error = Error::Transport("terminal_enrollment_route");
const DISCONNECTED: Error = Error::Transport("terminal_enrollment_disconnected");

enum AuthoritySource {
    Proof(Arc<dyn Fn(u64) -> bool + Send + Sync>),
    Command(Arc<AttachmentFence>),
}

pub(super) struct Authority {
    generation: u64,
    source: AuthoritySource,
    retired: AtomicBool,
    deadline: Mutex<Deadline>,
    grant: OnceLock<Arc<dyn Fn() -> bool + Send + Sync>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Listening,
    Bootstrap,
    Enrollment,
    Attached(terminal_frames::Binding),
}

struct Deadline {
    at: Instant,
    continuous: Option<Duration>,
    phase: Phase,
}

impl Deadline {
    fn new(at: Instant, phase: Phase, suspend_aware: bool) -> Result<Self> {
        let continuous = if suspend_aware {
            Some(
                crate::command_limits::continuous_time()
                    .map_err(|_| AUTHORITY)?
                    .checked_add(at.saturating_duration_since(Instant::now()))
                    .ok_or(AUTHORITY)?,
            )
        } else {
            None
        };
        Ok(Self {
            at,
            continuous,
            phase,
        })
    }

    fn expired(&self) -> Result<bool> {
        if Instant::now() >= self.at {
            return Ok(true);
        }
        if let Some(bound) = self.continuous {
            return Ok(crate::command_limits::continuous_time().map_err(|_| AUTHORITY)? >= bound);
        }
        Ok(false)
    }
}

impl Authority {
    pub(super) fn new(
        generation: u64,
        current: Arc<dyn Fn(u64) -> bool + Send + Sync>,
        deadline: Instant,
    ) -> Arc<Self> {
        Arc::new(Self {
            generation,
            source: AuthoritySource::Proof(current),
            retired: AtomicBool::new(false),
            deadline: Mutex::new(Deadline {
                at: deadline,
                continuous: None,
                phase: Phase::Bootstrap,
            }),
            grant: OnceLock::new(),
        })
    }

    pub(super) fn from_fence(fence: Arc<AttachmentFence>, deadline: Instant) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            generation: fence.generation().get(),
            source: AuthoritySource::Command(fence),
            retired: AtomicBool::new(false),
            deadline: Mutex::new(Deadline::new(deadline, Phase::Bootstrap, true)?),
            grant: OnceLock::new(),
        }))
    }

    pub(super) fn install_grant(&self, grant: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<()> {
        self.grant.set(grant).map_err(|_| AUTHORITY)?;
        self.check()
    }

    fn fence_state(&self) -> FenceState {
        if self.grant.get().is_some_and(|grant| !grant()) {
            return FenceState::Retired;
        }
        match &self.source {
            AuthoritySource::Proof(current) if current(self.generation) => FenceState::Live,
            AuthoritySource::Proof(_) => FenceState::Retired,
            AuthoritySource::Command(fence) => fence.state(),
        }
    }

    pub(super) fn check(&self) -> Result<()> {
        if self.retired.load(Ordering::Acquire) || self.fence_state() == FenceState::Retired {
            self.retire();
            return Err(AUTHORITY);
        }
        if self.deadline.lock().map_err(|_| AUTHORITY)?.expired()? {
            self.retire();
            return Err(Error::Timeout);
        }
        Ok(())
    }

    pub(super) async fn wait_live(&self) -> Result<()> {
        loop {
            self.check()?;
            match self.fence_state() {
                FenceState::Live => return Ok(()),
                FenceState::Suspect => tokio::time::sleep(Duration::from_millis(10)).await,
                FenceState::Retired => return Err(AUTHORITY),
            }
        }
    }

    /// Once issued, the operation keeps its cleanup ownership. Recheck the
    /// fence before returning a result to application state.
    async fn finish_issued<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        let result = tokio::time::timeout_at(self.deadline()?, future).await;
        self.wait_live().await?;
        result.map_err(|_| Error::Timeout)?
    }

    pub(super) fn begin_enrollment(&self, deadline: Instant) -> Result<()> {
        self.advance(Phase::Bootstrap, Phase::Enrollment, deadline)
    }

    pub(super) fn begin_listener(&self, deadline: Instant) -> Result<()> {
        self.advance(Phase::Bootstrap, Phase::Listening, deadline)
    }

    pub(super) fn begin_flight(&self, deadline: Instant) -> Result<()> {
        self.advance(Phase::Listening, Phase::Bootstrap, deadline)
    }

    pub(super) fn begin_attachment(
        &self,
        binding: terminal_frames::Binding,
        deadline: Instant,
    ) -> Result<()> {
        self.advance(Phase::Enrollment, Phase::Attached(binding), deadline)
    }

    pub(super) fn permits_frames(&self, binding: terminal_frames::Binding) -> Result<()> {
        self.check()?;
        match &self.source {
            AuthoritySource::Proof(_) if self.generation == binding.generation => Ok(()),
            AuthoritySource::Command(_)
                if self.deadline.lock().map_err(|_| AUTHORITY)?.phase
                    == Phase::Attached(binding) =>
            {
                Ok(())
            }
            _ => Err(AUTHORITY),
        }
    }

    fn advance(&self, from: Phase, to: Phase, at: Instant) -> Result<()> {
        self.check()?;
        let mut active = self.deadline.lock().map_err(|_| AUTHORITY)?;
        if active.phase != from || Instant::now() >= at || self.fence_state() != FenceState::Live {
            self.retire();
            return Err(AUTHORITY);
        }
        *active = Deadline::new(at, to, matches!(self.source, AuthoritySource::Command(_)))?;
        Ok(())
    }

    pub(super) fn deadline(&self) -> Result<Instant> {
        self.deadline
            .lock()
            .map(|active| active.at)
            .map_err(|_| AUTHORITY)
    }

    pub(super) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }

    pub(super) async fn run<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        self.check()?;
        let deadline = self.deadline()?;
        let result = match &self.source {
            AuthoritySource::Command(_) => tokio::time::timeout_at(deadline, future)
                .await
                .map_err(|_| Error::Timeout)
                .and_then(|result| result),
            AuthoritySource::Proof(_) => tokio::select! {
                biased;
                revoked = self.wait_for_retirement() => revoked,
                result = tokio::time::timeout_at(deadline, future) => {
                    result.map_err(|_| Error::Timeout).and_then(|result| result)
                }
            },
        };
        if result.is_err() {
            self.retire();
        }
        let value = result?;
        self.check()?;
        Ok(value)
    }

    async fn wait_for_retirement<T>(&self) -> Result<T> {
        loop {
            self.check()?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn enrollment_listener_starts_a_full_flight_after_initial_socket_budget() {
    use std::num::NonZeroU64;

    let fence = AttachmentFence::new(NonZeroU64::new(1).unwrap(), [3; 32]);
    let started = Instant::now();
    let socket_deadline = started + Duration::from_secs(30);
    let listener_deadline = started + Duration::from_secs(60);
    let authority = Authority::from_fence(fence.clone(), socket_deadline).unwrap();
    authority.begin_listener(listener_deadline).unwrap();
    tokio::time::advance(Duration::from_secs(35)).await;
    let flight_deadline = Instant::now() + Duration::from_secs(30);
    authority.begin_flight(flight_deadline).unwrap();
    tokio::time::advance(Duration::from_secs(26)).await;
    assert!(authority.check().is_ok());
    assert_eq!(authority.deadline().unwrap(), flight_deadline);

    let late_socket_deadline = Instant::now() + Duration::from_secs(30);
    let expired_listener = Authority::from_fence(fence, late_socket_deadline).unwrap();
    expired_listener
        .begin_listener(late_socket_deadline)
        .unwrap();
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(
        expired_listener
            .begin_flight(Instant::now() + Duration::from_secs(30))
            .is_err()
    );
}

#[tokio::test]
async fn enrollment_pauses_for_suspect_fence_and_installs_one_writer_lease() {
    use std::num::NonZeroU64;

    let digest = [3; 32];
    let cancel = AttachmentFence::new(NonZeroU64::new(1).unwrap(), digest);
    let short = Instant::now() + Duration::from_millis(100);
    let authority = Authority::from_fence(cancel.clone(), short).unwrap();
    let writer = terminal_frames::Binding {
        transcript: [4; 32],
        attachment: [5; 16],
        generation: 9,
        role: terminal_frames::Role::Writer,
    };
    assert!(authority.permits_frames(writer).is_err());
    authority.begin_enrollment(short).unwrap();
    let update = cancel.begin_update().unwrap();
    let live = authority.wait_live();
    tokio::pin!(live);
    assert!(futures_util::poll!(&mut live).is_pending());
    assert_eq!(update.resolve(Some(digest)), FenceState::Live);
    live.await.unwrap();
    authority
        .begin_attachment(writer, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert!(authority.permits_frames(writer).is_ok());
    assert!(
        authority
            .permits_frames(terminal_frames::Binding {
                generation: 1,
                ..writer
            })
            .is_err()
    );
    tokio::time::sleep_until(short).await;
    assert!(authority.permits_frames(writer).is_ok());
    assert!(
        authority
            .begin_attachment(writer, Instant::now() + Duration::from_secs(2))
            .is_err()
    );
    assert!(authority.permits_frames(writer).is_err());
}

#[derive(Clone)]
pub(super) struct SingleRoomRouting {
    endpoint: TransportEndpoint,
    peer: MemberId,
    group: Arc<OnceLock<TransportGroupSubscription>>,
}

impl SingleRoomRouting {
    pub(super) fn new(endpoint: TransportEndpoint, peer: MemberId) -> Self {
        Self {
            endpoint,
            peer,
            group: Arc::default(),
        }
    }

    pub(super) fn register(&self, subscription: TransportGroupSubscription) -> Result<()> {
        if subscription.endpoints.as_slice() != std::slice::from_ref(&self.endpoint) {
            return Err(ROUTE);
        }
        self.group.set(subscription).map_err(|_| ROUTE)
    }
}

impl TransportRoutingPolicy for SingleRoomRouting {
    fn local_inbox_endpoints(&self) -> Vec<TransportEndpoint> {
        vec![self.endpoint.clone()]
    }

    fn key_package_endpoints(&self) -> Vec<TransportEndpoint> {
        Vec::new()
    }

    fn group_subscriptions(&self) -> Vec<TransportGroupSubscription> {
        self.group.get().cloned().into_iter().collect()
    }

    fn publish_target(
        &self,
        message: &TransportMessage,
    ) -> std::result::Result<TransportPublishTarget, TransportRoutingError> {
        match &message.envelope {
            TransportEnvelope::Welcome { recipient } if *recipient == self.peer => {
                Ok(TransportPublishTarget::Inbox {
                    recipient: recipient.clone(),
                    endpoints: vec![self.endpoint.clone()],
                })
            }
            TransportEnvelope::Welcome { .. } => Err(TransportRoutingError::MissingInboxRoute),
            TransportEnvelope::GroupMessage { transport_group_id } => {
                let group = self
                    .group
                    .get()
                    .filter(|group| group.transport_group_id == *transport_group_id)
                    .ok_or(TransportRoutingError::MissingGroupRoute)?;
                Ok(TransportPublishTarget::Group {
                    group_id: group.group_id.clone(),
                    transport_group_id: group.transport_group_id.clone(),
                    endpoints: group.endpoints.clone(),
                })
            }
        }
    }

    fn required_acks(&self, _: &TransportPublishTarget) -> usize {
        1
    }
}

#[derive(Clone)]
pub(super) struct GuardedAdapter {
    pub(super) native: NostrTransportAdapter,
    pub(super) relay: Arc<NostrSdkRelayClient>,
    pub(super) account: MemberId,
    pub(super) endpoint: TransportEndpoint,
    pub(super) authority: Arc<Authority>,
    connection: Arc<OnceLock<(RelayConnection, SubscriptionAttempt)>>,
}

impl GuardedAdapter {
    pub(super) async fn check(&self) -> Result<()> {
        self.authority.check()?;
        let Some((connection, attempt)) = self.connection.get() else {
            return Err(DISCONNECTED);
        };
        if self.relay.relay_connection(&self.account, &self.endpoint) != Some(*connection)
            || self
                .native
                .account_subscription_attempt(&self.account)
                .await
                != Some(*attempt)
        {
            self.authority.retire();
            return Err(DISCONNECTED);
        }
        self.authority.check()
    }
}

#[async_trait]
impl TransportAdapter for GuardedAdapter {
    async fn activate_account(
        &self,
        activation: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        if self.authority.wait_live().await.is_err() || self.connection.get().is_some() {
            return Err(TransportAdapterError::Closed);
        }
        self.native.activate_account(activation).await
    }

    async fn sync_account_groups(
        &self,
        sync: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.check()
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        self.authority
            .wait_live()
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        self.native.sync_account_groups(sync).await?;
        self.check()
            .await
            .map_err(|_| TransportAdapterError::Closed)
    }

    async fn deactivate_account(
        &self,
        account: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        self.native.deactivate_account(account).await
    }

    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        self.native.receive().await
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        self.check()
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        if request.account_id != self.account
            || request.target.endpoints() != std::slice::from_ref(&self.endpoint)
        {
            self.authority.retire();
            return Err(TransportAdapterError::Closed);
        }
        self.authority
            .wait_live()
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        if matches!(&self.authority.source, AuthoritySource::Proof(_)) {
            let report = self.native.publish(request).await?;
            self.check()
                .await
                .map_err(|_| TransportAdapterError::Closed)?;
            return Ok(report);
        }
        let report = self
            .authority
            .finish_issued(async { self.native.publish(request).await.map_err(|_| ROUTE) })
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        self.check()
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        Ok(report)
    }
}

type Notifications =
    Pin<Box<dyn futures_util::Stream<Item = NotificationUpdate<NostrSdkNotification>> + Send>>;

pub(super) struct Lane {
    client: Client,
    connector: RelayConnector,
    pub(super) adapter: GuardedAdapter,
    connect_notifications: Option<Notifications>,
    incoming: Option<tokio::sync::mpsc::Receiver<Incoming>>,
    signed_auth: Option<Arc<SignedAuth>>,
    verified: bool,
    enrollment_active: Arc<AtomicU8>,
    staged: VecDeque<Incoming>,
    staged_losses: usize,
    pump: Option<tokio::task::JoinHandle<()>>,
    group: Arc<OnceLock<GroupRoute>>,
    inbox: Option<String>,
    inbox_cap: usize,
    closed: AtomicBool,
    deactivated: AtomicBool,
    acknowledged: VecDeque<[u8; 32]>,
}

impl Lane {
    pub(super) async fn open(
        connector: RelayConnector,
        endpoint: TransportEndpoint,
        keys: Arc<Keys>,
        authority: Arc<Authority>,
        inbox_cap: usize,
    ) -> Result<Self> {
        Self::open_inner(connector, endpoint, keys, authority, inbox_cap, None).await
    }

    pub(super) async fn open_verified(
        connector: RelayConnector,
        endpoint: TransportEndpoint,
        keys: Arc<Keys>,
        authority: Arc<Authority>,
        inbox_cap: usize,
    ) -> Result<Self> {
        Self::open_inner(
            connector,
            endpoint,
            keys,
            authority,
            inbox_cap,
            Some(Arc::default()),
        )
        .await
    }

    async fn open_inner(
        connector: RelayConnector,
        endpoint: TransportEndpoint,
        keys: Arc<Keys>,
        authority: Arc<Authority>,
        inbox_cap: usize,
        signed_auth: Option<Arc<SignedAuth>>,
    ) -> Result<Self> {
        let [configured] = connector.policy().urls() else {
            return Err(ROUTE);
        };
        if configured.as_str() != endpoint.0 || connector.policy().needs_tor() {
            return Err(ROUTE);
        }
        let client = match signed_auth.as_ref() {
            Some(evidence) => connector.sdk_client_with_authenticator(RecordAuth::new(
                keys.as_ref().clone(),
                evidence.clone(),
            ))?,
            None => connector.sdk_client(keys.clone())?,
        };
        authority
            .run(connector.bootstrap(authority.deadline()?))
            .await?;
        let account = MemberId::new(keys.public_key().to_bytes().to_vec());
        let relay = Arc::new(NostrSdkRelayClient::with_account_signer(
            client.clone(),
            account.clone(),
            keys,
        ));
        let notifications = relay.notifications();
        Ok(Self {
            client,
            connector,
            adapter: GuardedAdapter {
                native: NostrTransportAdapter::new(relay.clone()),
                relay,
                account,
                endpoint,
                authority,
                connection: Arc::default(),
            },
            connect_notifications: Some(notifications),
            incoming: None,
            verified: signed_auth.is_some(),
            signed_auth,
            enrollment_active: Arc::default(),
            staged: VecDeque::new(),
            staged_losses: 0,
            pump: None,
            group: Arc::default(),
            inbox: None,
            inbox_cap,
            closed: AtomicBool::new(false),
            deactivated: AtomicBool::new(false),
            acknowledged: VecDeque::with_capacity(REPLAY_WINDOW),
        })
    }

    pub(super) async fn activate_inbox(&mut self) -> Result<()> {
        let adapter = &self.adapter;
        adapter
            .authority
            .run(async {
                adapter
                    .activate_account(TransportAccountActivation {
                        account_id: adapter.account.clone(),
                        inbox_endpoints: vec![adapter.endpoint.clone()],
                        group_subscriptions: vec![],
                        since: None,
                    })
                    .await
                    .map_err(|_| ROUTE)
            })
            .await?;
        self.pin_inbox().await
    }

    pub(super) async fn pin_inbox(&mut self) -> Result<()> {
        let adapter = &self.adapter;
        let (connection, attempt) = adapter
            .authority
            .run(async {
                let attempt = adapter
                    .native
                    .account_subscription_attempt(&adapter.account)
                    .await
                    .ok_or(ROUTE)?;
                loop {
                    if let Some(connection) = adapter
                        .relay
                        .relay_connection(&adapter.account, &adapter.endpoint)
                    {
                        return Ok((connection, attempt));
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await?;
        adapter
            .connection
            .set((connection, attempt))
            .map_err(|_| ROUTE)?;
        let inbox = NostrSubscription::AccountInbox {
            account_id: adapter.account.clone(),
            endpoints: vec![adapter.endpoint.clone()],
            since: None,
            attempt,
        }
        .subscription_id();
        let (send, receive) = tokio::sync::mpsc::channel(FRAME_QUEUE);
        let ingress = Ingress {
            adapter: adapter.clone(),
            recipient: hex::encode(adapter.account.as_slice()),
            inbox: inbox.clone(),
            inbox_cap: self.inbox_cap,
            idle_cap: if self.signed_auth.is_some() {
                32 * 1024
            } else {
                self.inbox_cap
            },
            enrollment_active: self.enrollment_active.clone(),
            group: self.group.clone(),
        };
        let notifications = self.connect_notifications.take().ok_or(ROUTE)?;
        let (verified, ready) = tokio::sync::oneshot::channel();
        let challenged = Arc::new(AtomicBool::new(false));
        let verification = self
            .signed_auth
            .take()
            .map(|signed| PendingReadiness::new(signed, verified, challenged.clone()));
        let auth_required = verification.is_some();
        self.pump = Some(tokio::spawn(ingress.read(
            notifications,
            send,
            verification,
        )));
        self.inbox = Some(inbox);
        self.incoming = Some(receive);
        if auth_required {
            let buffered = adapter
                .authority
                .run(async {
                    tokio::time::timeout(AUTH_TIMEOUT, ready)
                        .await
                        .map_err(|_| Error::Timeout)?
                        .map_err(|_| AUTH)
                })
                .await;
            let (staged, dropped) = match buffered {
                Err(Error::Timeout) if !challenged.load(Ordering::Acquire) => {
                    return Err(UNAVAILABLE);
                }
                result => result?,
            };
            self.staged = staged;
            self.staged_losses = dropped;
        }
        adapter.check().await
    }

    pub(super) fn is_verified(&self) -> bool {
        self.verified
    }

    pub(super) fn stored_backlog(&self) -> (usize, usize) {
        (self.staged.len(), self.staged_losses)
    }

    pub(super) fn begin_active_enrollment(&self) {
        self.enrollment_active
            .store(ACTIVE_ENROLLMENT, Ordering::Release);
    }

    pub(super) fn begin_active_bootstrap(&self) {
        self.enrollment_active
            .store(ACTIVE_BOOTSTRAP, Ordering::Release);
    }

    pub(super) fn inbox_id(&self) -> Result<&str> {
        self.inbox.as_deref().ok_or(ROUTE)
    }

    pub(super) fn pin_group(&self, id: &str, route: &[u8; 32], cap: usize) -> Result<()> {
        self.group
            .set(GroupRoute {
                id: id.to_owned(),
                selector: hex::encode(route),
                cap,
            })
            .map_err(|_| ROUTE)
    }

    pub(super) async fn publish_alias(&self, sealed: &[u8]) -> Result<()> {
        self.adapter.check().await?;
        let event: Event = serde_json::from_slice(sealed).map_err(|_| ROUTE)?;
        let event = NostrTransportEvent::from_nostr_event(&event).map_err(|_| ROUTE)?;
        let adapter = &self.adapter;
        adapter.authority.wait_live().await?;
        adapter
            .authority
            .finish_issued(async {
                let result = adapter
                    .relay
                    .publish_event_for_account(
                        &adapter.account,
                        std::slice::from_ref(&adapter.endpoint),
                        &event,
                        1,
                    )
                    .await
                    .map_err(|_| ROUTE)?;
                if result.accepted.len() == 1 {
                    Ok(())
                } else {
                    Err(ROUTE)
                }
            })
            .await
    }

    pub(super) async fn receive(
        &mut self,
        plane: TransportDeliveryPlane,
        group_subscription: Option<&str>,
        cap: usize,
    ) -> Result<TransportDelivery> {
        let expected_id = match plane {
            TransportDeliveryPlane::AccountInbox => self.inbox_id()?.to_owned(),
            TransportDeliveryPlane::Group => group_subscription.ok_or(ROUTE)?.to_owned(),
            _ => return Err(ROUTE),
        };
        let authority = self.adapter.authority.clone();
        authority
            .run(self.read_exact(plane, &expected_id, cap))
            .await
    }

    async fn read_exact(
        &mut self,
        plane: TransportDeliveryPlane,
        subscription_id: &str,
        cap: usize,
    ) -> Result<TransportDelivery> {
        loop {
            self.adapter.check().await?;
            let incoming = match self.staged.pop_front() {
                Some(incoming) => incoming,
                None => {
                    let incoming = self.incoming.as_mut().ok_or(ROUTE)?;
                    tokio::select! {
                        biased;
                        retired = self.adapter.authority.wait_for_retirement::<Incoming>() => retired?,
                        value = incoming.recv() => value.ok_or(DISCONNECTED)?,
                    }
                }
            };
            if incoming.plane != plane
                || incoming.relay.subscription_id.as_deref() != Some(subscription_id)
            {
                continue;
            }
            let relay_event = incoming.relay;
            let native = &self.adapter.native;
            let id = incoming.id;
            if self.acknowledged.contains(&id) {
                continue;
            }
            let count = match plane {
                TransportDeliveryPlane::AccountInbox => {
                    native
                        .handle_reconciled_event(&self.adapter.account, relay_event)
                        .await
                }
                TransportDeliveryPlane::Group => {
                    native
                        .handle_group_relay_copy(relay_event, Some(&self.adapter.account))
                        .await
                }
                _ => return Err(ROUTE),
            }
            .map_err(|_| ROUTE)?;
            if count == 0 {
                continue;
            }
            if count != 1 {
                return Err(ROUTE);
            }
            // No second application queue is needed. Drain the single accepted
            // adapter placement before taking another SDK notification.
            let delivery = native.receive().await.map_err(|_| ROUTE)?.ok_or(ROUTE)?;
            self.adapter.check().await?;
            if delivery.account_id != self.adapter.account
                || delivery.message.id.as_slice() != id
                || delivery.source.plane != plane
                || delivery.source.endpoint.as_ref() != Some(&self.adapter.endpoint)
                || delivery.source.subscription_id.as_deref() != Some(subscription_id)
            {
                return Err(ROUTE);
            }
            if delivery.message.payload.len() > cap {
                self.acknowledge(&delivery.message.id).await;
                continue;
            }
            return Ok(delivery);
        }
    }

    pub(super) async fn acknowledge(&mut self, id: &MessageId) {
        if let Ok(id) = <[u8; 32]>::try_from(id.as_slice())
            && !self.acknowledged.contains(&id)
        {
            if self.acknowledged.len() == REPLAY_WINDOW {
                self.acknowledged.pop_front();
            }
            self.acknowledged.push_back(id);
        }
        self.adapter
            .native
            .ack_replay_delivery(&self.adapter.account, id)
            .await;
    }

    async fn deactivate(&mut self, retire: bool) -> Result<()> {
        if retire {
            self.adapter.authority.retire();
        }
        if let Some(pump) = self.pump.as_ref() {
            pump.abort();
        }
        if self.deactivated.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        tokio::time::timeout(
            Duration::from_secs(2),
            self.adapter
                .native
                .deactivate_account(&self.adapter.account),
        )
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| ROUTE)
    }

    pub(super) async fn close(self) -> Result<()> {
        self.close_inner(true).await
    }

    pub(super) async fn detach_alias(self) -> Result<()> {
        self.close_inner(false).await
    }

    async fn close_inner(mut self, retire: bool) -> Result<()> {
        let deactivated = self.deactivate(retire).await;
        if let Some(pump) = self.pump.take() {
            match pump.await {
                Ok(()) => {}
                Err(error) if error.is_cancelled() => {}
                Err(_) => {
                    let _ = shutdown(&self.client, &self.connector).await;
                    return Err(ROUTE);
                }
            }
        }
        let result = shutdown(&self.client, &self.connector).await;
        let reaped = if self.verified {
            self.connector
                .reap_terminal_socket(Instant::now() + Duration::from_secs(2))
                .await
        } else {
            Ok(())
        };
        let outcome = deactivated.and(result).and(reaped);
        if outcome.is_ok() {
            self.closed.store(true, Ordering::Release);
        }
        outcome
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.adapter.authority.retire();
        if let Some(pump) = self.pump.as_ref() {
            pump.abort();
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let (client, connector) = (self.client.clone(), self.connector.clone());
            runtime.spawn(async move {
                let _ = shutdown(&client, &connector).await;
            });
        } else {
            self.connector.cancel();
        }
    }
}

// The SDK owns the socket after requesting disconnect, including any final
// control writes. Cancel the connector only if that request itself times out.
async fn shutdown(client: &Client, connector: &RelayConnector) -> Result<()> {
    match tokio::time::timeout(Duration::from_secs(2), client.shutdown()).await {
        Ok(()) => Ok(()),
        Err(_) => {
            connector.cancel();
            Err(Error::Timeout)
        }
    }
}

struct GroupRoute {
    id: String,
    selector: String,
    cap: usize,
}

struct Incoming {
    size: usize,
    id: [u8; 32],
    plane: TransportDeliveryPlane,
    relay: NostrRelayEvent,
}

struct PendingReadiness {
    signed: Arc<SignedAuth>,
    state: Readiness,
    challenged: Arc<AtomicBool>,
    ready: Option<tokio::sync::oneshot::Sender<(VecDeque<Incoming>, usize)>>,
    pending: VecDeque<Incoming>,
    pending_bytes: usize,
    stored_losses: usize,
    idle_losses: VecDeque<Instant>,
}

impl PendingReadiness {
    fn new(
        signed: Arc<SignedAuth>,
        ready: tokio::sync::oneshot::Sender<(VecDeque<Incoming>, usize)>,
        challenged: Arc<AtomicBool>,
    ) -> Self {
        Self {
            signed,
            state: Readiness::default(),
            challenged,
            ready: Some(ready),
            pending: VecDeque::new(),
            pending_bytes: 0,
            stored_losses: 0,
            idle_losses: VecDeque::with_capacity(MAX_IDLE_LOSSES),
        }
    }

    fn buffer(&mut self, incoming: Incoming) {
        if self.pending.len() >= MAX_STORED_EVENTS
            || self.pending_bytes + incoming.size > MAX_STORED_BYTES
        {
            self.stored_losses = self.stored_losses.saturating_add(1);
            return;
        }
        self.pending_bytes += incoming.size;
        self.pending.push_back(incoming);
    }

    fn keep_idle_after_loss(&mut self, now: Instant) -> bool {
        while self
            .idle_losses
            .front()
            .is_some_and(|seen| now.duration_since(*seen) >= IDLE_LOSS_WINDOW)
        {
            self.idle_losses.pop_front();
        }
        self.idle_losses.push_back(now);
        self.idle_losses.len() < MAX_IDLE_LOSSES
    }
}

struct Ingress {
    adapter: GuardedAdapter,
    recipient: String,
    inbox: String,
    inbox_cap: usize,
    idle_cap: usize,
    enrollment_active: Arc<AtomicU8>,
    group: Arc<OnceLock<GroupRoute>>,
}

impl Ingress {
    async fn read(
        self,
        mut notifications: Notifications,
        send: tokio::sync::mpsc::Sender<Incoming>,
        mut verification: Option<PendingReadiness>,
    ) {
        let mut replay_epoch = None;
        while let Some(NotificationUpdate::Notification(notification)) = notifications.next().await
        {
            if self.adapter.check().await.is_err() {
                break;
            }
            match &notification {
                NostrSdkNotification::Replay {
                    endpoint, epoch, ..
                } if endpoint == &self.adapter.endpoint
                    && replay_epoch.is_none_or(|pinned| pinned == *epoch) =>
                {
                    replay_epoch = Some(*epoch);
                }
                NostrSdkNotification::Replay { .. }
                | NostrSdkNotification::Disconnected { .. }
                | NostrSdkNotification::Shutdown => break,
                NostrSdkNotification::Payload {
                    notification: ClientNotification::Message { relay_url, message },
                    epoch,
                    ..
                } => {
                    if replay_epoch != Some(*epoch) || relay_url.as_str() != self.adapter.endpoint.0
                    {
                        break;
                    }
                    if let Some(auth) = verification.as_mut() {
                        if auth
                            .state
                            .observe(relay_url, &self.inbox, message, &auth.signed)
                            .is_err()
                        {
                            break;
                        }
                        if matches!(message.as_ref(), RelayMessage::Auth { .. }) {
                            auth.challenged.store(true, Ordering::Release);
                        }
                        if auth.state.is_ready()
                            && let Some(ready) = auth.ready.take()
                            && ready
                                .send((std::mem::take(&mut auth.pending), auth.stored_losses))
                                .is_err()
                        {
                            break;
                        }
                    }
                    if let RelayMessage::Event {
                        subscription_id,
                        event,
                    } = message.as_ref()
                    {
                        if let Some(incoming) = self.admit(subscription_id.as_str(), event) {
                            if let Some(auth) =
                                verification.as_mut().filter(|auth| auth.ready.is_some())
                            {
                                auth.buffer(incoming);
                            } else if send.try_send(incoming).is_err()
                                && (self.enrollment_active.load(Ordering::Acquire) != 0
                                    || !verification.as_mut().is_some_and(|auth| {
                                        auth.keep_idle_after_loss(Instant::now())
                                    }))
                            {
                                break;
                            }
                        }
                        continue;
                    }
                }
                // The SDK emits a derived Event and a raw relay Message for the same
                // frame. Only the raw message carries the exact route gate above.
                NostrSdkNotification::Payload {
                    notification: ClientNotification::Event { .. },
                    ..
                } => continue,
                _ => {}
            }
            dispatch_sdk_replay(
                notification,
                &self.adapter.native,
                Some(&self.adapter.account),
                &|reason| sdk_subscription_refusal(reason, Some(true)),
            )
            .await;
        }
        self.adapter.authority.retire();
    }

    fn admit(&self, id: &str, event: &Event) -> Option<Incoming> {
        let (plane, selector, value, cap) = if id == self.inbox && event.kind == Kind::GiftWrap {
            (
                TransportDeliveryPlane::AccountInbox,
                "p",
                &self.recipient,
                if self.enrollment_active.load(Ordering::Acquire) == ACTIVE_ENROLLMENT {
                    self.inbox_cap
                } else {
                    self.inbox_cap.min(self.idle_cap)
                },
            )
        } else {
            let group = self
                .group
                .get()
                .filter(|group| id == group.id && event.kind == Kind::MlsGroupMessage)?;
            (
                TransportDeliveryPlane::Group,
                "h",
                &group.selector,
                group.cap,
            )
        };
        if event.content.len() > cap || !exact_selector(event, selector, value) {
            return None;
        }
        let size = serde_json::to_vec(event).ok()?.len();
        if size > cap {
            return None;
        }
        Some(Incoming {
            size,
            id: event.id.to_bytes(),
            plane,
            relay: NostrRelayEvent {
                endpoint: self.adapter.endpoint.clone(),
                subscription_id: Some(id.to_owned()),
                event: NostrTransportEvent::from_nostr_event(event).ok()?,
            },
        })
    }
}

pub(super) fn exact_selector(event: &Event, name: &str, expected: &str) -> bool {
    let mut matching = event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|kind| kind == name));
    matching
        .next()
        .is_some_and(|tag| tag.as_slice() == [name, expected])
        && matching.next().is_none()
}
