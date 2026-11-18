//! Opt-in local PTY and tmux qualification without network access.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(4);
const POLL: Duration = Duration::from_millis(5);
const MARKER: &[u8] = b"MWK_LOCAL_PANE_READY";
const KEYS: &[u8; 30] = b"0123456789ABCDEFGHIJKLMNOPQRST";
const PANE: &str = "terminal_local_tmux_pane";
const WATCHDOG: &str = "terminal_local_tmux_cleanup";
const ROOT_ENV: &str = "MARMISH_TEST_TMUX_CLEANUP_ROOT";
const TMUX_ENV: &str = "MARMISH_TEST_TMUX_CLEANUP_BIN";
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct OwnedChild {
    child: Child,
    session_leader: bool,
    reaped: bool,
}

impl OwnedChild {
    fn new(child: Child, session_leader: bool) -> Self {
        Self {
            child,
            session_leader,
            reaped: false,
        }
    }

    fn pid(&self) -> libc::pid_t {
        self.child.id().try_into().unwrap()
    }

    fn exited(&self) -> io::Result<bool> {
        // SAFETY: waitid initializes the siginfo and does not reap while WNOWAIT is set.
        unsafe {
            let mut info = std::mem::zeroed::<libc::siginfo_t>();
            let result = libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            );
            if result != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(info.si_pid() == self.pid())
        }
    }

    fn signal(&self, signal: libc::c_int) {
        // SAFETY: the Child still owns the unreaped, strictly positive PID.
        unsafe { libc::kill(self.pid(), signal) };
    }

    fn ready_to_reap(&self, limit: Duration) -> io::Result<bool> {
        let start = Instant::now();
        while !self.exited()? {
            if start.elapsed() >= limit {
                return Ok(false);
            }
            std::thread::sleep(POLL);
        }
        Ok(true)
    }

    fn reap(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        if !self.exited()? {
            self.signal(libc::SIGTERM);
            self.ready_to_reap(Duration::from_millis(300))?;
        }
        if self.session_leader {
            // SAFETY: successful spawn completed setsid and the leader PID is still reserved.
            unsafe { libc::kill(-self.pid(), libc::SIGKILL) };
        } else if !self.exited()? {
            self.signal(libc::SIGKILL);
        }
        if !self.ready_to_reap(Duration::from_millis(700))? {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        self.child.wait()?;
        self.reaped = true;
        Ok(())
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.reap();
    }
}

struct Fixture {
    server: OwnedChild,
    cleanup: Cleanup,
    root: tempfile::TempDir,
    socket: PathBuf,
    tmux: PathBuf,
    nonce: String,
}

struct Cleanup {
    pipe: Option<ChildStdin>,
    child: OwnedChild,
}

impl Cleanup {
    fn arm(tmux: &Path, root: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().expect("fixture_cleanup_exe"))
            .args(["--ignored", "--exact", WATCHDOG, "--nocapture"])
            .env_clear()
            .env(ROOT_ENV, root)
            .env(TMUX_ENV, tmux)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixture_cleanup_spawn");
        Self {
            pipe: child.stdin.take(),
            child: OwnedChild::new(child, false),
        }
    }

    fn finish(&mut self, disarm: bool) -> io::Result<()> {
        if let Some(mut pipe) = self.pipe.take()
            && disarm
        {
            pipe.write_all(b"q")?;
        }
        if !self.child.reaped {
            if !self
                .child
                .ready_to_reap(WAIT * 2 + Duration::from_secs(2))?
            {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            let status = self.child.child.wait()?;
            self.child.reaped = true;
            if !status.success() {
                return Err(io::Error::other("fixture_cleanup_exit"));
            }
        }
        Ok(())
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.finish(false);
    }
}

fn tmux_command(tmux: &Path, root: &Path) -> Command {
    let mut command = Command::new(tmux);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root)
        .env("TERM", "xterm")
        .env("LC_ALL", "C")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

impl Fixture {
    fn start(tmux: &Path) -> Self {
        let root = tempfile::Builder::new()
            .prefix("mwk-t-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .expect("fixture_directory");
        let socket = root.path().join("sock");
        assert!(
            socket.as_os_str().as_bytes().len() <= 103,
            "fixture_socket_length"
        );
        let nonce = root
            .path()
            .file_name()
            .and_then(|part| part.to_str())
            .expect("fixture_nonce")
            .to_owned();
        let mut manifest = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.path().join("cleanup"))
            .expect("fixture_manifest");
        manifest
            .write_all(socket.as_os_str().as_bytes())
            .expect("fixture_manifest");
        drop(manifest);
        let cleanup = Cleanup::arm(tmux, root.path());
        let server = tmux_command(tmux, root.path())
            .arg("-D")
            .arg("-S")
            .arg(&socket)
            .args(["-f", "/dev/null"])
            .spawn()
            .expect("fixture_server");
        let fixture = Self {
            server: OwnedChild::new(server, false),
            cleanup,
            root,
            socket,
            tmux: tmux.to_owned(),
            nonce,
        };
        let start = Instant::now();
        while !fixture.owned_socket() || UnixStream::connect(&fixture.socket).is_err() {
            assert!(
                start.elapsed() < WAIT && !fixture.server.exited().expect("fixture_server_wait"),
                "fixture_server_ready"
            );
            std::thread::sleep(POLL);
        }
        fixture
    }

