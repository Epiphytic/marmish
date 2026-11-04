//! Detached launch with an owned child and a private, bounded startup channel.
use crate::{Error, Result, identity::ProfileLease};
use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt},
            net::UnixStream,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(240);
const CLEANUP_GRACE: Duration = Duration::from_secs(45);
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const MARKER: &str = "MARMISH_INTERNAL_STARTUP";
const MARKER_VALUE: &str = "marmish-daemon-v1";
const CHANNEL_FD: RawFd = 3;
const MESSAGE_LIMIT: usize = 256;

fn failure(message: &str) -> Error {
    Error::DaemonStartup(message.to_owned())
}

/// The retained main clone can report failures before or after Tokio exists.
#[derive(Clone)]
pub(crate) struct StartupReporter(Arc<Mutex<Option<UnixStream>>>);

impl StartupReporter {
    /// Call once at single-threaded entry, before Tokio or provider access.
    pub(crate) fn take_from_env(foreground_daemon: bool) -> Result<Option<Self>> {
        let Some(marker) = std::env::var_os(MARKER) else {
            return Ok(None);
        };
        // SAFETY: this API's entrypoint contract requires single-threaded startup.
        unsafe { std::env::remove_var(MARKER) };
        if marker != MARKER_VALUE || !foreground_daemon {
            return Err(Error::Invalid("daemon_startup_context"));
        }
        validate_channel(CHANNEL_FD)?;
        set_cloexec(CHANNEL_FD)?;
        close_unrelated_fds()?;
        // SAFETY: validated inherited descriptor, uniquely transferred at entry.
        let stream = unsafe { UnixStream::from_raw_fd(CHANNEL_FD) };
        Ok(Some(Self(Arc::new(Mutex::new(Some(stream))))))
    }

    /// Success is not committed until the live parent acknowledges readiness.
    pub(crate) async fn ready(&self) -> Result<()> {
        let mut stream = self
            .take()?
            .ok_or_else(|| failure("daemon startup already reported"))?;
        tokio::task::spawn_blocking(move || {
            let end = Instant::now() + ACK_TIMEOUT;
            stream.set_write_timeout(Some(ACK_TIMEOUT))?;
            stream.write_all(b"R")?;
            let mut ack = [0];
            read_before(&mut stream, &mut ack, end)
                .map_err(|_| failure("daemon startup parent unavailable"))?;
            if ack != *b"A" {
                return Err(failure("daemon startup acknowledgement invalid"));
            }
            Ok(())
        })
        .await
        .map_err(|_| failure("daemon startup reporter failed"))?
    }

    /// Only finite public error text; never relay/provider stderr or log contents.
    pub(crate) fn failed(&self, error: &Error) {
        let Ok(Some(mut stream)) = self.take() else {
            return;
        };
        let text = error.to_string();
        let text = if valid_message(text.as_bytes()) {
            text.as_bytes()
        } else {
            b"daemon startup failed"
        };
        let _ = stream.set_write_timeout(Some(ACK_TIMEOUT));
        let length = (text.len() as u16).to_be_bytes();
        let _ = stream
            .write_all(&[b'F', length[0], length[1]])
            .and_then(|()| stream.write_all(text));
    }

    fn take(&self) -> Result<Option<UnixStream>> {
        self.0
            .lock()
            .map(|mut channel| channel.take())
            .map_err(|_| failure("daemon startup reporter failed"))
    }
}

fn valid_message(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.len() <= MESSAGE_LIMIT
        && bytes.iter().all(|b| (b' '..=b'~').contains(b))
}

fn validate_channel(fd: RawFd) -> Result<()> {
    let mut kind = 0i32;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: writable scalar and its exact byte length.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut i32).cast(),
            &mut length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Err(Error::Invalid("daemon_startup_context"));
    }
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
    // A stream socket alone is insufficient: only a Unix stream is accepted.
    if unsafe {
        libc::getsockname(
            fd,
            (&mut address as *mut libc::sockaddr_un).cast(),
            &mut length,
        )
    } != 0
        || address.sun_family != libc::AF_UNIX as libc::sa_family_t
    {
        return Err(Error::Invalid("daemon_startup_context"));
    }
    validate_peer(fd)
}

