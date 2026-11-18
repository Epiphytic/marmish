//! Deterministic receive decisions after the native Peer authority checks.

use super::{
    Description, Generation, Heartbeat, Hex32, PAGE_BYTES, PageContent, Payload, PermissionRecord,
    Record, WireError,
};
use std::{cmp::Ordering, num::NonZeroU64, time::Duration};

const SKEW: u64 = 120;
const PERMISSION_WINDOW: u64 = 900;
const DEFAULT_INTERVAL: u32 = 3600;
const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Caller-verified two-member room, with the accepted local client and remote server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClientBinding {
    pub(crate) room: Hex32,
    pub(crate) local_client: Hex32,
    pub(crate) pinned_server: Hex32,
    pub(crate) local_generation: NonZeroU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Clock {
    pub(crate) wall: u64,
    pub(crate) monotonic: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refused {
    Wire(WireError),
    Binding,
    Clock,
    SignedTime,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Applied {
    Ignored,
    Heartbeat,
    Watermark,
    Deferred,
    Pending,
    Permission,
    Poisoned,
}

#[derive(Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) enum HeartbeatView<'a> {
    Unknown,
    Fresh(TimedHeartbeat<'a>),
    Stale(Option<TimedHeartbeat<'a>>),
}

#[derive(Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct TimedHeartbeat<'a> {
    pub(crate) sample: &'a Heartbeat,
    pub(crate) age: Duration,
    pub(crate) received_at: u64,
}

#[derive(Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) enum PermissionView<'a> {
    Unavailable,
    Fresh(Generation, &'a Description),
    Cached(Generation, &'a Description),
    TooLarge(Generation, u32),
}

pub(crate) struct Receiver {
    binding: ClientBinding,
    seen: Option<(NonZeroU64, Clock)>,
    sample: Option<Sample>,
    current: Option<CurrentPermission>,
    stage: Option<Stage>,
    completed: Option<Completed>,
}

#[cfg_attr(test, derive(Debug))]
struct Sample {
    heartbeat: Heartbeat,
    stamp: Stamp,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug))]
struct Stamp {
    signed: u64,
    received: Clock,
}

impl Stamp {
    fn fresh_at(self, now: Clock, interval: u32) -> bool {
        let cap = Duration::from_secs(fresh_limit(interval));
        self.age(now).is_some_and(|age| age < cap)
            && now
                .monotonic
                .checked_sub(self.received.monotonic)
                .is_some_and(|age| age < cap)
    }

    fn age(self, now: Clock) -> Option<Duration> {
        now.wall.checked_sub(self.received.wall)?;
        now.monotonic.checked_sub(self.received.monotonic)?;
        Some(Duration::from_secs(now.wall.saturating_sub(self.signed)))
    }

    fn retained(self, now: Clock) -> bool {
        self.received.retained(now)
    }
}

impl Clock {
    fn retained(self, now: Clock) -> bool {
        now.wall
            .checked_sub(self.wall)
            .map(Duration::from_secs)
            .is_none_or(|age| age < RETENTION)
            && now
                .monotonic
                .checked_sub(self.monotonic)
                .is_none_or(|age| age < RETENTION)
    }

    fn comparable(self, now: Clock) -> bool {
        self.wall <= now.wall && self.monotonic <= now.monotonic
    }
}

fn jitter(interval: u32) -> u64 {
    u64::from((interval / 20).min(300))
}

fn fresh_limit(interval: u32) -> u64 {
    u64::from(interval) + jitter(interval) + SKEW
}

#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(test, derive(Debug))]
struct CurrentPermission {
    generation: Generation,
    issue_fence: Option<u64>,
    state: CurrentKind,
}

#[derive(Clone, Copy, PartialEq)]
#[cfg_attr(test, derive(Debug))]
enum CurrentKind {
    Pending,
    Completed,
    Poison,
    TooLarge { length: u32, receipt: Option<Clock> },
}

#[cfg_attr(test, derive(Debug))]
struct Completed {
    generation: Generation,
    description: Description,
    transfer: Stamp,
    heartbeat: Option<(Stamp, u32)>,
}

struct Stage {
    issued_at: u64,
    received_at: Duration,
    total: u8,
    present: u64,
    terminal_length: usize,
    bytes: Vec<u8>,
}

struct PageWrite {
    index: u8,
    bytes: Vec<u8>,
}

impl PageWrite {
    fn range(&self) -> std::ops::Range<usize> {
        let start = usize::from(self.index) * PAGE_BYTES;
        start..start + self.bytes.len()
    }

    fn bit(&self) -> u64 {
        1u64 << self.index
    }
}

enum Inserted {
    Awaiting,
    Duplicate,
    Full,
    Conflict,
}

impl Stage {
    fn new(stamp: Stamp, total: u8, page: PageWrite) -> Self {
        let mut stage = Self {
            issued_at: stamp.signed,
            received_at: stamp.received.monotonic,
            total,
            present: 0,
            terminal_length: 0,
            bytes: vec![0; usize::from(total) * PAGE_BYTES],
        };
        stage.write(page);
        stage
    }

    fn classify(&self, total: u8, page: &PageWrite) -> Inserted {
        if total != self.total {
            return Inserted::Conflict;
        }
        if self.present & page.bit() != 0 {
            if (self.is_terminal(page) && page.bytes.len() != self.terminal_length)
                || self.bytes[page.range()] != page.bytes
            {
                return Inserted::Conflict;
            }
            return Inserted::Duplicate;
        }
        if self.present | page.bit() == u64::MAX >> (64 - self.total) {
            Inserted::Full
        } else {
            Inserted::Awaiting
        }
    }

    fn is_terminal(&self, page: &PageWrite) -> bool {
        page.index + 1 == self.total
    }

    fn write(&mut self, page: PageWrite) {
        self.bytes[page.range()].copy_from_slice(&page.bytes);
        self.present |= page.bit();
        if self.is_terminal(&page) {
            self.terminal_length = page.bytes.len();
        }
    }

    fn description_with(&self, page: &PageWrite, digest: Hex32) -> Result<Description, WireError> {
        let terminal_length = if self.is_terminal(page) {
            page.bytes.len()
        } else {
            self.terminal_length
        };
        let length = (usize::from(self.total) - 1) * PAGE_BYTES + terminal_length;
        let mut assembled = self.bytes[..length].to_vec();
        assembled[page.range()].copy_from_slice(&page.bytes);
        Description::decode_canonical(&assembled, digest)
    }

    fn expired_issue(&self, now: Duration) -> Option<u64> {
        now.checked_sub(self.received_at)
            .filter(|age| *age >= Duration::from_secs(PERMISSION_WINDOW))
            .map(|_| self.issued_at)
    }
}

enum StageChange {
    Keep,
    Clear,
    Start(StageStart),
    Append(PageWrite),
}

struct StageStart {
    stamp: Stamp,
    total: u8,
    page: PageWrite,
}

enum CompletedChange {
    Keep,
    Clear,
    Replace(Completed),
    Refresh(Stamp, u32),
}

struct Changes {
    seen: Option<(NonZeroU64, Clock)>,
    sample: Option<Sample>,
    current: Option<CurrentPermission>,
    stage: StageChange,
    completed: CompletedChange,
}

struct Planner<'a> {
    receiver: &'a Receiver,
    current: Option<CurrentPermission>,
    stage: Option<&'a Stage>,
    changes: Changes,
}

#[must_use = "apply a prepared transition only after committing its changes"]
pub(crate) struct PreparedReceive<'a> {
    outcome: Applied,
    reserved_after: usize,
    stage: PreparedStage<'a>,
    completed: PreparedCompleted<'a>,
    seen: Option<Write<'a, (NonZeroU64, Clock)>>,
    sample: Option<Write<'a, Sample>>,
    current: Option<Write<'a, CurrentPermission>>,
}

