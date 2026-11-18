//! The native control owner's parked replacement for one exact tmux slot.
//!
//! The owner serializes the physical observations and validates the slot,
//! Source and authority on every turn. Only it can retire a Source or act on
//! an observation. The replacement tracks which observations can advance it.

use super::Claim;
use std::time::Duration;

const OLD_LINKS: usize = 4;
const CONFIRM_WINDOW: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DepartureEpoch {
    Old,
    Successor(Claim),
}

pub(super) enum Departure {
    Empty,
    Occupied,
    TimedOut,
    Uncertain,
}

pub(super) enum Startup {
    Ready { confirm_until: Duration },
    NeverStarted,
    NeedsCleanup,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    RetiringOld,
    VerifyDeparture(DepartureEpoch),
    StartSuccessor,
    LaunchingSuccessor,
    AwaitingConfirm,
    RetiringSuccessor(Claim),
    Detached,
    Quarantine,
    Transferred,
}

enum State {
    RetiringOld,
    VerifyDeparture(DepartureEpoch),
    StartSuccessor,
    Launching(Claim),
    AwaitingConfirm { claim: Claim, until: Duration },
    RetiringSuccessor(Claim),
    Detached,
    Quarantine,
    Transferred,
}

pub(super) struct Replacement {
    old: [Option<Claim>; OLD_LINKS],
    last_old_generation: u64,
    old_writer_nonce: [u8; 16],
    deadline: Duration,
    withdrawn: bool,
    state: State,
}

impl Replacement {
    /// Store this reservation before retiring the old writer and observers.
    /// The caller still owns the existing room and shared Start permits.
    pub(super) fn begin(
        old_writer: Claim,
        linked_observers: &[Claim],
        deadline: Duration,
        now: Duration,
    ) -> Option<Self> {
        if now >= deadline || linked_observers.len() >= OLD_LINKS {
            return None;
        }
        let mut old = [None; OLD_LINKS];
        let mut last_old_generation = 0;
        for (index, claim) in std::iter::once(old_writer)
            .chain(linked_observers.iter().copied())
            .enumerate()
        {
            if !valid_claim(claim)
                || old[..index]
                    .iter()
                    .flatten()
                    .any(|link: &Claim| link.generation == claim.generation)
            {
                return None;
            }
            old[index] = Some(claim);
            last_old_generation = last_old_generation.max(claim.generation);
        }
        Some(Self {
            old,
            last_old_generation,
            old_writer_nonce: old_writer.random,
            deadline,
            withdrawn: false,
            state: State::RetiringOld,
        })
    }

    /// Existing worker accounting, counted once. The old writer unit remains
    /// occupied even after its handle is gone. A successful confirmation
    /// transfers that unit back to the caller's active writer accounting.
    pub(super) fn units(&self) -> usize {
        match self.state {
            State::Detached | State::Quarantine | State::Transferred => 0,
            _ => 1 + self.old[1..].iter().flatten().count(),
        }
    }

    /// Cancellation, lost authority or shutdown can only stop the successor.
    /// Keep the reservation while any exact native cleanup remains outstanding.
    pub(super) fn withdraw(&mut self) {
        self.withdrawn = true;
    }

    pub(super) fn phase(&mut self, now: Duration) -> Phase {
        let expired = now >= self.deadline;
        self.withdrawn |= expired;
        match self.state {
            State::RetiringOld if self.old.iter().all(Option::is_none) => {
                self.state = State::VerifyDeparture(DepartureEpoch::Old);
            }
            State::StartSuccessor if self.withdrawn => self.state = State::Detached,
            State::AwaitingConfirm { claim, until } if self.withdrawn || now >= until => {
                self.withdraw();
                self.state = State::RetiringSuccessor(claim);
            }
            _ => (),
        }
        if expired && matches!(self.state, State::VerifyDeparture(_)) {
            self.state = State::Quarantine;
        }
        match self.state {
            State::RetiringOld => Phase::RetiringOld,
            State::VerifyDeparture(epoch) => Phase::VerifyDeparture(epoch),
            State::StartSuccessor => Phase::StartSuccessor,
            State::Launching(_) => Phase::LaunchingSuccessor,
            State::AwaitingConfirm { .. } => Phase::AwaitingConfirm,
            State::RetiringSuccessor(claim) => Phase::RetiringSuccessor(claim),
            State::Detached => Phase::Detached,
            State::Quarantine => Phase::Quarantine,
            State::Transferred => Phase::Transferred,
        }
    }

