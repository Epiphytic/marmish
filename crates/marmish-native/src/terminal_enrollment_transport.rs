//! Independent, generation-fenced relay lanes for one volatile enrollment.
use super::*;
use cgka_traits::TransportDeliveryPlane;
use futures_util::StreamExt as _;
use nostr::event::Event;
use nostr::message::RelayMessage;
use nostr_sdk::NotificationUpdate;
use nostr_sdk::client::ClientNotification;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use transport_nostr_adapter::{
    NostrRelayEvent, NostrSdkNotification, NostrSubscription, RelayConnection, SubscriptionAttempt,
    dispatch_sdk_replay, sdk_subscription_refusal,
};

const FRAME_QUEUE: usize = 4;
const REPLAY_WINDOW: usize = 32;
const AUTHORITY: Error = Error::Invalid("terminal_enrollment_authority");
const ROUTE: Error = Error::Transport("terminal_enrollment_route");
const DISCONNECTED: Error = Error::Transport("terminal_enrollment_disconnected");

pub(super) struct Authority {
    generation: u64,
    current: Arc<dyn Fn(u64) -> bool + Send + Sync>,
    retired: AtomicBool,
    deadline: Mutex<(Instant, bool)>,
}

impl Authority {
    pub(super) fn new(
        generation: u64,
        current: Arc<dyn Fn(u64) -> bool + Send + Sync>,
        deadline: Instant,
    ) -> Arc<Self> {
        Arc::new(Self {
            generation,
            current,
            retired: AtomicBool::new(false),
            deadline: Mutex::new((deadline, false)),
        })
    }

    pub(super) fn check(&self) -> Result<()> {
        if self.retired.load(Ordering::Acquire) || !(self.current)(self.generation) {
            self.retire();
            return Err(AUTHORITY);
        }
        if Instant::now() >= self.deadline()? {
            self.retire();
            return Err(Error::Timeout);
        }
        Ok(())
    }

    pub(super) fn begin_enrollment(&self, deadline: Instant) -> Result<()> {
        self.check()?;
        let mut active = self.deadline.lock().map_err(|_| AUTHORITY)?;
        if active.1 || Instant::now() >= deadline {
            self.retire();
            return Err(AUTHORITY);
        }
        *active = (deadline, true);
        Ok(())
    }

    fn deadline(&self) -> Result<Instant> {
        self.deadline
            .lock()
            .map(|active| active.0)
            .map_err(|_| AUTHORITY)
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }

    pub(super) async fn run<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        self.check()?;
        let result = tokio::select! {
            biased;
            revoked = self.wait_for_retirement() => revoked,
            result = tokio::time::timeout_at(self.deadline()?, future) => {
                result.map_err(|_| Error::Timeout).and_then(|result| result)
            }
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
        if self.authority.check().is_err() || self.connection.get().is_some() {
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
        let report = self.native.publish(request).await?;
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
        let [configured] = connector.policy().urls() else {
            return Err(ROUTE);
        };
        if configured.as_str() != endpoint.0 || connector.policy().needs_tor() {
            return Err(ROUTE);
        }
        let client = connector.sdk_client(keys.clone())?;
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
            group: self.group.clone(),
        };
        let notifications = self.connect_notifications.take().ok_or(ROUTE)?;
        self.pump = Some(tokio::spawn(ingress.read(notifications, send)));
        self.inbox = Some(inbox);
        self.incoming = Some(receive);
        adapter.check().await
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
        adapter
            .authority
            .run(async {
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
            let incoming = self
                .incoming
                .as_mut()
                .ok_or(ROUTE)?
                .recv()
                .await
                .ok_or(DISCONNECTED)?;
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
                || delivery.message.payload.len() > cap
            {
                return Err(ROUTE);
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

    pub(super) async fn deactivate(&self) -> Result<()> {
        self.adapter.authority.retire();
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

    pub(super) async fn close(&self) -> Result<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let deactivated = self.deactivate().await;
        let result = shutdown(&self.client, &self.connector).await;
        deactivated.and(result)
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        self.adapter.authority.retire();
        if let Some(pump) = self.pump.as_ref() {
            pump.abort();
        }
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
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
    id: [u8; 32],
    plane: TransportDeliveryPlane,
    relay: NostrRelayEvent,
}

struct Ingress {
    adapter: GuardedAdapter,
    recipient: String,
    inbox: String,
    inbox_cap: usize,
    group: Arc<OnceLock<GroupRoute>>,
}

impl Ingress {
    async fn read(
        self,
        mut notifications: Notifications,
        send: tokio::sync::mpsc::Sender<Incoming>,
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
                    if let RelayMessage::Event {
                        subscription_id,
                        event,
                    } = message.as_ref()
                    {
                        if let Some(incoming) = self.admit(subscription_id.as_str(), event)
                            && send.try_send(incoming).is_err()
                        {
                            break;
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
                self.inbox_cap,
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
        if event.content.len() > cap
            || serde_json::to_vec(event).ok()?.len() > cap
            || !exact_selector(event, selector, value)
        {
            return None;
        }
        Some(Incoming {
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