struct Write<'a, T> {
    slot: &'a mut Option<T>,
    value: T,
}

impl<'a, T> Write<'a, T> {
    fn prepare(slot: &'a mut Option<T>, value: Option<T>) -> Option<Self> {
        value.map(|value| Self { slot, value })
    }

    fn apply(self) {
        *self.slot = Some(self.value);
    }
}

enum PreparedStage<'a> {
    Keep,
    Replace(&'a mut Option<Stage>, Option<StageStart>),
    Append(&'a mut Stage, PageWrite),
}

enum PreparedCompleted<'a> {
    Keep,
    Replace(&'a mut Option<Completed>, Option<Completed>),
    Refresh(Write<'a, (Stamp, u32)>),
}

impl PreparedReceive<'_> {
    pub(crate) fn outcome(&self) -> Applied {
        self.outcome
    }

    pub(crate) fn is_noop(&self) -> bool {
        self.seen.is_none()
            && self.sample.is_none()
            && self.current.is_none()
            && matches!(self.stage, PreparedStage::Keep)
            && matches!(self.completed, PreparedCompleted::Keep)
    }

    pub(crate) fn reserved_after(&self) -> usize {
        self.reserved_after
    }

    pub(crate) fn apply(self) -> Applied {
        match self.stage {
            PreparedStage::Keep => {}
            PreparedStage::Replace(slot, start) => {
                *slot = start.map(|start| Stage::new(start.stamp, start.total, start.page));
            }
            PreparedStage::Append(stage, page) => stage.write(page),
        }
        match self.completed {
            PreparedCompleted::Keep => {}
            PreparedCompleted::Replace(slot, value) => *slot = value,
            PreparedCompleted::Refresh(receipt) => receipt.apply(),
        }
        if let Some(seen) = self.seen {
            seen.apply();
        }
        if let Some(sample) = self.sample {
            sample.apply();
        }
        if let Some(current) = self.current {
            current.apply();
        }
        self.outcome
    }
}

impl Changes {
    fn bind(self, receiver: &mut Receiver, outcome: Applied) -> PreparedReceive<'_> {
        let reserved_after = match &self.stage {
            StageChange::Keep | StageChange::Append(_) => receiver.reserved_bytes(),
            StageChange::Clear => 0,
            StageChange::Start(start) => usize::from(start.total) * PAGE_BYTES,
        };
        let stage = match self.stage {
            StageChange::Keep => PreparedStage::Keep,
            StageChange::Clear => PreparedStage::Replace(&mut receiver.stage, None),
            StageChange::Start(start) => PreparedStage::Replace(&mut receiver.stage, Some(start)),
            StageChange::Append(page) => {
                let stage = receiver
                    .stage
                    .as_mut()
                    .expect("planned append to current stage");
                PreparedStage::Append(stage, page)
            }
        };
        let completed = match self.completed {
            CompletedChange::Keep => PreparedCompleted::Keep,
            CompletedChange::Clear => PreparedCompleted::Replace(&mut receiver.completed, None),
            CompletedChange::Replace(value) => {
                PreparedCompleted::Replace(&mut receiver.completed, Some(value))
            }
            CompletedChange::Refresh(stamp, interval) => {
                let completed = receiver
                    .completed
                    .as_mut()
                    .expect("planned receipt refresh");
                PreparedCompleted::Refresh(Write {
                    slot: &mut completed.heartbeat,
                    value: (stamp, interval),
                })
            }
        };
        PreparedReceive {
            outcome,
            reserved_after,
            stage,
            completed,
            seen: Write::prepare(&mut receiver.seen, self.seen),
            sample: Write::prepare(&mut receiver.sample, self.sample),
            current: Write::prepare(&mut receiver.current, self.current),
        }
    }
}