    fn start_pane(&self) {
        let exe = std::env::current_exe().expect("fixture_exe");
        let exe = exe.to_str().expect("fixture_exe_encoding");
        let directory = self.root.path().to_str().expect("fixture_home_encoding");
        let nonce_arg = format!("MARMISH_TEST_PANE={}", self.nonce);
        self.control(&[
            "new-session",
            "-d",
            "-s",
            &self.nonce,
            "-x",
            "80",
            "-y",
            "24",
            "-c",
            directory,
            "-e",
            &nonce_arg,
            exe,
            "--ignored",
            "--exact",
            PANE,
            "--nocapture",
        ])
        .expect("fixture_create_pane");
        let server = self
            .control(&["display-message", "-p", "-t", &self.nonce, "#{pid}"])
            .expect("fixture_server_pid");
        assert!(
            trim(&server) == self.server.pid().to_string().as_bytes(),
            "fixture_server_ownership"
        );
        self.control(&["set-option", "-s", "exit-empty", "on"])
            .expect("fixture_exit_empty");
        self.control(&["set-option", "-t", &self.nonce, "destroy-unattached", "off"])
            .expect("fixture_detach_policy");
        self.control(&["set-option", "-t", &self.nonce, "status", "off"])
            .expect("fixture_status_policy");
    }

    fn control(&self, args: &[&str]) -> Option<Vec<u8>> {
        if !self.owned_socket() {
            return None;
        }
        let mut command = tmux_command(&self.tmux, self.root.path());
        command.arg("-N").arg("-S").arg(&self.socket).args(args);
        command_output(command, self.root.path())
    }

    fn owned_socket(&self) -> bool {
        owned_socket(self.root.path(), &self.socket)
    }

    fn stop(&mut self) -> io::Result<()> {
        if !self.server.reaped && !self.server.exited()? {
            let _ = self.control(&["kill-server"]);
        }
        self.server.reap()?;
        self.cleanup.finish(true)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn owned_directory(directory: &Path) -> bool {
    // SAFETY: geteuid has no arguments or preconditions.
    let user = unsafe { libc::geteuid() };
    directory.parent() == Some(Path::new("/tmp"))
        && directory
            .file_name()
            .is_some_and(|name| name.as_bytes().starts_with(b"mwk-t-"))
        && directory.symlink_metadata().is_ok_and(|metadata| {
            metadata.is_dir() && metadata.uid() == user && metadata.mode() & 0o777 == 0o700
        })
}

fn owned_socket(directory: &Path, socket: &Path) -> bool {
    // SAFETY: geteuid has no arguments or preconditions.
    let user = unsafe { libc::geteuid() };
    socket == directory.join("sock")
        && owned_directory(directory)
        && socket
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_socket() && metadata.uid() == user)
}

#[test]
#[ignore = "private watchdog for the local tmux fixture"]
fn terminal_local_tmux_cleanup() {
    let directory = PathBuf::from(std::env::var_os(ROOT_ENV).expect("fixture_cleanup_root"));
    let tmux = PathBuf::from(std::env::var_os(TMUX_ENV).expect("fixture_cleanup_bin"));
    let mut fd = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: fd points to initialized storage and stdin is the watchdog pipe.
    let ready = unsafe { libc::poll(&mut fd, 1, 60_000) };
    let mut signal = [0];
    if ready > 0 && io::stdin().read(&mut signal).ok() == Some(1) && signal[0] == b'q' {
        return;
    }
    assert!(owned_directory(&directory), "fixture_cleanup_owner");
    let socket = directory.join("sock");
    let mut manifest = Vec::new();
    File::open(directory.join("cleanup"))
        .expect("fixture_cleanup_manifest")
        .take(104)
        .read_to_end(&mut manifest)
        .expect("fixture_cleanup_manifest");
    assert!(
        manifest == socket.as_os_str().as_bytes(),
        "fixture_cleanup_manifest"
    );
    let started = Instant::now();
    while started.elapsed() < WAIT {
        if owned_socket(&directory, &socket) {
            let mut command = tmux_command(&tmux, &directory);
            command.arg("-N").arg("-S").arg(&socket).arg("kill-server");
            let _ = command_output(command, &directory);
            break;
        }
        std::thread::sleep(POLL);
    }
    let started = Instant::now();
    while UnixStream::connect(&socket).is_ok() {
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "fixture_cleanup_socket_live"
        );
        std::thread::sleep(POLL);
    }
    fs::remove_dir_all(&directory).expect("fixture_cleanup_directory");
}

