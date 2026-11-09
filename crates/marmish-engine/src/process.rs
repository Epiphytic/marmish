//! Owned, unreaped child supervision shared by finite and live execution.
#[cfg(target_os = "linux")]
use crate::launcher;
use crate::{live::LiveCleanupProgress, model::*, store::ProcessIdentity};
pub(crate) mod diagnostic;
use diagnostic::{Failure, Origin, Stage, Staged};
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::time::{Duration, Instant};

pub(crate) enum CleanupObservation {
    Complete,
    #[cfg(target_os = "macos")]
    Pending(Failure),
}
impl CleanupObservation {
    pub(crate) fn strict(self) -> Staged<()> {
        match self {
            Self::Complete => Ok(()),
            #[cfg(target_os = "macos")]
            Self::Pending(failure) => Err(failure),
        }
    }
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KillCleanup {
    #[default]
    NotStarted,
    Pending,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupAction {
    Start,
    Observe,
    Ready,
    Reject,
}
impl KillCleanup {
    fn action(self) -> CleanupAction {
        match self {
            Self::NotStarted => CleanupAction::Start,
            Self::Pending => CleanupAction::Observe,
            Self::Complete => CleanupAction::Ready,
            Self::Failed => CleanupAction::Reject,
        }
    }
    fn record(&mut self, result: &Staged<LiveCleanupProgress>) {
        *self = match result {
            Ok(LiveCleanupProgress::Complete) => Self::Complete,
            Ok(LiveCleanupProgress::Pending) => Self::Pending,
            Err(_) => Self::Failed,
        };
    }
    pub(crate) fn reap_allowed(self) -> bool {
        matches!(self, Self::NotStarted | Self::Complete)
    }
    fn resolve(
        &mut self,
        result: Staged<CleanupObservation>,
        final_observation: bool,
    ) -> Staged<LiveCleanupProgress> {
        let result = resolve_cleanup(result, final_observation);
        self.record(&result);
        result
    }
}
fn resolve_cleanup(
    result: Staged<CleanupObservation>,
    _final_observation: bool,
) -> Staged<LiveCleanupProgress> {
    match result? {
        CleanupObservation::Complete => Ok(LiveCleanupProgress::Complete),
        #[cfg(target_os = "macos")]
        CleanupObservation::Pending(failure) if _final_observation => Err(failure),
        #[cfg(target_os = "macos")]
        CleanupObservation::Pending(_) => Ok(LiveCleanupProgress::Pending),
    }
}

/// The leader remains unreaped until the last group signal. Its reserved PID
/// prevents PGID reuse. Descendants deliberately calling setsid can escape;
/// Phase 1 is same-user execution, not a hostile-process containment sandbox.
pub(crate) struct ChildOwner {
    pub(crate) child: Child,
    pub(crate) identity: ProcessIdentity,
    pub(crate) worker: ProcessIdentity,
    pub(crate) pgid: i32,
    pub(crate) reaped: bool,
}
impl ChildOwner {
    pub(crate) fn signal(&self, signal: i32) -> Result<()> {
        let origin = match signal {
            libc::SIGTERM => Origin::Term,
            libc::SIGKILL => Origin::Kill,
            _ => Origin::Other,
        };
        self.signal_with(signal, origin)
    }
    fn signal_with(&self, signal: i32, origin: Origin) -> Result<()> {
        let result = self
            .signal_staged(signal)
            .and_then(CleanupObservation::strict);
        diagnostic::emit(&result, origin);
        result.map_err(|failure| failure.error)
    }
    fn validate_signal_owner_staged(&self) -> Staged<()> {
        let uncertain = |stage| Failure::new(EngineError::RecoveryUncertain, stage, None);
        if self.reaped {
            return Err(uncertain(Stage::Reaped));
        }
        if self.child.id() != self.identity.pid as u32 {
            return Err(uncertain(Stage::ChildIdentity));
        }
        #[cfg(target_os = "linux")]
        let worker = launcher::process_identity(self.worker.pid)
            .map_err(|error| Failure::new(error, Stage::WorkerLookup, None))?;
        #[cfg(target_os = "macos")]
        let worker = crate::platform::process_identity_staged(self.worker.pid)
            .map_err(|failure| failure.context(diagnostic::Context::Worker))?;
        if worker != self.worker {
            return Err(uncertain(Stage::WorkerIdentity));
        }
        if self.pgid != self.identity.pid {
            return Err(uncertain(Stage::GroupIdentity));
        }
        #[cfg(target_os = "linux")]
        if launcher::process_identity(self.identity.pid)
            .map_err(|error| Failure::new(error, Stage::LeaderIdentity, None))?
            != self.identity
            || unsafe { libc::getpgid(self.identity.pid) } != self.pgid
        {
            return Err(uncertain(Stage::LeaderIdentity));
        }
        #[cfg(target_os = "macos")]
        if crate::platform::owned_child_identity_and_group_staged(self.identity.pid)
            .map_err(|failure| failure.context(diagnostic::Context::Leader))?
            != (self.identity.clone(), self.pgid)
        {
            return Err(uncertain(Stage::LeaderIdentity));
        }
        Ok(())
    }
    fn signal_staged(&self, signal: i32) -> Staged<CleanupObservation> {
        self.validate_signal_owner_staged()?;
        if unsafe { libc::kill(-self.pgid, signal) } < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error();
            #[cfg(target_os = "macos")]
            if errno == Some(libc::EPERM) {
                return crate::platform::observe_owned_zombie_group_staged(
                    &self.identity,
                    self.pgid,
                );
            }
            if errno != Some(libc::ESRCH) {
                return Err(Failure::new(
                    EngineError::RecoveryUncertain,
                    Stage::Kill,
                    errno,
                ));
            }
        }
        Ok(CleanupObservation::Complete)
    }
    pub(crate) fn advance_kill_cleanup(
        &self,
        state: &mut KillCleanup,
        final_observation: bool,
    ) -> Result<LiveCleanupProgress> {
        if state.action() == CleanupAction::Reject {
            return Err(EngineError::RecoveryUncertain);
        }
        let result = if self.reaped {
            Err(Failure::new(
                EngineError::RecoveryUncertain,
                Stage::Reaped,
                None,
            ))
        } else {
            match state.action() {
                CleanupAction::Start => self.signal_staged(libc::SIGKILL),
                CleanupAction::Observe => self.observe_cleanup_staged(),
                CleanupAction::Ready => return Ok(LiveCleanupProgress::Complete),
                CleanupAction::Reject => unreachable!("failed state rejected above"),
            }
        };
        let result = state.resolve(result, final_observation);
        diagnostic::emit(&result, Origin::Kill);
        result.map_err(|failure| failure.error)
    }

    fn observe_cleanup_staged(&self) -> Staged<CleanupObservation> {
        self.validate_signal_owner_staged()?;
        #[cfg(target_os = "macos")]
        {
            crate::platform::observe_owned_zombie_group_staged(&self.identity, self.pgid)
        }
        #[cfg(target_os = "linux")]
        {
            Err(Failure::new(
                EngineError::RecoveryUncertain,
                Stage::Kill,
                None,
            ))
        }
    }
    pub(crate) fn exited(&self) -> Result<bool> {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                self.identity.pid as u32,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } < 0
        {
            return Err(EngineError::RecoveryUncertain);
        }
        Ok(unsafe { info.si_pid() } != 0)
    }
    pub(crate) fn reap(&mut self) -> Result<ExitOutcome> {
        if !self.exited()? {
            return Err(EngineError::RecoveryUncertain);
        }
        let status = self
            .child
            .wait()
            .map_err(|_| EngineError::RecoveryUncertain)?;
        self.reaped = true;
        status
            .code()
            .map(ExitOutcome::Code)
            .or_else(|| status.signal().map(ExitOutcome::Signal))
            .ok_or(EngineError::RecoveryUncertain)
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if self.signal_with(libc::SIGKILL, Origin::DropKill).is_err() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.exited().unwrap_or(false) {
                let _ = self.reap();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // An uninterruptible kernel wait must not hang supervisor shutdown.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_owned_cleanup_state_actions() {
        let mut state = KillCleanup::default();
        assert_eq!(state.action(), CleanupAction::Start);
        state.record(&Ok(LiveCleanupProgress::Pending));
        assert_eq!(state.action(), CleanupAction::Observe);
        state.record(&Ok(LiveCleanupProgress::Pending));
        assert_eq!(state.action(), CleanupAction::Observe);
        assert!(matches!(
            state.resolve(Ok(CleanupObservation::Complete), false),
            Ok(LiveCleanupProgress::Complete)
        ));
        assert_eq!(state.action(), CleanupAction::Ready);
        // Both start and poll errors become sticky failure, never NotStarted.
        for initial in [KillCleanup::NotStarted, KillCleanup::Pending] {
            state = initial;
            let failure = Failure::new(EngineError::RecoveryUncertain, Stage::Kill, None);
            assert!(state.resolve(Err(failure), false).is_err());
            assert_eq!(state.action(), CleanupAction::Reject);
            assert_eq!(state.action(), CleanupAction::Reject);
            assert!(!state.reap_allowed());
        }
    }
    #[test]
    fn pending_owned_cleanup_reap_guard() {
        assert!(KillCleanup::NotStarted.reap_allowed());
        assert!(KillCleanup::Complete.reap_allowed());
        assert!(!KillCleanup::Pending.reap_allowed());
        assert!(!KillCleanup::Failed.reap_allowed());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn pending_owned_cleanup_resolver_preserves_failure() {
        use diagnostic::{Context, DarwinStage};
        let failure = || {
            Failure::darwin(
                EngineError::RecoveryUncertain,
                DarwinStage::ZombieStatus,
                None,
            )
            .context(Context::ProofFirstMembers)
        };
        assert!(matches!(
            resolve_cleanup(Ok(CleanupObservation::Complete), true),
            Ok(LiveCleanupProgress::Complete)
        ));
        let mut state = KillCleanup::NotStarted;
        assert!(matches!(
            state.resolve(Ok(CleanupObservation::Pending(failure())), false),
            Ok(LiveCleanupProgress::Pending)
        ));
        assert_eq!(state, KillCleanup::Pending);
        let final_failure = state
            .resolve(Ok(CleanupObservation::Pending(failure())), true)
            .expect_err("final pending must fail");
        assert_eq!(state, KillCleanup::Failed);
        assert!(matches!(
            final_failure.error,
            EngineError::RecoveryUncertain
        ));
        #[cfg(debug_assertions)]
        assert_eq!(final_failure.diagnostic_detail().0, "darwin-zombie-status");
        assert!(CleanupObservation::Pending(failure()).strict().is_err());
        assert!(resolve_cleanup(Err(failure()), false).is_err());
    }
}
