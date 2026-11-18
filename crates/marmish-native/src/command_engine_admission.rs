use crate::dispatcher_fatal::Fatal;
use crate::{Error, Result};
use std::{sync::Arc, thread::JoinHandle};

/// One daemon lifetime's shared latch and bounded, synchronous default VFS check.
/// Installation must pass the exact registration observer that trips under its
/// registry guard. The observer is called without holding any engine lock.
#[derive(Clone)]
pub(crate) struct Admission {
    fatal: Fatal,
    verify: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl Admission {
    #[allow(dead_code, reason = "candidate engine remains unmounted")]
    pub(crate) fn new(fatal: Fatal, verify: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            fatal,
            verify: Arc::new(verify),
        }
    }

    pub(super) fn tripped(&self) -> bool {
        self.fatal.is_tripped()
    }

    pub(super) fn admit(&self) -> bool {
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

    pub(super) fn tripped(&self) -> bool {
        self.checked.as_ref().is_some_and(Admission::tripped)
    }

    pub(super) fn admit(&self) -> bool {
        self.checked.as_ref().is_none_or(Admission::admit)
    }
}