fn command_output(mut command: Command, directory: &Path) -> Option<Vec<u8>> {
    let mut output = tempfile::tempfile_in(directory).ok()?;
    command.stdout(Stdio::from(output.try_clone().ok()?));
    let mut child = OwnedChild::new(command.spawn().ok()?, false);
    if !child.ready_to_reap(WAIT).ok()? {
        return None;
    }
    let status = child.child.wait().ok()?;
    child.reaped = true;
    output.seek(SeekFrom::Start(0)).ok()?;
    let mut bytes = Vec::new();
    output.take(1024).read_to_end(&mut bytes).ok()?;
    (status.success() && bytes.len() < 1024).then_some(bytes)
}

fn trim(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

fn terminal(fd: libc::c_int) -> libc::termios {
    // SAFETY: the caller passes the live terminal descriptor and the output points to storage.
    unsafe {
        let mut term = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(fd, &mut term), 0, "fixture_termios");
        term
    }
}

fn window(fd: libc::c_int, rows: u16, cols: u16) {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: the descriptor is the live PTY and size lives until the syscall finishes.
    assert_eq!(unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) }, 0);
}

fn attach(fixture: &Fixture) -> (OwnedChild, File, u64, Instant) {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: the output pointers are valid, and null requests default settings.
    unsafe {
        assert_eq!(
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            0,
            "fixture_openpty"
        );
        for fd in [master, slave] {
            assert_eq!(libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC), 0);
        }
    }
    // SAFETY: openpty returned both newly owned descriptors.
    let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    // SAFETY: the master is owned and open throughout these fcntl operations.
    unsafe {
        let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
        assert!(
            flags >= 0
                && libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) == 0,
            "fixture_nonblocking"
        );
    }
    window(master.as_raw_fd(), 24, 80);
    let device = slave.metadata().unwrap().rdev();
    let mut command = tmux_command(&fixture.tmux, fixture.root.path());
    command
        .arg("-N")
        .arg("-S")
        .arg(&fixture.socket)
        .args(["attach-session", "-t", &fixture.nonce])
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave));
    // SAFETY: runs only async-signal-safe syscalls. Child fd zero is the PTY slave.
    unsafe {
        command.pre_exec(|| {
            #[cfg(target_os = "linux")]
            let request = libc::TIOCSCTTY;
            #[cfg(target_os = "macos")]
            let request = libc::c_ulong::from(libc::TIOCSCTTY);
            if libc::setsid() < 0 || libc::ioctl(0, request, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::tcgetpgrp(0) != libc::getpgrp() {
                return Err(io::Error::from_raw_os_error(libc::ENOTTY));
            }
            Ok(())
        });
    }
    let started = Instant::now();
    let child = command.spawn().expect("fixture_attach");
    drop(command);
    (OwnedChild::new(child, true), master, device, started)
}

#[cfg(target_os = "linux")]
fn tty_index(entry: &str) -> Option<&str> {
    entry
        .lines()
        .find_map(|line| line.strip_prefix("tty-index:").map(str::trim))
}

#[cfg(target_os = "linux")]
fn master_index(master: &File) -> String {
    let master_info = fs::read_to_string(format!("/proc/self/fdinfo/{}", master.as_raw_fd()))
        .expect("fixture_proc_master");
    tty_index(&master_info)
        .expect("fixture_proc_master_index")
        .to_owned()
}

#[cfg(target_os = "linux")]
fn verify_proc_fds(pid: libc::pid_t, master_index: &str, forbidden_slave: Option<u64>) {
    let process = PathBuf::from(format!("/proc/{pid}"));
    for entry in fs::read_dir(process.join("fd")).expect("fixture_proc_fds") {
        let entry = entry.expect("fixture_proc_entry");
        match entry.path().metadata() {
            Ok(metadata) => assert!(
                Some(metadata.rdev()) != forbidden_slave,
                "fixture_proc_has_slave"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => panic!("fixture_proc_metadata"),
        }
        match fs::read_to_string(process.join("fdinfo").join(entry.file_name())) {
            Ok(info) => assert!(
                tty_index(&info) != Some(master_index),
                "fixture_proc_has_master"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => panic!("fixture_proc_info"),
        }
    }
}

fn read_through(master: &mut File, marker: &[u8], limit: Duration) -> Result<(), &'static str> {
    let deadline = Instant::now() + limit;
    let mut bytes = Vec::new();
    while Instant::now() < deadline && bytes.len() < 65_536 {
        let mut fd = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: fd points to one initialized pollfd and is live during poll.
        if unsafe { libc::poll(&mut fd, 1, 15) } <= 0 {
            continue;
        }
        let mut chunk = [0; 512];
        let capacity = chunk.len().min(65_536 - bytes.len());
        let size = match master.read(&mut chunk[..capacity]) {
            Ok(0) => return Err("fixture_pty_hangup"),
            Ok(size) => size,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                return Err("fixture_pty_hangup");
            }
            Err(_) => return Err("fixture_pty_read"),
        };
        bytes.extend_from_slice(&chunk[..size]);
        if bytes.windows(marker.len()).any(|window| window == marker) {
            return Ok(());
        }
    }
    if bytes.len() == 65_536 {
        Err("fixture_pty_budget")
    } else {
        Err("fixture_marker_timeout")
    }
}

