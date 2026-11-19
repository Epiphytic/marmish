//! Memory-only local projection. Durable and native proof stay with the actor.
#![allow(
    dead_code,
    reason = "private actor and listener mount are separately reviewed"
)]

use super::{Entry, Reply, Request, Selection};
use crate::{command_limits::continuous_time, peer_invitation::PendingObservation};
use std::{
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::Instant as ExchangeInstant,
};

const LIFETIME: Duration = Duration::from_secs(1);
const READ_LIMIT: usize = 4;
static NEXT_ACTOR: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Eq, PartialEq)]
struct Stamp {
    actor: u64,
    listener: NonZeroU64,
    sequence: u64,
}

pub(crate) struct Ticket {
    stamp: Stamp,
    origin: ClockOrigin,
}

#[derive(Clone, Copy)]
struct ClockOrigin {
    monotonic: Instant,
    continuous: Duration,
}

impl ClockOrigin {
    fn now() -> Option<Self> {
        Some(Self {
            monotonic: Instant::now(),
            continuous: continuous_time().ok()?,
        })
    }

    fn fresh(self, origin: Self) -> bool {
        self.monotonic
            .checked_duration_since(origin.monotonic)
            .is_some_and(|elapsed| elapsed < LIFETIME)
            && self
                .continuous
                .checked_sub(origin.continuous)
                .is_some_and(|elapsed| elapsed < LIFETIME)
    }
}

/// Timing from an already admitted view. It carries no ticket or authority.
#[derive(Clone, Copy)]
pub(crate) struct AcceptedTiming {
    origin: ClockOrigin,
    admission: ClockOrigin,
}

#[cfg(marmish_peer_receiver_candidate)]
impl AcceptedTiming {
    pub(crate) fn age(self) -> Option<crate::diagnostics::pending_refresh::Clocks> {
        self.admission.elapsed(self.origin)
    }

    pub(crate) fn gap(self, prior: Self) -> Option<crate::diagnostics::pending_refresh::Clocks> {
        self.admission.elapsed(prior.admission)
    }

    pub(crate) fn prior_slack(
        self,
        prior: Self,
    ) -> Option<crate::diagnostics::pending_refresh::Clocks> {
        let elapsed = self.admission.elapsed(prior.origin)?;
        let lifetime = crate::diagnostics::pending_refresh::nanos(LIFETIME);
        Some(crate::diagnostics::pending_refresh::Clocks {
            monotonic_ns: lifetime - elapsed.monotonic_ns,
            continuous_ns: lifetime - elapsed.continuous_ns,
        })
    }
}

#[cfg(marmish_peer_receiver_candidate)]
impl ClockOrigin {
    fn elapsed(self, origin: Self) -> Option<crate::diagnostics::pending_refresh::Clocks> {
        use crate::diagnostics::pending_refresh::{Clocks, nanos};

        Some(Clocks {
            monotonic_ns: nanos(self.monotonic.checked_duration_since(origin.monotonic)?),
            continuous_ns: nanos(self.continuous.checked_sub(origin.continuous)?),
        })
    }
}

#[derive(Clone, Copy)]
struct Sample {
    time: ClockOrigin,
    wall: u64,
}

impl Sample {
    fn now() -> Option<Self> {
        Some(Self {
            time: ClockOrigin::now()?,
            wall: SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs(),
        })
    }
}

enum Projection {
    Pending(Entry),
    #[cfg(test)]
    SyntheticEmpty,
}

struct Observation {
    projection: Projection,
    wall_floor: u64,
    verified_wall: u64,
}

impl Observation {
    fn entry(&self) -> Option<Entry> {
        match self.projection {
            Projection::Pending(entry) => Some(entry),
            #[cfg(test)]
            Projection::SyntheticEmpty => None,
        }
    }
}

struct Published {
    origin: ClockOrigin,
    observation: Observation,
}

enum State {
    Invalid,
    Live(Published),
    Stopped,
}

struct Inner {
    state: State,
    sequence: u64,
    expired: Option<Selection>,
}

impl Inner {
    fn advance(&mut self) -> bool {
        if matches!(self.state, State::Stopped) {
            return false;
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.state = State::Stopped;
            return false;
        };
        self.sequence = sequence;
        self.state = State::Invalid;
        true
    }

    fn observe(&mut self, sample: Option<Sample>) -> Option<Option<Entry>> {
        let State::Live(published) = &self.state else {
            return None;
        };
        let Some(sample) = sample.filter(|sample| {
            sample.time.fresh(published.origin)
                && sample.wall >= published.observation.verified_wall
                && sample.wall >= published.observation.wall_floor
        }) else {
            self.advance();
            return None;
        };
        let Some(entry) = published.observation.entry() else {
            return Some(None);
        };
        if sample.wall >= entry.expires_at {
            self.expired = Some(entry.selection);
        }
        Some((self.expired != Some(entry.selection)).then_some(entry))
    }
}

struct Shared {
    actor: u64,
    listener: NonZeroU64,
    view: Mutex<Inner>,
    reads: Semaphore,
}

impl Shared {
    fn stamp(&self, inner: &Inner) -> Stamp {
        Stamp {
            actor: self.actor,
            listener: self.listener,
            sequence: inner.sequence,
        }
    }

    fn stop(&self) {
        self.reads.close();
        if let Ok(mut inner) = self.view.lock() {
            inner.state = State::Stopped;
        }
    }
}

pub(crate) struct Producer(Arc<Shared>);

impl Producer {
    pub(crate) fn read_ticket(&self) -> Option<Ticket> {
        self.begin(false, ClockOrigin::now)
    }

    pub(crate) fn invalidate(&self) {
        if let Ok(mut inner) = self.0.view.lock() {
            inner.advance();
        }
    }

