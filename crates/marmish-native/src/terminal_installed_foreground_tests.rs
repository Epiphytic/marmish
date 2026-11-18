use super::*;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use tokio::io::unix::AsyncFd;
use tokio::process::Child;
#[path = "terminal_installed_foreground_handoff_tests.rs"]
mod handoff;
pub(super) use handoff::prove_handoff;

const SHELL: &[u8] =
    b"exec /usr/bin/env -i PATH=/usr/bin:/bin TERM=dumb LC_ALL=C PS1='MAR''MISH> ' /bin/sh -i\r";
const PROMPT: &[u8] = b"MARMISH> ";
const SCRIPT: &str = "\x15printf '[%s]\\n' 'two words; % 🐾'\r";
const EXPECTED: &str = "[two words; % 🐾]";
const MAX_SCAN: usize = 32 * 1024;
const COOKED_BUSY: &[u8] = b"Terminal busy. [o] Observe";
const RAW_FLAGS: libc::tcflag_t = libc::ECHO | libc::ICANON | libc::ISIG;

#[test]
#[ignore = "requires nonroot, installed executables, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_native_foreground() {
    measure(Workload::Foreground, "terminal_installed_foreground");
}

#[test]
#[ignore = "requires nonroot, installed executables, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_native_controls() {
    measure(Workload::Controls, "terminal_installed_controls");
}

fn measure(workload: Workload, label: &'static str) {
    run_native(move || async move {
        let started = Instant::now();
        let sample = prove_writer(workload).await;
        eprintln!(
            "{label} elapsed_us={} attach_us={} echo_us={} command_before_us={} command_after_us={}",
            started.elapsed().as_micros(),
            sample.cold.as_micros(),
            sample.echo.as_micros(),
            sample.unloaded.as_micros(),
            sample.after.as_micros(),
        );
    });
}

fn attributes(slave: &File) -> libc::termios {
    let mut modes = std::mem::MaybeUninit::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), modes.as_mut_ptr()) },
        0
    );
    unsafe { modes.assume_init() }
}

fn size(slave: &File) -> [u16; 4] {
    let mut value = std::mem::MaybeUninit::<libc::winsize>::uninit();
    assert_eq!(
        unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCGWINSZ, value.as_mut_ptr()) },
        0
    );
    let value = unsafe { value.assume_init() };
    [value.ws_row, value.ws_col, value.ws_xpixel, value.ws_ypixel]
}

fn assert_restored(slave: &File, before: &libc::termios, original_size: [u16; 4]) {
    let after = attributes(slave);
    assert_eq!(after.c_iflag, before.c_iflag);
    assert_eq!(after.c_oflag, before.c_oflag);
    assert_eq!(after.c_cflag, before.c_cflag);
    assert_eq!(after.c_lflag, before.c_lflag);
    assert_eq!(after.c_cc, before.c_cc);
    #[cfg(target_os = "linux")]
    assert_eq!(after.c_line, before.c_line);
    assert_eq!(unsafe { libc::cfgetispeed(&after) }, unsafe {
        libc::cfgetispeed(before)
    });
    assert_eq!(unsafe { libc::cfgetospeed(&after) }, unsafe {
        libc::cfgetospeed(before)
    });
    assert_eq!(size(slave), original_size);
}

async fn read_master(pty: &AsyncFd<File>, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        let mut ready = pty.readable().await?;
        if let Ok(result) = ready.try_io(|fd| fd.get_ref().read(buffer)) {
            return result;
        }
    }
}

async fn write_master(pty: &AsyncFd<File>, bytes: &[u8]) -> io::Result<usize> {
    loop {
        let mut ready = pty.writable().await?;
        if let Ok(result) = ready.try_io(|fd| fd.get_ref().write(bytes)) {
            return result;
        }
    }
}

async fn input(pty: &AsyncFd<File>, mut bytes: &[u8]) {
    timeout(IO, async {
        while !bytes.is_empty() {
            let sent = write_master(pty, bytes).await.unwrap();
            assert_ne!(sent, 0, "foreground PTY write made no progress");
            bytes = &bytes[sent..];
        }
    })
    .await
    .expect("foreground PTY write deadline");
}

async fn detach(pty: &AsyncFd<File>) {
    let sent = timeout(IO, write_master(pty, b"~."))
        .await
        .expect("tilde-dot write deadline")
        .unwrap();
    assert_eq!(sent, 2, "atomic tilde-dot input");
}

