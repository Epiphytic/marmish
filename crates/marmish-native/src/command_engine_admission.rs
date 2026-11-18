use crate::dispatcher_fatal::Fatal;
#[cfg(test)]
use crate::dispatcher_fatal::Trip;
use crate::peer_relay_trust::candidate_registration::RegisteredCommandFence;
use crate::{Error, Result};
use std::{sync::Arc, thread::JoinHandle};

/// One daemon lifetime's shared latch and bounded, synchronous default VFS check.
/// Installation must pass the exact registration observer that trips under its
/// registry guard. The observer is called without holding any engine lock and
/// must never panic, including when called during worker unwinding.
#[derive(Clone)]
pub(crate) struct Admission {
    fatal: Fatal,
    verify: Arc<dyn Fn() -> bool + Send + Sync>,
    owner_failed: Arc<dyn Fn() + Send + Sync>,
}

impl Admission {
    #[allow(dead_code, reason = "candidate engine remains unmounted")]
    pub(crate) fn from_registered(fence: RegisteredCommandFence) -> Self {
        let (fatal, forwarding, custody) = fence.into_parts();
        Self {
            fatal,
            verify: Arc::new(move || forwarding.recheck_default().is_ok()),
            owner_failed: Arc::new(move || custody.report()),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(trip: &Arc<Trip>, verify: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        let fatal = trip.observe();
        let trip = Arc::clone(trip);
        Self {
            fatal,
            verify: Arc::new(verify),
            owner_failed: Arc::new(move || trip.notify()),
        }
    }

    pub(super) fn tripped(&self) -> bool {
        self.fatal.is_tripped()
    }

    pub(crate) fn admit(&self) -> bool {
        !self.tripped() && (self.verify)() && !self.tripped()
    }
}

/// Extracted only after the exact engine's channel endpoints are disconnected.
pub(crate) struct FatalEngineJoin {
    worker: Option<JoinHandle<()>>,
    failed: bool,
}

#[allow(dead_code, reason = "candidate engine remains unmounted")]
impl FatalEngineJoin {
    pub(super) fn new(worker: Option<JoinHandle<()>>, failed: bool) -> Self {
        Self { worker, failed }
    }

    pub(crate) fn wait(self) -> Result<()> {
        let panicked = self.worker.is_some_and(|worker| worker.join().is_err());
        if panicked || self.failed {
            Err(Error::Storage("engine_thread_failed"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct Mode {
    pub(super) checked: Option<Admission>,
}

impl Mode {
    pub(super) fn candidate(&self) -> bool {
        self.checked.is_some()
    }

    pub(super) fn connection_fence(&self) -> Option<marmish_engine::ConnectionFence> {
        self.checked.as_ref().map(|checked| {
            let verify = checked.clone();
            let owner_failed = checked.owner_failed.clone();
            marmish_engine::ConnectionFence::new(move || verify.admit(), move || owner_failed())
        })
    }

    pub(super) fn tripped(&self) -> bool {
        self.checked.as_ref().is_some_and(Admission::tripped)
    }

    pub(super) fn admit(&self) -> bool {
        self.checked.as_ref().is_none_or(Admission::admit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_poison_notifies_only_the_original_admission_and_fatal() {
        let (trip, fatal) = Trip::new();
        let trip = Arc::new(trip);
        let (_, unrelated) = Trip::new();
        let admission = Admission::new(&trip, || true);
        assert!(admission.admit());
        (admission.owner_failed)();
        assert!(!admission.admit());
        assert!(fatal.is_tripped());
        assert!(!unrelated.is_tripped());
    }
}