    pub(crate) fn refuse(&self, ticket: Ticket) {
        if let Ok(mut inner) = self.0.view.lock()
            && self.0.stamp(&inner) == ticket.stamp
        {
            inner.advance();
        }
    }

    pub(crate) fn publish_pending(&self, proof: PendingObservation) -> Option<AcceptedTiming> {
        let (ticket, entry, verified_wall, wall_floor) = proof.into_parts();
        self.publish_timed(
            ticket,
            Observation {
                projection: Projection::Pending(entry),
                wall_floor,
                verified_wall,
            },
            Sample::now,
        )
    }

    fn begin(
        &self,
        changes_image: bool,
        origin: impl FnOnce() -> Option<ClockOrigin>,
    ) -> Option<Ticket> {
        let mut inner = self.0.view.lock().ok()?;
        if matches!(inner.state, State::Stopped) || (changes_image && !inner.advance()) {
            return None;
        }
        let Some(origin) = origin() else {
            inner.advance();
            return None;
        };
        Some(Ticket {
            stamp: self.0.stamp(&inner),
            origin,
        })
    }

    #[cfg(test)]
    fn publish(
        &self,
        ticket: Ticket,
        observation: Observation,
        sample: impl FnOnce() -> Option<Sample>,
    ) -> bool {
        self.publish_timed(ticket, observation, sample).is_some()
    }

    fn publish_timed(
        &self,
        ticket: Ticket,
        observation: Observation,
        sample: impl FnOnce() -> Option<Sample>,
    ) -> Option<AcceptedTiming> {
        let mut inner = self.0.view.lock().ok()?;
        if ticket.stamp != self.0.stamp(&inner) || matches!(inner.state, State::Stopped) {
            return None;
        }
        let Some(sample) = sample().filter(|sample| {
            sample.time.fresh(ticket.origin)
                && sample.wall >= observation.verified_wall
                && observation.verified_wall >= observation.wall_floor
        }) else {
            inner.advance();
            return None;
        };
        if let Some(entry) = observation.entry() {
            if entry.selection.generation == 0
                || entry.held_at >= entry.expires_at
                || entry.held_at > observation.verified_wall
            {
                inner.advance();
                return None;
            }
            if sample.wall >= entry.expires_at {
                inner.expired = Some(entry.selection);
            }
            if inner.expired == Some(entry.selection) {
                inner.advance();
                return None;
            }
        }
        if !inner.advance() {
            return None;
        }
        inner.state = State::Live(Published {
            origin: ticket.origin,
            observation,
        });
        Some(AcceptedTiming {
            origin: ticket.origin,
            admission: sample.time,
        })
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[derive(Clone)]
pub(crate) struct Reader(Arc<Shared>);

impl Reader {
    pub(crate) fn admit(&self, listener: NonZeroU64) -> Option<Read<'_>> {
        if listener != self.0.listener || self.0.view.is_poisoned() {
            return None;
        }
        Some(Read {
            shared: &self.0,
            _permit: self.0.reads.try_acquire().ok()?,
        })
    }

    pub(crate) fn stop(&self) {
        self.0.stop();
    }
}

pub(crate) struct Read<'view> {
    shared: &'view Shared,
    _permit: SemaphorePermit<'view>,
}

struct Selected {
    stamp: Stamp,
    entry: Option<Entry>,
    request: Request,
}

impl Read<'_> {
    /// Invoke on the admitted same-UID connection immediately before its write.
    /// The caller retains this permit through the write and uses the same deadline.
    pub(crate) fn reply_before_write(
        &self,
        request: Request,
        listener: NonZeroU64,
        deadline: ExchangeInstant,
    ) -> Option<Reply> {
        if listener != self.shared.listener || ExchangeInstant::now() >= deadline {
            return None;
        }
        let selected = self.select(request, Sample::now)?;
        if ExchangeInstant::now() >= deadline {
            return None;
        }
        self.reply(selected, Sample::now)
    }

    fn select(
        &self,
        request: Request,
        sample: impl FnOnce() -> Option<Sample>,
    ) -> Option<Selected> {
        let mut inner = self.shared.view.lock().ok()?;
        let entry = inner.observe(sample())?;
        if matches!(request, Request::Show { selection } if entry.map(|row| row.selection) != Some(selection))
        {
            return None;
        }
        Some(Selected {
            stamp: self.shared.stamp(&inner),
            entry,
            request,
        })
    }

    fn reply(&self, selected: Selected, sample: impl FnOnce() -> Option<Sample>) -> Option<Reply> {
        let mut inner = self.shared.view.lock().ok()?;
        if self.shared.stamp(&inner) != selected.stamp || inner.observe(sample())? != selected.entry
        {
            return None;
        }
        let entry = selected.entry;
        match selected.request {
            Request::List {} => Some(Reply::List {
                entries: entry.into_iter().collect(),
            }),
            Request::Show { .. } => entry.map(|entry| Reply::Show { entry }),
        }
    }
}

pub(crate) fn pair(listener: NonZeroU64) -> Option<(Producer, Reader)> {
    let actor = NEXT_ACTOR
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            last.checked_add(1)
        })
        .ok()?;
    let shared = Arc::new(Shared {
        actor,
        listener,
        view: Mutex::new(Inner {
            state: State::Invalid,
            sequence: 0,
            expired: None,
        }),
        reads: Semaphore::new(READ_LIMIT),
    });
    Some((Producer(Arc::clone(&shared)), Reader(shared)))
}

#[cfg(test)]
#[path = "peer_pending_read_view_tests.rs"]
mod tests;

#[cfg(test)]
pub(in crate::ipc) use tests::publish_live_fixture;
