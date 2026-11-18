use super::*;
use std::fs::File;
use std::io::{self, Read};
use tokio::io::unix::AsyncFd;
use tokio::process::Child;

const SHELL: &[u8] =
    b"exec /usr/bin/env -i PATH=/usr/bin:/bin TERM=dumb LC_ALL=C PS1='MAR''MISH> ' /bin/sh -i\r";
const PROMPT: &[u8] = b"MARMISH> ";
const SCRIPT: &str = "\x15printf '[%s]\\n' 'two words; % 🐾'\r";
const EXPECTED: &str = "[two words; % 🐾]";
const MAX_SCAN: usize = 32 * 1024;

#[test]
#[ignore = "requires nonroot, installed executables, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_native_foreground() {
    run_native(|| async {
        let sample = prove_writer(Workload::Foreground).await;
        eprintln!(
            "terminal_installed_foreground attach_us={} echo_us={} command_before_us={} command_after_us={}",
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

async fn input(pty: &AsyncFd<File>, mut bytes: &[u8]) {
    timeout(IO, async {
        while !bytes.is_empty() {
            let mut ready = pty.writable().await.unwrap();
            if let Ok(result) = ready.try_io(|fd| fd.get_ref().write(bytes)) {
                let sent = result.unwrap();
                assert_ne!(sent, 0, "foreground PTY write made no progress");
                bytes = &bytes[sent..];
            }
        }
    })
    .await
    .expect("foreground PTY write deadline");
}

async fn output(pty: &AsyncFd<File>, expected: &[u8], stage: &'static str) {
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
                return;
            }
        }
        panic!("foreground PTY {stage} exceeded scan limit");
    })
    .await
    .unwrap_or_else(|_| panic!("foreground PTY {stage} output deadline"));
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

pub(super) async fn prove(
    state: &Path,
    daemon: &Path,
    remote: PublicKey,
    relay: &Relay,
    cleanup: &OwnedTmux,
) -> (Duration, Duration) {
    write_cli_config(state, remote, relay);
    let (master, slave) = open_cli_pty();
    let original = attributes(&slave);
    let original_size = size(&slave);
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    // File owns the same open master descriptor for the entire registration.
    let pty = unsafe { AsyncFd::register(master) }.unwrap();
    let began = Instant::now();
    let mut child = spawn_cli(state, daemon, remote, &slave, false);
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
        timeout(ATTACH, async {
            while attributes(&slave).c_lflag & (libc::ECHO | libc::ICANON) != 0
                || cleanup.sockets().count() != 1
            {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "foreground exited before attach"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("foreground raw PTY and tmux attach deadline");
        let cold = began.elapsed();
        input(&pty, SHELL).await;
        output(&pty, PROMPT, "shell prompt").await;

        let key_sent = Instant::now();
        input(&pty, b"x").await;
        output(&pty, b"x", "single-key echo").await;
        let echo = key_sent.elapsed();

        assert!(!SCRIPT.contains(EXPECTED));
        input(&pty, SCRIPT.as_bytes()).await;
        output(&pty, EXPECTED.as_bytes(), "quoted UTF-8").await;
        input(&pty, b"exit 0\r").await;
        (cold, echo)
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
    let sample = checks.unwrap_or_else(|panic| resume_unwind(panic));
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
    assert_restored(&slave, &original, original_size);
    sample
}
