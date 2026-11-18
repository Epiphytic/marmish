//! Serialized authority changes for nonblocking terminal effects.
use std::num::NonZeroU64;
use std::sync::{
    Arc, Mutex, TryLockError,
    atomic::{AtomicU8, Ordering},
};

const CLEAR: u8 = 0;
const UPDATE: u8 = 1;
const RETIRE: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FenceState {
    Live,
    Suspect,
    Retired,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Effect<T> {
    Applied(T),
    Suspect,
    Contended,
    Retired,
}

pub(crate) struct AttachmentFence {
    generation: NonZeroU64,
    digest: [u8; 32],
    stopped: AtomicU8,
    state: Mutex<FenceState>,
}

#[must_use]
pub(crate) struct UpdateToken(Option<Arc<AttachmentFence>>);

impl AttachmentFence {
    pub(crate) fn new(generation: NonZeroU64, digest: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            generation,
            digest,
            stopped: AtomicU8::new(CLEAR),
            state: Mutex::new(FenceState::Live),
        })
    }

    pub(crate) fn generation(&self) -> NonZeroU64 {
        self.generation
    }

    pub(crate) fn state(&self) -> FenceState {
        match self.stopped.load(Ordering::Acquire) {
            RETIRE => FenceState::Retired,
            UPDATE => FenceState::Suspect,
            _ => match self.state.try_lock() {
                Ok(state) => match self.stopped.load(Ordering::Acquire) {
                    RETIRE => FenceState::Retired,
                    UPDATE => FenceState::Suspect,
                    _ => *state,
                },
                Err(TryLockError::Poisoned(_)) => FenceState::Retired,
                Err(TryLockError::WouldBlock) => FenceState::Suspect,
            },
        }
    }

    /// The caller supplies at most one nonblocking syscall. The Command owner
    /// may wait for that call while new effects see the stop flag.
    pub(crate) fn with_effect<T>(&self, effect: impl FnOnce() -> T) -> Effect<T> {
        match self.stopped.load(Ordering::Acquire) {
            RETIRE => return Effect::Retired,
            UPDATE => return Effect::Suspect,
            _ => {}
        }
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(_)) => return Effect::Retired,
            Err(TryLockError::WouldBlock) => {
                return match self.stopped.load(Ordering::Acquire) {
                    RETIRE => Effect::Retired,
                    UPDATE => Effect::Suspect,
                    _ => Effect::Contended,
                };
            }
        };
        match (self.stopped.load(Ordering::Acquire), *state) {
            (RETIRE, _) | (_, FenceState::Retired) => Effect::Retired,
            (UPDATE, _) | (_, FenceState::Suspect) => Effect::Suspect,
            _ => Effect::Applied(effect()),
        }
    }

    pub(crate) fn begin_update(self: &Arc<Self>) -> Option<UpdateToken> {
        self.stopped
            .compare_exchange(CLEAR, UPDATE, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.stopped.store(RETIRE, Ordering::Release);
                *poisoned.into_inner() = FenceState::Retired;
                return None;
            }
        };
        if *state != FenceState::Live || self.stopped.load(Ordering::Acquire) != UPDATE {
            self.stopped.store(RETIRE, Ordering::Release);
            *state = FenceState::Retired;
            return None;
        }
        *state = FenceState::Suspect;
        Some(UpdateToken(Some(Arc::clone(self))))
    }

    pub(crate) fn retire(&self) {
        self.stopped.store(RETIRE, Ordering::Release);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        *state = FenceState::Retired;
    }
}

impl UpdateToken {
    pub(crate) fn resolve(mut self, verified_digest: Option<[u8; 32]>) -> FenceState {
        let fence = self.0.take().expect("update token owns a fence");
        if verified_digest == Some(fence.digest)
            && let Ok(mut state) = fence.state.lock()
            && *state == FenceState::Suspect
            && fence
                .stopped
                .compare_exchange(UPDATE, CLEAR, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            *state = FenceState::Live;
            FenceState::Live
        } else {
            fence.retire();
            FenceState::Retired
        }
    }
}

impl Drop for UpdateToken {
    fn drop(&mut self) {
        if let Some(fence) = self.0.take() {
            fence.retire();
        }
    }
}

#[path = "terminal_fence_tests.rs"]
mod tests;