fn send_key(master: &mut File, key: u8) {
    let deadline = Instant::now() + WAIT;
    while Instant::now() < deadline {
        match master.write(&[key]) {
            Ok(1) => return,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(POLL)
            }
            _ => panic!("fixture_pty_write"),
        }
    }
    panic!("fixture_write_timeout");
}

fn terminal_group(fd: libc::c_int) -> libc::pid_t {
    // SAFETY: fd identifies a live terminal in the caller.
    unsafe { libc::tcgetpgrp(fd) }
}

fn current_group() -> libc::pid_t {
    // SAFETY: getpgrp has no arguments or preconditions.
    unsafe { libc::getpgrp() }
}

fn terminal_device(fd: libc::c_int) -> u64 {
    // SAFETY: live inherited descriptor and initialized writable output storage.
    unsafe {
        let mut info: libc::stat = std::mem::zeroed();
        assert_eq!(libc::fstat(fd, &mut info), 0, "fixture_pane_fd");
        let device = info.st_rdev as u128;
        u64::try_from(device).expect("fixture_pane_device")
    }
}

fn size(fd: libc::c_int) -> (u16, u16) {
    // SAFETY: live terminal and initialized writable winsize.
    unsafe {
        let mut win: libc::winsize = std::mem::zeroed();
        assert_eq!(
            libc::ioctl(fd, libc::TIOCGWINSZ, &mut win),
            0,
            "fixture_get_window"
        );
        (win.ws_row, win.ws_col)
    }
}

extern "C" fn pane_interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

#[test]
#[ignore = "child fixture, launched by local_tmux_attachment"]
fn terminal_local_tmux_pane() {
    let nonce = std::env::var("MARMISH_TEST_PANE").expect("fixture_pane_proof");
    assert!(nonce.starts_with("mwk-t-"), "fixture_pane_proof");
    let group = terminal_group(0);
    assert!(group > 0 && group == current_group(), "fixture_pane_group");
    let device = terminal_device(0);
    assert!(
        device != 0 && terminal_device(1) == device && terminal_device(2) == device,
        "fixture_pane_tty_identity"
    );
    // SAFETY: all descriptors belong to the fixture and the signal handler uses an atomic bool.
    unsafe {
        let mut term = terminal(0);
        term.c_lflag &= !(libc::ICANON | libc::ECHO);
        term.c_lflag |= libc::ISIG;
        term.c_cc[libc::VMIN] = 1;
        term.c_cc[libc::VTIME] = 0;
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &term), 0);
        let mut action = std::mem::zeroed::<libc::sigaction>();
        action.sa_sigaction = pane_interrupt as *const () as usize;
        assert_eq!(libc::sigemptyset(&mut action.sa_mask), 0);
        assert_eq!(
            libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()),
            0
        );
    }
    let mut output = io::stdout().lock();
    output.write_all(b"\r\n").unwrap();
    output.write_all(MARKER).unwrap();
    output.flush().unwrap();
    let mut proof = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open("pane-proof.tmp")
        .expect("fixture_pane_proof");
    writeln!(proof, "{nonce} {} {group} {device}", std::process::id()).expect("fixture_pane_proof");
    drop(proof);
    fs::rename("pane-proof.tmp", "pane-proof").expect("fixture_pane_proof");
    let mut dimensions = size(0);
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        let observed = size(0);
        if observed != dimensions {
            dimensions = observed;
            let (rows, cols) = observed;
            write!(output, "\r\nMWK_SIZE_{rows}_{cols}").unwrap();
            output.flush().unwrap();
        }
        if INTERRUPTED.swap(false, Ordering::Relaxed) {
            output.write_all(b"\r\nMWK_SIGINT").unwrap();
            output.flush().unwrap();
        }
        let mut fd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: fd is the valid foreground terminal and the pointer lives through poll.
        if unsafe { libc::poll(&mut fd, 1, 10) } <= 0 {
            continue;
        }
        let mut key = [0];
        // SAFETY: fd zero is the pane's terminal and key reserves one writable byte.
        let count = unsafe { libc::read(0, key.as_mut_ptr().cast(), key.len()) };
        if count == 0 {
            break;
        }
        if count < 0 {
            assert_eq!(
                io::Error::last_os_error().kind(),
                io::ErrorKind::Interrupted,
                "fixture_pane_read"
            );
            continue;
        }
        match key[0] {
            b'j' => output.write_all(b"\r\nMWK_FIRST_KEY").unwrap(),
            b'k' => output.write_all(b"\r\nMWK_REATTACH").unwrap(),
            b'v' => output
                .write_all("\r\nMWK_UTF8_é_界_END".as_bytes())
                .unwrap(),
            b'q' => break,
            key if KEYS.contains(&key) => {
                write!(output, "\r\nMWK_ACK_{}", char::from(key)).unwrap()
            }
            _ => (),
        }
        output.flush().unwrap();
    }
}

