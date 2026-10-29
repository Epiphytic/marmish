//! Local consuming quiescence proof for owned fixture state.
use crate::launcher::{FrontendGuard, IoGuard, JobGuard, StateRoot};
use crate::{platform, store::Store, EngineError, InvocationState, JobId, Result};
use std::collections::BTreeSet;
use std::time::Duration;

/// Holds the private frontend and every existing job lock through state removal.
/// This guard contains no open SQLCipher connection or storage encryption keys.
pub struct CleanupGuard {
    _root: StateRoot,
    _frontend: FrontendGuard,
    _locks: Vec<(JobGuard, IoGuard)>,
    migration: Option<(crate::NativeBinding, [u8; 32])>,
}
impl CleanupGuard {
    pub(crate) fn new(
        root: StateRoot,
        frontend: FrontendGuard,
        locks: Vec<(JobGuard, IoGuard)>,
    ) -> Self {
        Self {
            _root: root,
            _frontend: frontend,
            _locks: locks,
            migration: None,
        }
    }
    pub(crate) fn migration(
        root: StateRoot,
        frontend: FrontendGuard,
        locks: Vec<(JobGuard, IoGuard)>,
        binding: crate::NativeBinding,
        fingerprint: [u8; 32],
    ) -> Self {
        let mut guard = Self::new(root, frontend, locks);
        guard.migration = Some((binding, fingerprint));
        guard
    }
    pub fn commit_native(&mut self) -> Result<()> {
        let (binding, fingerprint) = self.migration.as_ref().ok_or(EngineError::InvalidRequest)?;
        self._root
            .commit_native(&self._frontend, binding, fingerprint)
    }
}

fn terminal(store: &mut Store, jobs: &[JobId]) -> Result<()> {
    for job in jobs {
        if store.job_state(*job)? != InvocationState::Exited {
            return Err(EngineError::RecoveryUncertain);
        }
        if let Some(recorded) = store.worker_identity(*job)? {
            match platform::process_identity(recorded.pid) {
                Ok(actual) if crate::launcher::same_identity(&recorded, &actual) => {
                    return Err(EngineError::Busy)
                }
                Ok(_) | Err(EngineError::WorkerUnavailable) => (),
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

pub(crate) fn acquire(root: &StateRoot, store: &mut Store) -> Result<Vec<(JobGuard, IoGuard)>> {
    let deadline = platform::Deadline::after_millis(5_000)?;
    loop {
        let attempt = (|| {
            if deadline.expired()? {
                return Err(EngineError::Busy);
            }
            let jobs = store.cleanup_jobs()?;
            let recorded: BTreeSet<_> = jobs.iter().map(|job| job.0).collect();
            let directories = root.cleanup_directories()?;
            if !directories.is_subset(&recorded) {
                return Err(EngineError::RecoveryUncertain);
            }
            platform::cleanup_descriptor_budget(directories.len())?;
            terminal(store, &jobs)?;
            let mut locks = Vec::with_capacity(directories.len());
            for id in &directories {
                if deadline.expired()? {
                    return Err(EngineError::Busy);
                }
                let worker = root.lock_job(JobId(*id))?;
                let io = root.lock_io(JobId(*id))?;
                locks.push((worker, io));
            }
            terminal(store, &jobs)?;
            if deadline.expired()? {
                return Err(EngineError::Busy);
            }
            Ok(locks)
        })();
        match attempt {
            Err(EngineError::Busy) if !deadline.expired()? => {
                std::thread::sleep(Duration::from_millis(25))
            }
            other => return other,
        }
    }
}
