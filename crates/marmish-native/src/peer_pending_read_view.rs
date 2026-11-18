//! Memory-only local projection. Durable and native proof stay with the actor.
use super::{Entry, Reply, Request, Selection};
use std::{
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, SemaphorePermit};

const LIFETIME: Duration = Duration::from_secs(1);
const READ_LIMIT: usize = 4;
static NEXT_ACTOR: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Eq, PartialEq)]
struct Stamp {
    actor: u64,
    listener: NonZeroU64,
    sequence: u64,
}

struct Ticket {
    stamp: Stamp,
    origin: Instant,
}

#[derive(Clone, Copy)]
struct Sample {
    monotonic: Instant,
    wall: u64,
}

impl Sample {
    fn now() -> Option<Self> {
        Some(Self {
            monotonic: Instant::now(),
            wall: SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs(),
        })
    }

    fn fresh(self, origin: Instant) -> bool {
        self.monotonic
            .checked_duration_since(origin)
            .is_some_and(|elapsed| elapsed < LIFETIME)
    }
}

// The durable bridge will construct this only from its exact fenced readback.
struct Observation {
    entry: Option<Entry>,
    wall_floor: u64,
    verified_wall: u64,
}

struct Published {
    origin: Instant,
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

    fn observe(&mut self, sample: Sample) -> Option<Option<Entry>> {
        let State::Live(published) = &self.state else {
            return None;
        };
        if !sample.fresh(published.origin) || sample.wall < published.observation.wall_floor {
            self.advance();
            return None;
        }
        let Some(entry) = published.observation.entry else {
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

struct Producer(Arc<Shared>);

impl Producer {
    fn begin(&self, changes_image: bool, origin: impl FnOnce() -> Instant) -> Option<Ticket> {
        let mut inner = self.0.view.lock().ok()?;
        if matches!(inner.state, State::Stopped) || (changes_image && !inner.advance()) {
            return None;
        }
        Some(Ticket {
            stamp: self.0.stamp(&inner),
            origin: origin(),
        })
    }

    fn publish(
        &self,
        ticket: Ticket,
        observation: Observation,
        sample: impl FnOnce() -> Option<Sample>,
    ) -> bool {
        let Ok(mut inner) = self.0.view.lock() else {
            return false;
        };
        let Some(sample) = sample() else { return false };
        if ticket.stamp != self.0.stamp(&inner)
            || !sample.fresh(ticket.origin)
            || observation.verified_wall < observation.wall_floor
            || sample.wall < observation.verified_wall
            || observation.entry.is_some_and(|entry| {
                entry.selection.generation == 0
                    || entry.held_at > observation.verified_wall
                    || observation.verified_wall >= entry.expires_at
                    || sample.wall >= entry.expires_at
                    || inner.expired == Some(entry.selection)
            })
            || !inner.advance()
        {
            return false;
        }
        inner.state = State::Live(Published {
            origin: ticket.origin,
            observation,
        });
        true
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[derive(Clone)]
struct Reader(Arc<Shared>);

impl Reader {
    fn admit(&self, listener: NonZeroU64) -> Option<Read<'_>> {
        if listener != self.0.listener || self.0.view.is_poisoned() {
            return None;
        }
        Some(Read {
            shared: &self.0,
            _permit: self.0.reads.try_acquire().ok()?,
        })
    }

    fn stop(&self) {
        self.0.stop();
    }
}

struct Read<'view> {
    shared: &'view Shared,
    _permit: SemaphorePermit<'view>,
}

struct Selected {
    stamp: Stamp,
    entry: Option<Entry>,
    request: Request,
}

impl Read<'_> {
    fn select(
        &self,
        request: Request,
        sample: impl FnOnce() -> Option<Sample>,
    ) -> Option<Selected> {
        let mut inner = self.shared.view.lock().ok()?;
        let entry = inner.observe(sample()?)?;
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
        if self.shared.stamp(&inner) != selected.stamp
            || inner.observe(sample()?)? != selected.entry
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

fn pair(listener: NonZeroU64) -> Option<(Producer, Reader)> {
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