fn trusted_tmux() -> (PathBuf, String) {
    // SAFETY: geteuid has no arguments or preconditions.
    let user = unsafe { libc::geteuid() };
    assert!(user != 0, "fixture_nonroot_required");
    let path = PathBuf::from(std::env::var_os("MARMISH_TEST_TMUX").expect("fixture_tmux_required"));
    assert!(path.is_absolute(), "fixture_tmux_absolute");
    let path = path.canonicalize().expect("fixture_tmux_path");
    for entry in path.ancestors() {
        let metadata = entry.symlink_metadata().expect("fixture_tmux_component");
        assert!(
            (metadata.uid() == 0 || metadata.uid() == user) && metadata.mode() & 0o022 == 0,
            "fixture_tmux_component_owner"
        );
        if entry == path {
            assert!(
                metadata.is_file() && metadata.mode() & 0o111 != 0,
                "fixture_tmux_executable"
            );
        } else {
            assert!(metadata.is_dir(), "fixture_tmux_component_directory");
        }
    }
    let scratch = tempfile::tempdir_in("/tmp").expect("fixture_version_directory");
    let mut command = tmux_command(&path, scratch.path());
    command.arg("-V");
    let output = command_output(command, scratch.path()).expect("fixture_tmux_version");
    let version = std::str::from_utf8(trim(&output))
        .ok()
        .and_then(|line| line.strip_prefix("tmux "))
        .expect("fixture_tmux_version");
    let (major, minor) = version.split_once('.').expect("fixture_tmux_version");
    let major = major.parse::<u32>().expect("fixture_tmux_version");
    let minor = minor
        .trim_end_matches(|byte: char| byte.is_ascii_alphabetic())
        .parse::<u32>()
        .expect("fixture_tmux_version");
    assert!((major, minor) >= (3, 2), "fixture_tmux_version_required");
    (path, version.to_owned())
}

#[derive(PartialEq, Eq)]
struct PaneProof {
    nonce: String,
    pid: libc::pid_t,
    group: libc::pid_t,
    device: u64,
    id: String,
}

fn pane_proof(fixture: &Fixture) -> PaneProof {
    let record =
        fs::read_to_string(fixture.root.path().join("pane-proof")).expect("fixture_pane_record");
    let fields: Vec<_> = record.split_ascii_whitespace().collect();
    let [nonce, pid, group, device] = fields.as_slice() else {
        panic!("fixture_pane_record");
    };
    assert!(*nonce == fixture.nonce, "fixture_pane_nonce");
    let pane = fixture
        .control(&[
            "display-message",
            "-p",
            "-t",
            &fixture.nonce,
            "#{pane_id} #{pane_pid}",
        ])
        .expect("fixture_pane_lookup");
    let pane = std::str::from_utf8(trim(&pane)).expect("fixture_pane_lookup");
    let (id, tmux_pid) = pane.split_once(' ').expect("fixture_pane_lookup");
    assert!(
        id.starts_with('%') && tmux_pid == *pid,
        "fixture_pane_identity"
    );
    PaneProof {
        nonce: (*nonce).to_owned(),
        pid: pid.parse().expect("fixture_pane_pid"),
        group: group.parse().expect("fixture_pane_group"),
        device: device.parse().expect("fixture_pane_device"),
        id: id.to_owned(),
    }
}

fn wait_pane_ready(fixture: &Fixture) -> PaneProof {
    let proof = fixture.root.path().join("pane-proof");
    let start = Instant::now();
    while !proof.is_file() {
        assert!(start.elapsed() < WAIT, "fixture_pane_ready");
        std::thread::sleep(POLL);
    }
    pane_proof(fixture)
}

fn verify_attachment(child: &OwnedChild, master: &File, device: u64, pane: &PaneProof) {
    // SAFETY: child owns this positive PID and has completed its pre-exec
    // check that the terminal's foreground group is the new group.
    let group = unsafe { libc::getpgid(child.pid()) };
    assert!(
        group == child.pid() && group != current_group() && group != pane.group,
        "fixture_attachment_group"
    );
    assert!(
        pane.group != current_group() && device != pane.device,
        "fixture_attachment_identity"
    );
    let term = terminal(master.as_raw_fd());
    assert_eq!(
        term.c_lflag & (libc::ICANON | libc::ISIG | libc::ECHO),
        0,
        "fixture_attachment_raw"
    );
}