async fn output(pty: &AsyncFd<File>, expected: &[u8], stage: &'static str) -> Vec<u8> {
    timeout(IO, async {
        let mut seen = Vec::new();
        let mut chunk = [0; 1024];
        while seen.len() < MAX_SCAN {
            let remaining = MAX_SCAN - seen.len();
            let read = read_master(pty, &mut chunk[..remaining.min(1024)])
                .await
                .unwrap_or_else(|error| panic!("foreground PTY {stage} read: {:?}", error.kind()));
            assert_ne!(read, 0, "foreground PTY {stage} closed");
            seen.extend_from_slice(&chunk[..read]);
            if seen.windows(expected.len()).any(|bytes| bytes == expected) {
                return seen;
            }
        }
        panic!("foreground PTY {stage} exceeded scan limit");
    })
    .await
    .unwrap_or_else(|_| panic!("foreground PTY {stage} output deadline"))
}

async fn drain_master(pty: &AsyncFd<File>) {
    let mut chunk = [0; 4096];
    while matches!(read_master(pty, &mut chunk).await, Ok(1..)) {}
}

async fn reap(
    child: &mut Child,
    pty: &AsyncFd<File>,
    kill: bool,
) -> io::Result<std::process::ExitStatus> {
    if kill {
        child.start_kill()?;
    }
    tokio::select! {
        status = child.wait() => status,
        () = drain_master(pty) => child.wait().await,
    }
}

fn raw(slave: &File) -> bool {
    attributes(slave).c_lflag & RAW_FLAGS == 0
}

fn owned_socket(tmux: &OwnedTmux) -> (PathBuf, u64, u64) {
    let mut sockets = tmux.sockets();
    let socket = sockets.next().expect("one live tmux socket");
    assert!(
        sockets.next().is_none(),
        "only the Default socket may exist"
    );
    let metadata = std::fs::symlink_metadata(&socket).unwrap();
    (socket, metadata.dev(), metadata.ino())
}

async fn wait_raw(
    pty: &AsyncFd<File>,
    slave: &File,
    child: &mut Child,
    tmux: &OwnedTmux,
    resume: bool,
) {
    timeout(ATTACH, async {
        let mut early = Vec::new();
        let mut chunk = [0; 1024];
        while !raw(slave) || tmux.sockets().count() != 1 {
            assert!(
                child.try_wait().unwrap().is_none(),
                "foreground exited before attach"
            );
            let tick = tokio::time::sleep(Duration::from_millis(25));
            if resume {
                tokio::select! {
                    read = read_master(pty, &mut chunk) => {
                        let read = read.expect("read resume admission");
                        assert_ne!(read, 0, "resume admission closed");
                        early.extend_from_slice(&chunk[..read]);
                        assert!(early.len() <= MAX_SCAN, "resume admission scan limit");
                        assert!(
                            !early.windows(COOKED_BUSY.len()).any(|bytes| bytes == COOKED_BUSY)
                                || attributes(slave).c_lflag & libc::ICANON == 0,
                            "resume_busy"
                        );
                    }
                    () = tick => {}
                }
            } else {
                tick.await;
            }
        }
    })
    .await
    .expect("foreground raw PTY and tmux attach deadline");
}

async fn confirm_resize(pty: &AsyncFd<File>, slave: &File) -> [u16; 4] {
    let [_, _, x, y] = size(slave);
    let geometry = libc::winsize {
        ws_row: 31,
        ws_col: 91,
        ws_xpixel: x,
        ws_ypixel: y,
    };
    // The ioctl updates the owned controlling PTY and sends SIGWINCH.
    assert_eq!(
        unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &geometry) },
        0
    );
    timeout(IO, async {
        for trial in 0..3 {
            let request = format!("stty size | tr ' ' :; printf '<%s>' geometry-{trial}\r");
            let marker = format!("<geometry-{trial}>");
            input(pty, request.as_bytes()).await;
            let bytes = output(pty, marker.as_bytes(), "resize probe").await;
            if bytes.windows(5).any(|bytes| bytes == b"31:91") {
                return;
            }
        }
        panic!("remote resize did not reach the requested geometry");
    })
    .await
    .expect("resize confirmation deadline");
    [31, 91, x, y]
}

async fn confirm_interrupt(pty: &AsyncFd<File>, slave: &File) {
    input(
        pty,
        concat!(
            "/bin/sh -c 'trap \"printf \\\"<%s>\\\" interrupted; exit 130\" INT; ",
            "printf \"<%s>\" interrupt-ready; /bin/sleep 60'\r"
        )
        .as_bytes(),
    )
    .await;
    output(pty, b"<interrupt-ready>", "interrupt readiness").await;
    input(pty, b"\x03").await;
    timeout(IO, async {
        output(pty, b"<interrupted>", "remote SIGINT").await;
        input(pty, b"printf '<%s>' after-interrupt\r").await;
        output(pty, b"<after-interrupt>", "shell after interrupt").await;
    })
    .await
    .expect("interrupt confirmation deadline");
    assert!(raw(slave), "foreground CLI remains raw after remote Ctrl-C");
}

