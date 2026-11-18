//! Independent, generation-fenced relay lanes for one volatile enrollment.
#[path = "terminal_live_enrollment_readiness.rs"]
mod readiness;

use super::terminal_live_enrollment::RELAY_AUTH_CHALLENGE_TIMEOUT;
use super::*;
#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
use crate::dispatcher_fatal::Fatal;
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
#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
const DISPATCHER_FATAL: Error = Error::Transport("dispatcher_fatal");
const DISCONNECTED: Error = Error::Transport("terminal_enrollment_disconnected");

enum ReadMode {
    Wait(Option<Instant>),
    Queued,
}

#[expect(
    clippy::large_enum_variant,
    reason = "keep deliveries inline to avoid allocating for every received Terminal event"
)]
pub(super) enum Read {
    Delivery(TransportDelivery),
    Empty,
    Tick,
}

async fn retry_tick(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

enum AuthoritySource {
    #[cfg(test)]
    Proof(Arc<dyn Fn(u64) -> bool + Send + Sync>),
    Command(Arc<AttachmentFence>),
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
async fn until_fatal<T>(
    fatal: &Fatal,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    let mut future = std::pin::pin!(future);
    tokio::select! {
        biased;
        _ = fatal.tripped() => Err(DISPATCHER_FATAL),
        result = std::future::poll_fn(|cx| {
            if fatal.is_tripped() {
                std::task::Poll::Ready(Err(DISPATCHER_FATAL))
            } else {
                future.as_mut().poll(cx)
            }
        }) => result,
    }
}

pub(crate) struct Authority {
    #[cfg(test)]
    generation: u64,
    source: AuthoritySource,
    retired: AtomicBool,
    deadline: Mutex<Deadline>,
    grant: OnceLock<Arc<dyn Fn() -> bool + Send + Sync>>,
    reapers: Option<Arc<Reapers>>,
    local_closed: Option<Arc<AtomicBool>>,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    physical_stop: OnceLock<Fatal>,
}

#[derive(Default)]
pub(crate) struct Reapers {
    active: std::sync::atomic::AtomicUsize,
    uncertain: std::sync::atomic::AtomicUsize,
    failed: AtomicBool,
}

struct Uncertain<'a>(&'a Reapers);

impl Drop for Uncertain<'_> {
    fn drop(&mut self) {
        self.0.uncertain.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) struct Reaping(Arc<Reapers>, bool);

impl Reaping {
    pub(super) fn completed(mut self, result: &Result<()>) {
        self.1 = result.is_ok();
    }
}

impl Drop for Reaping {
    fn drop(&mut self) {
        if !self.1 {
            self.0.failed.store(true, Ordering::Release);
        }
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Reapers {
    pub(crate) fn available(&self) -> bool {
        !self.failed.load(Ordering::Acquire) && self.uncertain.load(Ordering::Acquire) == 0
    }

    fn uncertain(&self) -> Uncertain<'_> {
        if self.uncertain.fetch_add(1, Ordering::AcqRel) == 0 {
            crate::diagnostics::event("terminal_owner", "relay_reap_pending");
        }
        Uncertain(self)
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn custody(&self) -> (usize, usize, bool) {
        (
            self.active.load(Ordering::Acquire),
            self.uncertain.load(Ordering::Acquire),
            self.failed.load(Ordering::Acquire),
        )
    }

    pub(crate) async fn finish(&self) -> Result<()> {
        while self.active.load(Ordering::Acquire) != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if self.failed.load(Ordering::Acquire) {
            Err(Error::Identity("owned_stop_incomplete"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Listening,
    Bootstrap,
    Enrollment,
    Attached(terminal_frames::Binding),
}

/// Absolute enrollment limits tied to the local, suspend-aware clock sample.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EnrollmentCeiling {
    monotonic: Instant,
    continuous: Duration,
    observed_monotonic: Instant,
    observed_continuous: Duration,
}

impl EnrollmentCeiling {
    pub(crate) fn start(budget: Duration) -> Result<Self> {
        let (monotonic, continuous) = Self::sample()?;
        Self::captured(monotonic, continuous, budget)
    }

    pub(super) fn captured(
        monotonic: Instant,
        continuous: Duration,
        budget: Duration,
    ) -> Result<Self> {
        Ok(Self {
            monotonic: monotonic.checked_add(budget).ok_or(AUTHORITY)?,
            continuous: continuous.checked_add(budget).ok_or(AUTHORITY)?,
            observed_monotonic: monotonic,
            observed_continuous: continuous,
        })
    }

    pub(crate) fn clamp(self, other: Self) -> Self {
        Self {
            monotonic: self.monotonic.min(other.monotonic),
            continuous: self.continuous.min(other.continuous),
            observed_monotonic: self.observed_monotonic.max(other.observed_monotonic),
            observed_continuous: self.observed_continuous.max(other.observed_continuous),
        }
    }

    pub(crate) fn check(self) -> Result<()> {
        self.deadline().map(|_| ())
    }

    pub(crate) fn deadline(self) -> Result<Instant> {
        let (monotonic, continuous) = Self::sample()?;
        self.deadline_at(monotonic, continuous)
    }

    pub(crate) fn continuous(self) -> Duration {
        self.continuous
    }

    fn sample() -> Result<(Instant, Duration)> {
        let monotonic = Instant::now();
        let continuous = crate::command_limits::continuous_time().map_err(|_| AUTHORITY)?;
        Ok((monotonic, continuous))
    }

    fn deadline_at(self, monotonic: Instant, continuous: Duration) -> Result<Instant> {
        if monotonic < self.observed_monotonic || continuous < self.observed_continuous {
            return Err(AUTHORITY);
        }
        let monotonic_remaining = self
            .monotonic
            .checked_duration_since(monotonic)
            .ok_or(Error::Timeout)?;
        let continuous_remaining = self
            .continuous
            .checked_sub(continuous)
            .ok_or(Error::Timeout)?;
        let remaining = monotonic_remaining.min(continuous_remaining);
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        monotonic.checked_add(remaining).ok_or(AUTHORITY)
    }
}

struct Deadline {
    at: Instant,
    continuous: Option<Duration>,
    phase: Phase,
    exact: Option<EnrollmentCeiling>,
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
            exact: None,
        })
    }

    fn paired(ceiling: EnrollmentCeiling, phase: Phase) -> Result<Self> {
        Ok(Self {
            at: ceiling.deadline()?,
            continuous: Some(ceiling.continuous()),
            phase,
            exact: Some(ceiling),
        })
    }

    fn effective(&self) -> Result<Instant> {
        match self.exact {
            Some(ceiling) => Ok(self.at.min(ceiling.deadline()?)),
            None => Ok(self.at),
        }
    }

    fn expired(&self) -> Result<bool> {
        if let Some(ceiling) = self.exact {
            return match ceiling.check() {
                Err(Error::Timeout) => Ok(true),
                result => result.map(|_| false),
            };
        }
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
    #[cfg(test)]
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
                exact: None,
            }),
            grant: OnceLock::new(),
            reapers: None,
            local_closed: None,
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            physical_stop: OnceLock::new(),
        })
    }

    pub(super) fn from_fence(
        fence: Arc<AttachmentFence>,
        deadline: Instant,
        reapers: Option<Arc<Reapers>>,
        local_closed: Option<Arc<AtomicBool>>,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            #[cfg(test)]
            generation: fence.generation().get(),
            source: AuthoritySource::Command(fence),
            retired: AtomicBool::new(false),
            deadline: Mutex::new(Deadline::new(deadline, Phase::Bootstrap, true)?),
            grant: OnceLock::new(),
            reapers,
            local_closed,
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            physical_stop: OnceLock::new(),
        }))
    }

    fn bind_physical_stop(&self, connector: &RelayConnector) -> Result<Option<Reaping>> {
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(fatal) = connector.dispatcher_fatal() {
            self.physical_stop
                .set(fatal.clone())
                .map_err(|_| AUTHORITY)?;
            self.check_physical_stop()?;
            return self.reap().map(Some).ok_or(ROUTE);
        }
        if connector.terminal_lane_only() || self.terminal_lane_only() {
            return Err(ROUTE);
        }
        Ok(None)
    }

    pub(super) fn terminal_lane_only(&self) -> bool {
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        {
            self.physical_stop.get().is_some()
        }
        #[cfg(not(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        )))]
        false
    }

    fn check_physical_stop(&self) -> Result<()> {
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if self.physical_stop.get().is_some_and(Fatal::is_tripped) {
            return Err(DISPATCHER_FATAL);
        }
        Ok(())
    }

    async fn through_physical_stop<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(fatal) = self.physical_stop.get() {
            return until_fatal(fatal, future).await;
        }
        future.await
    }

    async fn deactivate_physical(
        &self,
        future: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        self.through_physical_stop(async {
            tokio::time::timeout(Duration::from_secs(2), future)
                .await
                .map_err(|_| Error::Timeout)?
        })
        .await
    }

    pub(super) fn install_grant(&self, grant: Arc<dyn Fn() -> bool + Send + Sync>) -> Result<()> {
        if self
            .grant
            .get()
            .is_none_or(|active| !Arc::ptr_eq(active, &grant))
        {
            self.grant.set(grant).map_err(|_| AUTHORITY)?;
        }
        self.check()
    }

    pub(super) fn reap(&self) -> Option<Reaping> {
        let reapers = self.reapers.clone()?;
        reapers.active.fetch_add(1, Ordering::AcqRel);
        Some(Reaping(reapers, false))
    }

    fn fence_state(&self) -> FenceState {
        if self.grant.get().is_some_and(|grant| !grant()) {
            return FenceState::Retired;
        }
        match &self.source {
            #[cfg(test)]
            AuthoritySource::Proof(current) if current(self.generation) => FenceState::Live,
            #[cfg(test)]
            AuthoritySource::Proof(_) => FenceState::Retired,
            AuthoritySource::Command(fence) => fence.state(),
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        self.check_physical_stop()?;
        if self
            .local_closed
            .as_ref()
            .is_some_and(|closed| closed.load(Ordering::Acquire))
        {
            self.retire();
            return Err(Error::Invalid("terminal_start_closed"));
        }
        if self.retired.load(Ordering::Acquire) || self.fence_state() == FenceState::Retired {
            self.retire();
            return Err(AUTHORITY);
        }
        let expired = self.deadline.lock().map_err(|_| AUTHORITY)?.expired();
        match expired {
            Ok(false) => Ok(()),
            Ok(true) => {
                self.retire();
                Err(Error::Timeout)
            }
            Err(error) => {
                self.retire();
                Err(error)
            }
        }
    }

    pub(super) async fn wait_live(&self) -> Result<()> {
        self.through_physical_stop(async {
            loop {
                self.check()?;
                match self.fence_state() {
                    FenceState::Live => return Ok(()),
                    FenceState::Suspect => tokio::time::sleep(Duration::from_millis(10)).await,
                    FenceState::Retired => return Err(AUTHORITY),
                }
            }
        })
        .await
    }

    /// Once issued, the operation keeps its cleanup ownership. Recheck the
    /// fence before returning a result to application state.
    async fn finish_issued<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        let deadline = self.deadline()?;
        let result = self
            .through_physical_stop(async {
                tokio::time::timeout_at(deadline, future)
                    .await
                    .map_err(|_| Error::Timeout)?
            })
            .await;
        self.wait_live().await?;
        result
    }

    pub(super) fn begin_enrollment(&self, deadline: Instant) -> Result<()> {
        self.advance(Phase::Bootstrap, Phase::Enrollment, deadline)
    }

    pub(super) fn constrain_bootstrap(&self, mut ceiling: EnrollmentCeiling) -> Result<()> {
        self.check()?;
        let mut active = self.deadline.lock().map_err(|_| AUTHORITY)?;
        if active.phase != Phase::Bootstrap || active.expired()? {
            return Err(AUTHORITY);
        }
        ceiling.monotonic = ceiling.monotonic.min(active.at);
        if let Some(continuous) = active.continuous {
            ceiling.continuous = ceiling.continuous.min(continuous);
        }
        if let Some(previous) = active.exact {
            ceiling = ceiling.clamp(previous);
        }
        *active = Deadline::paired(ceiling, Phase::Bootstrap)?;
        Ok(())
    }

    pub(super) fn begin_enrollment_at(&self, ceiling: EnrollmentCeiling) -> Result<()> {
        self.advance_to(
            Phase::Bootstrap,
            Deadline::paired(ceiling, Phase::Enrollment)?,
        )
    }

    pub(super) fn begin_listener(&self, deadline: Instant) -> Result<()> {
        self.advance(Phase::Bootstrap, Phase::Listening, deadline)
    }

    pub(super) fn begin_flight(&self, deadline: Instant) -> Result<()> {
        self.advance(Phase::Listening, Phase::Bootstrap, deadline)
    }

    #[cfg(test)]
    fn begin_attachment(&self, binding: terminal_frames::Binding, deadline: Instant) -> Result<()> {
        self.advance(Phase::Enrollment, Phase::Attached(binding), deadline)
    }

    pub(super) fn begin_attachment_at(
        &self,
        binding: terminal_frames::Binding,
        ceiling: EnrollmentCeiling,
    ) -> Result<()> {
        self.advance_to(
            Phase::Enrollment,
            Deadline::paired(ceiling, Phase::Attached(binding))?,
        )
    }

    pub(super) fn permits_frames(&self, binding: terminal_frames::Binding) -> Result<()> {
        self.check()?;
        match &self.source {
            #[cfg(test)]
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
        let next = Deadline::new(at, to, matches!(self.source, AuthoritySource::Command(_)))?;
        self.advance_to(from, next)
    }

    fn advance_to(&self, from: Phase, next: Deadline) -> Result<()> {
        self.check()?;
        let mut active = self.deadline.lock().map_err(|_| AUTHORITY)?;
        let transition = (|| {
            if self.retired.load(Ordering::Acquire)
                || self.fence_state() != FenceState::Live
                || active.phase != from
                || active.expired()?
                || next.expired()?
            {
                return Err(AUTHORITY);
            }
            *active = next;
            Ok(())
        })();
        if transition.is_err() {
            self.retire();
        }
        transition
    }

    pub(super) fn deadline(&self) -> Result<Instant> {
        self.deadline.lock().map_err(|_| AUTHORITY)?.effective()
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
        let result = self
            .through_physical_stop(async {
                match &self.source {
                    AuthoritySource::Command(_) => tokio::time::timeout_at(deadline, future)
                        .await
                        .map_err(|_| Error::Timeout)
                        .and_then(|result| result),
                    #[cfg(test)]
                    AuthoritySource::Proof(_) => tokio::select! {
                        biased;
                        revoked = self.wait_for_retirement() => revoked,
                        result = tokio::time::timeout_at(deadline, future) => {
                            result.map_err(|_| Error::Timeout).and_then(|result| result)
                        }
                    },
                }
            })
            .await;
        if result.is_err() {
            self.retire();
        }
        let value = result?;
        self.check()?;
        Ok(value)
    }

    pub(crate) async fn wait_for_retirement<T>(&self) -> Result<T> {
        loop {
            self.check()?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[test]
fn terminal_runtime_core_uncertain_reaps_hold_only_the_affected_direction() {
    let server = Reapers::default();
    let client = Reapers::default();
    let first = server.uncertain();
    let second = server.uncertain();
    assert!(!server.available());
    assert!(client.available());
    drop(first);
    assert!(!server.available());
    drop(second);
    assert!(server.available());
}

#[test]
fn enrollment_ceiling_converts_the_earliest_absolute_bound() {
    let now = Instant::now();
    let tick = Duration::from_secs(100);
    let secs = Duration::from_secs;
    let confirmed = EnrollmentCeiling::captured(now, tick, secs(30)).unwrap();
    let earlier_start =
        EnrollmentCeiling::captured(now - secs(20), tick - secs(25), secs(45)).unwrap();
    let clamped = confirmed.clamp(earlier_start);
    assert_eq!(clamped.continuous(), tick + secs(20));
    assert_eq!(
        clamped.deadline_at(now + secs(1), tick + secs(11)).unwrap(),
        now + secs(10)
    );
    assert!(matches!(
        clamped.deadline_at(now + secs(1), tick + secs(20)),
        Err(Error::Timeout)
    ));
    assert!(matches!(
        clamped.deadline_at(now - secs(1), tick + secs(1)),
        Err(Error::Invalid("terminal_enrollment_authority"))
    ));
    assert!(matches!(
        clamped.deadline_at(now + secs(1), tick - secs(1)),
        Err(Error::Invalid("terminal_enrollment_authority"))
    ));
}

#[tokio::test(start_paused = true)]
async fn enrollment_listener_starts_a_full_flight_after_initial_socket_budget() {
    use std::num::NonZeroU64;

    let fence = AttachmentFence::new(NonZeroU64::new(1).unwrap(), [3; 32]);
    let started = Instant::now();
    let socket_deadline = started + Duration::from_secs(30);
    let listener_deadline = started + Duration::from_secs(60);
    let authority = Authority::from_fence(fence.clone(), socket_deadline, None, None).unwrap();
    authority.begin_listener(listener_deadline).unwrap();
    tokio::time::advance(Duration::from_secs(35)).await;
    let flight_deadline = Instant::now() + Duration::from_secs(30);
    authority.begin_flight(flight_deadline).unwrap();
    tokio::time::advance(Duration::from_secs(26)).await;
    assert!(authority.check().is_ok());
    assert_eq!(authority.deadline().unwrap(), flight_deadline);

    let late_socket_deadline = Instant::now() + Duration::from_secs(30);
    let expired_listener = Authority::from_fence(fence, late_socket_deadline, None, None).unwrap();
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
    let authority = Authority::from_fence(cancel.clone(), short, None, None).unwrap();
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

    async fn wait_for_bootstrap_auth(
        &self,
        mut ready: tokio::sync::oneshot::Receiver<ReadinessBuffer>,
        quiet: &Mutex<QuietObservation>,
        challenged: &AtomicBool,
    ) -> Result<ReadinessBuffer> {
        let buffered = self.authority.run(wait_for_auth(&mut ready)).await?;
        if let AuthWait::Ready(buffered) = buffered {
            return Ok(buffered);
        }

        let disposition = self.authority.run(self.check()).await;
        let result = disposition.and_then(|()| match ready.try_recv() {
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => Err(AUTH),
            Ok(_) => Err(Error::Timeout),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                classify_auth_timer(&self.authority, quiet, challenged)
            }
        });
        self.authority.retire();
        result
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
        #[cfg(test)]
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
    physical_custody: Option<Reaping>,
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
        let physical_custody = authority.bind_physical_stop(&connector)?;
        let account = MemberId::new(keys.public_key().to_bytes().to_vec());
        let relay = Arc::new(NostrSdkRelayClient::with_account_signer(
            client.clone(),
            account.clone(),
            keys,
        ));
        let notifications = relay.notifications();
        let lane = Self {
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
            physical_custody,
            acknowledged: VecDeque::with_capacity(REPLAY_WINDOW),
        };
        lane.adapter
            .authority
            .run(lane.connector.bootstrap(lane.adapter.authority.deadline()?))
            .await?;
        Ok(lane)
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
        self.pin_inbox_inner(InboxScope::Bootstrap).await
    }

    pub(super) async fn pin_inbox(&mut self) -> Result<()> {
        self.pin_inbox_inner(InboxScope::Room).await
    }

    async fn pin_inbox_inner(&mut self, scope: InboxScope) -> Result<()> {
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
        let quiet = (matches!(scope, InboxScope::Bootstrap) && self.signed_auth.is_some())
            .then(|| Arc::new(Mutex::new(QuietObservation::Unproven)));
        let verification = self.signed_auth.take().map(|signed| {
            PendingReadiness::new(signed, verified, challenged.clone(), quiet.clone())
        });
        let auth_required = verification.is_some();
        self.pump = Some(tokio::spawn(ingress.read(
            notifications,
            send,
            verification,
        )));
        self.inbox = Some(inbox);
        self.incoming = Some(receive);
        if auth_required {
            let (staged, dropped) = if let Some(quiet) = quiet {
                adapter
                    .wait_for_bootstrap_auth(ready, &quiet, &challenged)
                    .await?
            } else {
                let buffered = adapter
                    .authority
                    .run(async {
                        tokio::time::timeout(AUTH_TIMEOUT, ready)
                            .await
                            .map_err(|_| Error::Timeout)?
                            .map_err(|_| AUTH)
                    })
                    .await;
                match buffered {
                    Err(Error::Timeout) if !challenged.load(Ordering::Acquire) => {
                        return Err(UNAVAILABLE);
                    }
                    result => result?,
                }
            };
            self.staged = staged;
            self.staged_losses = dropped;
        }
        adapter.check().await
    }

    pub(super) fn is_verified(&self) -> bool {
        self.verified
    }

    pub(super) fn discard_stored_alias_inbox(&mut self) -> Result<()> {
        if !self.verified {
            return Err(AUTH);
        }
        let inbox = self.inbox_id()?.to_owned();
        // The authenticated EOSE separates staged history from the live channel.
        self.staged.retain(|incoming| {
            incoming.plane != TransportDeliveryPlane::AccountInbox
                || incoming.relay.subscription_id.as_deref() != Some(inbox.as_str())
        });
        Ok(())
    }

    #[cfg(test)]
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
        match authority
            .run(self.read_exact(plane, &expected_id, cap, ReadMode::Wait(None)))
            .await?
        {
            Read::Delivery(delivery) => Ok(delivery),
            Read::Empty | Read::Tick => Err(DISCONNECTED),
        }
    }

    pub(super) async fn receive_inbox_until(
        &mut self,
        cap: usize,
        retry: Option<Instant>,
    ) -> Result<Read> {
        let inbox = self.inbox_id()?.to_owned();
        let authority = self.adapter.authority.clone();
        authority
            .run(self.read_exact(
                TransportDeliveryPlane::AccountInbox,
                &inbox,
                cap,
                ReadMode::Wait(retry),
            ))
            .await
    }

    pub(super) async fn receive_queued_group(
        &mut self,
        subscription_id: &str,
        cap: usize,
    ) -> Result<Option<TransportDelivery>> {
        let authority = self.adapter.authority.clone();
        match authority
            .run(self.read_exact(
                TransportDeliveryPlane::Group,
                subscription_id,
                cap,
                ReadMode::Queued,
            ))
            .await?
        {
            Read::Delivery(delivery) => Ok(Some(delivery)),
            Read::Empty => Ok(None),
            Read::Tick => Err(ROUTE),
        }
    }

    async fn read_exact(
        &mut self,
        plane: TransportDeliveryPlane,
        subscription_id: &str,
        cap: usize,
        mode: ReadMode,
    ) -> Result<Read> {
        let mut remaining = self.staged.len() + self.incoming.as_ref().ok_or(ROUTE)?.len();
        loop {
            self.adapter.check().await?;
            if matches!(mode, ReadMode::Queued) {
                let Some(next) = remaining.checked_sub(1) else {
                    return Ok(Read::Empty);
                };
                remaining = next;
            }
            let incoming = match self.staged.pop_front() {
                Some(incoming) => incoming,
                None => {
                    let incoming = self.incoming.as_mut().ok_or(ROUTE)?;
                    match mode {
                        ReadMode::Wait(retry) => tokio::select! {
                            biased;
                            retired = self.adapter.authority.wait_for_retirement::<Incoming>() => retired?,
                            value = incoming.recv() => value.ok_or(DISCONNECTED)?,
                            () = retry_tick(retry) => return Ok(Read::Tick),
                        },
                        ReadMode::Queued => match incoming.try_recv() {
                            Ok(incoming) => incoming,
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                                return Ok(Read::Empty);
                            }
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                                return Err(DISCONNECTED);
                            }
                        },
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
            return Ok(Read::Delivery(delivery));
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
        self.adapter
            .authority
            .deactivate_physical(async {
                self.adapter
                    .native
                    .deactivate_account(&self.adapter.account)
                    .await
                    .map_err(|_| ROUTE)
            })
            .await
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn hold_fixture_pump(&mut self, pump: tokio::task::JoinHandle<()>) {
        assert!(self.connector.terminal_lane_only());
        assert!(self.pump.is_none());
        self.pump = Some(pump);
    }

    pub(super) async fn close(self) -> Result<()> {
        self.close_inner(true).await
    }

    pub(super) async fn detach_alias(self) -> Result<()> {
        self.close_inner(false).await
    }

    async fn close_inner(mut self, retire: bool) -> Result<()> {
        let _ = self.deactivate(retire).await;
        let outcome = stop_lane(
            &mut self.pump,
            &self.client,
            &self.connector,
            self.adapter.authority.reapers.as_deref(),
        )
        .await;
        if outcome.is_ok() {
            self.closed.store(true, Ordering::Release);
            if let Some(custody) = self.physical_custody.take() {
                custody.completed(&outcome);
            }
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
            let mut pump = self.pump.take();
            let authority = self.adapter.authority.clone();
            let cleanup = self.physical_custody.take().or_else(|| authority.reap());
            runtime.spawn(async move {
                let result =
                    stop_lane(&mut pump, &client, &connector, authority.reapers.as_deref()).await;
                if let Some(cleanup) = cleanup {
                    cleanup.completed(&result);
                }
            });
        } else if self.connector.terminal_lane_only() {
            self.connector.cancel_terminal_connects();
        } else {
            self.connector.cancel();
        }
    }
}

async fn stop_lane(
    pump: &mut Option<tokio::task::JoinHandle<()>>,
    client: &Client,
    connector: &RelayConnector,
    reapers: Option<&Reapers>,
) -> Result<()> {
    if connector.terminal_lane_only() {
        connector.cancel_terminal_connects();
    }
    if let Some(pump) = pump.as_mut() {
        // Keep the handle under Lane ownership until termination. A canceled
        // close can then transfer it to the same counted cleanup as the slot.
        let _ = pump.await;
    }
    pump.take();
    let stopped = tokio::time::timeout(Duration::from_secs(2), client.shutdown())
        .await
        .map_err(|_| Error::Timeout);
    let reaped = connector
        .reap_terminal_socket(Instant::now() + Duration::from_secs(2))
        .await;
    let Some(reapers) = reapers else {
        return stopped.and(reaped);
    };
    // Exact slot acquisition after connector cancellation proves both SDK
    // stream halves are gone even if the SDK disconnect request timed out.
    if reaped.is_ok() {
        return Ok(());
    }
    let _uncertain = reapers.uncertain();
    let turn = Duration::from_millis(250);
    loop {
        let next = Instant::now() + turn;
        if connector.reap_terminal_socket(next).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep_until(next).await;
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[tokio::test]
async fn terminal_fatal_deactivation_cancels_before_second_poll() {
    use futures_util::task::{ArcWake, waker};
    use std::cell::Cell;
    use std::task::{Context, Poll};

    #[derive(Default)]
    struct WakeCount(AtomicUsize);
    impl ArcWake for WakeCount {
        fn wake_by_ref(this: &Arc<Self>) {
            this.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct DropFlag<'a>(&'a Cell<bool>);
    impl Drop for DropFlag<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let (trip, fatal) = crate::dispatcher_fatal::Trip::new();
    let authority = Authority::from_fence(
        AttachmentFence::new(std::num::NonZeroU64::MIN, [1; 32]),
        Instant::now() + Duration::from_secs(5),
        None,
        None,
    )
    .unwrap();
    assert!(authority.physical_stop.set(fatal).is_ok());
    let polls = Cell::new(0);
    let dropped = Cell::new(false);
    let mut deactivation = std::pin::pin!(authority.deactivate_physical(async {
        let _drop_flag = DropFlag(&dropped);
        std::future::poll_fn(|_| {
            polls.set(polls.get() + 1);
            Poll::Pending
        })
        .await
    }));
    let wakes = Arc::new(WakeCount::default());
    let waker = waker(wakes.clone());
    let mut context = Context::from_waker(&waker);
    assert!(deactivation.as_mut().poll(&mut context).is_pending());
    assert_eq!(polls.get(), 1);
    assert!(!dropped.get());
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    trip.trip();
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    assert!(matches!(
        deactivation.await,
        Err(Error::Transport("dispatcher_fatal"))
    ));
    assert_eq!(polls.get(), 1);
    assert!(dropped.get());
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

enum InboxScope {
    Bootstrap,
    Room,
}

type ReadinessBuffer = (VecDeque<Incoming>, usize);

enum AuthWait {
    Ready(ReadinessBuffer),
    Elapsed,
}

async fn wait_for_auth(
    ready: &mut tokio::sync::oneshot::Receiver<ReadinessBuffer>,
) -> Result<AuthWait> {
    tokio::select! {
        biased;
        result = ready => result.map(AuthWait::Ready).map_err(|_| AUTH),
        () = tokio::time::sleep(AUTH_TIMEOUT) => Ok(AuthWait::Elapsed),
    }
}

#[derive(PartialEq, Eq)]
enum QuietObservation {
    Unproven,
    Watching,
    Inhibited,
    Elapsed,
}

fn classify_auth_timer(
    authority: &Authority,
    quiet: &Mutex<QuietObservation>,
    challenged: &AtomicBool,
) -> Result<ReadinessBuffer> {
    let mut quiet = quiet.lock().map_err(|_| UNAVAILABLE)?;
    authority.check()?;
    if authority.fence_state() != FenceState::Live {
        return Err(UNAVAILABLE);
    }
    if challenged.load(Ordering::Acquire) {
        return Err(Error::Timeout);
    }
    if *quiet == QuietObservation::Watching {
        *quiet = QuietObservation::Elapsed;
        return Err(Error::Transport(RELAY_AUTH_CHALLENGE_TIMEOUT));
    }
    Err(UNAVAILABLE)
}

struct PendingReadiness {
    signed: Arc<SignedAuth>,
    state: Readiness,
    challenged: Arc<AtomicBool>,
    ready: Option<tokio::sync::oneshot::Sender<ReadinessBuffer>>,
    quiet: Option<Arc<Mutex<QuietObservation>>>,
    pending: VecDeque<Incoming>,
    pending_bytes: usize,
    stored_losses: usize,
    idle_losses: VecDeque<Instant>,
}

impl PendingReadiness {
    fn new(
        signed: Arc<SignedAuth>,
        ready: tokio::sync::oneshot::Sender<ReadinessBuffer>,
        challenged: Arc<AtomicBool>,
        quiet: Option<Arc<Mutex<QuietObservation>>>,
    ) -> Self {
        Self {
            signed,
            state: Readiness::default(),
            challenged,
            ready: Some(ready),
            quiet,
            pending: VecDeque::new(),
            pending_bytes: 0,
            stored_losses: 0,
            idle_losses: VecDeque::with_capacity(MAX_IDLE_LOSSES),
        }
    }

    fn watch_quiet(&self) {
        if let Some(observation) = &self.quiet
            && let Ok(mut observation) = observation.lock()
            && *observation == QuietObservation::Unproven
        {
            *observation = QuietObservation::Watching;
        }
    }

    fn inhibit_quiet(&self) {
        if let Some(observation) = &self.quiet
            && let Ok(mut observation) = observation.lock()
            && *observation != QuietObservation::Elapsed
        {
            *observation = QuietObservation::Inhibited;
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

fn inhibits_quiet(message: &RelayMessage<'_>, inbox: &str) -> bool {
    match message {
        RelayMessage::Auth { .. } | RelayMessage::Notice { .. } => true,
        RelayMessage::Closed {
            subscription_id, ..
        } => subscription_id.as_str() == inbox,
        _ => false,
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
    // Use the ordered raw stream. The SDK also publishes derived Events,
    // which have no independent authority to establish readiness.
    fn observes_source(
        &self,
        notification: &NostrSdkNotification,
        replay_epoch: Option<[u64; 2]>,
        verification: Option<&PendingReadiness>,
    ) -> bool {
        let mut inhibitor = false;
        let current = match notification {
            NostrSdkNotification::Replay {
                endpoint, epoch, ..
            } => {
                endpoint == &self.adapter.endpoint
                    && replay_epoch.is_none_or(|pinned| pinned == *epoch)
            }
            NostrSdkNotification::Disconnected { .. } | NostrSdkNotification::Shutdown => false,
            NostrSdkNotification::Payload {
                notification: ClientNotification::Message { relay_url, message },
                epoch,
                ..
            } => {
                inhibitor = inhibits_quiet(message, &self.inbox);
                replay_epoch == Some(*epoch) && relay_url.as_str() == self.adapter.endpoint.0
            }
            _ => true,
        };
        if let Some(auth) = verification
            && (!current || inhibitor)
        {
            auth.inhibit_quiet();
        }
        current
    }

    async fn read(
        self,
        mut notifications: Notifications,
        send: tokio::sync::mpsc::Sender<Incoming>,
        mut verification: Option<PendingReadiness>,
    ) {
        let mut replay_epoch = None;
        while let Some(NotificationUpdate::Notification(notification)) = notifications.next().await
        {
            if !self.observes_source(&notification, replay_epoch, verification.as_ref()) {
                break;
            }
            if self.adapter.check().await.is_err() {
                break;
            }
            match &notification {
                NostrSdkNotification::Replay { epoch, .. } => {
                    replay_epoch = Some(*epoch);
                    if let Some(auth) = verification.as_ref() {
                        auth.watch_quiet();
                    }
                }
                NostrSdkNotification::Payload {
                    notification: ClientNotification::Message { relay_url, message },
                    ..
                } => {
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
        if let Some(auth) = verification.as_ref() {
            auth.inhibit_quiet();
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

#[cfg(test)]
mod auth_diagnostics_tests {
    use super::*;
    use nostr::message::SubscriptionId;
    use std::borrow::Cow;

    fn fenced(until: Instant) -> (Arc<AttachmentFence>, Arc<Authority>) {
        let fence = AttachmentFence::new(std::num::NonZeroU64::new(1).unwrap(), [1; 32]);
        let authority = Authority::from_fence(fence.clone(), until, None, None).unwrap();
        (fence, authority)
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_runtime_core_readiness_timer_uses_inner_provenance() {
        for allowance in [
            AUTH_TIMEOUT - Duration::from_secs(1),
            AUTH_TIMEOUT,
            AUTH_TIMEOUT + Duration::from_secs(1),
        ] {
            let (_, authority) = fenced(Instant::now() + allowance);
            let (_hold, mut ready) = tokio::sync::oneshot::channel();
            let result = authority.run(wait_for_auth(&mut ready)).await;
            if allowance <= AUTH_TIMEOUT {
                assert!(matches!(result, Err(Error::Timeout)));
            } else {
                assert!(matches!(result, Ok(AuthWait::Elapsed)));
            }
        }

        for ready_at_cutoff in [false, true] {
            let deadline = Instant::now() + AUTH_TIMEOUT;
            let (send, mut receive) = tokio::sync::oneshot::channel();
            let wait = wait_for_auth(&mut receive);
            tokio::pin!(wait);
            assert!(futures_util::poll!(&mut wait).is_pending());
            tokio::time::advance(deadline - Instant::now()).await;
            if ready_at_cutoff {
                assert!(send.send((VecDeque::new(), 0)).is_ok());
                assert!(matches!(wait.await, Ok(AuthWait::Ready((_, 0)))));
            } else {
                drop(send);
                assert!(matches!(
                    wait.await,
                    Err(Error::Transport("terminal_enrollment_auth"))
                ));
            }
        }
    }

    #[test]
    fn terminal_runtime_core_quiet_disposition_needs_current_exact_scope() {
        let (fence, authority) = fenced(Instant::now() + AUTH_TIMEOUT);
        let challenged = AtomicBool::new(false);
        for state in [QuietObservation::Unproven, QuietObservation::Inhibited] {
            let observation = Mutex::new(state);
            let result = classify_auth_timer(&authority, &observation, &challenged);
            assert!(matches!(
                result,
                Err(Error::Transport("terminal_transport_unavailable"))
            ));
        }

        let observation = Mutex::new(QuietObservation::Watching);
        let updating = fence.begin_update().unwrap();
        let result = classify_auth_timer(&authority, &observation, &challenged);
        assert!(matches!(
            result,
            Err(Error::Transport("terminal_transport_unavailable"))
        ));
        assert_eq!(updating.resolve(Some([1; 32])), FenceState::Live);
        let result = classify_auth_timer(&authority, &observation, &challenged);
        assert!(matches!(
            result,
            Err(Error::Transport(RELAY_AUTH_CHALLENGE_TIMEOUT))
        ));
        let repeated = classify_auth_timer(&authority, &observation, &challenged);
        assert!(matches!(
            repeated,
            Err(Error::Transport("terminal_transport_unavailable"))
        ));

        challenged.store(true, Ordering::Release);
        for state in [QuietObservation::Watching, QuietObservation::Inhibited] {
            let observation = Mutex::new(state);
            assert!(matches!(
                classify_auth_timer(&authority, &observation, &challenged),
                Err(Error::Timeout)
            ));
        }
        let observation = Mutex::new(QuietObservation::Inhibited);
        authority.retire();
        assert!(matches!(
            classify_auth_timer(&authority, &observation, &challenged),
            Err(Error::Invalid("terminal_enrollment_authority"))
        ));
    }

    #[test]
    fn terminal_runtime_core_quiet_wait_counts_exact_auth_and_refusal_before_validation() {
        let exact = SubscriptionId::new("exact");
        let foreign = SubscriptionId::new("different");
        let (send, _receive) = tokio::sync::oneshot::channel();
        let observation = Arc::new(Mutex::new(QuietObservation::Unproven));
        let pending = PendingReadiness::new(
            Arc::default(),
            send,
            Arc::default(),
            Some(observation.clone()),
        );
        pending.watch_quiet();
        let closed = |subscription_id| RelayMessage::Closed {
            subscription_id: Cow::Borrowed(subscription_id),
            message: Cow::Borrowed("auth-required: fixture"),
        };
        assert!(!inhibits_quiet(
            &RelayMessage::EndOfStoredEvents(Cow::Borrowed(&exact)),
            "exact"
        ));
        assert!(!inhibits_quiet(
            &RelayMessage::EndOfStoredEvents(Cow::Borrowed(&foreign)),
            "exact"
        ));
        assert!(!inhibits_quiet(&closed(&foreign), "exact"));
        assert!(inhibits_quiet(&closed(&exact), "exact"));
        assert!(inhibits_quiet(
            &RelayMessage::Auth {
                challenge: Cow::Borrowed("")
            },
            "exact"
        ));
        pending.inhibit_quiet();
        pending.watch_quiet();
        assert!(*observation.lock().unwrap() == QuietObservation::Inhibited);
    }
}
