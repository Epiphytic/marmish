//! Owned, unreaped child supervision shared by finite and live execution.
#[cfg(target_os = "linux")]
use crate::launcher;
use crate::{model::*, store::ProcessIdentity};
pub(crate) mod diagnostic;
use diagnostic::{Failure, Origin, Stage, Staged};
use std::os::unix::process::ExitStatusExt;
use std::process::Child;
use std::time::{Duration, Instant};

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
        let result = self.signal_staged(signal);
        diagnostic::emit(&result, origin);
        result.map_err(|failure| failure.error)
    }
    fn signal_staged(&self, signal: i32) -> Staged<()> {
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
        if unsafe { libc::kill(-self.pgid, signal) } < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error();
            #[cfg(target_os = "macos")]
            if errno == Some(libc::EPERM) {
                return crate::platform::owned_zombie_group_staged(&self.identity, self.pgid);
            }
            if errno != Some(libc::ESRCH) {
                return Err(Failure::new(
                    EngineError::RecoveryUncertain,
                    Stage::Kill,
                    errno,
                ));
            }
        }
        Ok(())
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
