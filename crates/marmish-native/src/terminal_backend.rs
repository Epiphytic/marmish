//! Local checked attachment primitives for the future authenticated supervisor.
#[path = "terminal_backend_os.rs"]
mod os;
#[path = "terminal_backend_paths.rs"]
mod paths;
#[cfg(test)]
#[path = "terminal_supervisor.rs"]
mod supervisor;

use crate::{Error, Result};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const WORKER_ENTRYPOINT: &str = "_terminal-worker";
const LAUNCH: Error = Error::Invalid("terminal_worker_launch");
const IO: Error = Error::Invalid("terminal_attachment_io");
const READ_ONLY: Error = Error::Invalid("terminal_attachment_read_only");
const STALE: Error = Error::Invalid("terminal_attachment_generation");
const SESSION: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;
const EXEC_FAILED: u8 = 4;
const STATUS_DEADLINE: Duration = Duration::from_secs(3);
const EXEC_GRACE: Duration = Duration::from_millis(50);
const CHILD_POLL: Duration = Duration::from_millis(10);
const TERMINATE_GRACE: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Writer,
    Observer,
}

impl Role {
    fn argument(self) -> &'static str {
        match self {
            Self::Writer => "writer",
            Self::Observer => "observer",
        }
    }

    fn from_argument(arg: &std::ffi::OsStr) -> Result<Self> {
        match arg.to_str() {
            Some("writer") => Ok(Self::Writer),
            Some("observer") => Ok(Self::Observer),
            _ => Err(LAUNCH),
        }
    }
}

/// Verified paths and exact session name for one local attach operation.
pub struct CheckedLaunch {
    daemon: PathBuf,
    tmux: PathBuf,
    socket: PathBuf,
    session: String,
    role: Role,
}

struct LaunchFailure {
    error: Error,
    pending: Option<Attachment>,
}

impl From<Error> for LaunchFailure {
    fn from(error: Error) -> Self {
        Self {
            error,
            pending: None,
        }
    }
}

impl CheckedLaunch {
    pub fn new(daemon: &Path, tmux: &Path, socket: &Path, session: &str) -> Result<Self> {
        Self::for_role(daemon, tmux, socket, session, Role::Writer)
    }

    fn for_role(
        daemon: &Path,
        tmux: &Path,
        socket: &Path,
        session: &str,
        role: Role,
    ) -> Result<Self> {
        let uid = paths::checked_uid()?;
        paths::daemon(daemon)?;
        let tmux = paths::tmux(tmux, uid)?;
        paths::socket(socket, uid)?;
        paths::session(session)?;
        Ok(Self {
            daemon: daemon.to_owned(),
            tmux,
            socket: socket.to_owned(),
            session: session.into(),
            role,
        })
    }

    pub fn launch(&self, generation: u64, rows: u16, cols: u16) -> Result<Attachment> {
        self.launch_owned(generation, rows, cols)
            .map_err(|failure| {
                drop(failure.pending);
                failure.error
            })
    }

    fn launch_owned(
        &self,
        generation: u64,
        rows: u16,
        cols: u16,
    ) -> std::result::Result<Attachment, LaunchFailure> {
        if generation == 0 {
            return Err(STALE.into());
        }
        let uid = paths::checked_uid()?;
        paths::daemon(&self.daemon)?;
        if paths::tmux(&self.tmux, uid)? != self.tmux {
            return Err(LAUNCH.into());
        }
        paths::socket(&self.socket, uid)?;
        let (master, slave) = os::pty(uid)?;
        os::resize(master.as_raw_fd(), rows, cols)?;
        let (reader, writer) = os::status_pipe()?;
        let null = OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .map_err(|_| LAUNCH)?;
        let reaper = reserve_reaper()?;
        let child = Command::new(&self.daemon)
            .env_clear()
            .arg(WORKER_ENTRYPOINT)
            .arg(self.role.argument())
            .arg(&self.tmux)
            .arg(&self.socket)
            .arg(&self.session)
            .stdin(slave)
            .stdout(writer)
            .stderr(null)
            .spawn()
            .map_err(|_| LAUNCH)?;
        let mut attachment = Attachment {
            master: Some(master),
            generation,
            role: self.role,
            child: Some(ChildState::new(child)),
            reaper,
        };
        let child = attachment.child.as_mut().ok_or(LAUNCH)?;
        if let Err(error) = await_status(reader, child, Instant::now() + STATUS_DEADLINE)
            .and_then(|()| exec_grace(child, Instant::now() + EXEC_GRACE))
        {
            return Err(LaunchFailure {
                error,
                pending: Some(attachment),
            });
        }
        Ok(attachment)
    }
}

