//! Three real foreground clients sharing one canonical account and tmux pane.
use super::*;
use std::cell::RefCell;
use std::process::ExitStatus;
use std::rc::Rc;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::timeout_at;

const BUSY_QUESTION: &[u8] = b"Terminal busy. [o] Observe, [t] Takeover, [n] New, [c] Cancel: ";
const CANDIDATE: &[u8] = b"printf '<forbidden-%s>' observer-input-candidate\r";
const CANDIDATE_TOKEN: &[u8] = b"observer-input-candidate";
const CANDIDATE_OUTPUT: &[u8] = b"<forbidden-observer-input-candidate>";
const POLL: Duration = Duration::from_millis(25);
const STDERR_CAP: usize = 4096;

#[test]
#[ignore = "requires nonroot, installed executables, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_native_foreground_handoff() {
    run_native(|| async {
        prove_writer(Workload::ForegroundHandoff).await;
    });
}

pub(in super::super) struct HandoffSample {
    observed: Duration,
    choice: Instant,
    ready: Instant,
    marker: Duration,
    during: CommandTiming,
}

impl HandoffSample {
    pub(in super::super) fn report(&self, before: Duration, after: Duration, attempts: usize) {
        let offset = |time: Instant, origin: Instant| {
            if time >= origin {
                (time - origin).as_micros() as i128
            } else {
                -((origin - time).as_micros() as i128)
            }
        };
        eprintln!(
            "terminal_installed_foreground_handoff observe_raw_us={} choice_to_raw_us={} successor_marker_us={} command_before_us={} command_during_us={} command_after_us={} command_during_start_from_choice_us={} command_during_end_from_choice_us={} command_during_start_from_raw_us={} command_during_end_from_raw_us={} relay_publication_attempts={attempts}",
            self.observed.as_micros(),
            (self.ready - self.choice).as_micros(),
            self.marker.as_micros(),
            before.as_micros(),
            (self.during.finished - self.during.began).as_micros(),
            after.as_micros(),
            offset(self.during.began, self.choice),
            offset(self.during.finished, self.choice),
            offset(self.during.began, self.ready),
            offset(self.during.finished, self.ready),
        );
    }
}

#[derive(Default)]
struct Capture {
    state: RefCell<Captured>,
    changed: Notify,
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    overflow: bool,
    closed: bool,
}

impl Capture {
    async fn read(&self, pty: &AsyncFd<File>) {
        let mut chunk = [0; 1024];
        while let Ok(1..=1024) = read_master(pty, &mut chunk).await.inspect(|count| {
            let mut state = self.state.borrow_mut();
            let remaining = MAX_SCAN - state.bytes.len();
            state.overflow |= *count > remaining;
            state
                .bytes
                .extend_from_slice(&chunk[..(*count).min(remaining)]);
            self.changed.notify_one();
        }) {}
        self.state.borrow_mut().closed = true;
        self.changed.notify_one();
    }

    fn offset(&self) -> usize {
        self.state.borrow().bytes.len()
    }