enum GenerationMatch {
    Active,
    Obsolete,
    Conflict,
}

impl Receiver {
    pub(crate) fn new(binding: ClientBinding) -> Self {
        Self {
            binding,
            seen: None,
            sample: None,
            current: None,
            stage: None,
            completed: None,
        }
    }

    pub(crate) fn reserved_bytes(&self) -> usize {
        self.stage.as_ref().map_or(0, |stage| stage.bytes.len())
    }

    pub(crate) fn receive(
        &mut self,
        active: ClientBinding,
        created_at: u64,
        bytes: &[u8],
        now: Clock,
        allow_reservation: bool,
    ) -> Result<Applied, Refused> {
        Ok(self
            .plan_receive(active, created_at, bytes, now, allow_reservation)?
            .apply())
    }

    pub(crate) fn plan_receive(
        &mut self,
        active: ClientBinding,
        created_at: u64,
        bytes: &[u8],
        now: Clock,
        allow_reservation: bool,
    ) -> Result<PreparedReceive<'_>, Refused> {
        if active != self.binding {
            return Err(Refused::Binding);
        }
        let record = Record::decode(bytes).map_err(Refused::Wire)?;
        if record.room != self.binding.room {
            return Err(Refused::Binding);
        }
        if now.wall == 0 {
            return Err(Refused::Clock);
        }
        let stamp = Stamp {
            signed: created_at,
            received: now,
        };
        let mut planner = Planner::new(self);
        let outcome = match record.payload {
            Payload::Heartbeat(heartbeat) => {
                let accept = fresh_limit(heartbeat.interval) + u64::from(heartbeat.interval);
                let age = check_time(heartbeat.sampled_at, stamp, accept)?;
                planner.heartbeat(heartbeat, stamp, age)
            }
            Payload::Permission(permission) => {
                check_time(permission.issued_at, stamp, PERMISSION_WINDOW)?;
                planner.permission(permission, stamp, allow_reservation)
            }
        };
        let changes = planner.finish();
        Ok(changes.bind(self, outcome))
    }

    fn expire_stage(&mut self, now: Duration) {
        if let Some(issue) = self
            .stage
            .as_ref()
            .and_then(|stage| stage.expired_issue(now))
        {
            let current = self.current.as_mut().expect("staged permission generation");
            current.issue_fence = Some(issue);
            self.stage = None;
        }
    }

    pub(crate) fn sweep(&mut self, now: Clock) {
        self.expire_stage(now.monotonic);
        if self
            .sample
            .as_ref()
            .is_some_and(|sample| !sample.stamp.retained(now))
        {
            self.sample = None;
        }
        if self
            .completed
            .as_ref()
            .is_some_and(|completed| !completed.transfer.retained(now))
        {
            self.completed = None;
        }
        if let Some(current) = &mut self.current
            && let CurrentKind::TooLarge { receipt, .. } = &mut current.state
            && receipt.is_some_and(|stamp| !stamp.retained(now))
        {
            *receipt = None;
        }
    }

    pub(crate) fn heartbeat(&self, now: Clock) -> HeartbeatView<'_> {
        let Some((_, seen)) = self.seen else {
            return HeartbeatView::Unknown;
        };
        let Some(sample) = self
            .sample
            .as_ref()
            .filter(|sample| sample.stamp.retained(now))
        else {
            return if seen.comparable(now) {
                HeartbeatView::Stale(None)
            } else {
                HeartbeatView::Unknown
            };
        };
        let Some(age) = sample.stamp.age(now) else {
            return HeartbeatView::Unknown;
        };
        let timed = TimedHeartbeat {
            sample: &sample.heartbeat,
            age,
            received_at: sample.stamp.received.wall,
        };
        if sample.stamp.fresh_at(now, sample.heartbeat.interval) {
            HeartbeatView::Fresh(timed)
        } else {
            HeartbeatView::Stale(Some(timed))
        }
    }

    pub(crate) fn permission(&self, now: Clock) -> PermissionView<'_> {
        let Some(current) = &self.current else {
            return PermissionView::Unavailable;
        };
        match current.state {
            CurrentKind::Poison => return PermissionView::Unavailable,
            CurrentKind::TooLarge { length, receipt } => {
                return if receipt.is_some_and(|stamp| stamp.retained(now) && stamp.comparable(now))
                {
                    PermissionView::TooLarge(current.generation, length)
                } else {
                    PermissionView::Unavailable
                };
            }
            _ => {}
        }
        let Some(completed) = self
            .completed
            .as_ref()
            .filter(|completed| completed.transfer.retained(now))
        else {
            return PermissionView::Unavailable;
        };
        let confirmed = completed.transfer.fresh_at(now, DEFAULT_INTERVAL)
            || completed
                .heartbeat
                .is_some_and(|(stamp, interval)| stamp.fresh_at(now, interval));
        if completed.generation == current.generation && confirmed {
            PermissionView::Fresh(completed.generation, &completed.description)
        } else {
            PermissionView::Cached(completed.generation, &completed.description)
        }
    }
}

