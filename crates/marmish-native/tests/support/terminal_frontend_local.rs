//! Opt-in controlling terminal verification on an isolated process.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use marmish_native::{Error, Result};
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../../src/terminal_frontend.rs"]
mod terminal_frontend;
#[path = "../../src/terminal_frontend_display.rs"]
mod terminal_frontend_display;
#[expect(
    dead_code,
    reason = "job-control stop is exercised by the installed binary"
)]
#[path = "../../src/terminal_frontend_signals.rs"]
mod terminal_frontend_signals;
#[path = "../../src/terminal_frontend_tty.rs"]
mod terminal_frontend_tty;
#[path = "../../src/terminal_input.rs"]
mod terminal_input;
#[expect(
    dead_code,
    reason = "the standalone foreground harness omits network batch admission"
)]
#[path = "../../src/terminal_output.rs"]
mod terminal_output;
#[path = "../../src/terminal_socket_frontend.rs"]
mod terminal_socket_frontend;
#[path = "../../src/terminal_socket_io.rs"]
mod terminal_socket_io;
#[path = "../../src/terminal_socket_session.rs"]
mod terminal_socket_session;
#[path = "../../src/terminal_socket_state.rs"]
mod terminal_socket_state;

use terminal_frontend_signals as signals;
use terminal_frontend_tty as tty;

const LIMIT: Duration = Duration::from_secs(5);
const READY: &[u8] = b"<marmish-fixture-ready>";
const RESUMED: &[u8] = b"<marmish-fixture-resumed>";
const RESIZED: &[u8] = b"<marmish-fixture-resized>";
const REFUSED: &[u8] = b"<marmish-fixture-refused>";
const CHILD: &str = "frontend_local::terminal_frontend_child";
const MODE: &str = "MARMISH_TERMINAL_TEST_MODE";
const DETACH_MARKER: &[u8] = b"\r\n[marmish: detached]\r\n";
const CHANNEL_FD: libc::c_int = 97;

fn poll(fd: libc::c_int, events: libc::c_short, limit_ms: libc::c_int) -> bool {
    let mut poll = libc::pollfd {
        fd,
        events,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut poll, 1, limit_ms) };
    result > 0 && poll.revents & events != 0
}

mod fixture {
    use super::*;

    pub(super) struct Pty {
        pub(super) master: File,
        pub(super) slave: File,
        pub(super) captured: Vec<u8>,
    }

    impl Pty {
        pub(super) fn new() -> Self {
            let mut master = -1;
            let mut slave = -1;
            let size = libc::winsize {
                ws_row: 37,
                ws_col: 99,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            assert_eq!(
                unsafe {
                    libc::openpty(
                        &mut master,
                        &mut slave,
                        std::ptr::null_mut(),
                        std::ptr::null(),
                        &size,
                    )
                },
                0
            );
            let master = unsafe { File::from_raw_fd(master) };
            let slave = unsafe { File::from_raw_fd(slave) };
            for file in [&master, &slave] {
                assert_eq!(
                    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
                    0
                );
            }
            Self {
                master,
                slave,
                captured: Vec::new(),
            }
        }

        pub(super) fn inherited_job_pty() -> Self {
            let slave = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
            assert!(slave >= 3);
            assert_eq!(
                unsafe { libc::fcntl(CHANNEL_FD, libc::F_SETFD, libc::FD_CLOEXEC) },
                0
            );
            Self {
                master: unsafe { File::from_raw_fd(CHANNEL_FD) },
                slave: unsafe { File::from_raw_fd(slave) },
                captured: Vec::new(),
            }
        }

        pub(super) fn attrs(&self) -> libc::termios {
            let mut attrs = MaybeUninit::zeroed();
            assert_eq!(
                unsafe { libc::tcgetattr(self.slave.as_raw_fd(), attrs.as_mut_ptr()) },
                0
            );
            unsafe { attrs.assume_init() }
        }

        pub(super) fn read_through(&mut self, marker: &[u8]) {
            let start = Instant::now();
            let start_offset = self.captured.len();
            let mut chunk = [0; 512];
            while start.elapsed() < LIMIT && self.captured.len() < 65536 {
                if poll(self.master.as_raw_fd(), libc::POLLIN, 10) {
                    let size = self.master.read(&mut chunk).unwrap();
                    self.captured.extend_from_slice(&chunk[..size]);
                    if self.captured[start_offset..]
                        .windows(marker.len())
                        .any(|window| window == marker)
                    {
                        return;
                    }
                }
            }
            panic!(
                "frontend marker missing: {:?}",
                String::from_utf8_lossy(&self.captured[start_offset..])
            );
        }

        pub(super) fn drain_after_exit(&mut self) {
            let mut chunk = [0; 512];
            while self.captured.len() < 65536 && poll(self.master.as_raw_fd(), libc::POLLIN, 0) {
                let size = self.master.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                self.captured.extend_from_slice(&chunk[..size]);
            }
            assert!(self.captured.len() < 65536);
        }

        pub(super) fn resize(&self, rows: u16, cols: u16) {
            let size = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            assert_eq!(
                unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
                0
            );
        }
    }

    pub(super) struct Held(Child);

    impl Held {
        pub(super) fn spawn(in_tty: &Pty, out_tty: &Pty, mode: &str) -> Self {
            Self::spawn_inner(in_tty, out_tty, mode, None)
        }

        pub(super) fn spawn_inner(
            in_tty: &Pty,
            out_tty: &Pty,
            mode: &str,
            channel: Option<&UnixStream>,
        ) -> Self {
            let cmd = Self::reexecute(in_tty, out_tty, mode);
            Self::spawn_with_tty(
                cmd,
                mode != "no-control",
                channel.map(AsRawFd::as_raw_fd),
                false,
            )
        }

        fn reexecute(in_tty: &Pty, out_tty: &Pty, mode: &str) -> Command {
            let mut cmd = Command::new(std::env::current_exe().unwrap());
            cmd.args(["--exact", CHILD, "--ignored", "--nocapture"])
                .env_clear()
                .env(MODE, mode)
                .stdin(Stdio::from(in_tty.slave.try_clone().unwrap()))
                .stdout(Stdio::from(out_tty.slave.try_clone().unwrap()))
                .stderr(Stdio::inherit());
            cmd
        }

        pub(super) fn spawn_job_fixture(terminal: &Pty, mode: &str) -> Self {
            let cmd = Self::reexecute(terminal, terminal, mode);
            Self::spawn_with_tty(cmd, true, Some(terminal.master.as_raw_fd()), false)
        }

        pub(super) fn spawn_installed(
            terminal: &Pty,
            arguments: &[&std::ffi::OsStr],
            job_child: bool,
        ) -> Self {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_marmish"));
            cmd.args(arguments)
                .stdin(Stdio::from(terminal.slave.try_clone().unwrap()))
                .stdout(Stdio::from(terminal.slave.try_clone().unwrap()))
                .stderr(Stdio::from(terminal.slave.try_clone().unwrap()));
            Self::spawn_with_tty(cmd, true, None, job_child)
        }