    async fn find(&self, from: usize, marker: &[u8], deadline: Instant, stage: &'static str) {
        timeout_at(deadline, async {
            loop {
                {
                    let state = self.state.borrow();
                    assert!(!state.overflow, "foreground {stage} capture exceeded cap");
                    if has(&state.bytes[from..], marker) {
                        return;
                    }
                    assert!(!state.closed, "foreground {stage} PTY closed before marker");
                }
                self.changed.notified().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("foreground {stage} output deadline"));
    }

    fn inspect(&self, from: usize, check: impl FnOnce(&[u8])) {
        let state = self.state.borrow();
        assert!(!state.overflow, "foreground capture exceeded cap");
        check(&state.bytes[from..]);
    }
}

fn has(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|bytes| bytes == marker)
}

struct Participant {
    pty: Rc<AsyncFd<File>>,
    slave: File,
    child: Child,
    saved: libc::termios,
    original_size: [u16; 4],
    began: Instant,
    capture: Rc<Capture>,
    reader: JoinHandle<()>,
    stderr: JoinHandle<io::Result<(Vec<u8>, bool)>>,
}

impl Participant {
    fn launch(state: &Path, daemon: &Path, remote: PublicKey, mode: CliMode) -> Self {
        let (master, slave) = open_cli_pty();
        if matches!(mode, CliMode::Observe) {
            let larger = libc::winsize {
                ws_row: 40,
                ws_col: 120,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            assert_eq!(
                unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &larger) },
                0
            );
        }
        let saved = attributes(&slave);
        let original_size = size(&slave);
        let pty = Rc::new(register_master(master));
        let began = Instant::now();
        let mut child = spawn_cli(state, daemon, remote, &slave, mode);
        let stderr = tokio::spawn(read_stderr(child.stderr.take().unwrap()));
        let capture = Rc::new(Capture::default());
        let reader = tokio::task::spawn_local({
            let pty = pty.clone();
            let capture = capture.clone();
            async move { capture.read(&pty).await }
        });
        Self {
            pty,
            slave,
            child,
            saved,
            original_size,
            began,
            capture,
            reader,
            stderr,
        }
    }

    async fn wait_raw(&mut self, deadline: Instant, stage: &'static str) -> Instant {
        timeout_at(deadline, async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    panic!("{stage} exited before raw Ready ({status})");
                }
                if raw(&self.slave) {
                    return Instant::now();
                }
                tokio::time::sleep(POLL).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{stage} raw Ready deadline"))
    }

    async fn find(&self, from: usize, marker: &[u8], stage: &'static str) {
        self.capture
            .find(from, marker, Instant::now() + IO, stage)
            .await;
    }

    async fn finish(&mut self) -> io::Result<Exit> {
        let status = self.child.wait().await?;
        let (stderr, overflow) = (&mut self.stderr).await.map_err(io::Error::other)??;
        // The fixture holds the slave to audit restoration, so there is
        // no PTY EOF after the child exits. Retire its sole reader here.
        self.reader.abort();
        let _ = (&mut self.reader).await;
        Ok(Exit {
            status,
            stderr,
            overflow,
        })
    }
}

impl Drop for Participant {
    fn drop(&mut self) {
        self.reader.abort();
        self.stderr.abort();
    }
}

async fn read_stderr(mut stderr: tokio::process::ChildStderr) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    let mut overflow = false;
    let mut chunk = [0; 1024];
    loop {
        let count = stderr.read(&mut chunk).await?;
        if count == 0 {
            return Ok((bytes, overflow));
        }
        let remaining = STDERR_CAP - bytes.len();
        overflow |= count > remaining;
        bytes.extend_from_slice(&chunk[..count.min(remaining)]);
    }
}

struct Exit {
    status: ExitStatus,
    stderr: Vec<u8>,
    overflow: bool,
}

impl Exit {
    fn check(&self, who: &'static str, replaced: bool) {
        let reason = match (self.status.code(), self.stderr.as_slice()) {
            (Some(0), b"") => "ended",
            (Some(255), b"marmish: invalid input: terminal_replaced\n") if replaced => "replaced",
            (Some(255), b"marmish: invalid input: terminal_transport_closed\n") if replaced => {
                "transport_closed"
            }
            _ => panic!("{who} exited with unexpected status or stderr category"),
        };
        assert!(!self.overflow, "{who} stderr exceeded cap");
        eprintln!("terminal_installed_foreground_handoff {who}={reason}");
    }
}

type Children = [Option<Participant>; 3];

fn kill_all(children: &mut Children) {
    for participant in children.iter_mut().flatten() {
        let _ = participant.child.start_kill();
    }
}