pub(super) async fn prove(
    state: &Path,
    daemon: &Path,
    remote: PublicKey,
    relay: &Relay,
    cleanup: &OwnedTmux,
    controls: bool,
) -> (Duration, Duration) {
    write_cli_config(state, remote, relay);
    let mut socket = None;
    let sample = run_cli(
        state,
        daemon,
        remote,
        cleanup,
        false,
        async |pty, slave, original_size| {
            let mut restored_size = original_size;
            input(pty, SHELL).await;
            output(pty, PROMPT, "shell prompt").await;

            let key_sent = Instant::now();
            input(pty, b"x").await;
            output(pty, b"x", "single-key echo").await;
            let echo = key_sent.elapsed();

            assert!(!SCRIPT.contains(EXPECTED));
            input(pty, SCRIPT.as_bytes()).await;
            output(pty, EXPECTED.as_bytes(), "quoted UTF-8").await;
            if controls {
                restored_size = confirm_resize(pty, slave).await;
                confirm_interrupt(pty, slave).await;
                socket = Some(owned_socket(cleanup));
                input(pty, b"MARMISH_CLI_KEEP=STORED; printf '<%s>' saved\r").await;
                output(pty, b"<saved>", "save state before detach").await;
                detach(pty).await;
            } else {
                input(pty, b"exit 0\r").await;
            }
            (echo, restored_size)
        },
    )
    .await;
    if let Some(socket) = socket {
        assert_eq!(owned_socket(cleanup), socket);
        let (resumed, _) = run_cli(
            state,
            daemon,
            remote,
            cleanup,
            true,
            async |pty, _slave, original_size| {
                assert_eq!(owned_socket(cleanup), socket);
                input(pty, b"printf '<fresh-%s>' \"$MARMISH_CLI_KEEP\"\r").await;
                output(pty, b"<fresh-STORED>", "resumed shell state").await;
                input(pty, b"exit 0\r").await;
                (Duration::ZERO, original_size)
            },
        )
        .await;
        eprintln!(
            "terminal_installed_controls resume_attach_us={}",
            resumed.as_micros()
        );
    }
    sample
}

async fn run_cli(
    state: &Path,
    daemon: &Path,
    remote: PublicKey,
    cleanup: &OwnedTmux,
    resume: bool,
    workload: impl AsyncFnOnce(&AsyncFd<File>, &File, [u16; 4]) -> (Duration, [u16; 4]),
) -> (Duration, Duration) {
    let (master, slave) = open_cli_pty();
    let original = attributes(&slave);
    let original_size = size(&slave);
    let pty = register_master(master);
    let began = Instant::now();
    let mut child = spawn_cli(state, daemon, remote, &slave, CliMode::Default);
    let mut stderr = child.stderr.take().unwrap();
    let mut stderr_drain = tokio::spawn(async move {
        let mut chunk = [0; 4096];
        let mut total = 0usize;
        loop {
            let size = stderr.read(&mut chunk).await?;
            if size == 0 {
                return Ok::<_, io::Error>(total);
            }
            total = total.saturating_add(size);
        }
    });
    let checks = AssertUnwindSafe(async {
        wait_raw(&pty, &slave, &mut child, cleanup, resume).await;
        let cold = began.elapsed();
        let (echo, restored_size) = workload(&pty, &slave, original_size).await;
        (cold, echo, restored_size)
    })
    .catch_unwind()
    .await;
    let exit = timeout(IO, reap(&mut child, &pty, checks.is_err())).await;
    if exit.is_err() {
        let _ = timeout(IO, reap(&mut child, &pty, true)).await;
    }
    let stderr_size = timeout(IO, &mut stderr_drain).await;
    if stderr_size.is_err() {
        stderr_drain.abort();
        let _ = stderr_drain.await;
    }
    let (cold, echo, restored_size) = checks.unwrap_or_else(|panic| resume_unwind(panic));
    assert_eq!(
        exit.expect("foreground reap deadline").unwrap().code(),
        Some(0)
    );
    assert_eq!(
        stderr_size
            .expect("foreground stderr drain deadline")
            .unwrap()
            .unwrap(),
        0
    );
    assert_restored(&slave, &original, restored_size);
    (cold, echo)
}

fn register_master(master: File) -> AsyncFd<File> {
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    // File owns the same open master descriptor for the entire registration.
    unsafe { AsyncFd::register(master) }.unwrap()
}
