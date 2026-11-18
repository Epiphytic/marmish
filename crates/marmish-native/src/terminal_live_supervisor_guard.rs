//! One exact source claim and its suspend-aware native effect fence.
use super::{Claim, GuardResult, Reason, SourceId};
use crate::command_limits::continuous_time;
use crate::terminal_fence::{AttachmentFence, Effect, FenceState};
use std::num::NonZeroU64;
use std::sync::{
    Arc, Mutex, MutexGuard, TryLockError,
    atomic::{AtomicU8, Ordering},
};
use std::time::{Duration, SystemTime};

const IDLE: Duration = Duration::from_secs(11 * 60);
const LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
const DRIFT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub continuous: Duration,
    pub wall: Duration,
}

pub(super) fn sample() -> Option<Sample> {
    Some(Sample {
        continuous: continuous_time().ok()?,
        wall: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?,
    })
}

struct TimeState {
    claim: Claim,
    started: Sample,
    observed: Sample,
    renewed: Duration,
    lifetime: Duration,
}

impl TimeState {
    fn next(&self) -> Option<Duration> {
        Some(self.lifetime.min(self.renewed.checked_add(IDLE)?))
    }

    fn observe(&mut self, at: Sample) -> Option<Reason> {
        if at.continuous < self.observed.continuous || at.wall < self.observed.wall {
            return Some(Reason::Clock);
        }
        let traveled = at.continuous - self.started.continuous;
        let wall = at.wall - self.started.wall;
        if traveled.abs_diff(wall) >= DRIFT {
            return Some(Reason::Clock);
        }
        self.observed = at;
        if at.continuous >= self.next().unwrap_or_default() {
            return Some(if at.continuous >= self.lifetime {
                Reason::Budget
            } else {
                Reason::Idle
            });
        }
        None
    }
}

pub(super) struct Source {
    pub id: SourceId,
    command_generation: NonZeroU64,
    command: Arc<AttachmentFence>,
    stopped: AtomicU8,
    local: Mutex<Option<TimeState>>,
    ceiling: Option<Duration>,
}

impl Source {
    #[cfg(test)]
    pub(super) fn new(id: SourceId, command: Arc<AttachmentFence>) -> Arc<Self> {
        Self::with_ceiling(id, command, None)
    }