#[cfg(target_os = "linux")]
fn validate_peer(fd: RawFd) -> Result<()> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    } != 0
        || credentials.uid != unsafe { libc::geteuid() }
        || credentials.pid != unsafe { libc::getppid() }
    {
        return Err(Error::Invalid("daemon_startup_context"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_peer(fd: RawFd) -> Result<()> {
    let (mut uid, mut gid) = (0, 0);
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of_val(&pid) as libc::socklen_t;
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0
        || uid != unsafe { libc::geteuid() }
        || unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut length,
            )
        } != 0
        || pid != unsafe { libc::getppid() }
    {
        return Err(Error::Invalid("daemon_startup_context"));
    }
    Ok(())
}

fn set_cloexec(fd: RawFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn close_unrelated_fds() -> Result<()> {
    let mut limit: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    for fd in 4..limit.rlim_cur.min(i32::MAX as _) as i32 {
        unsafe {
            libc::close(fd);
        }
    }
    Ok(())
}

fn append_log(state: &Path, identity: &str) -> Result<File> {
    let profile = ProfileLease::acquire(state, identity)?;
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(profile.root().join("daemon.log"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(Error::Identity("unsafe_profile_file"));
    }
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(file)
}

pub(crate) fn launch(
    executable: &Path,
    argv: &[OsString],
    state: &Path,
    identity: &str,
) -> Result<()> {
    let log = append_log(state, identity)?;
    let mut foreground = Vec::with_capacity(argv.len() + 1);
    foreground.push(OsString::from("--foreground"));
    foreground.extend_from_slice(argv);
    let (child, channel) = spawn_child(executable, &foreground, log, CLEANUP_GRACE)?;
    await_startup(child, channel, STARTUP_TIMEOUT)
}

fn spawn_child(
    executable: &Path,
    argv: &[OsString],
    log: File,
    grace: Duration,
) -> Result<(OwnedChild, UnixStream)> {
    let (parent, child) = UnixStream::pair()?;
    set_cloexec(parent.as_raw_fd())?;
    set_cloexec(child.as_raw_fd())?;
    // Reserve a non-stdio descriptor before Command configures fd0/1/2. A
    // caller with closed stdio can otherwise receive its socket at fd0/1/2.
    let reserved = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if reserved < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fcntl returned a new uniquely owned descriptor.
    let child = unsafe { OwnedFd::from_raw_fd(reserved) };
    let child_fd = child.as_raw_fd();
    let mut command = Command::new(executable);
    command
        .args(argv)
        .env(MARKER, MARKER_VALUE)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    // SAFETY: only async-signal-safe libc calls; no allocation, locks or Rust runtime.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            inherit_channel(child_fd)
        });
    }
    let spawned = command.spawn()?;
    drop(child);
    Ok((
        OwnedChild {
            child: Some(spawned),
            grace,
        },
        parent,
    ))
}