impl<'a> Planner<'a> {
    fn new(receiver: &'a Receiver) -> Self {
        Self {
            receiver,
            current: receiver.current,
            stage: receiver.stage.as_ref(),
            changes: Changes {
                seen: None,
                sample: None,
                current: None,
                stage: StageChange::Keep,
                completed: CompletedChange::Keep,
            },
        }
    }

    fn finish(mut self) -> Changes {
        if self.current != self.receiver.current {
            self.changes.current = self.current;
        }
        self.changes
    }

    fn heartbeat(&mut self, heartbeat: Heartbeat, stamp: Stamp, age: u64) -> Applied {
        if self
            .receiver
            .seen
            .is_some_and(|(sequence, _)| heartbeat.sequence <= sequence)
        {
            return Applied::Ignored;
        }
        self.changes.seen = Some((heartbeat.sequence, stamp.received));
        if age >= fresh_limit(heartbeat.interval) {
            return Applied::Watermark;
        }
        if let Some(generation) = heartbeat.permission
            && matches!(self.observe(generation), GenerationMatch::Active)
            && self.completed_matches(generation)
        {
            self.changes.completed = CompletedChange::Refresh(stamp, heartbeat.interval);
        }
        self.changes.sample = Some(Sample { heartbeat, stamp });
        Applied::Heartbeat
    }

