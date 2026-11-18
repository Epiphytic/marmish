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

struct Sample {
    heartbeat: Heartbeat,
    stamp: Stamp,
}

#[derive(Clone, Copy)]
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

struct CurrentPermission {
    generation: Generation,
    issue_fence: Option<u64>,
    state: CurrentKind,
}

enum CurrentKind {
    Pending,
    Completed,
    Poison,
    TooLarge { length: u32, receipt: Option<Clock> },
}

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

enum Inserted {
    Awaiting,
    Duplicate,
    Full,
    Conflict,
}

impl Stage {
    fn new(issued_at: u64, received_at: Duration, total: u8) -> Self {
        Self {
            issued_at,
            received_at,
            total,
            present: 0,
            terminal_length: 0,
            bytes: vec![0; usize::from(total) * PAGE_BYTES],
        }
    }

    fn insert(&mut self, page: u8, total: u8, data: &[u8]) -> Inserted {
        if total != self.total {
            return Inserted::Conflict;
        }
        let bit = 1u64 << page;
        let start = usize::from(page) * PAGE_BYTES;
        let range = start..start + data.len();
        let terminal = page + 1 == self.total;

        if self.present & bit != 0 {
            if (terminal && data.len() != self.terminal_length) || &self.bytes[range] != data {
                return Inserted::Conflict;
            }
            return Inserted::Duplicate;
        }

        self.bytes[range].copy_from_slice(data);
        self.present |= bit;
        if terminal {
            self.terminal_length = data.len();
        }
        if self.present == u64::MAX >> (64 - self.total) {
            Inserted::Full
        } else {
            Inserted::Awaiting
        }
    }

    fn description(&self, digest: Hex32) -> Result<Description, WireError> {
        let length = (usize::from(self.total) - 1) * PAGE_BYTES + self.terminal_length;
        Description::decode_canonical(&self.bytes[..length], digest)
    }

    fn expired_at(&self, now: Duration) -> bool {
        now.checked_sub(self.received_at)
            .is_some_and(|age| age >= Duration::from_secs(PERMISSION_WINDOW))
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
        match record.payload {
            Payload::Heartbeat(heartbeat) => {
                let accept = fresh_limit(heartbeat.interval) + u64::from(heartbeat.interval);
                let age = check_time(heartbeat.sampled_at, stamp, accept)?;
                Ok(self.apply_heartbeat(heartbeat, stamp, age))
            }
            Payload::Permission(permission) => {
                check_time(permission.issued_at, stamp, PERMISSION_WINDOW)?;
                Ok(self.apply_permission(permission, stamp, allow_reservation))
            }
        }
    }

    fn apply_heartbeat(&mut self, heartbeat: Heartbeat, stamp: Stamp, age: u64) -> Applied {
        if self
            .seen
            .is_some_and(|(sequence, _)| heartbeat.sequence <= sequence)
        {
            return Applied::Ignored;
        }
        self.seen = Some((heartbeat.sequence, stamp.received));
        if age >= fresh_limit(heartbeat.interval) {
            return Applied::Watermark;
        }
        if let Some(generation) = heartbeat.permission
            && matches!(self.observe(generation), GenerationMatch::Active)
            && let Some(completed) = self.completed.as_mut()
            && completed.generation == generation
        {
            completed.heartbeat = Some((stamp, heartbeat.interval));
        }
        self.sample = Some(Sample { heartbeat, stamp });
        Applied::Heartbeat
    }

    fn apply_permission(
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
        self.expire_stage(stamp.received.monotonic);
        match permission.content {
            PageContent::TooLarge { description_len } => {
                self.apply_oversize(description_len, stamp)
            }
            PageContent::Chunk {
                page,
                total,
                encoded,
            } => self.apply_chunk(stamp, page, total, &encoded.0, allow_reservation),
        }
    }

    fn apply_oversize(&mut self, length: u32, stamp: Stamp) -> Applied {
        let current = self
            .current
            .as_mut()
            .expect("observed permission generation");
        // The signed representation itself conflicts, even at a replayed issue time.
        match current.state {
            CurrentKind::Completed => return self.poison(),
            CurrentKind::TooLarge { length: known, .. } if known != length => return self.poison(),
            _ if self.stage.is_some() => return self.poison(),
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

    fn apply_chunk(
        &mut self,
        stamp: Stamp,
        page: u8,
        total: u8,
        data: &[u8],
        allow_reservation: bool,
    ) -> Applied {
        let current = self
            .current
            .as_ref()
            .expect("observed permission generation");
        if matches!(current.state, CurrentKind::TooLarge { .. }) {
            return self.poison();
        }
        if current
            .issue_fence
            .is_some_and(|fence| stamp.signed <= fence)
        {
            return Applied::Ignored;
        }
        if self.stage.is_none() {
            if !allow_reservation {
                return Applied::Deferred;
            }
            self.stage = Some(Stage::new(stamp.signed, stamp.received.monotonic, total));
        }

        let stage = self.stage.as_mut().expect("active permission stage");
        if stage.issued_at != stamp.signed {
            return Applied::Ignored;
        }
        match stage.insert(page, total, data) {
            Inserted::Awaiting => Applied::Pending,
            Inserted::Duplicate => Applied::Ignored,
            Inserted::Conflict => self.poison(),
            Inserted::Full => self.finish(stamp),
        }
    }

    fn finish(&mut self, stamp: Stamp) -> Applied {
        let stage = self.stage.take().expect("complete permission stage");
        let current = self
            .current
            .as_mut()
            .expect("observed permission generation");
        let generation = current.generation;
        let Ok(description) = stage.description(generation.digest) else {
            return self.poison();
        };
        current.issue_fence = Some(stage.issued_at);
        current.state = CurrentKind::Completed;
        let heartbeat = self
            .completed
            .as_ref()
            .filter(|completed| completed.generation == generation)
            .and_then(|completed| completed.heartbeat);
        self.completed = Some(Completed {
            generation,
            description,
            transfer: stamp,
            heartbeat,
        });
        Applied::Permission
    }

    fn observe(&mut self, generation: Generation) -> GenerationMatch {
        if let Some(current) = &self.current {
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

        self.stage = None;
        self.current = Some(CurrentPermission {
            generation,
            issue_fence: None,
            state: CurrentKind::Pending,
        });
        GenerationMatch::Active
    }

    fn poison(&mut self) -> Applied {
        let current = self
            .current
            .as_mut()
            .expect("observed permission generation");
        current.state = CurrentKind::Poison;
        self.stage = None;
        if self
            .completed
            .as_ref()
            .is_some_and(|completed| completed.generation == current.generation)
        {
            self.completed = None;
        }
        Applied::Poisoned
    }

    fn expire_stage(&mut self, now: Duration) {
        if let Some(stage) = &self.stage
            && stage.expired_at(now)
        {
            let current = self.current.as_mut().expect("staged permission generation");
            current.issue_fence = Some(stage.issued_at);
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