    pub(super) fn with_ceiling(
        id: SourceId,
        command: Arc<AttachmentFence>,
        ceiling: Option<Duration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            command_generation: command.generation(),
            command,
            stopped: AtomicU8::new(0),
            local: Mutex::new(None),
            ceiling,
        })
    }

    pub(super) fn check(&self) -> GuardResult<()> {
        if let Some(reason) = self.reason() {
            return GuardResult::Retired(reason);
        }
        if self.command_generation != self.command.generation() {
            return GuardResult::Retired(Reason::Authority);
        }
        match self.command.state() {
            FenceState::Live => GuardResult::Applied(()),
            FenceState::Suspect => GuardResult::Withheld,
            FenceState::Retired => GuardResult::Retired(Reason::Authority),
        }
    }

    pub(super) fn install(&self, claim: Claim, at: Sample) -> GuardResult<()> {
        let mut state = match self.lock() {
            Ok(state) => state,
            Err(result) => return result,
        };
        if state.is_some() {
            return GuardResult::Retired(Reason::Authority);
        }
        match self.check() {
            GuardResult::Applied(()) => {
                let Some(lifetime) = self.ceiling.or_else(|| at.continuous.checked_add(LIFETIME))
                else {
                    self.stop(Reason::Clock);
                    return GuardResult::Retired(Reason::Clock);
                };
                if lifetime <= at.continuous {
                    self.stop(Reason::Budget);
                    return GuardResult::Retired(Reason::Budget);
                }
                *state = Some(TimeState {
                    claim,
                    started: at,
                    observed: at,
                    renewed: at.continuous,
                    lifetime,
                });
                GuardResult::Applied(())
            }
            result => result,
        }
    }

    pub(super) fn reason(&self) -> Option<Reason> {
        Reason::from_code(self.stopped.load(Ordering::Acquire))
    }

    pub(super) fn retire(&self, reason: Reason) {
        // Publish the stop before waiting on the one possible native effect.
        self.stop(reason);
        drop(
            self.local
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        );
    }

    fn stop(&self, reason: Reason) {
        let _ = self
            .stopped
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire);
    }

    fn lock<T>(&self) -> Result<MutexGuard<'_, Option<TimeState>>, GuardResult<T>> {
        match self.local.try_lock() {
            Ok(state) => Ok(state),
            Err(TryLockError::WouldBlock) => Err(GuardResult::Withheld),
            Err(TryLockError::Poisoned(_)) => {
                self.stop(Reason::Authority);
                Err(GuardResult::Retired(Reason::Authority))
            }
        }
    }

    fn verify<T>(
        &self,
        value: &mut Option<TimeState>,
        claim: Claim,
        at: Sample,
    ) -> Result<(), GuardResult<T>> {
        if let Some(reason) = self.reason() {
            return Err(GuardResult::Retired(reason));
        }
        let Some(value) = value.as_mut().filter(|value| value.claim == claim) else {
            return Err(GuardResult::Retired(Reason::Authority));
        };
        if let Some(reason) = value.observe(at) {
            self.stop(reason);
            return Err(GuardResult::Retired(reason));
        }
        Ok(())
    }

    pub(super) fn health(
        &self,
        claim: Claim,
        clock: impl FnOnce() -> Option<Sample>,
    ) -> GuardResult<Option<Duration>> {
        let mut state = match self.lock() {
            Ok(state) => state,
            Err(result) => return result,
        };
        let Some(at) = clock() else {
            self.stop(Reason::Clock);
            return GuardResult::Retired(Reason::Clock);
        };
        if let Err(result) = self.verify(&mut state, claim, at) {
            return result;
        }
        if self.command.state() == FenceState::Retired {
            self.stop(Reason::Authority);
            return GuardResult::Retired(Reason::Authority);
        }
        GuardResult::Applied(state.as_ref().and_then(TimeState::next))
    }

    pub(super) fn renew(
        &self,
        id: SourceId,
        claim: Claim,
        accepted: Duration,
        clock: impl FnOnce() -> Option<Sample>,
    ) -> GuardResult<bool> {
        if id != self.id {
            return GuardResult::Applied(false);
        }
        let mut state = match self.lock() {
            Ok(state) => state,
            Err(result) => return result,
        };
        let Some(at) = clock() else {
            self.stop(Reason::Clock);
            return GuardResult::Retired(Reason::Clock);
        };
        if let Err(result) = self.verify(&mut state, claim, at) {
            return result;
        }
        let Some(state) = state.as_mut() else {
            return GuardResult::Retired(Reason::Authority);
        };
        if accepted > at.continuous || accepted <= state.renewed {
            return GuardResult::Applied(false);
        }
        match self.check() {
            GuardResult::Applied(()) => {
                state.renewed = accepted;
                GuardResult::Applied(true)
            }
            GuardResult::Withheld => GuardResult::Withheld,
            GuardResult::Retired(reason) => GuardResult::Retired(reason),
        }
    }

    pub(super) fn with_effect<T>(
        &self,
        claim: Claim,
        deadline: Duration,
        clock: impl FnOnce() -> Option<Sample>,
        perform: impl FnOnce() -> T,
    ) -> GuardResult<T> {
        let mut state = match self.lock() {
            Ok(state) => state,
            Err(result) => return result,
        };
        match self.command.with_effect(|| {
            let Some(at) = clock() else {
                self.stop(Reason::Clock);
                return GuardResult::Retired(Reason::Clock);
            };
            if let Err(result) = self.verify(&mut state, claim, at) {
                return result;
            }
            if at.continuous >= deadline {
                self.stop(Reason::Deadline);
                return GuardResult::Retired(Reason::Deadline);
            }
            GuardResult::Applied(perform())
        }) {
            Effect::Applied(result) => result,
            Effect::Suspect | Effect::Contended => GuardResult::Withheld,
            Effect::Retired => {
                self.stop(Reason::Authority);
                GuardResult::Retired(Reason::Authority)
            }
        }
    }
}