    fn permission(
        &mut self,
        permission: PermissionRecord,
        stamp: Stamp,
        allow_reservation: bool,
    ) -> Applied {
        match self.observe(permission.generation) {
            GenerationMatch::Obsolete => return Applied::Ignored,
            GenerationMatch::Conflict => return Applied::Poisoned,
            GenerationMatch::Active => {}
        }
        if let Some(issue) = self
            .stage
            .and_then(|stage| stage.expired_issue(stamp.received.monotonic))
        {
            self.permission_current().issue_fence = Some(issue);
            self.clear_stage();
        }
        match permission.content {
            PageContent::TooLarge { description_len } => self.oversize(description_len, stamp),
            PageContent::Chunk {
                page,
                total,
                encoded,
            } => self.chunk(
                stamp,
                total,
                PageWrite {
                    index: page,
                    bytes: encoded.0,
                },
                allow_reservation,
            ),
        }
    }

    fn permission_current(&mut self) -> &mut CurrentPermission {
        self.current
            .as_mut()
            .expect("observed permission generation")
    }

    fn oversize(&mut self, length: u32, stamp: Stamp) -> Applied {
        let has_stage = self.stage.is_some();
        let current = self.permission_current();
        // The signed representation itself conflicts, even at a replayed issue time.
        match current.state {
            CurrentKind::Completed => return self.poison(),
            CurrentKind::TooLarge { length: known, .. } if known != length => return self.poison(),
            _ if has_stage => return self.poison(),
            _ => {}
        }
        if current
            .issue_fence
            .is_some_and(|fence| stamp.signed <= fence)
        {
            return Applied::Ignored;
        }
        current.issue_fence = Some(stamp.signed);
        current.state = CurrentKind::TooLarge {
            length,
            receipt: Some(stamp.received),
        };
        Applied::Permission
    }