    /// Only call after the I/O owner returned the exact handle and its native
    /// shutdown reported Reaped. A delayed or unrelated handle has no effect.
    pub(super) fn reaped(&mut self, claim: Claim) -> bool {
        match self.state {
            State::RetiringOld => {
                let Some(link) = self.old.iter_mut().find(|link| **link == Some(claim)) else {
                    return false;
                };
                *link = None;
            }
            State::AwaitingConfirm { claim: exact, .. } | State::RetiringSuccessor(exact)
                if exact == claim =>
            {
                self.withdraw();
                self.state = State::VerifyDeparture(DepartureEpoch::Successor(claim));
            }
            _ => return false,
        }
        true
    }

    /// Record one checked sample from the current slot and departure epoch.
    /// Failed inner waits never count as a zero-client observation.
    pub(super) fn record_departure(
        &mut self,
        epoch: DepartureEpoch,
        result: Departure,
        now: Duration,
    ) -> bool {
        if self.phase(now) != Phase::VerifyDeparture(epoch) {
            return false;
        }
        self.state = match result {
            Departure::Empty if self.withdrawn => State::Detached,
            Departure::Empty => State::StartSuccessor,
            Departure::Occupied | Departure::TimedOut => State::VerifyDeparture(epoch),
            Departure::Uncertain => State::Quarantine,
        };
        true
    }

    /// Claim this one launch before entering native code. The generation must
    /// come from the same monotonically increasing slot registry.
    pub(super) fn begin_successor(&mut self, claim: Claim, now: Duration) -> bool {
        if self.phase(now) != Phase::StartSuccessor
            || !valid_claim(claim)
            || claim.random == self.old_writer_nonce
            || claim.generation <= self.last_old_generation
        {
            return false;
        }
        self.state = State::Launching(claim);
        true
    }

    /// A late native handle still has to be retired and reaped. NeverStarted
    /// is reserved for proof that native effects could not have started.
    pub(super) fn startup(&mut self, claim: Claim, outcome: Startup, now: Duration) -> bool {
        if !matches!(self.state, State::Launching(exact) if exact == claim) {
            return false;
        }
        let until = match outcome {
            Startup::Ready { confirm_until } => self
                .deadline
                .min(confirm_until)
                .min(now.saturating_add(CONFIRM_WINDOW)),
            Startup::NeverStarted | Startup::NeedsCleanup => now,
        };
        self.state = match outcome {
            Startup::NeverStarted => State::Detached,
            Startup::Ready { .. } if !self.withdrawn && now < until => {
                State::AwaitingConfirm { claim, until }
            }
            _ => {
                self.withdraw();
                State::RetiringSuccessor(claim)
            }
        };
        true
    }

    /// The caller has checked exact Source identity, the paired confirmation
    /// ceilings and native readiness. The callback records the caller's
    /// active transition and sends its acknowledgement in the same owner turn.
    /// That callback is nonblocking and performs no native readiness calls.
    /// False requires fencing that Source before finishing the owner turn.
    pub(super) fn confirm(
        &mut self,
        claim: Claim,
        now: Duration,
        acknowledge: impl FnOnce() -> bool,
    ) -> bool {
        self.phase(now);
        if !matches!(self.state, State::AwaitingConfirm { claim: exact, .. } if exact == claim) {
            return false;
        }

        self.state = State::RetiringSuccessor(claim);
        if !acknowledge() {
            self.withdraw();
            return false;
        }
        self.state = State::Transferred;
        true
    }
}

fn valid_claim(claim: Claim) -> bool {
    claim.generation != 0 && claim.random != [0; 16]
}

#[cfg(test)]
#[path = "terminal_live_takeover_tests.rs"]
mod tests;
