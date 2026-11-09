//! Same-user live execution. Persisted process identifiers are diagnostics only.
use crate::copy_launch::OwnedCopyContext;
use crate::launcher::{self, ChildSpec};
use crate::model::*;
use crate::process::{ChildOwner, KillCleanup};
use crate::Engine;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::File;
use std::os::fd::{AsRawFd, OwnedFd};

/// Only the engine can construct an authorized execution preparation.
/// This value is intentionally neither Clone nor serializable.
pub struct PreparedLiveExec {
    spec: ChildSpec,
    copy_context: Option<OwnedCopyContext>,
}

/// Diagnostic identity; this record never confers signalling or wait ownership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveProcessRecord {
    pub boot_id: [u8; 16],
    pub pid: i32,
    pub start_ticks: u64,
    pub pgid: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordedProcessDisposition {
    Absent,
    PresentUnowned,
    Unknown,
}

/// Inspect only the recorded leader, never signal a recorded numeric PID/PGID.
/// An absent leader does not establish that escaped descendants are absent.
pub fn probe_recorded_process(record: &LiveProcessRecord) -> RecordedProcessDisposition {
    use RecordedProcessDisposition::*;
    if record.pid <= 0 || record.pgid != record.pid || record.start_ticks == 0 {
        return Unknown;
    }
    let Ok(now) = launcher::boot_time() else {
        return Unknown;
    };
    if now.boot_id != record.boot_id {
        return Absent;
    }
    match launcher::process_identity(record.pid) {
        Ok(identity)
            if identity.boot_id == record.boot_id && identity.start_ticks == record.start_ticks =>
        {
            PresentUnowned
        }
        Ok(_) => Absent,
        Err(_) => {
            // ESRCH is definitive for this leader only. Permission or inspection
            // failures cannot be converted into a safe cleanup claim.
            if unsafe { libc::kill(record.pid, 0) } < 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                Absent
            } else {
                Unknown
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LiveSpawnError {
    #[error("live process not started: {0:?}")]
    NotStarted(LaunchFailure),
    #[error("live process startup uncertain")]
    Uncertain,
}

/// Nonblocking process pipes, transferred exactly once from their owner.
pub struct LivePipes {
    pub stdin: File,
    pub stdout: File,
    pub stderr: File,
}

/// Progress of the live owner's final group-signal obligation. Pending is
/// neither successful signalling nor permission to reap the reserved leader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCleanupProgress {
    Complete,
    Pending,
}

/// A process owner retaining its unreaped leader until final group signalling.
pub struct LiveChild {
    owner: ChildOwner,
    pipes: Option<LivePipes>,
    kill_cleanup: KillCleanup,
}

impl Engine {
    pub fn prepare_live_exec(
        &self,
        ctx: &AuthorizedContext,
        argv: Vec<Vec<u8>>,
        search_path: Option<&OsStr>,
    ) -> Result<PreparedLiveExec> {
        // Check authority before inspecting a remote-selected filesystem path.
        self.owner(ctx)?;
        let request = self.resolve_exec(&CommandRequest::Exec { argv }, search_path)?;
        self.check_execute_request(ctx, &request)?;
        Ok(self.prepared_live(request, None))
    }
    pub(crate) fn prepared_live(
        &self,
        request: CommandRequest,
        copy_context: Option<OwnedCopyContext>,
    ) -> PreparedLiveExec {
        PreparedLiveExec {
            spec: ChildSpec {
                account: self.account.clone(),
                environment: launcher::child_environment(&self.account),
                cwd: self.account.cwd.clone(),
                request,
                umask: 0o077,
                canonical_exec_required: true,
            },
            copy_context,
        }
    }
}

fn nonblocking_file(pipe: impl Into<OwnedFd>) -> Result<File> {
    let file = File::from(pipe.into());
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(EngineError::WorkerUnavailable);
    }
    Ok(file)
}

impl PreparedLiveExec {
    pub fn spawn(self) -> std::result::Result<LiveChild, LiveSpawnError> {
        let worker = launcher::process_identity(std::process::id() as i32)
            .map_err(|_| LiveSpawnError::NotStarted(LaunchFailure::OtherStartupFailure))?;
        if let Some(context) = &self.copy_context {
            context
                .validate_image()
                .map_err(|_| LiveSpawnError::NotStarted(LaunchFailure::OtherStartupFailure))?;
        }
        let locator = self.copy_context.as_ref().map(OwnedCopyContext::locator);
        let spawned =
            launcher::spawn_child_with_context(&self.spec, locator).map_err(
                |error| match error {
                    launcher::ChildSpawnError::NotStarted(reason) => {
                        LiveSpawnError::NotStarted(reason)
                    }
                    launcher::ChildSpawnError::Uncertain => LiveSpawnError::Uncertain,
                },
            )?;
        let owner = ChildOwner {
            child: spawned.child,
            identity: spawned.identity,
            worker,
            pgid: spawned.pgid,
            reaped: false,
        };
        let pipes = (|| {
            Ok::<_, EngineError>(LivePipes {
                stdin: nonblocking_file(spawned.stdin)?,
                stdout: nonblocking_file(spawned.stdout)?,
                stderr: nonblocking_file(spawned.stderr)?,
            })
        })()
        .map_err(|_| LiveSpawnError::Uncertain)?;
        Ok(LiveChild {
            owner,
            pipes: Some(pipes),
            kill_cleanup: KillCleanup::default(),
        })
    }
}

impl LiveChild {
    pub fn take_pipes(&mut self) -> Result<LivePipes> {
        self.pipes.take().ok_or(EngineError::InvalidRequest)
    }
    pub fn identity(&self) -> LiveProcessRecord {
        LiveProcessRecord {
            boot_id: self.owner.identity.boot_id,
            pid: self.owner.identity.pid,
            start_ticks: self.owner.identity.start_ticks,
            pgid: self.owner.pgid,
        }
    }
    pub fn signal_term(&self) -> Result<()> {
        self.owner.signal(libc::SIGTERM)
    }
    pub fn signal_kill(&self) -> Result<()> {
        self.owner.signal(libc::SIGKILL)
    }
    /// Send KILL at most once, then observe an incomplete owned terminal proof.
    /// The caller owns the deadline and requests a final fresh observation.
    pub fn advance_kill_cleanup(&mut self, final_observation: bool) -> Result<LiveCleanupProgress> {
        self.owner
            .advance_kill_cleanup(&mut self.kill_cleanup, final_observation)
    }
    pub fn try_exit(&self) -> Result<bool> {
        if self.owner.reaped {
            return Err(EngineError::RecoveryUncertain);
        }
        self.owner.exited()
    }
    /// Call only after the final required group signal; all future signals fail.
    pub fn reap_last(&mut self) -> Result<ExitOutcome> {
        if self.owner.reaped || !self.kill_cleanup.reap_allowed() {
            return Err(EngineError::RecoveryUncertain);
        }
        self.owner.reap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::time::{Duration, Instant};

    fn prepared(argv: Vec<Vec<u8>>) -> PreparedLiveExec {
        let account = launcher::local_account(None).unwrap();
        let request = crate::resolve::resolve_exec(
            &CommandRequest::Exec { argv },
            Some(OsStr::new("/usr/bin:/bin")),
            &account.cwd,
        )
        .unwrap();
        PreparedLiveExec {
            spec: ChildSpec {
                environment: launcher::child_environment(&account),
                cwd: account.cwd.clone(),
                account,
                request,
                umask: 0o077,
                canonical_exec_required: true,
            },
            copy_context: None,
        }
    }
    fn await_exit(child: &LiveChild) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !child.try_exit().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn live_binary_pipes_eof_identity_and_reap_last() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let mut child = prepared(vec![b"/bin/cat".to_vec()]).spawn().unwrap();
        let record = child.identity();
        assert_eq!(
            probe_recorded_process(&record),
            RecordedProcessDisposition::PresentUnowned
        );
        let mut pipes = child.take_pipes().unwrap();
        assert!(child.take_pipes().is_err());
        let bytes = b"\0\xff$`'\"\\\n";
        pipes.stdin.write_all(bytes).unwrap();
        drop(pipes.stdin);
        await_exit(&child);
        let mut got = Vec::new();
        pipes.stdout.read_to_end(&mut got).unwrap();
        assert_eq!(got, bytes);
        child.signal_kill().unwrap();
        assert_eq!(child.reap_last().unwrap(), ExitOutcome::Code(0));
        assert!(child.signal_kill().is_err());
        assert!(child.signal_term().is_err());
        assert!(child.try_exit().is_err());
        assert!(child.reap_last().is_err());
        assert_eq!(
            probe_recorded_process(&record),
            RecordedProcessDisposition::Absent
        );
    }
    #[test]
    fn live_owned_cancellation_keeps_identity_until_last_reap() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let mut child = prepared(vec![b"/bin/sleep".to_vec(), b"30".to_vec()])
            .spawn()
            .unwrap();
        child.signal_term().unwrap();
        await_exit(&child);
        child.signal_kill().unwrap();
        assert_eq!(
            child.reap_last().unwrap(),
            ExitOutcome::Signal(libc::SIGTERM)
        );
        assert!(child.signal_kill().is_err());
    }
    #[test]
    fn recorded_identity_never_grants_child_ownership() {
        let worker = launcher::process_identity(std::process::id() as i32).unwrap();
        let mut record = LiveProcessRecord {
            boot_id: worker.boot_id,
            pid: worker.pid,
            start_ticks: worker.start_ticks,
            pgid: worker.pid,
        };
        assert_eq!(
            probe_recorded_process(&record),
            RecordedProcessDisposition::PresentUnowned
        );
        record.start_ticks += 1;
        assert_eq!(
            probe_recorded_process(&record),
            RecordedProcessDisposition::Absent
        );
        record.pid = -1;
        assert_eq!(
            probe_recorded_process(&record),
            RecordedProcessDisposition::Unknown
        );
    }
    #[test]
    #[ignore = "exec-only descriptor probe, invoked by multithreaded live-spawn test"]
    fn fd_probe_child() {
        for fd in 3..65_536 {
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) },
                -1,
                "inherited descriptor {fd}"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::EBADF)
            );
        }
    }
    #[test]
    fn multithreaded_live_spawn_inherits_only_stdio() {
        use std::os::fd::AsRawFd;
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
        let other = std::thread::spawn(move || {
            let file = File::open("/dev/null").unwrap();
            assert_eq!(
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) },
                0
            );
            ready_tx.send(()).unwrap();
            done_rx.recv().unwrap();
            drop(file);
        });
        ready_rx.recv().unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut child = prepared(vec![
            executable.as_os_str().as_bytes().to_vec(),
            b"--exact".to_vec(),
            b"live::tests::fd_probe_child".to_vec(),
            b"--ignored".to_vec(),
            b"--nocapture".to_vec(),
            b"--test-threads=1".to_vec(),
        ])
        .spawn()
        .unwrap();
        done_tx.send(()).unwrap();
        other.join().unwrap();
        await_exit(&child);
        child.signal_kill().unwrap();
        let result = child.reap_last().unwrap();
        let mut pipes = child.take_pipes().unwrap();
        let mut stdout = String::new();
        let mut stderr = String::new();
        pipes.stdout.read_to_string(&mut stdout).unwrap();
        pipes.stderr.read_to_string(&mut stderr).unwrap();
        assert_eq!(result, ExitOutcome::Code(0), "{stdout}\n{stderr}");
    }
}

#[cfg(test)]
#[path = "copy_context_tests.rs"]
mod copy_context_tests;