// This also handles source == destination: dup2(fd,fd) does not clear CLOEXEC.
fn inherit_channel(source: RawFd) -> std::io::Result<()> {
    if source != CHANNEL_FD && unsafe { libc::dup2(source, CHANNEL_FD) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let flags = unsafe { libc::fcntl(CHANNEL_FD, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(CHANNEL_FD, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn await_startup(mut child: OwnedChild, mut channel: UnixStream, budget: Duration) -> Result<()> {
    let end = Instant::now() + budget;
    channel.set_write_timeout(Some(ACK_TIMEOUT))?;
    let mut tag = [0];
    if read_before(&mut channel, &mut tag, end).is_err() {
        let early = child
            .child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten();
        return Err(match early {
            Some(status) => early_exit(status),
            None => failure("daemon startup unavailable or timed out; see daemon.log"),
        });
    }
    match tag[0] {
        b'R' => {
            if let Some(status) = child
                .child
                .as_mut()
                .ok_or_else(|| failure("daemon child unavailable"))?
                .try_wait()?
            {
                return Err(early_exit(status));
            }
            let remaining = end
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .ok_or_else(|| failure("daemon startup timed out; see daemon.log"))?;
            channel.set_write_timeout(Some(remaining.min(ACK_TIMEOUT)))?;
            channel.write_all(b"A")?;
            std::thread::Builder::new()
                .name("marmish-daemon-wait".into())
                .spawn(move || {
                    child.wait();
                })?;
            Ok(())
        }
        b'F' => {
            let mut length = [0; 2];
            read_before(&mut channel, &mut length, end)?;
            let length = u16::from_be_bytes(length) as usize;
            if length == 0 || length > MESSAGE_LIMIT {
                return Err(failure("daemon startup report invalid"));
            }
            let mut text = vec![0; length];
            read_before(&mut channel, &mut text, end)?;
            if !valid_message(&text) {
                return Err(failure("daemon startup report invalid"));
            }
            // The child's failure notifier runs only after joined owner cleanup.
            // Wait normally; a child violating its post-cleanup exit contract is
            // still reaped within the finite grace, rather than left orphaned.
            child.wait_after_failure()?;
            Err(Error::DaemonStartup(
                String::from_utf8(text).map_err(|_| failure("daemon startup report invalid"))?,
            ))
        }
        _ => Err(failure("daemon startup report invalid")),
    }
}

fn early_exit(status: ExitStatus) -> Error {
    let detail = match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit {code}"),
        (_, Some(signal)) => format!("signal {signal}"),
        _ => "unknown status".to_owned(),
    };
    failure(&format!(
        "daemon exited before readiness ({detail}); see daemon.log"
    ))
}

fn read_before(stream: &mut UnixStream, mut bytes: &mut [u8], end: Instant) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let remaining = end
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::TimedOut))?;
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => bytes = &mut bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

struct OwnedChild {
    child: Option<Child>,
    grace: Duration,
}
impl OwnedChild {
    fn wait(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
    fn wait_after_failure(&mut self) -> Result<()> {
        self.reap(false)
    }
    fn reap(&mut self, terminate: bool) -> Result<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if terminate && child.try_wait()?.is_none() {
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
        }
        let end = Instant::now() + self.grace;
        while child.try_wait()?.is_none() {
            if Instant::now() >= end {
                child.kill()?;
                child.wait()?;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.child = None;
        Ok(())
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.reap(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    fn private_root() -> tempfile::TempDir {
        let parent = std::env::temp_dir().canonicalize().unwrap();
        tempfile::Builder::new()
            .prefix("marmish-launch-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap()
    }

    fn spawn_fixture(case: &str, root: &Path) -> (OwnedChild, UnixStream) {
        let args = [
            OsString::from("--exact"),
            OsString::from(format!("daemon_launch::tests::{case}")),
            OsString::from("--nocapture"),
        ];
        spawn_child(
            &std::env::current_exe().unwrap(),
            &args,
            append_log(root, "test").unwrap(),
            Duration::from_millis(500),
        )
        .unwrap()
    }

    fn is_fixture() -> bool {
        std::env::var_os(MARKER).is_some()
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    // Entry tests are selected individually by a real re-exec, never by fork
    // inside the multithreaded test harness. Ordinary suite invocations are inert.
    #[test]
    fn child_ready() {
        if !is_fixture() {
            return;
        }
        let reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        assert!(std::env::var_os(MARKER).is_none());
        assert_ne!(
            unsafe { libc::fcntl(3, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        assert_eq!(unsafe { libc::getsid(0) }, unsafe { libc::getpid() });
        // close_unrelated_fds runs before any runtime is constructed.
        assert_eq!(unsafe { libc::fcntl(4, libc::F_GETFD) }, -1);
        runtime().block_on(reporter.ready()).unwrap();
        reporter.failed(&Error::Identity("must_not_send_twice"));
        println!("ready-acknowledged");
        let status = Command::new("/bin/sh")
            .args(["-c", "if ( : >&3 ) 2>/dev/null; then exit 91; fi"])
            .status()
            .unwrap();
        assert!(
            status.success(),
            "readiness descriptor survived exec: {status}"
        );
    }

    #[test]
    fn child_fd_three() {
        if !is_fixture() {
            return;
        }
        set_cloexec(CHANNEL_FD).unwrap();
        inherit_channel(CHANNEL_FD).unwrap();
        assert_eq!(
            unsafe { libc::fcntl(CHANNEL_FD, libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        let reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        runtime().block_on(reporter.ready()).unwrap();
        println!("fd-three-collision-ok");
    }

    #[test]
    fn fixed_fd_source_collision_clears_close_on_exec() {
        let root = private_root();
        let (child, channel) = spawn_fixture("child_fd_three", root.path());
        await_startup(child, channel, Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn child_closed_stdio() {
        if !is_fixture() {
            return;
        }
        let reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        let root = private_root();
        let args = [
            OsString::from("--exact"),
            OsString::from("daemon_launch::tests::child_ready"),
            OsString::from("--nocapture"),
        ];
        let executable = std::env::current_exe().unwrap();
        let mut backups = [-1; 3];
        for (fd, backup) in backups.iter_mut().enumerate() {
            // SAFETY: preserve this subprocess's stdio at independent CLOEXEC fds.
            *backup = unsafe { libc::fcntl(fd as i32, libc::F_DUPFD_CLOEXEC, 4) };
            assert!(*backup >= 4);
        }
        for fd in 0..3 {
            unsafe {
                libc::close(fd);
            }
        }
        // No panic/unwrap until original stdio is restored. This intentionally
        // makes socketpair allocate a stdio descriptor before the reservation.
        let nested = append_log(root.path(), "nested")
            .and_then(|log| spawn_child(&executable, &args, log, Duration::from_millis(500)))
            // The parent socket itself may occupy fd0/1/2. Complete the
            // handshake before restoring stdio over those descriptor numbers.
            .and_then(|(child, channel)| await_startup(child, channel, Duration::from_secs(5)));
        for (fd, backup) in backups.into_iter().enumerate() {
            // SAFETY: each backup is still owned here, and only this isolated
            // reexec subprocess's descriptors are restored and closed.
            unsafe {
                libc::dup2(backup, fd as i32);
                libc::close(backup);
            }
        }
        nested.unwrap();
        runtime().block_on(reporter.ready()).unwrap();
    }

    #[test]
    fn closed_stdio_cannot_overwrite_the_inherited_startup_socket() {
        let root = private_root();
        let (child, channel) = spawn_fixture("child_closed_stdio", root.path());
        await_startup(child, channel, Duration::from_secs(10)).unwrap();
    }

    #[test]
    fn child_error() {
        if !is_fixture() {
            return;
        }
        let reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        reporter.failed(&Error::Identity("profile_in_use"));
        std::thread::sleep(Duration::from_millis(75));
        println!("failure-normal-exit");
    }

    #[test]
    fn child_eof() {
        if !is_fixture() {
            return;
        }
        drop(StartupReporter::take_from_env(true).unwrap());
    }

    #[test]
    fn child_pending() {
        if !is_fixture() {
            return;
        }
        let _reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        std::thread::sleep(Duration::from_secs(10));
        panic!("parent failed to terminate its child");
    }

    #[test]
    fn child_bad_frame() {
        if !is_fixture() {
            return;
        }
        let reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        let mut channel = reporter.take().unwrap().unwrap();
        channel.write_all(b"X").unwrap();
        std::thread::sleep(Duration::from_secs(10));
    }

    #[test]
    fn child_parent_gone() {
        if !is_fixture() {
            return;
        }
        let reporter = StartupReporter::take_from_env(true).unwrap().unwrap();
        assert!(runtime().block_on(reporter.ready()).is_err());
        println!("parent-loss-detected");
    }

    #[test]
    fn child_wrong_operation() {
        if !is_fixture() {
            return;
        }
        assert!(matches!(
            StartupReporter::take_from_env(false),
            Err(Error::Invalid("daemon_startup_context"))
        ));
        assert!(std::env::var_os(MARKER).is_none());
    }

    #[test]
    fn detached_ready_is_acknowledged_and_channel_does_not_survive_exec() {
        let root = private_root();
        let (child, channel) = spawn_fixture("child_ready", root.path());
        await_startup(child, channel, Duration::from_secs(5)).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let log = fs::read_to_string(root.path().join("test/daemon.log")).unwrap();
            if log.contains("test result: ok.") {
                assert!(log.contains("ready-acknowledged"));
                break;
            }
            assert!(Instant::now() < end, "child did not complete: {log}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn finite_failure_waits_for_normal_child_exit() {
        let root = private_root();
        let (child, channel) = spawn_fixture("child_error", root.path());
        let message = await_startup(child, channel, Duration::from_secs(5))
            .unwrap_err()
            .to_string();
        assert_eq!(message, "identity: profile_in_use");
        assert!(
            fs::read_to_string(root.path().join("test/daemon.log"))
                .unwrap()
                .contains("failure-normal-exit")
        );
    }

    #[test]
    fn eof_malformed_and_timeout_reap_only_the_owned_child() {
        let root = private_root();
        let mut unrelated = Command::new("/bin/sleep").arg("10").spawn().unwrap();
        for case in ["child_eof", "child_bad_frame", "child_pending"] {
            let (child, channel) = spawn_fixture(case, root.path());
            let pid = child.child.as_ref().unwrap().id();
            let started = Instant::now();
            assert!(await_startup(child, channel, Duration::from_millis(100)).is_err());
            assert!(started.elapsed() < Duration::from_secs(2));
            assert_eq!(
                unsafe { libc::kill(pid as i32, 0) },
                -1,
                "owned child remains"
            );
            assert!(
                unrelated.try_wait().unwrap().is_none(),
                "unrelated child was affected"
            );
        }
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }

    #[test]
    fn child_requires_parent_ack_and_rejects_inapplicable_context() {
        let root = private_root();
        let (mut child, channel) = spawn_fixture("child_parent_gone", root.path());
        drop(channel);
        child.wait_after_failure().unwrap();
        let log = fs::read_to_string(root.path().join("test/daemon.log")).unwrap();
        assert!(log.contains("parent-loss-detected"), "{log}");
        let (mut child, channel) = spawn_fixture("child_wrong_operation", root.path());
        drop(channel);
        child.wait_after_failure().unwrap();
        let log = fs::read_to_string(root.path().join("test/daemon.log")).unwrap();
        assert!(!log.contains("test result: FAILED"), "{log}");
    }

    #[test]
    fn log_is_private_append_and_rejects_foreign_shapes() {
        let root = private_root();
        append_log(root.path(), "test")
            .unwrap()
            .write_all(b"first\n")
            .unwrap();
        append_log(root.path(), "test")
            .unwrap()
            .write_all(b"second\n")
            .unwrap();
        let path = root.path().join("test/daemon.log");
        assert_eq!(fs::read(&path).unwrap(), b"first\nsecond\n");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            unsafe {
                libc::fcntl(
                    append_log(root.path(), "test").unwrap().as_raw_fd(),
                    libc::F_GETFL,
                )
            } & libc::O_NONBLOCK,
            0
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(append_log(root.path(), "test").is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::hard_link(&path, root.path().join("hardlink")).unwrap();
        assert!(append_log(root.path(), "test").is_err());
        fs::remove_file(root.path().join("hardlink")).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(root.path().join("not-created"), &path).unwrap();
        assert!(append_log(root.path(), "test").is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(append_log(root.path(), "test").is_err());
        fs::remove_dir(&path).unwrap();
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let before = Instant::now();
        assert!(append_log(root.path(), "test").is_err());
        assert!(before.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn early_exit_status_is_finite_and_does_not_read_logs() {
        assert_eq!(
            early_exit(ExitStatus::from_raw(23 << 8)).to_string(),
            "daemon exited before readiness (exit 23); see daemon.log"
        );
        assert_eq!(
            early_exit(ExitStatus::from_raw(libc::SIGABRT)).to_string(),
            "daemon exited before readiness (signal 6); see daemon.log"
        );
    }

    #[test]
    fn partial_frame_uses_original_deadline_and_text_is_closed() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        let sender = std::thread::spawn(move || {
            writer.write_all(b"a").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            writer.write_all(b"b").unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let start = Instant::now();
        assert!(read_before(&mut reader, &mut [0; 3], start + Duration::from_millis(100)).is_err());
        assert!(start.elapsed() < Duration::from_millis(200));
        sender.join().unwrap();
        for bytes in [b"".as_slice(), b"line\nbreak", b"\x1b[0m", &[b'x'; 257]] {
            assert!(!valid_message(bytes));
        }
        assert!(valid_message(b"identity: keychain_locked_or_denied"));
    }
}