pub(in super::super) async fn prove_handoff(
    state: &Path,
    daemon: &Path,
    remote: PublicKey,
    relay: &Relay,
    cleanup: &OwnedTmux,
    turns: &mpsc::Sender<CommandTurn>,
    bindings: &mut watch::Receiver<u64>,
) -> HandoffSample {
    write_cli_config(state, remote, relay);
    let mut children = [None, None, None];
    let launch = |mode| Participant::launch(state, daemon, remote, mode);
    let checks = AssertUnwindSafe(handoff(&mut children, launch, cleanup, turns, bindings))
        .catch_unwind()
        .await;
    if checks.is_err() {
        kill_all(&mut children);
    }
    let exits = timeout(
        IO,
        futures_util::future::join_all(children.iter_mut().flatten().map(Participant::finish)),
    )
    .await;
    if exits.is_err() {
        kill_all(&mut children);
    }
    let sample = checks.unwrap_or_else(|panic| {
        if let Ok(exits) = &exits {
            for (who, exit) in ["writer", "observer", "successor"].into_iter().zip(exits) {
                if let Ok(exit) = exit {
                    let category = match exit.stderr.as_slice() {
                        b"" => "empty",
                        b"marmish: invalid input: terminal_takeover_uncertain\n" => {
                            "takeover_uncertain"
                        }
                        b"marmish: invalid input: terminal_display_restore\n" => "display_restore",
                        b"marmish: invalid input: terminal_offer_stale\n" => "offer_stale",
                        _ => "unclassified",
                    };
                    eprintln!("foreground {who} {} stderr={category}", exit.status);
                }
            }
        }
        resume_unwind(panic)
    });
    let exits = exits.expect("shared foreground cleanup deadline");
    for ((participant, exit), who) in
        children
            .iter()
            .flatten()
            .zip(exits)
            .zip(["writer", "observer", "successor"])
    {
        exit.expect("foreground reap and stderr drain")
            .check(who, who != "successor");
        participant.capture.inspect(0, assert_no_candidate);
        assert_restored(
            &participant.slave,
            &participant.saved,
            participant.original_size,
        );
    }
    sample
}

async fn handoff(
    children: &mut Children,
    launch: impl Fn(CliMode) -> Participant,
    cleanup: &OwnedTmux,
    turns: &mpsc::Sender<CommandTurn>,
    bindings: &mut watch::Receiver<u64>,
) -> HandoffSample {
    let [writer, observer, successor] = children;
    let writer = writer.insert(launch(CliMode::Default));
    assert_eq!(writer.original_size[..2], [24, 80]);
    writer.wait_raw(writer.began + ATTACH, "writer").await;
    let socket = owned_socket(cleanup);
    input(&writer.pty, SHELL).await;
    writer.find(0, PROMPT, "writer shell").await;
    let offset = writer.capture.offset();
    input(
        &writer.pty,
        b"MARMISH_HANDOFF_KEEP=saved; printf '<%s>' writer-state\r",
    )
    .await;
    writer.find(offset, b"<writer-state>", "writer state").await;

    let observer = observer.insert(launch(CliMode::Observe));
    assert_eq!(observer.original_size[..2], [40, 120]);
    let observed = observer.wait_raw(observer.began + ATTACH, "observer").await - observer.began;
    check_observer(writer, observer).await;

    let successor = successor.insert(launch(CliMode::Default));
    successor
        .capture
        .find(0, BUSY_QUESTION, successor.began + ATTACH, "cooked Busy")
        .await;
    let cooked = attributes(&successor.slave).c_lflag;
    assert_eq!(
        cooked & (libc::ICANON | libc::ISIG),
        libc::ICANON | libc::ISIG
    );
    let previous_binding = *bindings.borrow_and_update();
    assert_eq!(
        timeout(IO, write_master(&successor.pty, b"t\r"))
            .await
            .unwrap()
            .unwrap(),
        2
    );
    let choice = Instant::now();
    let (ready, during) = tokio::join!(
        successor.wait_raw(
            (choice + Duration::from_secs(60)).min(successor.began + 2 * ATTACH),
            "successor"
        ),
        async {
            // The fixture drives both Command owners serially. Let the
            // successor finish its binding lookup before occupying that actor.
            let completed = timeout(IO, bindings.wait_for(|count| *count > previous_binding))
                .await
                .expect("successor binding lookup deadline")
                .unwrap();
            assert_eq!(*completed, previous_binding + 1);
            drop(completed);
            command_timing(turns).await
        },
    );
    assert!(
        during.began >= choice && during.began <= ready,
        "ordinary Command turn starts in the handoff window"
    );
    assert_eq!(
        owned_socket(cleanup),
        socket,
        "same tmux socket through takeover"
    );
    timeout(IO, async {
        tokio::try_join!(writer.child.wait(), observer.child.wait()).unwrap();
    })
    .await
    .expect("superseded clients did not exit after takeover");
    let marker_sent = Instant::now();
    input(
        &successor.pty,
        b"printf '<successor-%s>' \"$MARMISH_HANDOFF_KEEP\"\r",
    )
    .await;
    successor
        .find(0, b"<successor-saved>", "preserved successor state")
        .await;
    let marker = marker_sent.elapsed();
    successor.capture.inspect(0, assert_no_candidate);
    input(&successor.pty, b"exit 0\r").await;
    timeout(IO, successor.child.wait())
        .await
        .expect("successor shell exit deadline")
        .unwrap();
    HandoffSample {
        observed,
        choice,
        ready,
        marker,
        during,
    }
}

