//! Test-mounted planning decisions for one accepted directional Peer.
use crate::command_limits::continuous_time;
use crate::peer_heartbeat_sampler::{
    AddressSelection, BaseFields, Families, Progress, Request, Sampler, StartError,
};
use crate::peer_status_wire::state::{
    Applied, ClientBinding, Clock, HeartbeatView, PermissionView, Receiver, Refused,
};
use crate::peer_status_wire::{
    Capability, Description, Generation, Heartbeat, Hex32, HostStatus, Payload, PermissionPlan,
    Record, WireError,
};
use marmish_engine::{CommandPolicy, PrincipalId};
use sha2::{Digest, Sha256};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SAMPLE_WINDOW: Duration = Duration::from_secs(2);
const SPACING: Duration = Duration::from_secs(30);
const ISSUE_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_INTERVAL: u32 = 3600;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Binding {
    room: Hex32,
    server: Hex32,
    client: Hex32,
    generation: NonZeroU64,
}

impl Binding {
    pub(crate) fn new(
        room: Hex32,
        server: Hex32,
        client: Hex32,
        generation: NonZeroU64,
    ) -> Option<Self> {
        (server != client).then_some(Self {
            room,
            server,
            client,
            generation,
        })
    }

    fn receiver(self) -> ClientBinding {
        ClientBinding {
            room: self.room,
            local_client: self.client,
            pinned_server: self.server,
            local_generation: self.generation,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Direction {
    Server,
    Client,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Route {
    Unavailable,
    Confidential,
    AddressSafe,
}

/// Current proof supplied by the accepted-room authority owner.
#[derive(Clone, Copy)]
pub(crate) struct Accepted {
    pub(crate) binding: Binding,
    pub(crate) local: Hex32,
    pub(crate) side: Direction,
    pub(crate) route: Route,
}

impl Accepted {
    fn correct_side(self, binding: Binding, side: Direction) -> bool {
        self.binding == binding
            && self.side == side
            && self.local
                == match side {
                    Direction::Server => binding.server,
                    Direction::Client => binding.client,
                }
    }

    fn allows(self, binding: Binding, side: Direction) -> bool {
        self.correct_side(binding, side) && self.route != Route::Unavailable
    }
}

pub(crate) struct Fields {
    pub(crate) base: BaseFields,
    pub(crate) disks: Vec<(String, PathBuf)>,
    pub(crate) interfaces: Vec<(String, Families)>,
}

impl Fields {
    fn addresses(&self, route: Route) -> Option<AddressSelection> {
        (route == Route::AddressSafe)
            .then(|| AddressSelection::new(self.interfaces.clone()))
            .flatten()
    }

    fn request(&self, route: Route) -> Option<Request> {
        Request::new(
            BaseFields {
                username: self.base.username,
                uptime: self.base.uptime,
                load: self.base.load,
            },
            self.disks.clone(),
            self.addresses(route),
        )
    }

    fn narrow(&self, route: Route, host: &mut HostStatus) {
        if !self.base.username {
            host.username = None;
        }
        if !self.base.uptime {
            host.uptime = None;
        }
        if !self.base.load {
            host.load = None;
        }
        if let Some(disks) = &mut host.disks {
            disks.retain(|disk| self.disks.iter().any(|(label, _)| disk.label == *label));
            if disks.is_empty() {
                host.disks = None;
            }
        }
        if route != Route::AddressSafe || self.interfaces.is_empty() {
            host.ips = None;
        }
    }
}

/// An engine-validated policy and capabilities from the current owner turn.
pub(crate) struct Policy<'a> {
    pub(crate) command: &'a CommandPolicy,
    pub(crate) ordinal: NonZeroU64,
    pub(crate) interactive: Capability,
    pub(crate) shell_command: Capability,
}

pub(crate) struct Settings {
    binding: Binding,
    interval: u32,
    fields: Fields,
    permission: Option<PermissionPlan>,
}

impl Settings {
    pub(crate) fn enable(
        binding: Binding,
        interval: Option<u32>,
        fields: Fields,
        policy: Option<Policy<'_>>,
    ) -> Result<Rc<Self>, Denied> {
        let interval = interval.unwrap_or(DEFAULT_INTERVAL);
        if !(300..=86_400).contains(&interval)
            || (!fields.interfaces.is_empty() && fields.addresses(Route::AddressSafe).is_none())
            || fields.request(Route::Confidential).is_none()
        {
            return Err(Denied::Settings);
        }
        let permission = policy
            .map(|policy| {
                let description = Description::from_engine(
                    policy
                        .command
                        .describe_for_principal(PrincipalId(binding.client.0)),
                    policy.interactive,
                    policy.shell_command,
                )?;
                PermissionPlan::new(policy.ordinal, &description)
            })
            .transpose()?;
        Ok(Rc::new(Self {
            binding,
            interval,
            fields,
            permission,
        }))
    }

    fn next_delay(&self) -> Duration {
        let mut hash = Sha256::new();
        hash.update(b"marmish/peer/heartbeat-jitter/v1\0");
        hash.update(self.binding.room.0);
        let mut word = [0; 8];
        word.copy_from_slice(&hash.finalize()[..8]);
        let jitter = u64::from_be_bytes(word) % (u64::from((self.interval / 20).min(300)) + 1);
        Duration::from_secs(u64::from(self.interval) + jitter)
    }
}

pub(crate) struct View<'a> {
    pub(crate) accepted: Accepted,
    pub(crate) execution_granted: bool,
    pub(crate) revision: NonZeroU64,
    pub(crate) settings: Option<&'a Rc<Settings>>,
}

struct Installed {
    settings: Rc<Settings>,
    route: Route,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Denied {
    Authority,
    Disabled,
    Revision,
    Epoch,
    Clock,
    Settings,
    Sequence,
    Ticket,
    Policy,
    Wire(WireError),
}

impl From<WireError> for Denied {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

pub(crate) trait SamplerPort {
    fn start(&mut self, request: Request, budget: Duration) -> Result<(), StartError>;
    fn poll(&mut self) -> Progress;
}

impl SamplerPort for Sampler {
    fn start(&mut self, request: Request, budget: Duration) -> Result<(), StartError> {
        Self::start(self, request, budget)
    }
    fn poll(&mut self) -> Progress {
        Self::poll(self)
    }
}

pub(crate) fn native_time() -> Option<Clock> {
    Some(Clock {
        wall: SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs(),
        monotonic: continuous_time().ok()?,
    })
}

struct Sample {
    epoch: u64,
    expires: Duration,
    complete: Option<Complete>,
}

struct Complete {
    nonce: Rc<()>,
    sampled_at: u64,
    host: HostStatus,
}

/// Held by the caller exactly once. The private snapshot stays in the planner.
pub(crate) struct Ready {
    nonce: Rc<()>,
    binding: Binding,
    revision: NonZeroU64,
    epoch: u64,
    expires: Duration,
}

pub(crate) enum Tick {
    Idle,
    Started,
    Pending,
    Draining,
    Busy,
    Retry,
    Ready(Ready),
}

/// Prepared application bytes for the fixture. No method publishes them.
pub(crate) struct Encoded {
    pub(crate) room: Hex32,
    pub(crate) created_at: u64,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) struct Reservation {
    pub(crate) binding: Binding,
    pub(crate) sequence: NonZeroU64,
}

#[derive(Clone, Copy)]
pub(crate) struct PolicyFence {
    pub(crate) generation: Generation,
    pub(crate) issued_at: Option<u64>,
}

struct Issue {
    epoch: u64,
    started: Clock,
    expires: Duration,
    wall_expires: u64,
    page: usize,
}

pub(crate) struct Planner<P, C> {
    binding: Binding,
    port: P,
    clock: C,
    clock_floor: Option<Clock>,
    revision: Option<NonZeroU64>,
    revoked: bool,
    epoch: u64,
    active: bool,
    installed: Option<Installed>,
    period: Option<Duration>,
    retry: Option<Duration>,
    last_start: Option<Duration>,
    worker: bool,
    sample: Option<Sample>,
    sequence: Option<NonZeroU64>,
    policy: Option<PolicyFence>,
    last_issue: Option<Duration>,
    issue: Option<Issue>,
}

impl<P: SamplerPort, C: FnMut() -> Option<Clock>> Planner<P, C> {
    pub(crate) fn new(
        binding: Binding,
        port: P,
        clock: C,
        sequence: Option<NonZeroU64>,
        policy: Option<PolicyFence>,
    ) -> Self {
        Self {
            binding,
            port,
            clock,
            clock_floor: None,
            revision: None,
            revoked: false,
            epoch: 0,
            active: false,
            installed: None,
            period: None,
            retry: None,
            last_start: None,
            worker: false,
            sample: None,
            sequence,
            policy,
            last_issue: None,
            issue: None,
        }
    }

    fn change(&mut self, active: bool) -> Result<(), Denied> {
        self.active = false;
        self.sample = None;
        self.issue = None;
        self.epoch = self.epoch.checked_add(1).ok_or(Denied::Epoch)?;
        self.active = active;
        Ok(())
    }

    fn pause(&mut self, reason: Denied) -> Result<(), Denied> {
        if self.active {
            self.change(false)?;
        }
        Err(reason)
    }

    fn view(&mut self, current: Option<View<'_>>) -> Result<(), Denied> {
        let Some(view) = current else {
            return self.pause(Denied::Authority);
        };
        if !view.accepted.correct_side(self.binding, Direction::Server) {
            return self.pause(Denied::Authority);
        }
        if self.revision.is_some_and(|prior| view.revision < prior) {
            return Err(Denied::Revision);
        }
        if self.revision != Some(view.revision) {
            self.change(false)?;
            self.revision = Some(view.revision);
            self.installed = None;
            self.revoked = false;
        }
        if !view.execution_granted {
            self.revoked = true;
            return self.pause(Denied::Authority);
        }
        if view.accepted.route == Route::Unavailable {
            return self.pause(Denied::Authority);
        }
        let Some(settings) = view.settings else {
            self.revoked = true;
            return self.pause(Denied::Disabled);
        };
        if self.revoked {
            return self.pause(Denied::Revision);
        }
        if settings.binding != self.binding {
            return self.pause(Denied::Settings);
        }
        if self.installed.as_ref().is_some_and(|prior| {
            prior.route != view.accepted.route || !Rc::ptr_eq(&prior.settings, settings)
        }) {
            return self.pause(Denied::Revision);
        }
        if let Some(plan) = &settings.permission {
            if self.policy.is_some_and(|prior| {
                prior.generation.ordinal > plan.generation.ordinal
                    || (prior.generation.ordinal == plan.generation.ordinal
                        && prior.generation != plan.generation)
            }) {
                return self.pause(Denied::Policy);
            }
            if self
                .policy
                .is_none_or(|prior| prior.generation != plan.generation)
            {
                self.policy = Some(PolicyFence {
                    generation: plan.generation,
                    issued_at: None,
                });
            }
        }
        if !self.active {
            self.change(true)?;
            self.period = None;
            self.retry = None;
            self.installed = Some(Installed {
                settings: Rc::clone(settings),
                route: view.accepted.route,
            });
        }
        Ok(())
    }

    fn now(&mut self) -> Result<Clock, Denied> {
        let reading = (self.clock)().filter(|now| {
            now.wall != 0
                && self
                    .clock_floor
                    .is_none_or(|prior| now.wall >= prior.wall && now.monotonic >= prior.monotonic)
        });
        let Some(reading) = reading else {
            if self.sample.is_some() {
                self.fail_sample(
                    self.clock_floor
                        .map_or(Duration::ZERO, |clock| clock.monotonic),
                )?;
            }
            self.issue = None;
            return Err(Denied::Clock);
        };
        self.clock_floor = Some(reading);
        Ok(reading)
    }

    fn fail_sample(&mut self, now: Duration) -> Result<(), Denied> {
        self.sample = None;
        self.retry = Some(after(now, SPACING)?);
        Ok(())
    }

    fn drain(&mut self) -> Tick {
        if matches!(self.port.poll(), Progress::Ready(_) | Progress::Idle) {
            self.worker = false;
        }
        Tick::Draining
    }

    pub(crate) fn tick(&mut self, current: Option<View<'_>>) -> Result<Tick, Denied> {
        if let Err(error) = self.view(current) {
            if self.worker && self.sample.is_none() {
                self.drain();
            }
            return Err(error);
        }
        let now = self.now()?;
        if self
            .sample
            .as_ref()
            .is_some_and(|sample| now.monotonic >= sample.expires)
        {
            self.fail_sample(now.monotonic)?;
        }
        if self.worker {
            return if self.sample.is_some() {
                self.poll_sample()
            } else {
                Ok(self.drain())
            };
        }
        if self.sample.is_some()
            || self
                .retry
                .or(self.period)
                .is_some_and(|due| now.monotonic < due)
            || !spaced(self.last_start, now.monotonic)?
        {
            return Ok(Tick::Idle);
        }

        let installed = self.installed.as_ref().ok_or(Denied::Authority)?;
        let request = installed
            .settings
            .fields
            .request(installed.route)
            .ok_or(Denied::Settings)?;
        let expires = after(now.monotonic, SAMPLE_WINDOW)?;
        let submitting = self.now()?;
        let Some(budget) = expires
            .checked_sub(submitting.monotonic)
            .filter(|budget| !budget.is_zero())
        else {
            self.fail_sample(submitting.monotonic)?;
            return Ok(Tick::Retry);
        };
        match self.port.start(request, budget) {
            Ok(()) => {
                self.last_start = Some(submitting.monotonic);
                self.worker = true;
                self.sample = Some(Sample {
                    epoch: self.epoch,
                    expires,
                    complete: None,
                });
                Ok(Tick::Started)
            }
            Err(StartError::Busy) => Ok(Tick::Busy),
            Err(StartError::Unavailable | StartError::InvalidBudget) => {
                self.fail_sample(submitting.monotonic)?;
                Ok(Tick::Retry)
            }
        }
    }

    fn poll_sample(&mut self) -> Result<Tick, Denied> {
        let progress = self.port.poll();
        if matches!(progress, Progress::Ready(_) | Progress::Idle) {
            self.worker = false;
        }
        let now = self.now()?;
        match progress {
            Progress::Ready(mut host) => {
                let sample = self.sample.as_mut().ok_or(Denied::Ticket)?;
                if now.monotonic >= sample.expires || sample.epoch != self.epoch {
                    self.fail_sample(now.monotonic)?;
                    return Ok(Tick::Retry);
                }
                let installed = self.installed.as_ref().ok_or(Denied::Authority)?;
                installed.settings.fields.narrow(installed.route, &mut host);
                let nonce = Rc::new(());
                sample.complete = Some(Complete {
                    nonce: Rc::clone(&nonce),
                    sampled_at: now.wall,
                    host,
                });
                Ok(Tick::Ready(Ready {
                    nonce,
                    binding: self.binding,
                    revision: self.revision.ok_or(Denied::Revision)?,
                    epoch: sample.epoch,
                    expires: sample.expires,
                }))
            }
            Progress::Pending => Ok(Tick::Pending),
            Progress::Expired | Progress::Unavailable | Progress::Idle => {
                self.fail_sample(now.monotonic)?;
                Ok(Tick::Retry)
            }
        }
    }

    pub(crate) fn release(
        &mut self,
        current: Option<View<'_>>,
        ready: Ready,
        reserved: Reservation,
    ) -> Result<Encoded, Denied> {
        if reserved.binding != self.binding
            || self.sequence.is_some_and(|past| reserved.sequence <= past)
        {
            return Err(Denied::Sequence);
        }
        self.sequence = Some(reserved.sequence);
        self.view(current)?;
        let now = self.now()?;
        let pin_matches = self
            .sample
            .as_ref()
            .and_then(|sample| sample.complete.as_ref())
            .is_some_and(|complete| Rc::ptr_eq(&complete.nonce, &ready.nonce))
            && self.binding == ready.binding
            && self.revision == Some(ready.revision)
            && self.epoch == ready.epoch;
        if !pin_matches {
            return Err(Denied::Ticket);
        }
        if now.monotonic >= ready.expires {
            self.fail_sample(now.monotonic)?;
            return Err(Denied::Ticket);
        }
        let complete = self
            .sample
            .take()
            .and_then(|sample| sample.complete)
            .ok_or(Denied::Ticket)?;
        let installed = self.installed.as_ref().ok_or(Denied::Authority)?;
        let record = Record::new(
            self.binding.room,
            Payload::Heartbeat(Heartbeat {
                sequence: reserved.sequence,
                sampled_at: complete.sampled_at,
                interval: installed.settings.interval,
                host: complete.host,
                permission: installed
                    .settings
                    .permission
                    .as_ref()
                    .map(|plan| plan.generation),
            }),
        );
        let delay = installed.settings.next_delay();
        let bytes = match record.encode() {
            Ok(bytes) => bytes,
            Err(error) => {
                self.fail_sample(now.monotonic)?;
                return Err(error.into());
            }
        };
        let released = match self.now() {
            Ok(released) => released,
            Err(error) => {
                self.fail_sample(now.monotonic)?;
                return Err(error);
            }
        };
        if released.monotonic >= ready.expires {
            self.fail_sample(released.monotonic)?;
            return Err(Denied::Ticket);
        }
        self.period = Some(after(released.monotonic, delay)?);
        self.retry = None;
        Ok(Encoded {
            room: self.binding.room,
            created_at: complete.sampled_at,
            bytes,
        })
    }

    pub(crate) fn start_issue(&mut self, current: Option<View<'_>>) -> Result<bool, Denied> {
        self.view(current)?;
        let now = self.now()?;
        if self
            .issue
            .as_ref()
            .is_some_and(|issue| !issue.live(now, self.epoch))
        {
            self.issue = None;
        }
        if self.issue.is_some() || !spaced(self.last_issue, now.monotonic)? {
            return Ok(false);
        }
        let plan = self
            .installed
            .as_ref()
            .and_then(|installed| installed.settings.permission.as_ref())
            .ok_or(Denied::Policy)?;
        let fence = self
            .policy
            .as_mut()
            .filter(|fence| fence.generation == plan.generation)
            .ok_or(Denied::Policy)?;
        if fence.issued_at.is_some_and(|past| now.wall <= past) {
            return Err(Denied::Policy);
        }
        let expires = after(now.monotonic, ISSUE_WINDOW)?;
        let wall_expires = now
            .wall
            .checked_add(ISSUE_WINDOW.as_secs())
            .ok_or(Denied::Clock)?;
        fence.issued_at = Some(now.wall);
        self.last_issue = Some(now.monotonic);
        self.issue = Some(Issue {
            epoch: self.epoch,
            started: now,
            expires,
            wall_expires,
            page: 0,
        });
        Ok(true)
    }

    pub(crate) fn next_issue(
        &mut self,
        current: Option<View<'_>>,
    ) -> Result<Option<Encoded>, Denied> {
        self.view(current)?;
        let now = self.now()?;
        let Some(mut issue) = self.issue.take() else {
            return Ok(None);
        };
        if !issue.live(now, self.epoch) {
            return Err(Denied::Ticket);
        }
        let plan = self
            .installed
            .as_ref()
            .and_then(|installed| installed.settings.permission.as_ref())
            .ok_or(Denied::Policy)?;
        let Some(bytes) = plan
            .records(self.binding.room, issue.started.wall)
            .nth(issue.page)
            .transpose()?
        else {
            return Ok(None);
        };
        let now = self.now()?;
        if !issue.live(now, self.epoch) {
            return Err(Denied::Ticket);
        }
        let encoded = Encoded {
            room: self.binding.room,
            created_at: issue.started.wall,
            bytes,
        };
        issue.page += 1;
        self.issue = Some(issue);
        Ok(Some(encoded))
    }
}

impl Issue {
    fn live(&self, now: Clock, epoch: u64) -> bool {
        self.epoch == epoch && now.monotonic < self.expires && now.wall < self.wall_expires
    }
}

fn spaced(prior: Option<Duration>, now: Duration) -> Result<bool, Denied> {
    prior.map_or(Ok(true), |prior| Ok(now >= after(prior, SPACING)?))
}

fn after(now: Duration, duration: Duration) -> Result<Duration, Denied> {
    now.checked_add(duration).ok_or(Denied::Clock)
}

pub(crate) struct Authenticated {
    pub(crate) author: Hex32,
    pub(crate) leaf: Hex32,
    pub(crate) current_epoch: bool,
    pub(crate) created_at: u64,
}

pub(crate) struct Projection {
    binding: Binding,
    receiver: Option<Receiver>,
}

impl Projection {
    pub(crate) fn new(binding: Binding) -> Self {
        Self {
            binding,
            receiver: Some(Receiver::new(binding.receiver())),
        }
    }

    pub(crate) fn retire(&mut self) {
        self.receiver = None;
    }

    fn currently_allowed(&self, current: Option<Accepted>) -> bool {
        current.is_some_and(|proof| proof.allows(self.binding, Direction::Client))
    }

    pub(crate) fn receive(
        &mut self,
        current: Option<Accepted>,
        event: Authenticated,
        bytes: &[u8],
        now: Clock,
        allow_reservation: bool,
    ) -> Result<Applied, Refused> {
        if !self.currently_allowed(current)
            || event.author != self.binding.server
            || event.leaf != self.binding.server
            || !event.current_epoch
        {
            return Err(Refused::Binding);
        }
        self.receiver.as_mut().ok_or(Refused::Binding)?.receive(
            self.binding.receiver(),
            event.created_at,
            bytes,
            now,
            allow_reservation,
        )
    }

    pub(crate) fn view(
        &self,
        current: Option<Accepted>,
        now: Clock,
    ) -> (HeartbeatView<'_>, PermissionView<'_>) {
        if self.currently_allowed(current)
            && let Some(receiver) = &self.receiver
        {
            return (receiver.heartbeat(now), receiver.permission(now));
        }
        (HeartbeatView::Unknown, PermissionView::Unavailable)
    }
}

#[cfg(test)]
#[path = "peer_heartbeat_planner_tests.rs"]
mod tests;