    fn chunk(
        &mut self,
        stamp: Stamp,
        total: u8,
        page: PageWrite,
        allow_reservation: bool,
    ) -> Applied {
        let current = *self.permission_current();
        if matches!(current.state, CurrentKind::TooLarge { .. }) {
            return self.poison();
        }
        if current
            .issue_fence
            .is_some_and(|fence| stamp.signed <= fence)
        {
            return Applied::Ignored;
        }
        let Some(stage) = self.stage else {
            if !allow_reservation {
                return Applied::Deferred;
            }
            if total == 1 {
                let description =
                    Description::decode_canonical(&page.bytes, current.generation.digest);
                return self.complete(stamp, description);
            }
            self.changes.stage = StageChange::Start(StageStart { stamp, total, page });
            return Applied::Pending;
        };
        if stage.issued_at != stamp.signed {
            return Applied::Ignored;
        }
        match stage.classify(total, &page) {
            Inserted::Awaiting => {
                self.changes.stage = StageChange::Append(page);
                Applied::Pending
            }
            Inserted::Duplicate => Applied::Ignored,
            Inserted::Conflict => self.poison(),
            Inserted::Full => {
                let description = stage.description_with(&page, current.generation.digest);
                self.complete(stamp, description)
            }
        }
    }

    fn complete(&mut self, stamp: Stamp, description: Result<Description, WireError>) -> Applied {
        let Ok(description) = description else {
            return self.poison();
        };
        self.clear_stage();
        let current = self.permission_current();
        let generation = current.generation;
        current.issue_fence = Some(stamp.signed);
        current.state = CurrentKind::Completed;
        let heartbeat = self
            .receiver
            .completed
            .as_ref()
            .filter(|completed| completed.generation == generation)
            .and_then(|completed| completed.heartbeat);
        self.changes.completed = CompletedChange::Replace(Completed {
            generation,
            description,
            transfer: stamp,
            heartbeat,
        });
        Applied::Permission
    }

    fn observe(&mut self, generation: Generation) -> GenerationMatch {
        if let Some(current) = self.current {
            match generation.ordinal.cmp(&current.generation.ordinal) {
                Ordering::Less => return GenerationMatch::Obsolete,
                Ordering::Equal if matches!(current.state, CurrentKind::Poison) => {
                    return GenerationMatch::Obsolete;
                }
                Ordering::Equal if generation == current.generation => {
                    return GenerationMatch::Active;
                }
                Ordering::Equal => {
                    self.poison();
                    return GenerationMatch::Conflict;
                }
                Ordering::Greater => {}
            }
        }
        self.clear_stage();
        self.current = Some(CurrentPermission {
            generation,
            issue_fence: None,
            state: CurrentKind::Pending,
        });
        GenerationMatch::Active
    }

    fn poison(&mut self) -> Applied {
        let current = self.permission_current();
        current.state = CurrentKind::Poison;
        let generation = current.generation;
        self.clear_stage();
        if self.completed_matches(generation) {
            self.changes.completed = CompletedChange::Clear;
        }
        Applied::Poisoned
    }

    fn completed_matches(&self, generation: Generation) -> bool {
        self.receiver
            .completed
            .as_ref()
            .is_some_and(|completed| completed.generation == generation)
    }

    fn clear_stage(&mut self) {
        if self.stage.take().is_some() {
            self.changes.stage = StageChange::Clear;
        }
    }
}

fn check_time(declared: u64, stamp: Stamp, age_limit: u64) -> Result<u64, Refused> {
    if declared != stamp.signed || declared.saturating_sub(stamp.received.wall) > SKEW {
        return Err(Refused::SignedTime);
    }
    let age = stamp.received.wall.saturating_sub(declared);
    if age >= age_limit {
        return Err(Refused::Expired);
    }
    Ok(age)
}

#[cfg(test)]
#[path = "peer_status_plan_tests.rs"]
mod plan_tests;