async fn check_observer(writer: &Participant, observer: &Participant) {
    let from = [writer.capture.offset(), observer.capture.offset()];
    input(&observer.pty, CANDIDATE).await;
    wait_input_consumed(&observer.slave).await;

    let command_line = b"printf '<%s>\\n' observe-fence";
    let marker = b"<observe-fence>";
    input(&writer.pty, &[command_line.as_slice(), b"\r"].concat()).await;
    both_find([writer, observer], from, marker, "observer input barrier").await;
    for (side, offset) in [writer, observer].into_iter().zip(from) {
        side.capture.inspect(offset, |bytes| {
            assert_no_candidate(bytes);
            assert!(has(bytes, command_line), "writer delimiter echo intact");
        });
    }

    let from = [writer.capture.offset(), observer.capture.offset()];
    input(
        &writer.pty,
        b"stty size | tr ' ' :; printf '<%s>' observe-geometry\r",
    )
    .await;
    both_find(
        [writer, observer],
        from,
        b"<observe-geometry>",
        "observer geometry",
    )
    .await;
    for (side, offset) in [writer, observer].into_iter().zip(from) {
        side.capture.inspect(offset, |bytes| {
            assert!(has(bytes, b"24:80"), "writer geometry reaches both PTYs");
            assert!(
                !has(bytes, b"40:120"),
                "observer geometry never reaches tmux"
            );
            assert_no_candidate(bytes);
        });
    }
}

async fn both_find(sides: [&Participant; 2], from: [usize; 2], marker: &[u8], stage: &'static str) {
    tokio::join!(
        sides[0].find(from[0], marker, stage),
        sides[1].find(from[1], marker, stage)
    );
}

fn assert_no_candidate(bytes: &[u8]) {
    assert!(!has(bytes, CANDIDATE_OUTPUT), "observer candidate ran");
    assert!(!has(bytes, CANDIDATE_TOKEN), "observer input was echoed");
}

async fn wait_input_consumed(slave: &File) {
    timeout(IO, async {
        let mut was_empty = false;
        loop {
            assert!(raw(slave), "observer remains raw while reading its input");
            let mut pending: libc::c_int = -1;
            assert_eq!(
                unsafe { libc::ioctl(slave.as_raw_fd(), libc::FIONREAD, &mut pending) },
                0
            );
            if pending == 0 && was_empty {
                return;
            }
            was_empty = pending == 0;
            tokio::time::sleep(POLL).await;
        }
    })
    .await
    .expect("foreground observer input consumption deadline");
}
