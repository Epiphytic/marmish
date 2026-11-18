//! Scheduling state for one exclusive installed Delivery fixture owner.

use std::time::{Duration, Instant};

use crate::peer_invitation::{
    Error,
    socket::wire::Failure,
    store::{ServiceTurn, Turn, consent_checkpoint, pages::Cursor},
};

const RETIRE_DELAYS: [u64; 7] = [1, 2, 4, 8, 16, 32, 60];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Retirement {
    Grammar,
    Size,
    LexicalConflict,
    PageContract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    RetireRelay(Retirement),
    Stop,
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Self {
        match error {
            Error::Socket(Failure::Grammar) => Self::RetireRelay(Retirement::Grammar),
            Error::Socket(Failure::Size) => Self::RetireRelay(Retirement::Size),
            Error::Socket(Failure::ConflictingHistory) => {
                Self::RetireRelay(Retirement::PageContract)
            }
            Error::Checkpoint(consent_checkpoint::Error::ObservedLexicalConflict) => {
                Self::RetireRelay(Retirement::LexicalConflict)
            }
            Error::Checkpoint(consent_checkpoint::Error::ObservedPageConflict) => {
                Self::RetireRelay(Retirement::PageContract)
            }
            // Generic cursor and checkpoint errors can originate locally.
            _ => Self::Stop,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConnectionAction {
    Connect,
    WaitUntil(Instant),
    Idle,
    Stop,
}

enum Connection {
    Offline(Instant),
    Authenticating,
    Qualified,
    Stopped,
}

pub(super) struct Service {
    connection: Connection,
    previous: Instant,
    next_delay: usize,
}

impl Service {
    /// Keep this service with the same owner and origin across socket replacement.
    pub(super) fn new(now: Instant) -> Self {
        Self {
            connection: Connection::Offline(now),
            previous: now,
            next_delay: 0,
        }
    }

    /// Use monotonic samples from the same anchored owner clock for every turn.
    pub(super) fn poll_connect(&mut self, now: Instant) -> ConnectionAction {
        if !self.observe(now) {
            return ConnectionAction::Stop;
        }
        match self.connection {
            Connection::Offline(ready) if now < ready => ConnectionAction::WaitUntil(ready),
            Connection::Offline(_) => {
                self.connection = Connection::Authenticating;
                ConnectionAction::Connect
            }
            Connection::Authenticating | Connection::Qualified => ConnectionAction::Idle,
            Connection::Stopped => ConnectionAction::Stop,
        }
    }

    /// A checked physical port qualified for this owner and origin completed AUTH.
    pub(super) fn authenticated(&mut self, now: Instant) -> bool {
        if !self.observe(now) || !matches!(self.connection, Connection::Authenticating) {
            return false;
        }
        self.connection = Connection::Qualified;
        true
    }

    /// Record an owner-directed disconnect after dropping the current socket.
    /// This preserves prior relay penalties, including any active wait.
    pub(super) fn disconnected(&mut self, now: Instant) {
        if self.observe(now)
            && matches!(
                self.connection,
                Connection::Authenticating | Connection::Qualified
            )
        {
            self.connection = Connection::Offline(now);
        }
    }

    /// The owner must discard the socket on retirement or stop.
    pub(super) fn refuse(&mut self, now: Instant, refusal: Refusal) {
        if !self.observe(now) || refusal == Refusal::Stop {
            self.connection = Connection::Stopped;
            return;
        }
        if matches!(
            self.connection,
            Connection::Authenticating | Connection::Qualified
        ) {
            let delay = Duration::from_secs(RETIRE_DELAYS[self.next_delay]);
            self.connection = now
                .checked_add(delay)
                .map_or(Connection::Stopped, Connection::Offline);
            self.next_delay = (self.next_delay + 1).min(RETIRE_DELAYS.len() - 1);
        }
    }

    pub(super) fn batch(&mut self) -> Option<Batch<'_>> {
        matches!(self.connection, Connection::Qualified).then_some(Batch {
            service: self,
            finite: ServiceTurn::default(),
            live_taken: false,
            history_selected: false,
        })
    }

    fn observe(&mut self, now: Instant) -> bool {
        if now < self.previous {
            self.connection = Connection::Stopped;
            return false;
        }
        self.previous = now;
        !matches!(self.connection, Connection::Stopped)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Work {
    Retry,
    History,
    Live,
    Yield,
}

pub(super) struct Batch<'a> {
    service: &'a mut Service,
    finite: ServiceTurn,
    live_taken: bool,
    history_selected: bool,
}

impl Batch<'_> {
    pub(super) fn choose(&mut self, page_ready: bool, retry_due: bool, live_ready: bool) -> Work {
        self.history_selected = false;
        match self.finite.choose(page_ready, retry_due) {
            Turn::Retry => Work::Retry,
            Turn::Page => {
                self.history_selected = true;
                Work::History
            }
            Turn::Yield if live_ready && !self.live_taken => {
                self.live_taken = true;
                Work::Live
            }
            Turn::Yield => Work::Yield,
        }
    }

    /// Pass the result of the selected page transaction. Successful completion
    /// with no next cursor increments the stored revision and ends the scan.
    pub(super) fn history_result<T, E>(
        &mut self,
        before: &Cursor,
        result: &std::result::Result<(T, Option<Cursor>), E>,
    ) {
        if std::mem::take(&mut self.history_selected)
            && let Ok((_, after)) = result
            && after.as_ref().is_none_or(|after| {
                after.origin == before.origin
                    && before.revision.checked_add(1) == Some(after.revision)
            })
        {
            self.service.next_delay = 0;
        }
    }
}

#[cfg(test)]
#[path = "peer_installed_delivery_service_tests.rs"]
mod tests;