        fn spawn_with_tty(
            mut cmd: Command,
            take_controlling: bool,
            inherited: Option<libc::c_int>,
            job_child: bool,
        ) -> Self {
            assert!(inherited != Some(CHANNEL_FD));
            // Only async-signal-safe operations run between fork and exec.
            unsafe {
                cmd.pre_exec(move || {
                    if let Some(fd) = inherited
                        && libc::dup2(fd, CHANNEL_FD) != CHANNEL_FD
                    {
                        return Err(io::Error::last_os_error());
                    }
                    let mut mask = MaybeUninit::uninit();
                    if libc::sigemptyset(mask.as_mut_ptr()) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    let mut mask = mask.assume_init();
                    for signal in signals::MANAGED {
                        if libc::sigaddset(&mut mask, signal) != 0 {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    let error = libc::pthread_sigmask(libc::SIG_BLOCK, &mask, std::ptr::null_mut());
                    if error != 0 {
                        return Err(io::Error::from_raw_os_error(error));
                    }
                    if job_child {
                        // The test parent owns the same tty session, just as a
                        // job-control shell owns the calling client's session.
                        if libc::setpgid(0, 0) == -1
                            || libc::tcsetpgrp(libc::STDIN_FILENO, libc::getpgrp()) == -1
                        {
                            return Err(io::Error::last_os_error());
                        }
                    } else if libc::setsid() == -1
                        || (take_controlling
                            && libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1)
                    {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            Self(cmd.spawn().unwrap())
        }

        pub(super) fn signal(&self, signal: libc::c_int) {
            assert_eq!(
                unsafe { libc::kill(self.0.id().try_into().unwrap(), signal) },
                0
            );
        }

        pub(super) fn stopped(&self) {
            let began = Instant::now();
            while began.elapsed() < LIMIT {
                let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        self.0.id(),
                        info.as_mut_ptr(),
                        libc::WSTOPPED | libc::WNOWAIT | libc::WNOHANG,
                    )
                };
                assert_eq!(result, 0);
                let info = unsafe { info.assume_init() };
                if unsafe { info.si_pid() } != 0 {
                    assert_eq!(info.si_code, libc::CLD_STOPPED);
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("frontend failed to stop");
        }

        pub(super) fn finish(&mut self, code: i32) {
            let start = Instant::now();
            while start.elapsed() < LIMIT {
                if let Some(status) = self.0.try_wait().unwrap() {
                    assert_eq!(status.code(), Some(code));
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            panic!("frontend failed to exit");
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
}

fn child_write(tty: &mut tty::Tty, bytes: &[u8]) {
    let start = Instant::now();
    let mut rest = bytes;
    while !rest.is_empty() && start.elapsed() < LIMIT {
        if poll(tty.fd(), libc::POLLOUT, 10) {
            let count = tty.write(rest).unwrap().unwrap_or(0);
            rest = &rest[count..];
        }
    }
    assert!(rest.is_empty());
}

fn run_child() -> i32 {
    let mode = std::env::var(MODE).unwrap();
    if mode == "signal-rollback" {
        signals::verify_install_rollback();
        return 0;
    }
    match mode.as_str() {
        "installed-job-default" => {
            installed_cooked_admission_resize_and_resume(fixture::Pty::inherited_job_pty());
            return 0;
        }
        "installed-job-observer" => {
            installed_observer_suspend(fixture::Pty::inherited_job_pty());
            return 0;
        }
        "installed-job-new" => {
            installed_cancels_new(fixture::Pty::inherited_job_pty());
            return 0;
        }
        "installed-job-prompt" | "installed-job-offered-new" => {
            installed_offer_suspend(
                fixture::Pty::inherited_job_pty(),
                mode == "installed-job-offered-new",
            );
            return 0;
        }
        _ => {}
    }
    let signals = signals::Signals::install().unwrap();
    assert!(signals::Signals::install().is_err());
    match mode.as_str() {
        "refuse" | "no-control" => {
            assert!(tty::Tty::open().is_err());
            io::stdout().write_all(REFUSED).unwrap();
            io::stdout().flush().unwrap();
            return 0;
        }
        "unwind" => {
            let failed = std::panic::catch_unwind(|| {
                let mut tty = tty::Tty::open().unwrap();
                tty.require_cooked().unwrap();
                tty.flush_input().unwrap();
                tty.raw().unwrap();
                panic!("injected fixture unwind");
            });
            assert!(failed.is_err());
            return 0;
        }
        "bridge" | "observer" | "suspend" | "hup" | "backpressure" | "closed" => {
            let observer = mode == "observer";
            let mut tty = tty::Tty::open().unwrap();
            // Child-only descriptor duplicated by this test's pre_exec.
            let socket = unsafe { UnixStream::from_raw_fd(CHANNEL_FD) };
            socket.set_nonblocking(true).unwrap();
            if mode == "backpressure" {
                let size: libc::c_int = 4096;
                assert_eq!(
                    unsafe {
                        libc::setsockopt(
                            socket.as_raw_fd(),
                            libc::SOL_SOCKET,
                            libc::SO_SNDBUF,
                            std::ptr::from_ref(&size).cast(),
                            std::mem::size_of_val(&size) as libc::socklen_t,
                        )
                    },
                    0
                );
            }
            let mut bridge = Bridge { socket, observer };
            let result =
                terminal_frontend::run(&mut tty, &signals, &mut bridge, observer, None, false);
            result.restored_termios.unwrap();
            assert!(result.restored_display);
            use terminal_frontend::Exit;
            let expected = match mode.as_str() {
                "suspend" => Exit::Suspended,
                "hup" => Exit::Interrupted(libc::SIGHUP),
                "backpressure" => Exit::Interrupted(libc::SIGTERM),
                "closed" => Exit::Closed,
                _ => Exit::Detached,
            };
            assert_eq!(result.reason.unwrap(), expected);
            drop(bridge);
            if mode == "suspend" {
                assert_eq!(unsafe { libc::raise(libc::SIGSTOP) }, 0);
            }
            return 0;
        }
        "framed-detach" | "framed-close" => {
            let mut tty = tty::Tty::open().unwrap();
            // Child-only descriptor duplicated by this test's pre_exec.
            let socket = unsafe { UnixStream::from_raw_fd(CHANNEL_FD) };
            let mut channel = terminal_socket_session::ForegroundSocket::new(
                socket,
                crate::terminal_ipc_wire::Role::Writer,
            )
            .unwrap();
            let result =
                terminal_frontend::run(&mut tty, &signals, &mut channel, false, None, false);
            result.restored_termios.unwrap();
            assert!(result.restored_display);
            let expected = if mode == "framed-close" {
                terminal_frontend::Exit::PeerClosed(crate::terminal_ipc_wire::Close::Ended)
            } else {
                terminal_frontend::Exit::Detached
            };
            assert_eq!(result.reason.unwrap(), expected);
            return 0;
        }
        "active" => {}
        _ => panic!("unexpected fixture mode"),
    }
    let mut tty = tty::Tty::open().unwrap();
    assert_eq!(tty.geometry().unwrap(), (37, 99));
    let before = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL) };
    tty.raw().unwrap();
    assert_eq!(
        unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL) },
        before
    );
    child_write(&mut tty, READY);

    let started = Instant::now();
    let mut inbuf = [0; 32];
    while started.elapsed() < LIMIT * 2 {
        let mut fds = [
            libc::pollfd {
                fd: tty.fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: signals.fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        unsafe { libc::poll(fds.as_mut_ptr(), 2, 10) };
        let events = signals.take();
        if let Some(signal) = events.termination {
            tty.restore().unwrap();
            return 128 + signal;
        }
        if events.has(libc::SIGTSTP) {
            tty.restore().unwrap();
            assert_eq!(unsafe { libc::raise(libc::SIGSTOP) }, 0);
            if let Some(signal) = signals.take().termination {
                return 128 + signal;
            }
            tty.raw().unwrap();
            child_write(&mut tty, RESUMED);
        }
        if events.has(libc::SIGWINCH) && matches!(tty.geometry(), Ok((44, 111))) {
            child_write(&mut tty, RESIZED);
        }
        if fds[0].revents & libc::POLLIN != 0
            && let Some(size) = tty.read(&mut inbuf).unwrap()
        {
            child_write(&mut tty, &inbuf[..size]);
        }
    }
    panic!("frontend fixture deadline");
}

#[test]
#[ignore = "re-executed with isolated terminal by the local fixture"]
fn terminal_frontend_child() {
    if std::env::var_os(MODE).is_some() {
        std::process::exit(run_child());
    }
}

fn assert_attrs(before: &libc::termios, current: &libc::termios) {
    assert_eq!(current.c_iflag, before.c_iflag);
    assert_eq!(current.c_oflag, before.c_oflag);
    assert_eq!(current.c_cflag, before.c_cflag);
    assert_eq!(current.c_lflag, before.c_lflag);
    assert_eq!(current.c_cc, before.c_cc);
}

#[test]
#[ignore = "isolated process-wide native signal installation"]
fn terminal_frontend_rolls_back_each_failed_signal_installation() {
    let terminal = fixture::Pty::new();
    let before = terminal.attrs();
    let mut child = fixture::Held::spawn(&terminal, &terminal, "signal-rollback");
    child.finish(0);
    assert_attrs(&before, &terminal.attrs());
}

#[test]
#[ignore = "native real controlling terminal and process signals"]
fn terminal_frontend_owns_restores_and_fences_tty() {
    let mut terminal = fixture::Pty::new();
    let before = terminal.attrs();
    let mut child = fixture::Held::spawn(&terminal, &terminal, "active");
    terminal.read_through(READY);
    assert_eq!(terminal.attrs().c_lflag & (libc::ISIG | libc::ICANON), 0);
    child.signal(libc::SIGCONT);
    terminal.master.write_all(b"X\x03\x1aY").unwrap();
    terminal.read_through(b"X\x03\x1aY");
    terminal.resize(44, 111);
    terminal.read_through(RESIZED);
    child.signal(libc::SIGTSTP);
    child.stopped();
    assert_attrs(&before, &terminal.attrs());
    child.signal(libc::SIGCONT);
    terminal.read_through(RESUMED);
    assert_eq!(terminal.attrs().c_lflag & (libc::ISIG | libc::ICANON), 0);
    child.signal(libc::SIGHUP);
    child.finish(128 + libc::SIGHUP);
    assert_attrs(&before, &terminal.attrs());

    let mut other = fixture::Pty::new();
    let mut denied = fixture::Held::spawn(&terminal, &other, "refuse");
    other.read_through(REFUSED);
    denied.finish(0);

    let mut no_control = fixture::Held::spawn(&terminal, &terminal, "no-control");
    terminal.read_through(REFUSED);
    no_control.finish(0);
    assert_attrs(&before, &terminal.attrs());

    let mut unwind = fixture::Held::spawn(&terminal, &terminal, "unwind");
    unwind.finish(0);
    assert_attrs(&before, &terminal.attrs());
}

struct Bridge {
    socket: UnixStream,
    observer: bool,
}

impl terminal_frontend::Channel for Bridge {
    fn fd(&self) -> libc::c_int {
        self.socket.as_raw_fd()
    }

    fn read_output(&mut self, bytes: &mut [u8]) -> Result<Option<usize>> {
        bridge_io(self.socket.read(bytes))
    }

    fn write_input(&mut self, bytes: &[u8]) -> Result<Option<usize>> {
        assert!(!self.observer);
        bridge_io(self.socket.write(bytes))
    }

    fn resize(&mut self, rows: u16, cols: u16) -> Result<bool> {
        assert!(!self.observer);
        assert_eq!((rows, cols), (44, 111));
        let Some(count) = bridge_io(self.socket.write(RESIZED))? else {
            return Ok(false);
        };
        assert_eq!(count, RESIZED.len());
        Ok(true)
    }
}

fn bridge_io(result: io::Result<usize>) -> Result<Option<usize>> {
    match result {
        Ok(count) => Ok(Some(count)),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(Error::Invalid("terminal_fixture_channel")),
    }
}

#[test]
#[ignore = "native foreground PTY with local output and input bridge"]
fn terminal_frontend_displays_sanitized_output_and_restores() {
    for mode in ["bridge", "observer"] {
        let mut frontend = fixture::Pty::new();
        let before = frontend.attrs();
        let (mut owner, child_socket) = UnixStream::pair().unwrap();
        owner.set_read_timeout(Some(LIMIT)).unwrap();
        let mut child = fixture::Held::spawn_inner(&frontend, &frontend, mode, Some(&child_socket));
        drop(child_socket);
        owner
            .write_all(b"\x1b[?1049h\x1b]52;c;blocked\x07\x1b[32;1mrender\x1b[0m\x1b[?2004h")
            .unwrap();
        frontend.read_through(b"\x1b[?1049h\x1b[32;1mrender\x1b[0m\x1b[?2004h");
        frontend
            .master
            .write_all(b"literal\x03\x1a\r\x1b[200~pasted\r~.inside\x1b[201~\r~.extra")
            .unwrap();
        frontend.read_through(DETACH_MARKER);
        child.finish(0);
        frontend.drain_after_exit();
        assert_eq!(
            frontend.captured.strip_prefix(b"\r\nrunning 1 test\r\n").unwrap(),
            b"\x1b[?1049h\x1b[32;1mrender\x1b[0m\x1b[?2004h\x1b[?1049l\x1b[0m\x1b[0 q\x1b[?25h\x1b[r\x1b[?6l\x1b[?7h\x1b[4l\x1b(B\x1b)B\x0f\x1b[?1l\x1b>\x1b[?2004l\r\n[marmish: detached]\r\n"
        );
        let mut actual = Vec::new();
        owner.read_to_end(&mut actual).unwrap();
        assert_eq!(
            actual,
            if mode == "bridge" {
                b"literal\x03\x1a\r\x1b[200~pasted\r~.inside\x1b[201~\r".as_slice()
            } else {
                b""
            }
        );
        assert_attrs(&before, &frontend.attrs());
    }
}

struct InstalledIpc {
    state: tempfile::TempDir,
    socket: std::os::unix::net::UnixListener,
    path: std::path::PathBuf,
}

impl InstalledIpc {
    const ACCOUNT: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    const SESSION: &str = "0123456789abcdef0123456789abcdef";
    const SLOT: &str = "a123456789abcdef0123456789abcdef";
    const OTHER_SLOT: &str = "b123456789abcdef0123456789abcdef";
    const RELAY: &str = "wss://relay.example/";

    fn new() -> Self {
        use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
        let state = tempfile::Builder::new()
            .prefix("mw-tty-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let home = state.path().join("default");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&home)
            .unwrap();
        let config = serde_json::json!({
            "version": 1, "role": "client", "tor": "auto", "relays": [Self::RELAY],
            "trusted_remotes": [], "execution_clients": [], "command_policy": {"version": 1},
            "cwd": null, "limits": {}, "aliases": {}
        });
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(home.join("config.json"))
            .unwrap();
        serde_json::to_writer(&mut file, &config).unwrap();
        let path = state.path().join("m");
        let socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
        Self {
            state,
            socket,
            path,
        }
    }

    fn spawn(&self, tty: &fixture::Pty, new: bool, job_child: bool) -> fixture::Held {
        let flags: &[&str] = if new { &["--new"] } else { &[] };
        self.spawn_with(tty, flags, job_child)
    }

    fn spawn_with(&self, tty: &fixture::Pty, flags: &[&str], job_child: bool) -> fixture::Held {
        use nostr::nips::nip19::ToBech32 as _;
        use std::ffi::OsStr;
        let npub = nostr::key::PublicKey::from_hex(Self::ACCOUNT)
            .unwrap()
            .to_bech32()
            .unwrap();
        let mut args = vec![
            OsStr::new("--state-dir"),
            self.state.path().as_os_str(),
            OsStr::new("--socket"),
            self.path.as_os_str(),
            OsStr::new("terminal"),
            OsStr::new(&npub),
        ];
        args.extend(flags.iter().map(OsStr::new));
        fixture::Held::spawn_installed(tty, &args, job_child)
    }

    fn accept(&self) -> UnixStream {
        assert!(poll(
            self.socket.as_raw_fd(),
            libc::POLLIN,
            LIMIT.as_millis() as i32
        ));
        let (stream, _) = self.socket.accept().unwrap();
        stream.set_read_timeout(Some(LIMIT)).unwrap();
        stream.set_write_timeout(Some(LIMIT)).unwrap();
        stream
    }

    fn reply(stream: &mut UnixStream, value: serde_json::Value) {
        let data = value.to_string();
        stream
            .write_all(&(data.len() as u32).to_be_bytes())
            .unwrap();
        stream.write_all(data.as_bytes()).unwrap();
    }

    fn read(stream: &mut UnixStream) -> serde_json::Value {
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let length = u32::from_be_bytes(length) as usize;
        assert!((1..=4096).contains(&length));
        let mut data = vec![0; length];
        stream.read_exact(&mut data).unwrap();
        serde_json::from_slice(&data).unwrap()
    }

    fn preparing(&self) -> UnixStream {
        use serde_json::json;
        let mut status = self.accept();
        assert_eq!(Self::read(&mut status), json!({"operation":"status"}));
        Self::reply(
            &mut status,
            json!({"ok":true,"result":{"state":"ready"},"error":null}),
        );
        drop(status);
        let mut prepare = self.accept();
        let request = Self::read(&mut prepare);
        assert_eq!(request["operation"], "command_prepare");
        assert_eq!(request["remote"], Self::ACCOUNT);
        assert_eq!(request["relays"], json!([Self::RELAY]));
        assert!(request["fresh"].is_null());
        prepare
    }

    fn pending(&self, attach: &str, geometry: (u16, u16)) -> UnixStream {
        self.pending_target(attach, geometry, None, None)
    }

    fn pending_target(
        &self,
        attach: &str,
        geometry: (u16, u16),
        slot: Option<&str>,
        offer: Option<&str>,
    ) -> UnixStream {
        use serde_json::json;
        let mut prepare = self.preparing();
        Self::reply(
            &mut prepare,
            json!({
                "ok": true,
                "command": {"state":"ready","session":Self::SESSION,"local_account":Self::ACCOUNT}
            }),
        );
        drop(prepare);
        self.next_start(attach, geometry, slot, offer)
    }

    fn next_start(
        &self,
        attach: &str,
        (rows, columns): (u16, u16),
        slot: Option<&str>,
        offer: Option<&str>,
    ) -> UnixStream {
        use serde_json::json;
        let mut expect = json!({"operation":"terminal_start","request":{
            "remote":Self::ACCOUNT,"session":Self::SESSION,"relays":[Self::RELAY],
            "rows":rows,"columns":columns,"attach":attach
        }});
        if let Some(slot) = slot {
            expect["request"]["session_slot"] = json!(slot);
        }
        if let Some(offer) = offer {
            expect["request"]["offer"] = json!(offer);
        }
        let mut start = self.accept();
        assert_eq!(Self::read(&mut start), expect);
        start
    }

    fn ready(stream: &mut UnixStream, role: &str, slot: &str) {
        Self::reply(
            stream,
            serde_json::json!({"ok":true,"terminal_stream":role,"slot":slot}),
        );
    }
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_frontend_cooked_admission_resize_and_resume() {
    let tty = fixture::Pty::new();
    fixture::Held::spawn_job_fixture(&tty, "installed-job-default").finish(0);
}

fn installed_cooked_admission_resize_and_resume(mut tty: fixture::Pty) {
    use super::terminal_ipc_wire::{Close, Direction, Record, Role, encode};
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, true);
    let mut first = ipc.pending("default", (37, 99));
    assert_attrs(&before, &tty.attrs());
    tty.resize(42, 102);
    InstalledIpc::ready(&mut first, "writer", InstalledIpc::SLOT);
    let resize = encode(
        Direction::ToDaemon,
        Role::Writer,
        Record::Resize {
            rows: 42,
            columns: 102,
        },
    )
    .unwrap();
    let mut bytes = vec![0; resize.len()];
    first.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, resize);
    assert_ne!(
        tty.attrs().c_lflag & libc::ICANON,
        before.c_lflag & libc::ICANON
    );
    child.signal(libc::SIGTSTP);
    tty.read_through(b"\r\n[marmish: suspended]\r\n");
    child.stopped();
    assert_attrs(&before, &tty.attrs());
    assert_eq!(first.read(&mut [0]).unwrap(), 0);

    child.signal(libc::SIGCONT);
    let mut resumed = ipc.pending_target("default", (42, 102), Some(InstalledIpc::SLOT), None);
    assert_attrs(&before, &tty.attrs());
    InstalledIpc::ready(&mut resumed, "writer", InstalledIpc::SLOT);
    let ended = encode(
        Direction::ToClient,
        Role::Writer,
        Record::Close(Close::Ended),
    )
    .unwrap();
    resumed.write_all(&ended).unwrap();
    resumed.shutdown(std::net::Shutdown::Write).unwrap();
    tty.read_through(b"\r\n[marmish: terminal ended]\r\n");
    child.finish(0);
    assert_attrs(&before, &tty.attrs());
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_frontend_rejects_old_or_malformed_daemon_and_cancels_new() {
    use serde_json::json;
    for (reply, category) in [
        (
            json!({"ok":false,"result":null,"error":"invalid_request"}),
            b"terminal_daemon_upgrade_required".as_slice(),
        ),
        (
            json!({"ok":true,"terminal_stream":"writer"}),
            b"terminal_daemon_upgrade_required".as_slice(),
        ),
        (
            json!({"ok":false,"error":"terminal_mode_unavailable"}),
            b"terminal_daemon_upgrade_required".as_slice(),
        ),
        (
            json!({"ok":false,"result":null,"error":"terminal_grant_denied"}),
            b"terminal_grant_denied".as_slice(),
        ),
        (
            json!({"ok":true,"terminal_stream":"writer","extra":true}),
            b"terminal_start_response".as_slice(),
        ),
    ] {
        let mut tty = fixture::Pty::new();
        let before = tty.attrs();
        let ipc = InstalledIpc::new();
        let mut child = ipc.spawn(&tty, false, false);
        let mut request = ipc.pending("default", (37, 99));
        assert_attrs(&before, &tty.attrs());
        InstalledIpc::reply(&mut request, reply);
        tty.read_through(category);
        child.finish(255);
        assert_attrs(&before, &tty.attrs());
    }

    let tty = fixture::Pty::new();
    fixture::Held::spawn_job_fixture(&tty, "installed-job-new").finish(0);
}

fn installed_cancels_new(mut tty: fixture::Pty) {
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, true, true);
    let mut request = ipc.pending("new", (37, 99));
    child.signal(libc::SIGTSTP);
    child.stopped();
    assert_eq!(request.read(&mut [0]).unwrap(), 0);
    assert_attrs(&before, &tty.attrs());
    child.signal(libc::SIGCONT);
    tty.read_through(b"terminal_new_reattach_unavailable");
    child.finish(255);
    assert!(!poll(ipc.socket.as_raw_fd(), libc::POLLIN, 0));
    assert_attrs(&before, &tty.attrs());
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_frontend_orphan_suspend_and_cooked_interrupt() {
    let tty = fixture::Pty::new();
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, false);
    let mut first = ipc.pending("default", (37, 99));
    assert_attrs(&before, &tty.attrs());

    child.signal(libc::SIGTSTP);
    assert_eq!(first.read(&mut [0]).unwrap(), 0);
    // This process group is orphaned. It progresses without any SIGCONT.
    let mut resumed = ipc.pending("default", (37, 99));
    child.signal(libc::SIGINT);
    assert_eq!(resumed.read(&mut [0]).unwrap(), 0);
    child.finish(128 + libc::SIGINT);
    assert_attrs(&before, &tty.attrs());
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_frontend_reports_frozen_command_binding() {
    use serde_json::json;
    let mut tty = fixture::Pty::new();
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, false);
    let mut prepare = ipc.preparing();
    InstalledIpc::reply(
        &mut prepare,
        json!({
            "ok": true,
            "command": {"state":"failure","category":"command_preparation_binding","handle":null}
        }),
    );
    tty.read_through(b"terminal_command_binding");
    child.finish(255);
    assert!(!poll(ipc.socket.as_raw_fd(), libc::POLLIN, 0));
    assert_attrs(&before, &tty.attrs());
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_frontend_reports_typed_idle_close() {
    use super::terminal_ipc_wire::{Close, Direction, Record, Role, encode};
    let mut tty = fixture::Pty::new();
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, false);
    let mut stream = ipc.pending("default", (37, 99));
    InstalledIpc::ready(&mut stream, "writer", InstalledIpc::SLOT);
    let idle = encode(
        Direction::ToClient,
        Role::Writer,
        Record::Close(Close::Idle),
    )
    .unwrap();
    stream.write_all(&idle).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    tty.read_through(b"terminal_idle");
    child.finish(255);
    assert_attrs(&before, &tty.attrs());
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_frontend_preserves_hangup_exit() {
    use super::terminal_ipc_wire::{Direction, Record, Role, encode};
    let mut tty = fixture::Pty::new();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, false);
    let mut stream = ipc.pending("default", (37, 99));
    InstalledIpc::ready(&mut stream, "writer", InstalledIpc::SLOT);
    let output = encode(
        Direction::ToClient,
        Role::Writer,
        Record::Output {
            id: 1.try_into().unwrap(),
            bytes: READY,
        },
    )
    .unwrap();
    stream.write_all(&output).unwrap();
    tty.read_through(READY);
    drop(tty.master);
    child.finish(128 + libc::SIGHUP);
}

#[test]
#[ignore = "installed frontend process"]
fn terminal_installed_frontend_requires_tty_before_preparation() {
    let output = Command::new(env!("CARGO_BIN_EXE_marmish"))
        .args([
            "--state-dir",
            "/nonexistent-marmish-fixture",
            "terminal",
            "fixture",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(255));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("terminal_tty"));
}

#[test]
#[ignore = "native PTY with real frontend resize, suspend, termination and EOF"]
fn terminal_frontend_routes_signals_and_restores_on_ipc_loss() {
    for (mode, marker) in [
        ("suspend", b"\r\n[marmish: suspended]\r\n".as_slice()),
        ("hup", b"\r\n[marmish: interrupted]\r\n"),
        ("backpressure", b"\r\n[marmish: interrupted]\r\n"),
        ("closed", b"\r\n[marmish: terminal error]\r\n"),
    ] {
        let mut frontend = fixture::Pty::new();
        let before = frontend.attrs();
        let (mut owner, socket) = UnixStream::pair().unwrap();
        owner.set_read_timeout(Some(LIMIT)).unwrap();
        let mut child = fixture::Held::spawn_inner(&frontend, &frontend, mode, Some(&socket));
        drop(socket);
        owner.write_all(READY).unwrap();
        frontend.read_through(READY);
        frontend.resize(44, 111);
        let mut resize = [0u8; RESIZED.len()];
        owner.read_exact(&mut resize).unwrap();
        assert_eq!(&resize, RESIZED);
        match mode {
            "suspend" => child.signal(libc::SIGTSTP),
            "hup" => child.signal(libc::SIGHUP),
            "backpressure" => {
                let fd = frontend.master.as_raw_fd();
                let before = unsafe { libc::fcntl(fd, libc::F_GETFL) };
                assert!(before >= 0);
                assert_eq!(
                    unsafe { libc::fcntl(fd, libc::F_SETFL, before | libc::O_NONBLOCK) },
                    0
                );
                let until = Instant::now() + LIMIT;
                let mut filled = 0;
                while Instant::now() < until {
                    match frontend.master.write(&[b'x'; 4096]) {
                        Ok(count) => filled += count,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            if filled >= 40 * 1024 && !poll(fd, libc::POLLOUT, 10) {
                                break;
                            }
                        }
                        Err(error) => panic!("fixture PTY write: {error}"),
                    }
                }
                assert!(filled >= 40 * 1024 && Instant::now() < until);
                child.signal(libc::SIGTERM);
            }
            "closed" => drop(owner),
            _ => unreachable!(),
        }
        frontend.read_through(marker);
        if mode == "suspend" {
            child.stopped();
            assert_attrs(&before, &frontend.attrs());
            child.signal(libc::SIGCONT);
        }
        child.finish(0);
        assert_attrs(&before, &frontend.attrs());
    }
}

#[test]
#[ignore = "native foreground frontend plus the real private tmux attachment worker"]
fn terminal_frontend_renders_native_tmux() {
    use super::{
        CheckedLaunch, Fixture, KEYS, Shutdown, native_key, pane_proof, trusted_tmux,
        wait_pane_ready,
    };
    use marmish_native::terminal_backend::ReadStatus;
    use std::os::fd::AsFd;
    use std::path::Path;

    let (tmux, version) = trusted_tmux();
    let tmux_fixture = Fixture::start(&tmux);
    tmux_fixture.start_pane();
    let proof = wait_pane_ready(&tmux_fixture);
    let daemon = Path::new(env!("CARGO_BIN_EXE_marmishd"))
        .canonicalize()
        .unwrap();
    let socket = tmux_fixture.socket.canonicalize().unwrap();
    let launch = CheckedLaunch::new(&daemon, &tmux, &socket, &tmux_fixture.nonce).unwrap();
    let started = Instant::now();
    let mut attachment = launch.launch(1, 37, 99).unwrap();

    let mut frontend = fixture::Pty::new();
    let before = frontend.attrs();
    let (mut owner, child_socket) = UnixStream::pair().unwrap();
    owner
        .set_read_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    owner
        .set_write_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    let mut child = fixture::Held::spawn_inner(&frontend, &frontend, "bridge", Some(&child_socket));
    drop(child_socket);
    let (first_render, restore, mut samples, output_writes) = std::thread::scope(|scope| {
        let bridge = scope.spawn(move || {
            let mut output = [0; 4096];
            let mut keyboard = [0; 4096];
            let until = Instant::now() + LIMIT * 2;
            let mut writes = 0usize;
            owner
                .write_all(b"\x1b]52;c;not-visible\x07\x1b[?1049h\x1b[32;1mfixture-color\x1b[0m")
                .unwrap();
            loop {
                assert!(Instant::now() < until, "frontend fixture bridge limit");
                let ready = attachment.poll(false, Some(owner.as_fd()), until).unwrap();
                if ready.wake {
                    let count = owner.read(&mut keyboard).unwrap();
                    if count == 0 {
                        break;
                    }
                    for &key in &keyboard[..count] {
                        native_key(&mut attachment, 1, key);
                    }
                }
                if ready.readable {
                    match attachment.read(&mut output).unwrap() {
                        ReadStatus::Data(n) => {
                            owner.write_all(&output[..n]).unwrap();
                            writes += 1;
                        }
                        ReadStatus::WouldBlock => {}
                        ReadStatus::Hangup => panic!("unexpected native tmux end"),
                    }
                }
            }
            assert_eq!(
                attachment.shutdown(Instant::now() + LIMIT).unwrap(),
                Shutdown::Reaped
            );
            writes
        });
        frontend.read_through(b"\x1b[?1049h\x1b[32;1mfixture-color\x1b[0m");
        // Tmux may publish the initial redraw in the same read as the
        // synthetic controls, so use a separate pane-generated response.
        frontend.master.write_all(b"j").unwrap();
        frontend.read_through(b"MWK_FIRST_KEY");
        let first_render = started.elapsed();
        frontend.master.write_all(b"v").unwrap();
        frontend.read_through("MWK_UTF8_é_界_END".as_bytes());
        frontend.master.write_all(b"\x03").unwrap();
        frontend.read_through(b"MWK_SIGINT");
        let mut samples = Vec::with_capacity(KEYS.len());
        for key in KEYS {
            std::thread::sleep(Duration::from_millis(5));
            let marker = format!("MWK_ACK_{}", char::from(*key));
            let sample = Instant::now();
            frontend.master.write_all(&[*key]).unwrap();
            frontend.read_through(marker.as_bytes());
            samples.push(sample.elapsed());
        }
        let before_restore = Instant::now();
        frontend.master.write_all(b"\r~.").unwrap();
        frontend.read_through(DETACH_MARKER);
        child.finish(0);
        let restore = before_restore.elapsed();
        frontend.drain_after_exit();
        assert!(!frontend.captured.contains(&0x07));
        assert!(!frontend.captured.windows(2).any(|bytes| bytes == b"\x1b]"));
        assert!(
            !frontend
                .captured
                .windows(11)
                .any(|bytes| bytes == b"not-visible")
        );
        assert!(frontend.captured.ends_with(DETACH_MARKER));
        assert_attrs(&before, &frontend.attrs());
        (first_render, restore, samples, bridge.join().unwrap())
    });
    assert!(pane_proof(&tmux_fixture) == proof);
    drop(tmux_fixture);
    samples.sort_unstable();
    println!(
        "terminal-frontend platform={}-{} tmux={version} samples={} p50_us={} p95_us={} first_pane_response_us={} detach_and_restore_us={} bridge_output_writes={output_writes}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        samples.len(),
        samples[14].as_micros(),
        samples[28].as_micros(),
        first_render.as_micros(),
        restore.as_micros(),
    );
}

#[test]
#[ignore = "native controlling PTY and real framed IPC socket"]
fn terminal_socket_frontend_native_pty() {
    use super::terminal_ipc_wire::{Close, Role};
    use terminal_socket_session::{DaemonSocket, ServerRead};
    use terminal_socket_state::InputEffect;

    for mode in ["framed-detach", "framed-close"] {
        let mut frontend = fixture::Pty::new();
        let before = frontend.attrs();
        let (owner, socket) = UnixStream::pair().unwrap();
        owner.set_nonblocking(true).unwrap();
        let mut daemon = DaemonSocket::new(owner, Role::Writer);
        let mut child = fixture::Held::spawn_inner(&frontend, &frontend, mode, Some(&socket));
        drop(socket);
        let started = Instant::now();
        let limit = started + LIMIT;
        daemon.flush(Instant::now(), limit).unwrap();

        let screen = b"\x1b]52;c;refuse\x07\x1b[32;1m\xe2\x98\x83ready\x1b[0m";
        assert_eq!(
            daemon.offer(7, screen, Instant::now(), limit).unwrap(),
            None
        );
        daemon.flush(Instant::now(), limit).unwrap();
        assert_eq!(
            daemon.offer(7, screen, Instant::now(), limit).unwrap(),
            None
        );
        frontend.read_through(b"\x1b[32;1m\xe2\x98\x83ready\x1b[0m");
        assert!(
            !frontend
                .captured
                .windows(6)
                .any(|window| window == b"refuse")
        );
        assert!(poll(daemon.fd(), libc::POLLIN, 2000));
        assert_eq!(daemon.read(Instant::now()).unwrap(), ServerRead::Received);
        assert_eq!(
            daemon.offer(7, screen, Instant::now(), limit).unwrap(),
            Some(screen.len())
        );
        eprintln!(
            "framed PTY first physical display and receipt: {} us",
            started.elapsed().as_micros()
        );

        if mode == "framed-detach" {
            frontend.master.write_all(b"raw\x03\x1a").unwrap();
            assert!(poll(daemon.fd(), libc::POLLIN, 2000));
            assert_eq!(daemon.read(Instant::now()).unwrap(), ServerRead::Received);
            assert!(matches!(
                daemon.next_input(),
                Some(InputEffect::Bytes(b"raw\x03\x1a"))
            ));
            daemon.input_written(5).unwrap();

            frontend.resize(44, 111);
            assert!(poll(daemon.fd(), libc::POLLIN, 2000));
            assert_eq!(daemon.read(Instant::now()).unwrap(), ServerRead::Received);
            assert!(matches!(
                daemon.next_input(),
                Some(InputEffect::Resize((44, 111)))
            ));
            daemon.resize_applied().unwrap();
            daemon.flush(Instant::now(), limit).unwrap();

            frontend.master.write_all(b"\r~.ignored").unwrap();
            assert!(poll(daemon.fd(), libc::POLLIN, 2000));
            assert_eq!(daemon.read(Instant::now()).unwrap(), ServerRead::Detached);
        } else {
            daemon.begin_close(Close::Ended);
            daemon.flush(Instant::now(), limit).unwrap();
            drop(daemon);
        }
        let marker = if mode == "framed-close" {
            b"\r\n[marmish: terminal ended]\r\n".as_slice()
        } else {
            DETACH_MARKER
        };
        frontend.read_through(marker);
        child.finish(0);
        frontend.drain_after_exit();
        assert_attrs(&before, &frontend.attrs());
    }
}

fn installed_offer() -> String {
    format!(
        "{}{}{}0000000000000001",
        "0a".repeat(16),
        InstalledIpc::SLOT,
        "0b".repeat(16)
    )
}

fn local_frame(
    role: super::terminal_ipc_wire::Role,
    record: super::terminal_ipc_wire::Record<'_>,
) -> Vec<u8> {
    super::terminal_ipc_wire::encode(super::terminal_ipc_wire::Direction::ToClient, role, record)
        .unwrap()
}

fn check_local_frame(
    stream: &mut UnixStream,
    role: super::terminal_ipc_wire::Role,
    record: super::terminal_ipc_wire::Record<'_>,
) {
    let bytes = super::terminal_ipc_wire::encode(
        super::terminal_ipc_wire::Direction::ToDaemon,
        role,
        record,
    )
    .unwrap();
    let mut actual = vec![0; bytes.len()];
    stream.read_exact(&mut actual).unwrap();
    assert_eq!(actual, bytes);
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_foreground_offered_observer_is_read_only() {
    use super::terminal_ipc_wire::{Record, Role};
    use serde_json::json;
    let mut tty = fixture::Pty::new();
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, false);
    let mut first = ipc.pending("default", (37, 99));
    let offer = installed_offer();
    InstalledIpc::reply(
        &mut first,
        json!({"ok":false,"terminal_busy":{"offer":offer}}),
    );
    assert_eq!(first.read(&mut [0]).unwrap(), 0);
    tty.read_through(b"[o] Observe, [t] Takeover, [n] New, [c] Cancel: ");
    assert_attrs(&before, &tty.attrs());
    tty.master.write_all(b"o\nno remote input\n").unwrap();

    let mut observed = ipc.next_start("observe", (37, 99), None, Some(&offer));
    InstalledIpc::ready(&mut observed, "observer", InstalledIpc::SLOT);
    let id = 1.try_into().unwrap();
    observed
        .write_all(&local_frame(
            Role::Observer,
            Record::Output { id, bytes: READY },
        ))
        .unwrap();
    tty.read_through(READY);
    check_local_frame(&mut observed, Role::Observer, Record::Ack(id));
    tty.resize(41, 109);
    tty.master.write_all(b"observer bytes\r~.").unwrap();
    check_local_frame(&mut observed, Role::Observer, Record::Detach);
    assert_eq!(observed.read(&mut [0]).unwrap(), 0);
    child.finish(0);
    assert_attrs(&before, &tty.attrs());
    assert!(!String::from_utf8_lossy(&tty.captured).contains(&offer));
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_foreground_offer_flushes_typeahead_before_writer_raw_mode() {
    for explicit_takeover in [false, true] {
        installed_offered_writer_flushes_typeahead(explicit_takeover);
    }
}

fn installed_offered_writer_flushes_typeahead(explicit_takeover: bool) {
    use super::terminal_ipc_wire::{Close, Record, Role};
    use serde_json::json;
    let mut tty = fixture::Pty::new();
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let (mut child, mut first) = if explicit_takeover {
        (
            ipc.spawn_with(&tty, &["--takeover"], false),
            ipc.pending("takeover", (37, 99)),
        )
    } else {
        (
            ipc.spawn(&tty, false, false),
            ipc.pending("default", (37, 99)),
        )
    };
    let offer = installed_offer();
    if !explicit_takeover {
        // Pretyping a different choice must never authorize it.
        tty.master.write_all(b"t\n").unwrap();
        tty.read_through(b"t\r\n");
    }
    InstalledIpc::reply(
        &mut first,
        json!({"ok":false,"terminal_busy":{"offer":offer}}),
    );
    let (mode, slot) = if explicit_takeover {
        ("takeover", InstalledIpc::SLOT)
    } else {
        tty.read_through(b"[n] New, [c] Cancel: ");
        tty.master.write_all(b"n\n").unwrap();
        ("new", InstalledIpc::OTHER_SLOT)
    };
    let mut writer = ipc.next_start(mode, (37, 99), None, Some(&offer));
    assert_attrs(&before, &tty.attrs());
    tty.master.write_all(b"discard-me\n").unwrap();
    tty.read_through(b"discard-me\r\n");
    InstalledIpc::ready(&mut writer, "writer", slot);
    let id = 1.try_into().unwrap();
    writer
        .write_all(&local_frame(
            Role::Writer,
            Record::Credit(32.try_into().unwrap()),
        ))
        .unwrap();
    writer
        .write_all(&local_frame(
            Role::Writer,
            Record::Output { id, bytes: READY },
        ))
        .unwrap();
    tty.read_through(READY);
    let captured = String::from_utf8_lossy(&tty.captured);
    if explicit_takeover {
        assert!(!captured.contains("Terminal busy."));
    } else {
        assert!(captured.contains(InstalledIpc::OTHER_SLOT));
    }
    check_local_frame(&mut writer, Role::Writer, Record::Ack(id));
    tty.master.write_all(b"fresh").unwrap();
    check_local_frame(&mut writer, Role::Writer, Record::Input(b"fresh"));
    writer
        .write_all(&local_frame(Role::Writer, Record::Close(Close::Ended)))
        .unwrap();
    writer.shutdown(std::net::Shutdown::Write).unwrap();
    child.finish(0);
    assert_attrs(&before, &tty.attrs());
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_foreground_explicit_takeover_reports_loss_or_definitive_refusal() {
    use serde_json::json;
    for refused in [false, true] {
        let mut tty = fixture::Pty::new();
        let before = tty.attrs();
        let ipc = InstalledIpc::new();
        let mut child = ipc.spawn_with(
            &tty,
            &["--takeover", "--session", InstalledIpc::SLOT],
            false,
        );
        let mut first = ipc.pending_target("takeover", (37, 99), Some(InstalledIpc::SLOT), None);
        let offer = installed_offer();
        InstalledIpc::reply(
            &mut first,
            json!({"ok":false,"terminal_busy":{"offer":offer}}),
        );
        let mut second = ipc.next_start("takeover", (37, 99), None, Some(&offer));
        assert_attrs(&before, &tty.attrs());
        if refused {
            InstalledIpc::reply(
                &mut second,
                json!({"ok":false,"error":"terminal_offer_stale"}),
            );
        }
        drop(second);
        tty.read_through(if refused {
            b"terminal_offer_stale".as_slice()
        } else {
            b"terminal_takeover_uncertain"
        });
        child.finish(255);
        assert!(!String::from_utf8_lossy(&tty.captured).contains("Terminal busy."));
        assert!(!poll(ipc.socket.as_raw_fd(), libc::POLLIN, 0));
        assert_attrs(&before, &tty.attrs());
    }
}

#[test]
#[ignore = "native controlling PTY and installed frontend process"]
fn terminal_installed_foreground_prompt_interrupt_or_eof_discards_offer() {
    use serde_json::json;
    for interrupt in [true, false] {
        let mut tty = fixture::Pty::new();
        let before = tty.attrs();
        let ipc = InstalledIpc::new();
        let mut child = ipc.spawn(&tty, false, false);
        let mut first = ipc.pending("default", (37, 99));
        InstalledIpc::reply(
            &mut first,
            json!({"ok":false,"terminal_busy":{"offer":installed_offer()}}),
        );
        tty.read_through(b"[n] New, [c] Cancel: ");
        let code = if interrupt {
            child.signal(libc::SIGINT);
            128 + libc::SIGINT
        } else {
            tty.master.write_all(&[before.c_cc[libc::VEOF]]).unwrap();
            0
        };
        child.finish(code);
        assert!(!poll(ipc.socket.as_raw_fd(), libc::POLLIN, 0));
        assert_attrs(&before, &tty.attrs());
    }
}

#[test]
#[ignore = "native job control with an installed frontend process"]
fn terminal_installed_foreground_observer_continuation_requires_explicit_attach() {
    let tty = fixture::Pty::new();
    fixture::Held::spawn_job_fixture(&tty, "installed-job-observer").finish(0);
}

fn installed_observer_suspend(mut tty: fixture::Pty) {
    use super::terminal_ipc_wire::{Record, Role};
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn_with(&tty, &["--observe", "--session", InstalledIpc::SLOT], true);
    let mut stream = ipc.pending_target("observe", (37, 99), Some(InstalledIpc::SLOT), None);
    InstalledIpc::ready(&mut stream, "observer", InstalledIpc::SLOT);
    let id = 1.try_into().unwrap();
    stream
        .write_all(&local_frame(
            Role::Observer,
            Record::Output { id, bytes: READY },
        ))
        .unwrap();
    tty.read_through(READY);
    check_local_frame(&mut stream, Role::Observer, Record::Ack(id));
    child.signal(libc::SIGTSTP);
    tty.read_through(b"\r\n[marmish: suspended]\r\n");
    child.stopped();
    assert_attrs(&before, &tty.attrs());
    assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    child.signal(libc::SIGCONT);
    child.finish(0);
    assert!(!poll(ipc.socket.as_raw_fd(), libc::POLLIN, 0));
}

#[test]
#[ignore = "native job control with an installed frontend process"]
fn terminal_installed_foreground_suspend_discards_prompt_and_never_repeats_offered_new() {
    let tty = fixture::Pty::new();
    for mode in ["installed-job-prompt", "installed-job-offered-new"] {
        fixture::Held::spawn_job_fixture(&tty, mode).finish(0);
    }
}

fn installed_offer_suspend(mut tty: fixture::Pty, choose_new: bool) {
    use serde_json::json;
    let before = tty.attrs();
    let ipc = InstalledIpc::new();
    let mut child = ipc.spawn(&tty, false, true);
    let mut first = ipc.pending("default", (37, 99));
    let offer = installed_offer();
    InstalledIpc::reply(
        &mut first,
        json!({"ok":false,"terminal_busy":{"offer":offer}}),
    );
    tty.read_through(b"[n] New, [c] Cancel: ");
    let offered_leg = choose_new.then(|| {
        tty.master.write_all(b"n\n").unwrap();
        ipc.next_start("new", (37, 99), None, Some(&offer))
    });

    child.signal(libc::SIGTSTP);
    child.stopped();
    assert_attrs(&before, &tty.attrs());
    if let Some(mut stopped) = offered_leg {
        assert_eq!(stopped.read(&mut [0]).unwrap(), 0);
    }
    child.signal(libc::SIGCONT);
    if choose_new {
        tty.read_through(b"terminal_new_reattach_unavailable");
        child.finish(255);
    } else {
        child.finish(0);
    }
    assert!(!poll(ipc.socket.as_raw_fd(), libc::POLLIN, 0));
}