pub(crate) fn worker_entry(args: &[OsString]) -> ! {
    std::panic::set_hook(Box::new(|_| worker_exit()));
    let Ok(uid) = paths::checked_uid() else {
        worker_exit()
    };
    if os::worker_endpoints(uid).is_err() {
        worker_exit();
    }
    let stage = |status| report_status(1, status);
    let Ok(mut command) = worker_command(args, uid) else {
        stage(FAILED);
        worker_exit()
    };
    if os::reset_signals().is_err() {
        stage(FAILED);
        worker_exit();
    }
    let Ok(writer) = os::duplicate_status() else {
        stage(FAILED);
        worker_exit()
    };
    if os::close_unrelated(writer)
        .and_then(|()| os::map_slave())
        .and_then(|()| os::controlling_terminal())
        .is_err()
    {
        report_status(writer, FAILED);
        worker_exit();
    }
    report_status(writer, SESSION);
    report_status(writer, READY);
    let _ = command.exec();
    report_status(writer, EXEC_FAILED);
    worker_exit()
}

fn worker_command(args: &[OsString], uid: libc::uid_t) -> Result<Command> {
    crate::validate_executable_layout()?;
    paths::daemon(&std::env::current_exe().map_err(|_| LAUNCH)?)?;
    let [role, tmux, socket, session] = args else {
        return Err(LAUNCH);
    };
    let role = Role::from_argument(role)?;
    let tmux = Path::new(tmux);
    let socket = Path::new(socket);
    let session = session.to_str().ok_or(LAUNCH)?;
    if paths::tmux(tmux, uid)? != tmux {
        return Err(LAUNCH);
    }
    paths::socket(socket, uid)?;
    paths::session(session)?;
    let user = paths::user(uid)?;
    let locale = if cfg!(target_os = "macos") {
        "en_US.UTF-8"
    } else {
        "C.UTF-8"
    };
    let mut command = Command::new(tmux);
    command
        .args(["-u", "-N", "-S"])
        .arg(socket)
        .args(["-f", "/dev/null", "attach-session", "-E"]);
    if role == Role::Observer {
        command.args(["-f", "read-only,ignore-size"]);
    }
    command
        .args(["-t", &format!("={session}")])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", user.home)
        .env("USER", user.name)
        .env("TERM", "xterm-256color")
        .env("LC_ALL", locale);
    Ok(command)
}

fn report_status(fd: RawFd, stage: u8) {
    loop {
        // SAFETY: the worker has validated its dedicated writer and supplies one byte.
        let result = unsafe { libc::write(fd, (&raw const stage).cast(), 1) };
        if result == 1 {
            return;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            worker_exit();
        }
    }
}

fn worker_exit() -> ! {
    // SAFETY: this is the dedicated executable worker, with no caller work to unwind.
    unsafe { libc::_exit(125) }
}

