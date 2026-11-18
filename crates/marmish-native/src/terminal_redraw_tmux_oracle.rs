//! The offline replay and this private tmux driver use the same test binary.

use super::{Filter, Frame, HEIGHT, PROBE_ROW, Screen, WIDTH, position};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(4);
const POLL: Duration = Duration::from_millis(5);
const SOCKET_ENV: &str = "MARMISH_TEST_REDRAW_PANE";
const PANE_TEST: &str =
    "terminal_output_batches::terminal_redraw_evaluation_tests::tmux_oracle::injector";
const FORMAT: &str =
    "#{pane_width} #{pane_height} #{alternate_on} #{cursor_flag} #{cursor_y} #{cursor_x}";

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) struct Pane {
    id: String,
    input: UnixStream,
}

impl Pane {
    fn send(&mut self, bytes: &[u8]) {
        Filter::validate_batch(bytes).expect("redraw_oracle_filter");
        self.input.write_all(bytes).expect("redraw_oracle_write");
    }
}

pub(super) struct Oracle {
    // Fields drop in order, so the server stops before its private root is removed.
    _server: OwnedChild,
    home: tempfile::TempDir,
    tmux: PathBuf,
    socket: PathBuf,
}

fn command(tmux: &Path, home: &Path) -> Command {
    let mut command = Command::new(tmux);
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm")
        .env("LC_ALL", "C")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

fn until<T>(deadline: Instant, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

fn output(mut command: Command, deadline: Instant) -> Vec<u8> {
    let mut child = OwnedChild(command.spawn().expect("redraw_tmux_control_spawn"));
    let status = until(deadline, || {
        child.0.try_wait().expect("redraw_tmux_control_wait")
    })
    .expect("redraw_tmux_control_timeout");
    assert!(status.success(), "redraw_tmux_control_status");
    let mut bytes = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(4096)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() < 4096, "redraw_tmux_control_output_limit");
    bytes
}

impl Oracle {
    pub(super) fn start() -> Self {
        let tmux =
            PathBuf::from(std::env::var_os("MARMISH_TEST_TMUX").expect("redraw_tmux_required"));
        assert!(
            tmux.is_absolute() && tmux.is_file(),
            "redraw_tmux_not_absolute"
        );
        let home = tempfile::Builder::new()
            .prefix("mr-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .expect("redraw_tmux_directory");
        let socket = home.path().join("tmux");
        let mut version = command(&tmux, home.path());
        version.arg("-V");
        let version = output(version, Instant::now() + WAIT);
        println!("redraw oracle={}", String::from_utf8_lossy(&version).trim());
        let server = OwnedChild(
            command(&tmux, home.path())
                .arg("-D")
                .arg("-S")
                .arg(&socket)
                .args(["-f", "/dev/null"])
                .stdout(Stdio::null())
                .spawn()
                .expect("redraw_tmux_server"),
        );
        until(Instant::now() + WAIT, || {
            let meta = fs::symlink_metadata(&socket).ok()?;
            (meta.file_type().is_socket() && UnixStream::connect(&socket).is_ok()).then_some(())
        })
        .expect("redraw_tmux_ready");
        Self {
            _server: server,
            home,
            tmux,
            socket,
        }
    }

    fn control(&self, args: &[&OsStr], deadline: Instant) -> Vec<u8> {
        let mut command = command(&self.tmux, self.home.path());
        command.arg("-N").arg("-S").arg(&self.socket).args(args);
        output(command, deadline)
    }

    fn pane(&self, name: &str) -> Pane {
        let path = self.home.path().join(name);
        let listener = UnixListener::bind(&path).expect("redraw_pane_bind");
        listener.set_nonblocking(true).unwrap();
        let exe = std::env::current_exe().expect("redraw_pane_exe");
        let mut env = OsString::from(SOCKET_ENV);
        env.push("=");
        env.push(&path);
        let deadline = Instant::now() + WAIT;
        let pane = self.control(
            &[
                OsStr::new("new-session"),
                OsStr::new("-d"),
                OsStr::new("-P"),
                OsStr::new("-F"),
                OsStr::new("#{pane_id}"),
                OsStr::new("-s"),
                OsStr::new(name),
                OsStr::new("-x"),
                OsStr::new("80"),
                OsStr::new("-y"),
                OsStr::new("24"),
                OsStr::new("-e"),
                &env,
                exe.as_os_str(),
                OsStr::new("--ignored"),
                OsStr::new("--exact"),
                OsStr::new(PANE_TEST),
                OsStr::new("--nocapture"),
                OsStr::new("--quiet"),
            ],
            deadline,
        );
        let id = String::from_utf8(pane).unwrap().trim().to_owned();
        let number = id.strip_prefix('%').expect("redraw_pane_id");
        assert!(!number.is_empty() && number.bytes().all(|item| item.is_ascii_digit()));
        let input = until(deadline, || match listener.accept() {
            Ok((socket, _)) => Some(socket),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => None,
            Err(_) => panic!("redraw_pane_accept"),
        })
        .expect("redraw_pane_ready");
        input.set_write_timeout(Some(WAIT)).unwrap();
        fs::remove_file(path).unwrap();
        Pane { id, input }
    }

    pub(super) fn three_panes(&self) -> [Pane; 3] {
        ["baseline", "timed", "ideal"].map(|name| self.pane(name))
    }

    pub(super) fn close_panes<const N: usize>(&self, panes: [Pane; N]) {
        for pane in panes {
            self.control(
                &[
                    OsStr::new("kill-pane"),
                    OsStr::new("-t"),
                    OsStr::new(&pane.id),
                ],
                Instant::now() + WAIT,
            );
        }
    }

    fn snapshot(&self, pane: &Pane, deadline: Instant) -> Option<Screen> {
        let id = OsStr::new(&pane.id);
        let output = self.control(
            &[
                OsStr::new("display-message"),
                OsStr::new("-p"),
                OsStr::new("-t"),
                id,
                OsStr::new(FORMAT),
                OsStr::new(";"),
                OsStr::new("capture-pane"),
                OsStr::new("-p"),
                OsStr::new("-t"),
                id,
            ],
            deadline,
        );
        let (header, grid) = output.split_at(output.iter().position(|byte| *byte == b'\n')?);
        let mut fields = std::str::from_utf8(header).ok()?.split_ascii_whitespace();
        let mut number = || fields.next()?.parse::<usize>().ok();
        if [number()?, number()?, number()?, number()?] != [WIDTH, HEIGHT, 0, 1] {
            return None;
        }
        let cursor = (number()?, number()?);
        let mut rows = grid
            .get(1..)?
            .strip_suffix(b"\n")?
            .split(|byte| *byte == b'\n');
        let mut cells = [[b' '; WIDTH]; HEIGHT];
        for line in &mut cells {
            let bytes = rows.next()?;
            line.get_mut(..bytes.len())?.copy_from_slice(bytes);
        }
        rows.next().is_none().then_some(Screen { cells, cursor })
    }

    fn expect(&self, pane: &Pane, expected: &Screen, deadline: Instant) {
        until(deadline, || {
            (self.snapshot(pane, deadline).as_ref() == Some(expected)).then_some(())
        })
        .expect("redraw_oracle_screen_mismatch");
    }

    pub(super) fn check_turn(
        &self,
        pane: &mut Pane,
        frames: &[Frame],
        expected: &Screen,
        index: usize,
    ) {
        let deadline = Instant::now() + WAIT;
        for frame in frames {
            pane.send(&frame.bytes);
        }
        self.expect(pane, expected, deadline);

        let (row, col) = expected.cursor;
        assert!(row < PROBE_ROW && col + 2 < WIDTH);
        // The probe row is outside all typed writes. A marker here certifies
        // processing after the tested update without masking a painted cell.
        let marker = b'A' + u8::try_from(index).unwrap();
        pane.send(format!("\x1b[{}B\x1b[C{}", PROBE_ROW - row, char::from(marker)).as_bytes());
        let mut probed = expected.clone();
        probed.cells[PROBE_ROW][col + 1] = marker;
        probed.cursor = (PROBE_ROW, col + 2);
        self.expect(pane, &probed, deadline);

        let mut restore = Vec::new();
        position(&mut restore, (PROBE_ROW, col + 1));
        restore.push(expected.cells[PROBE_ROW][col + 1]);
        position(&mut restore, expected.cursor);
        pane.send(&restore);
        self.expect(pane, expected, deadline);
    }

    pub(super) fn check_negative(&self, initial: &Screen, frames: &[Frame]) {
        let mut pane = self.pane("negative");
        self.check_turn(&mut pane, frames, initial, 0);

        let mut wrong_cell = initial.clone();
        wrong_cell.cells[0][0] = b'X';
        let mut bytes = Vec::new();
        position(&mut bytes, (0, 0));
        bytes.push(b'X');
        position(&mut bytes, initial.cursor);
        pane.send(&bytes);
        let deadline = Instant::now() + WAIT;
        self.expect(&pane, &wrong_cell, deadline);
        assert_ne!(self.snapshot(&pane, deadline).as_ref(), Some(initial));

        let mut wrong_cursor = initial.clone();
        wrong_cursor.cursor.1 += 1;
        let mut bytes = initial.full_paint();
        position(&mut bytes, wrong_cursor.cursor);
        pane.send(&bytes);
        let deadline = Instant::now() + WAIT;
        self.expect(&pane, &wrong_cursor, deadline);
        assert_ne!(self.snapshot(&pane, deadline).as_ref(), Some(initial));
        self.close_panes([pane]);
    }
}

#[test]
#[ignore = "child injector exclusively started inside the private test pane"]
fn injector() {
    let path = PathBuf::from(std::env::var_os(SOCKET_ENV).expect("redraw_injector_socket"));
    let stdout = io::stdout();
    let mut termios = std::mem::MaybeUninit::uninit();
    // SAFETY: tcgetattr fills termios on success and stdout is this pane's PTY.
    unsafe {
        assert_eq!(libc::tcgetattr(stdout.as_raw_fd(), termios.as_mut_ptr()), 0);
        let mut termios = termios.assume_init();
        libc::cfmakeraw(&mut termios);
        assert_eq!(
            libc::tcsetattr(stdout.as_raw_fd(), libc::TCSANOW, &termios),
            0
        );
    }
    let mut socket = UnixStream::connect(path).expect("redraw_injector_connect");
    let mut buffer = [0; 4096];
    let mut out = stdout.lock();
    loop {
        let read = socket.read(&mut buffer).expect("redraw_injector_read");
        if read == 0 {
            break;
        }
        out.write_all(&buffer[..read])
            .expect("redraw_injector_write");
        out.flush().expect("redraw_injector_flush");
    }
}