fn verify_exit(mut fixture: Fixture) {
    fixture.stop().expect("fixture_server_reap");
    assert!(
        fixture.server.reaped && UnixStream::connect(&fixture.socket).is_err(),
        "fixture_socket_still_open"
    );
    let root = fixture.root.path().to_owned();
    drop(fixture);
    assert!(!root.exists(), "fixture_directory_retained");
}

fn verify_unwind_cleanup(tmux: &Path) {
    let fixture = Fixture::start(tmux);
    let root = fixture.root.path().to_owned();
    let socket = fixture.socket.clone();
    let unwind = std::panic::catch_unwind(move || {
        let fixture = fixture;
        fixture.start_pane();
        let (_child, mut master, _, _) = attach(&fixture);
        read_through(&mut master, MARKER, WAIT).expect("fixture_unwind_barrier");
        std::panic::resume_unwind(Box::new("fixture_injected_unwind"));
    });
    assert!(
        unwind
            .is_err_and(|reason| reason.downcast_ref::<&str>() == Some(&"fixture_injected_unwind")),
        "fixture_unwind_missing"
    );
    assert!(
        UnixStream::connect(socket).is_err() && !root.exists(),
        "fixture_unwind_cleanup"
    );
}

#[test]
#[ignore = "operator fixture requires a trusted absolute tmux executable"]
fn local_tmux_attachment() {
    let (tmux, version) = trusted_tmux();
    verify_unwind_cleanup(&tmux);
    let mut failed = Fixture::start(&tmux);
    failed.start_pane();
    let (mut attach_failure, mut master, _, _) = attach(&failed);
    read_through(&mut master, MARKER, WAIT).expect("fixture_failure_barrier");
    let injected = read_through(
        &mut master,
        b"MWK_INJECTED_ABSENT",
        Duration::from_millis(100),
    );
    assert_eq!(injected, Err("fixture_marker_timeout"));
    attach_failure
        .reap()
        .expect("fixture_failure_attachment_reap");
    drop(master);
    failed
        .cleanup
        .finish(false)
        .expect("fixture_failure_watchdog");
    assert!(
        failed
            .server
            .ready_to_reap(WAIT)
            .expect("fixture_failure_server_wait"),
        "fixture_failure_watchdog_server"
    );
    verify_exit(failed);

    let fixture = Fixture::start(&tmux);
    fixture.start_pane();
    let pane = wait_pane_ready(&fixture);
    let (mut first, mut master, first_dev, started) = attach(&fixture);
    read_through(&mut master, MARKER, WAIT).expect("fixture_first_render");
    let first_render = started.elapsed();
    verify_attachment(&first, &master, first_dev, &pane);
    #[cfg(target_os = "linux")]
    let detached_index = {
        let index = master_index(&master);
        verify_proc_fds(fixture.server.pid(), &index, None);
        verify_proc_fds(pane.pid, &index, Some(first_dev));
        index
    };
    window(master.as_raw_fd(), 32, 100);
    read_through(&mut master, b"MWK_SIZE_32_100", WAIT).expect("fixture_resize");
    send_key(&mut master, b'j');
    read_through(&mut master, b"MWK_FIRST_KEY", WAIT).expect("fixture_first_key");
    send_key(&mut master, 0x03);
    read_through(&mut master, b"MWK_SIGINT", WAIT).expect("fixture_pane_sigint");
    assert!(
        !first.exited().expect("fixture_attachment_wait"),
        "fixture_attachment_interrupted"
    );
    first.reap().expect("fixture_detach_reap");
    assert_eq!(
        read_through(&mut master, b"MWK_UNEXPECTED_AFTER_DETACH", WAIT),
        Err("fixture_pty_hangup"),
        "fixture_detach_hangup"
    );
    drop(master);
    #[cfg(target_os = "linux")]
    {
        verify_proc_fds(fixture.server.pid(), &detached_index, Some(first_dev));
        verify_proc_fds(pane.pid, &detached_index, Some(first_dev));
    }
    assert!(
        first.reaped && pane_proof(&fixture) == pane,
        "fixture_pane_after_detach"
    );

    let (mut second, mut master, second_dev, started) = attach(&fixture);
    read_through(&mut master, MARKER, WAIT).expect("fixture_reattach_render");
    let reattach = started.elapsed();
    assert!(pane_proof(&fixture) == pane, "fixture_pane_after_reattach");
    verify_attachment(&second, &master, second_dev, &pane);
    send_key(&mut master, b'k');
    read_through(&mut master, b"MWK_REATTACH", WAIT).expect("fixture_reattach_key");

    let mut samples = Vec::with_capacity(KEYS.len());
    for key in KEYS {
        std::thread::sleep(POLL);
        let marker = format!("MWK_ACK_{}", char::from(*key));
        let started = Instant::now();
        send_key(&mut master, *key);
        read_through(&mut master, marker.as_bytes(), WAIT).expect("fixture_key_response");
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    second.reap().expect("fixture_reattach_reap");
    assert!(second.reaped, "fixture_reattach_child_retained");
    drop(master);
    verify_exit(fixture);
    println!(
        "local-tmux platform={}-{} tmux={version} samples={} p50_us={} p95_us={} first_render_us={} reattach_us={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        samples.len(),
        samples[14].as_micros(),
        samples[28].as_micros(),
        first_render.as_micros(),
        reattach.as_micros()
    );
}

#[test]
fn core_tests_terminal_backend_reserved_dispatch() {
    for path in [
        env!("CARGO_BIN_EXE_marmish"),
        env!("CARGO_BIN_EXE_marmishd"),
    ] {
        let denied = Command::new(path)
            .env_clear()
            .arg("_terminal-worker")
            .args(["bad", "bad", "bad"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(denied.status.code(), Some(125));
        assert!(denied.stdout.is_empty() && denied.stderr.is_empty());
        let help = Command::new(path)
            .env_clear()
            .arg("--help")
            .output()
            .unwrap();
        assert!(help.status.success());
        assert!(
            !help
                .stdout
                .windows(b"_terminal-worker".len())
                .any(|part| part == b"_terminal-worker")
        );
    }
}

use marmish_native::terminal_backend::{
    Attachment, CheckedLaunch, ReadStatus, Shutdown, WriteStatus,
};

fn native_marker(attachment: &mut Attachment, marker: &[u8]) {
    let deadline = Instant::now() + WAIT;
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        assert!(
            Instant::now() < deadline && output.len() < 65_536,
            "terminal_native_marker_budget"
        );
        let ready = attachment
            .poll(false, None, deadline)
            .expect("terminal_native_poll");
        match attachment.read(&mut buffer).expect("terminal_native_read") {
            ReadStatus::Data(size) => output.extend_from_slice(&buffer[..size]),
            ReadStatus::WouldBlock => assert!(!ready.child_exited, "terminal_native_child_exited"),
            ReadStatus::Hangup => panic!("terminal_native_hangup"),
        }
        if output.windows(marker.len()).any(|part| part == marker) {
            return;
        }
    }
}

fn native_key(attachment: &mut Attachment, generation: u64, key: u8) {
    let deadline = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < deadline, "terminal_native_write_budget");
        match attachment
            .write(generation, &[key])
            .expect("terminal_native_write")
        {
            WriteStatus::Complete => return,
            WriteStatus::WouldBlock => {
                let ready = attachment
                    .poll(true, None, deadline)
                    .expect("terminal_native_poll");
                assert!(
                    !ready.hangup && !ready.child_exited,
                    "terminal_native_write_exited"
                );
            }
            status => panic!("terminal_native_invalid_write_status {status:?}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn native_slave(fixture: &Fixture) -> u64 {
    let tty = fixture
        .control(&["list-clients", "-F", "#{client_tty}"])
        .expect("terminal_native_client");
    let tty = std::str::from_utf8(trim(&tty)).expect("terminal_native_client_tty");
    assert!(
        tty.strip_prefix("/dev/pts/").is_some_and(
            |index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
        ),
        "terminal_native_client_tty"
    );
    fs::metadata(tty)
        .expect("terminal_native_client_device")
        .rdev()
}

#[test]
#[ignore = "requires a trusted tmux path and a native daemon worker"]
fn terminal_native_backend_tmux() {
    let (tmux, version) = trusted_tmux();
    let fixture = Fixture::start(&tmux);
    fixture.start_pane();
    let proof = wait_pane_ready(&fixture);
    let daemon = Path::new(env!("CARGO_BIN_EXE_marmishd"))
        .canonicalize()
        .expect("terminal_native_daemon");
    let socket = fixture
        .socket
        .canonicalize()
        .expect("terminal_native_socket");
    let launch =
        CheckedLaunch::new(&daemon, &tmux, &socket, &fixture.nonce).expect("terminal_native_paths");
    let nonexistent = CheckedLaunch::new(&daemon, &tmux, &socket, "missing-session").unwrap();
    assert!(
        nonexistent.launch(1, 24, 80).is_err(),
        "terminal_native_exact_target"
    );

    let started = Instant::now();
    let mut first = launch.launch(1, 24, 80).expect("terminal_native_launch");
    native_marker(&mut first, MARKER);
    let first_render = started.elapsed();
    #[cfg(target_os = "linux")]
    let old_slave = native_slave(&fixture);
    first.resize(1, 32, 100).expect("terminal_native_resize");
    native_marker(&mut first, b"MWK_SIZE_32_100");
    native_key(&mut first, 1, b'v');
    native_marker(&mut first, "MWK_UTF8_é_界_END".as_bytes());
    native_key(&mut first, 1, 3);
    native_marker(&mut first, b"MWK_SIGINT");
    assert!(
        !first
            .poll(false, None, Instant::now())
            .unwrap()
            .child_exited
    );

    let started = Instant::now();
    assert_eq!(first.shutdown(started + WAIT).unwrap(), Shutdown::Reaped);
    let detach = started.elapsed();
    assert_eq!(first.read(&mut [0; 32]).unwrap(), ReadStatus::Hangup);
    assert!(
        pane_proof(&fixture) == proof,
        "terminal_native_retained_pane"
    );
    #[cfg(target_os = "linux")]
    {
        verify_proc_fds(fixture.server.pid(), "", Some(old_slave));
        verify_proc_fds(proof.pid, "", Some(old_slave));
    }

    let started = Instant::now();
    let mut second = launch.launch(2, 24, 80).expect("terminal_native_reattach");
    native_marker(&mut second, MARKER);
    let reattach = started.elapsed();
    assert!(
        pane_proof(&fixture) == proof,
        "terminal_native_retained_pane"
    );
    native_key(&mut second, 2, b'k');
    native_marker(&mut second, b"MWK_REATTACH");
    let mut samples = Vec::with_capacity(KEYS.len());
    for key in KEYS {
        std::thread::sleep(POLL);
        let marker = format!("MWK_ACK_{}", char::from(*key));
        let started = Instant::now();
        native_key(&mut second, 2, *key);
        native_marker(&mut second, marker.as_bytes());
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    // Let the real pane finish so the attach child exits without an API shutdown.
    native_key(&mut second, 2, b'q');
    let deadline = Instant::now() + WAIT;
    let (mut child_exited, mut hangup) = (false, false);
    while Instant::now() < deadline && !(child_exited && hangup) {
        let event = second.poll(false, None, deadline).unwrap();
        child_exited |= event.child_exited;
        hangup |= second.read(&mut [0; 1024]).unwrap() == ReadStatus::Hangup;
    }
    assert!(child_exited && hangup, "terminal_native_child_hangup");
    assert_eq!(
        second.shutdown(Instant::now() + WAIT).unwrap(),
        Shutdown::Reaped
    );
    verify_exit(fixture);
    println!(
        "terminal-native platform={}-{} tmux={version} samples={} p50_us={} p95_us={} first_render_us={} detach_us={} reattach_us={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        samples.len(),
        samples[14].as_micros(),
        samples[28].as_micros(),
        first_render.as_micros(),
        detach.as_micros(),
        reattach.as_micros()
    );
}

#[test]
fn core_tests_terminal_backend_failed_exec_and_inherited_descriptor() {
    // SAFETY: geteuid has no arguments or preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped native worker exec and descriptor probe as root");
        return;
    }
    let daemon = Path::new(env!("CARGO_BIN_EXE_marmishd"))
        .canonicalize()
        .unwrap();
    let work = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(daemon.parent().unwrap())
        .unwrap();
    let sock_dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let socket = sock_dir.path().canonicalize().unwrap().join("sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let helper = work.path().join("terminal-helper");
    for (script, expected, exit) in [
        (
            &b"#!/no-such-marmish-interpreter\n"[..],
            &b"\x01\x02\x04"[..],
            125,
        ),
        (
            &b"#!/bin/sh\n[ ! -e /dev/fd/98 ] && [ -t 0 ] && [ -t 1 ] && [ -t 2 ]\n"[..],
            &b"\x01\x02"[..],
            0,
        ),
    ] {
        fs::write(&helper, script).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: both descriptor outputs are writable; null requests standard PTY settings.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        // SAFETY: openpty returned these exclusively owned descriptors.
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            // SAFETY: fd belongs to the held PTY; the setter only changes descriptor flags.
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
        }
        let inherited = File::open(&helper).unwrap();
        let inherited_fd = inherited.as_raw_fd();
        assert_ne!(inherited_fd, 98);
        let mut command = Command::new(&daemon);
        command
            .env_clear()
            .arg("_terminal-worker")
            .arg(&helper)
            .arg(&socket)
            .arg("unrelated")
            .stdin(slave)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: the test child sets one deliberate inherited probe fd with async-safe calls.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(inherited_fd, 98) == 98 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            });
        }
        let mut child = OwnedChild::new(command.spawn().unwrap(), false);
        drop(command);
        drop(inherited);
        assert!(
            child.ready_to_reap(WAIT).unwrap(),
            "terminal_worker_test_deadline"
        );
        let result = child.child.wait().unwrap();
        child.reaped = true;
        assert_eq!(result.code(), Some(exit));
        let mut bytes = Vec::new();
        child
            .child
            .stdout
            .take()
            .unwrap()
            .take(9)
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
        drop(master);
    }
}