fn await_status(mut pipe: File, child: &mut ChildState, deadline: Instant) -> Result<()> {
    let mut next = SESSION;
    let mut bytes = [0; 8];
    while Instant::now() < deadline {
        match pipe.read(&mut bytes) {
            Ok(0) if next == FAILED => return Ok(()),
            Ok(0) => return Err(LAUNCH),
            Ok(count) => {
                for stage in &bytes[..count] {
                    if *stage != next || next >= FAILED {
                        return Err(LAUNCH);
                    }
                    if *stage == SESSION {
                        child.claim_group()?;
                    }
                    next += 1;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut fd = poll_descriptor(pipe.as_raw_fd(), libc::POLLIN);
                poll_fds(std::slice::from_mut(&mut fd), deadline).map_err(|_| LAUNCH)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(LAUNCH),
        }
    }
    Err(LAUNCH)
}

fn exec_grace(child: &ChildState, deadline: Instant) -> Result<()> {
    loop {
        if child.waitable().map_err(|_| LAUNCH)? {
            return Err(LAUNCH);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::sleep(remaining.min(CHILD_POLL));
    }
}

fn poll_descriptor(fd: RawFd, events: libc::c_short) -> libc::pollfd {
    libc::pollfd {
        fd,
        events,
        revents: 0,
    }
}

fn poll_fds(descriptors: &mut [libc::pollfd], deadline: Instant) -> Result<()> {
    loop {
        let delay = deadline.saturating_duration_since(Instant::now());
        let timeout = delay.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32;
        // SAFETY: poll receives a writable array of the specified size.
        if unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                timeout,
            )
        } >= 0
        {
            return Ok(());
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(IO);
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
    }
}

/// Local terminal event flags. Child exit can accompany pending readable output.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Poll {
    pub readable: bool,
    pub writable: bool,
    pub wake: bool,
    pub hangup: bool,
    pub child_exited: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadStatus {
    Data(usize),
    WouldBlock,
    Hangup,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteStatus {
    Complete,
    Partial(usize),
    WouldBlock,
    Hangup,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Shutdown {
    Reaped,
    Pending,
}

/// One local attachment, with exclusive ownership of the master and unreaped child.
pub struct Attachment {
    master: Option<File>,
    generation: u64,
    role: Role,
    child: Option<ChildState>,
    reaper: SyncSender<ChildState>,
}

impl Attachment {
    pub fn supersede(&mut self, next: u64) -> Result<()> {
        if next <= self.generation || self.master.is_none() {
            return Err(STALE);
        }
        self.generation = next;
        Ok(())
    }

    pub fn poll(
        &self,
        write: bool,
        wake: Option<BorrowedFd<'_>>,
        deadline: Instant,
    ) -> Result<Poll> {
        let events = libc::POLLIN | if write { libc::POLLOUT } else { 0 };
        let master = self.master.as_ref().map_or(-1, AsRawFd::as_raw_fd);
        loop {
            let child_exited = self.child.as_ref().map_or(Ok(true), ChildState::waitable)?;
            let tick = if child_exited {
                Instant::now()
            } else {
                deadline.min(Instant::now() + CHILD_POLL)
            };
            let mut fds = [
                poll_descriptor(master, events),
                poll_descriptor(wake.map_or(-1, |fd| fd.as_raw_fd()), libc::POLLIN),
            ];
            poll_fds(&mut fds, tick)?;
            if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
                return Err(IO);
            }
            let flags = fds[0].revents;
            let state = Poll {
                readable: flags & libc::POLLIN != 0,
                writable: flags & libc::POLLOUT != 0,
                wake: fds[1].revents != 0,
                hangup: master < 0 || flags & (libc::POLLHUP | libc::POLLERR) != 0,
                child_exited,
            };
            if state != Poll::default() || Instant::now() >= deadline {
                return Ok(state);
            }
        }
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> Result<ReadStatus> {
        if buffer.is_empty() {
            return Ok(ReadStatus::Data(0));
        }
        let Some(master) = self.master.as_mut() else {
            return Ok(ReadStatus::Hangup);
        };
        match master.read(buffer) {
            Ok(0) => Ok(ReadStatus::Hangup),
            Ok(count) => Ok(ReadStatus::Data(count)),
            Err(error) if io_hangup(&error) => Ok(ReadStatus::Hangup),
            Err(error) if retryable(&error) => Ok(ReadStatus::WouldBlock),
            Err(_) => Err(IO),
        }
    }

    pub fn write(&mut self, generation: u64, buffer: &[u8]) -> Result<WriteStatus> {
        self.check_sink(generation)?;
        let Some(master) = self.master.as_mut() else {
            return Ok(WriteStatus::Hangup);
        };
        match master.write(buffer) {
            Ok(count) if count == buffer.len() => Ok(WriteStatus::Complete),
            Ok(0) => Ok(WriteStatus::WouldBlock),
            Ok(count) => Ok(WriteStatus::Partial(count)),
            Err(error) if io_hangup(&error) => Ok(WriteStatus::Hangup),
            Err(error) if retryable(&error) => Ok(WriteStatus::WouldBlock),
            Err(_) => Err(IO),
        }
    }

    pub fn resize(&mut self, generation: u64, rows: u16, cols: u16) -> Result<()> {
        self.check_sink(generation)?;
        os::resize(self.master.as_ref().ok_or(IO)?.as_raw_fd(), rows, cols)
    }

    fn check_sink(&self, generation: u64) -> Result<()> {
        if self.generation != generation {
            Err(STALE)
        } else if self.role == Role::Observer {
            Err(READ_ONLY)
        } else {
            Ok(())
        }
    }

    pub fn shutdown(&mut self, deadline: Instant) -> Result<Shutdown> {
        self.master.take();
        if let Some(child) = self.child.as_mut() {
            if child.shutdown(deadline)? == Shutdown::Pending {
                return Ok(Shutdown::Pending);
            }
            self.child.take();
        }
        Ok(Shutdown::Reaped)
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.master.take();
        if let Some(child) = self.child.take() {
            // The reserved receiver cannot exit before receiving this one job while the sender lives.
            if self.reaper.try_send(child).is_err() {
                std::process::abort();
            }
        }
    }
}

fn io_hangup(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::EIO | libc::EPIPE))
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

struct ChildState {
    child: Child,
    pid: libc::pid_t,
    group: bool,
    termination: Option<Instant>,
    killed: bool,
}

impl ChildState {
    fn new(child: Child) -> Self {
        Self {
            pid: child.id() as libc::pid_t,
            child,
            group: false,
            termination: None,
            killed: false,
        }
    }

    fn claim_group(&mut self) -> Result<()> {
        // SAFETY: pid refers to our held, unreaped child.
        let separate =
            unsafe { libc::getsid(self.pid) == self.pid && libc::getpgid(self.pid) == self.pid };
        if !separate {
            return Err(LAUNCH);
        }
        self.group = true;
        Ok(())
    }

    fn waitable(&self) -> Result<bool> {
        loop {
            // SAFETY: the zeroed siginfo is writable; WNOWAIT retains the PID reservation.
            let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            let outcome = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if outcome == 0 {
                // SAFETY: the kernel writes si_pid when the owned child has a waitable status.
                return Ok(unsafe { info.si_pid() } == self.pid);
            }
            if io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return Err(IO);
            }
        }
    }

    fn signal(&self, signal: libc::c_int) -> Result<()> {
        let target = if self.group { -self.pid } else { self.pid };
        // SAFETY: pid and any verified group are reserved by the unreaped child.
        if unsafe { libc::kill(target, signal) } == 0
            || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            Ok(())
        } else {
            Err(IO)
        }
    }

    fn shutdown(&mut self, deadline: Instant) -> Result<Shutdown> {
        while Instant::now() < deadline {
            if self.waitable()? {
                if self.group && !self.killed {
                    self.signal(libc::SIGKILL)?;
                }
                self.child.wait().map_err(|_| IO)?;
                return Ok(Shutdown::Reaped);
            }
            if let Some(sent) = self.termination {
                if !self.killed && sent.elapsed() >= TERMINATE_GRACE {
                    self.signal(libc::SIGKILL)?;
                    self.killed = true;
                }
            } else {
                self.signal(libc::SIGTERM)?;
                self.termination = Some(Instant::now());
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(CHILD_POLL),
            );
        }
        Ok(Shutdown::Pending)
    }
}

fn reserve_reaper() -> Result<SyncSender<ChildState>> {
    let (send, receive) = sync_channel::<ChildState>(1);
    thread::Builder::new()
        .name("marmish-terminal-reaper".into())
        .spawn(move || {
            if let Ok(mut child) = receive.recv() {
                loop {
                    match child.shutdown(Instant::now() + Duration::from_secs(1)) {
                        Ok(Shutdown::Reaped) => return,
                        Ok(Shutdown::Pending) => {}
                        Err(_) => thread::sleep(Duration::from_millis(100)),
                    }
                }
            }
        })
        .map_err(|_| LAUNCH)?;
    Ok(send)
}

#[cfg(test)]
#[path = "terminal_backend_tests.rs"]
mod tests;
