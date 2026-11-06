//! Owned, unreaped child supervision shared by finite and live execution.
use crate::{launcher, model::*, store::ProcessIdentity};
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
        if self.reaped
            || self.child.id() != self.identity.pid as u32
            || launcher::process_identity(self.worker.pid)? != self.worker
            || self.pgid != self.identity.pid
        {
            return Err(EngineError::RecoveryUncertain);
        }
        #[cfg(target_os = "linux")]
        if launcher::process_identity(self.identity.pid)? != self.identity
            || unsafe { libc::getpgid(self.identity.pid) } != self.pgid
        {
            return Err(EngineError::RecoveryUncertain);
        }
        #[cfg(target_os = "macos")]
        if crate::platform::owned_child_identity_and_group(self.identity.pid)?
            != (self.identity.clone(), self.pgid)
        {
            return Err(EngineError::RecoveryUncertain);
        }
        if unsafe { libc::kill(-self.pgid, signal) } < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error();
            #[cfg(target_os = "macos")]
            if errno == Some(libc::EPERM) {
                return crate::platform::owned_zombie_group(&self.identity, self.pgid);
            }
            if errno != Some(libc::ESRCH) {
                return Err(EngineError::RecoveryUncertain);
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
        if self.signal(libc::SIGKILL).is_err() {
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
