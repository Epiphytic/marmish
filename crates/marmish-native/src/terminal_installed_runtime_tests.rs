//! Drive the native Terminal runtime through its same-user local wire and a real TLS relay.
//!
//! The marker scan checks relay records and raw durable files. SQLCipher hides
//! content from a raw scan. This fixture neither decrypts rows nor captures
//! process-daemon log files or tmux scrollback.
use super::*;
use crate::ipc::{self, Response, terminal_handshake as handshake};
use crate::terminal_ipc_wire::{self as local, Decoder, Direction, Record, Role};
use crate::terminal_owner::{Control, Prepared, Runtime};
use futures_util::FutureExt;
use nostr::nips::nip19::ToBech32;
use std::future::Future;
use std::io::Write as _;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt as _};
use std::panic::{AssertUnwindSafe, resume_unwind};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;

#[path = "terminal_installed_foreground_tests.rs"]
mod foreground;
#[path = "terminal_installed_observe_tests.rs"]
mod observe;

const ATTACH: Duration = Duration::from_secs(35);
const IO: Duration = Duration::from_secs(15);
const MARKER: &[u8] = "two words; % 🐾".as_bytes();

#[test]
#[ignore = "requires nonroot, native binaries, tmux, SQLCipher and a private loopback TLS relay"]
fn terminal_installed_runtime_native_writer() {
    run_native(|| async {
        Measurements::report(&[prove_writer(Workload::Verify).await]);
    });
}

#[test]
#[ignore = "requires installed binaries, a local PTY and a private loopback TLS relay"]
fn terminal_installed_runtime_relay_auth_challenge_timeout() {
    run_native(|| async {
        prove_writer(Workload::QuietAuth).await;
    });
}

#[test]
#[ignore = "requires a real pending flight and a private loopback TLS relay"]
fn terminal_installed_runtime_pending_eof() {
    run_native(|| async {
        prove_writer(Workload::Cancel).await;
    });
}

#[test]
#[ignore = "requires native worker, tmux and a private loopback TLS relay"]
fn terminal_installed_runtime_shell_exit() {
    run_native(|| async {
        prove_writer(Workload::ShellExit).await;
    });
}

#[test]
#[ignore = "requires restarting a real native Terminal owner with preserved tmux"]
fn terminal_installed_runtime_restart() {
    run_native(|| async {
        prove_writer(Workload::Restart).await;
    });
}

#[test]
#[ignore = "requires native workers and a relay that suppresses duplicate event IDs"]
fn terminal_installed_runtime_probe_retry() {
    run_native(|| async {
        prove_writer(Workload::ProbeRetry).await;
    });
}

#[test]
#[ignore = "report-only: repeats real TLS, SQLCipher, Command and terminal workloads"]
fn terminal_installed_runtime_benchmark() {
    run_native(|| async {
        const TRIALS: usize = 20;
        let mut samples = Vec::with_capacity(TRIALS);
        for trial in 0..TRIALS {
            samples.push(
                prove_writer(Workload::Benchmark {
                    throughput: trial == 0,
                })
                .await,
            );
            eprintln!(
                "terminal_installed_runtime completed_trials={}/{TRIALS}",
                trial + 1
            );
        }
        Measurements::report(&samples);
    });
}

fn run_native<F, Fut>(body: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    if std::env::var_os("MARMISH_TEST_TERMINAL_DEBUG").is_some() {
        crate::diagnostics::enable();
    }
    let worker = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            tokio::task::LocalSet::new().block_on(&runtime, body());
        })
        .unwrap();
    if let Err(panic) = worker.join() {
        resume_unwind(panic);
    }
}

#[derive(Clone, Copy)]
enum Workload {
    Verify,
    QuietAuth,
    Foreground,
    Cancel,
    ShellExit,
    Restart,
    ProbeRetry,
    Observe,
    ObserveBenchmark,
    Takeover,
    Benchmark { throughput: bool },
}

#[derive(Default)]
struct Measurements {
    cold: Duration,
    warm: Duration,
    echo: Duration,
    unloaded: Duration,
    loaded: Duration,
    after: Duration,
    payload: Vec<Duration>,
    output: usize,
    throughput: Duration,
    warm_busy_retries: usize,
    observer: Option<observe::Benchmark>,
}

impl Measurements {
    fn report(samples: &[Self]) {
        let summary = |name, mut durations: Vec<Duration>| {
            durations.sort_unstable();
            let count = durations.len();
            let percentile = |p: usize| durations[(p * count).div_ceil(100) - 1].as_micros();
            eprintln!(
                "terminal_installed_runtime workload={name} n={count} p50_us={} p95_us={}",
                percentile(50),
                percentile(95)
            );
        };
        eprintln!(
            "terminal_installed_runtime platform={}-{} transport=private_loopback_tls client_and_command_fixture=single_thread debug_assertions={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(debug_assertions),
        );
        summary(
            "cold_ready",
            samples.iter().map(|sample| sample.cold).collect(),
        );
        summary(
            "warm_ready",
            samples.iter().map(|sample| sample.warm).collect(),
        );
        let busy_retries: usize = samples.iter().map(|sample| sample.warm_busy_retries).sum();
        eprintln!("terminal_installed_runtime warm_busy_retries={busy_retries}");
        summary(
            "single_key_echo",
            samples.iter().map(|sample| sample.echo).collect(),
        );
        summary(
            "command_unloaded",
            samples.iter().map(|sample| sample.unloaded).collect(),
        );
        summary(
            "command_with_terminal",
            samples.iter().map(|sample| sample.loaded).collect(),
        );
        summary(
            "command_after_terminal",
            samples.iter().map(|sample| sample.after).collect(),
        );
        for sample in samples.iter().filter(|sample| sample.output > 0) {
            summary("4096_requested_output", sample.payload.clone());
            eprintln!(
                "terminal_installed_runtime workload=serial_shell_display delivered_bytes={} elapsed_us={} delivered_bytes_per_second={}",
                sample.output,
                sample.throughput.as_micros(),
                (sample.output as f64 / sample.throughput.as_secs_f64()) as u64,
            );
        }
    }
}

fn server_config(relay: &Relay, client: PublicKey, cat: &Path, profile: &Path) -> CommandConfig {
    let config = serde_json::to_vec(&json!({
        "version": 1, "role": "server", "relays": [relay.url.as_str()], "tor": "off",
        "cwd": "/", "limits": {}, "aliases": {}, "trusted_remotes": [],
        "execution_clients": [client.to_hex()],
        "terminal": {"clients": [client.to_hex()]},
        "command_policy": {"version": 1, "defaults": {"allow_exec": [cat]}},
    }))
    .unwrap();
    std::fs::OpenOptions::new()
        .mode(0o600)
        .write(true)
        .create_new(true)
        .open(profile.join("config.json"))
        .unwrap()
        .write_all(&config)
        .unwrap();
    CommandConfig::from_bytes(&config).unwrap()
}

struct OwnedTmux {
    directory: PathBuf,
    executable: PathBuf,
}

impl OwnedTmux {
    fn sockets(&self) -> impl Iterator<Item = PathBuf> {
        std::fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join("s"))
            .filter(|socket| {
                std::fs::symlink_metadata(socket).is_ok_and(|state| state.file_type().is_socket())
            })
    }
}

impl Drop for OwnedTmux {
    fn drop(&mut self) {
        // The fixture alone creates this private directory. Always name its
        // exact socket and reap the cleanup client, including on assertion failure.
        for socket in self.sockets() {
            let Ok(mut child) = Command::new(&self.executable)
                .arg("-S")
                .arg(socket)
                .arg("kill-server")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            else {
                continue;
            };
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while matches!(child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

enum CommandTurn {
    Run(oneshot::Sender<Duration>),
    ReplaceServer(Control),
    Stop,
}

struct CommandOwners {
    client: MdkOwner,
    server: MdkOwner,
    client_control: Control,
    server_control: Control,
    command: CommandRoom,
    cat: PathBuf,
    relays: Vec<String>,
}

impl CommandOwners {
    async fn run(
        mut self,
        mut turns: mpsc::Receiver<CommandTurn>,
        mut ordinary: mpsc::Receiver<ipc::Pending>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            self.server_control.refresh(&mut self.server);
            tokio::select! {
                Some(request) = self.client_control.requests.recv() => request.complete(&mut self.client),
                Some(request) = self.server_control.requests.recv() => request.complete(&mut self.server),
                Some(pending) = ordinary.recv() => {
                    let response = match pending.request {
                        ref request @ ipc::Request::CommandPrepare {
                            ref remote, ref relays, fresh: None
                        } if remote == &hex::encode(self.command.peer) && relays == &self.relays => {
                            let result = self.client.command_local("c", request)
                                .expect("mature Command preparation");
                            assert!(matches!(&result, crate::command_local::LocalResult::Ready { session, .. }
                                if session == &hex::encode(self.command.session)));
                            Response::command(result)
                        }
                        _ => Response::failure("fixture_unexpected_ordinary"),
                    };
                    let _ = pending.respond.send(response);
                }
                turn = turns.recv() => match turn {
                    Some(CommandTurn::Run(reply)) => {
                        let began = Instant::now();
                        run_command(&mut self.client, &mut self.server, &self.command, &self.cat).await;
                        let _ = reply.send(began.elapsed());
                    }
                    Some(CommandTurn::ReplaceServer(control)) => self.server_control = control,
                    Some(CommandTurn::Stop) | None => break,
                },
                // Both real actors are driven by Command turns in this
                // fixture. Idle polling only refreshes the production
                // snapshot, without serializing two network-maintenance
                // turns onto one local task during the binding lookup.
                _ = interval.tick() => (),
            }
        }
        self.client.shutdown(Instant::now() + STEP).await.unwrap();
        self.server.shutdown(Instant::now() + STEP).await.unwrap();
    }
}

async fn command(turns: &mpsc::Sender<CommandTurn>) -> Duration {
    let (reply, response) = oneshot::channel();
    turns.send(CommandTurn::Run(reply)).await.unwrap();
    timeout(IO, response)
        .await
        .expect("Command round trip")
        .unwrap()
}

async fn open_terminal(path: &Path, request: handshake::Start) -> Result<LocalTerminal> {
    open_terminal_until(path, request, Instant::now() + ATTACH).await
}

async fn open_terminal_until(
    path: &Path,
    request: handshake::Start,
    deadline: Instant,
) -> Result<LocalTerminal> {
    match handshake::start(path, request, deadline).await? {
        handshake::StartOutcome::Ready { stream, role, slot } => {
            LocalTerminal::from_ready(stream, role, slot)
        }
        handshake::StartOutcome::Busy(_) => Err(Error::Invalid("terminal_busy")),
    }
}

async fn resume_default(
    path: &Path,
    start: impl Fn(handshake::Attach) -> handshake::Start,
) -> Result<(LocalTerminal, usize)> {
    let deadline = Instant::now() + ATTACH;
    let mut retries = 0;
    loop {
        match open_terminal_until(path, start(handshake::Attach::Default), deadline).await {
            Err(Error::Invalid("terminal_busy" | "terminal_native_unavailable"))
                if retries < 2 && Instant::now() < deadline =>
            {
                retries += 1;
            }
            result => return result.map(|terminal| (terminal, retries)),
        }
    }
}

struct LocalTerminal {
    stream: UnixStream,
    decoder: Decoder,
    role: Role,
    slot: [u8; 16],
    data: Vec<u8>,
    credit: u64,
    sent: u64,
    input_closed: bool,
    output_records: usize,
    output_acks: usize,
}

enum End {
    Detach,
    Eof,
    ShellExit,
}

enum Event {
    Output,
    Resize,
    Credit,
    Close(local::Close),
    Eof,
}

impl LocalTerminal {
    fn from_ready(
        stream: std::os::unix::net::UnixStream,
        role: Role,
        slot: [u8; 16],
    ) -> Result<Self> {
        Ok(Self {
            stream: UnixStream::from_std(stream)?,
            decoder: Decoder::new(Direction::ToClient, role),
            role,
            slot,
            data: Vec::new(),
            credit: 0,
            sent: 0,
            input_closed: false,
            output_records: 0,
            output_acks: 0,
        })
    }

    async fn send(&mut self, record: Record<'_>) {
        if let Record::Input(bytes) = record {
            let accepted = self.sent.checked_add(bytes.len() as u64).unwrap();
            let deadline = Instant::now() + IO;
            while self.credit < accepted {
                match self.next(deadline).await {
                    Event::Output | Event::Credit | Event::Resize => (),
                    Event::Close(reason) => panic!("stream closed before input credit: {reason:?}"),
                    Event::Eof => panic!("stream reached EOF before input credit"),
                }
            }
            self.sent = accepted;
        }
        self.write_record(record).await;
    }

    async fn write_record(&mut self, record: Record<'_>) {
        let encoded = local::encode(Direction::ToDaemon, self.role, record).unwrap();
        timeout(IO, self.stream.write_all(&encoded))
            .await
            .unwrap()
            .unwrap();
    }

    async fn next(&mut self, deadline: Instant) -> Event {
        let mut first = [0];
        let count = tokio::time::timeout_at(deadline, self.stream.read(&mut first))
            .await
            .expect("bounded local Terminal record")
            .unwrap();
        if count == 0 {
            self.decoder.end_of_input().unwrap();
            return Event::Eof;
        }
        self.finish_record(first[0], deadline).await
    }

    async fn finish_record(&mut self, first: u8, deadline: Instant) -> Event {
        tokio::time::timeout_at(deadline, async {
            let mut frame = [0; local::MAX_DATA + 12];
            frame[0] = first;
            self.stream.read_exact(&mut frame[1..2]).await.unwrap();
            let length = usize::from(u16::from_be_bytes(frame[..2].try_into().unwrap())) + 2;
            assert!(length <= frame.len());
            self.stream.read_exact(&mut frame[2..length]).await.unwrap();
            let (consumed, record) = self.decoder.push(&frame[..length]).unwrap();
            assert_eq!(consumed, length);
            match record.unwrap() {
                Record::Output { id, bytes } => {
                    self.data.extend_from_slice(bytes);
                    self.output_records += 1;
                    if !self.input_closed {
                        self.write_record(Record::Ack(id)).await;
                        self.output_acks += 1;
                    }
                    Event::Output
                }
                Record::Credit(credit) => {
                    assert!(credit.get() > self.credit, "cumulative terminal credit");
                    self.credit = credit.get();
                    Event::Credit
                }
                Record::ResizeApplied => Event::Resize,
                Record::Close(reason) => Event::Close(reason),
                unexpected => panic!("unexpected local Terminal reply {unexpected:?}"),
            }
        })
        .await
        .expect("bounded local Terminal record")
    }

    async fn until(&mut self, marker: &[u8], maximum: usize) -> Duration {
        let began = Instant::now();
        let mut search_from = 0;
        while !self.data[search_from..]
            .windows(marker.len())
            .any(|x| x == marker)
        {
            search_from = self.data.len().saturating_sub(marker.len() - 1);
            match self.next(began + IO).await {
                Event::Output | Event::Credit | Event::Resize => (),
                Event::Close(reason) => panic!("stream closed before marker: {reason:?}"),
                Event::Eof => panic!("stream reached EOF before marker"),
            }
            assert!(self.data.len() <= maximum, "bounded terminal output");
        }
        began.elapsed()
    }

    async fn echo(&mut self, input: &[u8], marker: &[u8], maximum: usize) -> Duration {
        self.data.clear();
        let began = Instant::now();
        self.send(Record::Input(input)).await;
        self.until(marker, maximum).await;
        began.elapsed()
    }

    async fn while_attached<T>(&mut self, task: impl Future<Output = T>) -> T {
        tokio::pin!(task);
        loop {
            let mut first = [0];
            // The first byte read is cancellation safe. Once consumed,
            // finish and acknowledge that record before yielding the socket.
            let count = tokio::select! {
                result = &mut task => return result,
                result = self.stream.read(&mut first) => result.unwrap(),
            };
            assert_eq!(
                count, 1,
                "active terminal reached EOF while another attaches"
            );
            match self.finish_record(first[0], Instant::now() + IO).await {
                Event::Output | Event::Credit | Event::Resize => self.data.clear(),
                Event::Close(reason) => {
                    panic!("active terminal closed while another attaches: {reason:?}")
                }
                Event::Eof => panic!("active terminal reached EOF while another attaches"),
            }
        }
    }

    async fn resize(&mut self, rows: u16, columns: u16) {
        self.send(Record::Resize { rows, columns }).await;
        let deadline = Instant::now() + IO;
        loop {
            match self.next(deadline).await {
                Event::Resize => return,
                Event::Output | Event::Credit => (),
                Event::Close(reason) => panic!("stream closed before resize: {reason:?}"),
                Event::Eof => panic!("stream reached EOF before resize"),
            }
        }
    }

    async fn close(mut self, end: End) {
        self.data.clear();
        match end {
            End::Detach => self.send(Record::Detach).await,
            End::Eof => self.stream.shutdown().await.unwrap(),
            End::ShellExit => {
                self.send(Record::Input(b"printf '<%s>' final-output; exit\r"))
                    .await;
            }
        }
        self.input_closed = !matches!(end, End::ShellExit);
        let deadline = Instant::now() + IO;
        let expected = match end {
            End::Detach | End::ShellExit => local::Close::Ended,
            End::Eof => local::Close::Transport,
        };
        loop {
            match self.next(deadline).await {
                Event::Close(reason) => {
                    assert_eq!(reason, expected, "typed terminal close");
                    break;
                }
                Event::Eof => panic!("terminal EOF lost its typed response"),
                _ => (),
            }
            assert!(
                self.data.len() <= 32 * 1024,
                "bounded terminal shutdown output"
            );
        }
        if matches!(end, End::ShellExit) {
            assert!(
                self.data
                    .windows(b"<final-output>".len())
                    .any(|bytes| bytes == b"<final-output>")
            );
        }
    }
}

async fn measure_output(terminal: &mut LocalTerminal, sample: &mut Measurements) {
    const MIB: usize = 1024 * 1024;
    terminal.resize(42, 120).await;
    let began = Instant::now();
    for run in 0..512 {
        let end = format!("display-end-{run:04}");
        // Fit the display and vary the ASCII workload so tmux does not
        // replace a repeated character run with a short control sequence.
        let input = format!(
            "printf '\\033[2J\\033[H'; awk -v phase={run} 'BEGIN {{for (i=0;i<64;i++) for (j=0;j<64;j++) printf \"%c\",33+(17*i+29*j+phase)%90}}'; printf '<%s>' '{end}'\r"
        );
        let marker = format!("<{end}>");
        sample.payload.push(
            terminal
                .echo(input.as_bytes(), marker.as_bytes(), 64 * 1024)
                .await,
        );
        sample.output += terminal.data.len();
        if sample.payload.len() >= 20 && sample.output >= MIB {
            sample.throughput = began.elapsed();
            return;
        }
    }
    panic!(
        "terminal output workload reached {} acknowledged bytes within 512 turns",
        sample.output,
    );
}

async fn predictable_shell(terminal: &mut LocalTerminal) {
    terminal
        .echo(
            b"exec /usr/bin/env -i PATH=/usr/bin:/bin TERM=dumb LC_ALL=C PS1='MAR''MISH> ' /bin/sh -i\r",
            b"MARMISH> ",
            32 * 1024,
        )
        .await;
}

async fn open_through_delayed_eose(
    relay: &Relay,
    server_alias: PublicKey,
    client_alias: PublicKey,
    path: &Path,
    start: handshake::Start,
) -> LocalTerminal {
    let began = Instant::now();
    let opening = open_terminal(path, start);
    tokio::pin!(opening);
    let wait_first = async {
        while relay.stored_gift_wraps_to(server_alias) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::select! {
        result = &mut opening => match result {
            Ok(_) => panic!("Terminal attached before authenticated inbox EOSE"),
            Err(error) => panic!("Terminal stopped before the first Probe: {error:?}"),
        },
        waited = timeout(IO, wait_first) => waited.expect("first signed Probe before EOSE"),
    }
    assert_eq!(relay.stored_gift_wraps_to(server_alias), 1);
    assert_eq!(relay.stored_gift_wraps_to(client_alias), 0);
    relay.release_alias_eose();
    let released = Instant::now();
    let terminal = opening
        .await
        .expect("fresh Probe retry after authenticated EOSE");
    let ready = Instant::now();
    assert!(ready.duration_since(began) < Duration::from_secs(30));
    assert!(
        relay.distinct_gift_wraps_before_reply(server_alias, client_alias) >= 2,
        "the first Challenge follows a distinct Probe envelope"
    );
    eprintln!(
        "terminal_installed_runtime workload=probe_retry ready_us={} eose_to_ready_us={}",
        ready.duration_since(began).as_micros(),
        ready.duration_since(released).as_micros(),
    );
    terminal
}

async fn cancel_held_probe(
    relay: &Relay,
    alias: PublicKey,
    socket: &Path,
    request: handshake::Start,
    owned_tmux: &OwnedTmux,
) {
    let alias_tag = json!(alias);
    let previous = relay.subscription_counts(alias, 1059, "#p", &alias_tag).0;
    relay.hold_next_gift_wrap(alias);
    let mut stream = timeout(IO, UnixStream::connect(socket))
        .await
        .unwrap()
        .unwrap();
    let request = serde_json::to_vec(&ipc::Request::TerminalStart { request }).unwrap();
    timeout(IO, async {
        stream
            .write_u32(u32::try_from(request.len()).unwrap())
            .await
            .unwrap();
        stream.write_all(&request).await.unwrap();
        while !relay.gift_wrap_is_held() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("relay held exact Probe");
    stream.shutdown().await.unwrap();
    relay.release_gift_wrap();
    let mut unexpected = [0; 1];
    let count = timeout(IO, stream.read(&mut unexpected))
        .await
        .expect("bounded pending cancel")
        .unwrap();
    assert_eq!(count, 0, "no Ready or refusal after pending socket EOF");
    renewed_subscription(relay, alias, previous).await;
    assert_eq!(
        owned_tmux.sockets().count(),
        0,
        "pending EOF allocated no tmux socket"
    );
}

async fn renewed_subscription(relay: &Relay, alias: PublicKey, previous: usize) {
    let alias_tag = json!(alias);
    timeout(ATTACH, async {
        while relay.subscription_counts(alias, 1059, "#p", &alias_tag).0 <= previous {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("server opens fresh authenticated Terminal alias listener");
}

fn assert_stores_omit_marker(client: &ProfileLease, server: &ProfileLease) {
    let engine = server.root().join("commands/engine.sqlite3");
    let terminal = server.root().join("terminal.sqlite3");
    for path in [
        client.mdk_database(),
        client.journal_database(),
        server.mdk_database(),
        server.journal_database(),
        &engine,
        &terminal,
    ] {
        let bytes = std::fs::read(path).expect("owned durable fixture database");
        assert!(!bytes.windows(MARKER.len()).any(|part| part == MARKER));
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut companion = path.as_os_str().to_owned();
            companion.push(suffix);
            match std::fs::read(companion) {
                Ok(bytes) => assert!(!bytes.windows(MARKER.len()).any(|part| part == MARKER)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => panic!("owned fixture database companion: {error:?}"),
            }
        }
    }
}

fn serve_ipc(
    listener: UnixListener,
    intake: handshake::Intake,
) -> (
    tokio::task::JoinHandle<Result<()>>,
    mpsc::Receiver<ipc::Pending>,
) {
    let (queue, ordinary) = mpsc::channel(1);
    let (_status, snapshots) = watch::channel(Response::success(json!({"state": "active"})));
    let listener = tokio::spawn(ipc::serve_inner(listener, queue, snapshots, Some(intake)));
    (listener, ordinary)
}

pub(crate) fn open_cli_pty() -> (std::fs::File, std::fs::File) {
    let (mut master, mut slave) = (-1, -1);
    let size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
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
    let master = unsafe { std::fs::File::from_raw_fd(master) };
    let slave = unsafe { std::fs::File::from_raw_fd(slave) };
    // Set size separately because openpty's final pointer differs by platform.
    assert_eq!(
        unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &size) },
        0
    );
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    (master, slave)
}

fn write_cli_config(state: &Path, remote: PublicKey, relay: &Relay) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(state.join("c/config.json"))
        .unwrap();
    serde_json::to_writer(
        &mut file,
        &json!({
            "version": 1, "role": "client", "tor": "off",
            "relays": [relay.url.as_str()], "trusted_remotes": [remote.to_hex()],
        "execution_clients": [], "command_policy": {"version": 1},
        "cwd": "/", "limits": {}, "aliases": {},
        }),
    )
    .unwrap();
}

fn spawn_cli(
    state: &Path,
    daemon: &Path,
    remote: PublicKey,
    slave: &std::fs::File,
    debug: bool,
) -> tokio::process::Child {
    let mut command = tokio::process::Command::new(daemon.with_file_name("marmish"));
    command.arg("--state-dir").arg(state).args(["-i", "c"]);
    if debug {
        command.arg("--debug");
    }
    command
        .arg("terminal")
        .arg(remote.to_bech32().unwrap())
        .env_clear()
        .env("TERM", "dumb")
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // The installed frontend checks foreground terminal ownership.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        })
    };
    command.spawn().expect("launch installed marmish")
}

async fn run_quiet_cli(state: &Path, daemon: &Path, remote: PublicKey, relay: &Relay) -> Duration {
    write_cli_config(state, remote, relay);
    let (_master, slave) = open_cli_pty();
    let began = Instant::now();
    let mut child = spawn_cli(state, daemon, remote, &slave, true);
    drop(slave);
    let mut stderr = child.stderr.take().unwrap().take(4097);
    let mut output = Vec::new();
    let child_result = timeout(ATTACH, async {
        tokio::try_join!(child.wait(), stderr.read_to_end(&mut output))
    })
    .await;
    let Ok(result) = child_result else {
        child.kill().await.unwrap();
        panic!("installed marmish did not exit after quiet relay");
    };
    let (status, bytes) = result.unwrap();
    assert_eq!(status.code(), Some(255), "quiet CLI refusal exit status");
    assert!(bytes < 4097);
    let error = std::str::from_utf8(&output).unwrap();
    let mut refusal = 0;
    let remote_npub = remote.to_bech32().unwrap();
    for line in error.lines() {
        if line == "marmish: invalid input: terminal_relay_auth_challenge_timeout" {
            refusal += 1;
        } else {
            let unexpected = [
                "config_schema",
                "invalid_remote_identity",
                "terminal_transport_unavailable",
                "terminal_owner_unavailable",
                "terminal_enrollment_auth",
                "terminal_timeout",
                "terminal_start_closed",
                "terminal_start_response",
                "command_preparation_binding",
            ]
            .into_iter()
            .find(|category| line.ends_with(category));
            let event: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|_| panic!("unexpected CLI diagnostic category: {unexpected:?}"));
            let keys = event.as_object().expect("diagnostic object");
            assert!(keys.keys().all(|key| matches!(
                key.as_str(),
                "architecture" | "event" | "platform" | "schema" | "version" | "stage" | "category"
            )));
            assert!(
                !line.contains(remote.to_hex().as_str())
                    && !line.contains(remote_npub.as_str())
                    && !line.contains(relay.url.as_str())
            );
        }
    }
    assert_eq!(refusal, 1, "installed CLI reports one exact category");
    began.elapsed()
}

async fn prove_writer(workload: Workload) -> Measurements {
    crate::terminal_backend::paths::checked_uid().expect("nonroot terminal fixture");
    let temp = tempfile::Builder::new()
        .prefix("ir-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let relay = Relay::start().await;
    if matches!(
        workload,
        Workload::Benchmark { throughput: true } | Workload::ObserveBenchmark
    ) {
        relay.allow_installed_benchmark();
    } else {
        relay.allow_multi_room_workload();
    }
    let mut client = CommandPeer::new(temp.path(), "c");
    let mut server = CommandPeer::new(temp.path(), "s");
    let daemon = worker()
        .with_file_name("marmishd")
        .canonicalize()
        .expect("build marmishd and marmish-worker before this fixture: cargo build --bins -p marmish-native");
    let cat = std::fs::canonicalize("/bin/cat").unwrap();
    for account in [client.public(), server.public()] {
        relay.ordinary(account);
    }
    let config = server_config(&relay, client.public(), &cat, server.lease.root());
    let mut client_owner = client.open(&relay, server.public(), false, &cat).await;
    let mut server_owner = server
        .open_with_terminal(&relay, client.public(), true, &cat, true)
        .await;
    client_owner.relay_proof_ingress = None;
    server_owner.relay_proof_ingress = None;
    client
        .lease
        .mark_ready(client_owner.opened_binding())
        .unwrap();
    server
        .lease
        .mark_ready(server_owner.opened_binding())
        .unwrap();
    let remote = server.public();
    let command_room = client_owner
        .open_command_room(remote.to_bytes(), false, Instant::now() + STEP)
        .await
        .unwrap();
    // Complete one untimed ordinary turn to establish the durable server room.
    run_command(&mut client_owner, &mut server_owner, &command_room, &cat).await;
    let began = Instant::now();
    run_command(&mut client_owner, &mut server_owner, &command_room, &cat).await;
    let before = began.elapsed();
    let server_binding = server_owner.terminal_server_bindings().unwrap();
    assert_eq!(
        server_binding.len(),
        1,
        "exact prepared Command server binding"
    );
    let server_alias_codec = crate::terminal_bootstrap::Codec::new(
        &server_binding[0].context,
        &server.material.signer(),
        &[],
    )
    .unwrap();
    let server_alias = server_alias_codec.inbound_alias().public_key();
    let client_alias = server_alias_codec.outbound_alias();
    drop(server_alias_codec);
    drop(server_binding);
    if matches!(workload, Workload::ProbeRetry) {
        relay.acknowledge_duplicates();
        relay.hold_next_alias_eose(server_alias);
    }
    let relays = vec![relay.url.to_string()];
    let prepare = |lease: &ProfileLease, material: &IdentityMaterial, commands| {
        let policy = relay.connector(lease.root(), true, None).policy().clone();
        Prepared::for_loopback_fixture(
            lease,
            material,
            commands,
            policy,
            daemon.clone(),
            relay.roots(),
        )
        .unwrap()
    };
    let mut client_prepared = prepare(&client.lease, &client.material, None);
    let mut server_prepared = prepare(&server.lease, &server.material, Some(config.clone()));
    let quiet_client = Arc::new(AtomicBool::new(false));
    let quiet_server = Arc::new(AtomicBool::new(false));
    if matches!(workload, Workload::QuietAuth) {
        relay.arm_quiet_aliases([server_alias, client_alias]);
        client_prepared.watch_fixture_quiet_open(quiet_client.clone());
        server_prepared.watch_fixture_quiet_open(quiet_server.clone());
    }
    let cleanup = OwnedTmux {
        directory: server.lease.root().join("t"),
        executable: crate::terminal_owner::tmux_path().expect("checked fixture tmux"),
    };
    let socket = client.lease.socket_path();
    let listener = ipc::bind(&socket).unwrap();
    let session = hex::encode(command_room.session);
    let client_lease = Arc::new(client.lease);
    let server_lease = Arc::new(server.lease);
    let (client_runtime, client_control) =
        Runtime::start(client_prepared, client_lease.clone()).unwrap();
    let (server_runtime, server_control) =
        match Runtime::start(server_prepared, server_lease.clone()) {
            Ok(running) => running,
            Err(error) => {
                client_runtime.join().await.unwrap();
                panic!("server fixture runtime start: {error:?}");
            }
        };
    let mut server_runtime = Some(server_runtime);
    let (ipc, ordinary) = serve_ipc(listener, client_control.intake.clone());
    let (turns, receive) = mpsc::channel(1);
    let actor = tokio::task::spawn_local(
        CommandOwners {
            client: client_owner,
            server: server_owner,
            client_control,
            server_control,
            command: command_room,
            cat,
            relays: relays.clone(),
        }
        .run(receive, ordinary),
    );
    let start = |attach| handshake::Start {
        remote: remote.to_hex(),
        session: session.clone(),
        relays: relays.clone(),
        rows: 24,
        columns: 80,
        attach,
        session_slot: None,
        offer: None,
    };
    let verdict = AssertUnwindSafe(async {
        if matches!(workload, Workload::QuietAuth) {
            let baseline = [relay.publication_attempts(1059), relay.publication_attempts(445)];
            let elapsed = run_quiet_cli(temp.path(), &daemon, remote, &relay).await;
            timeout(IO, async {
                while !quiet_server.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }).await.expect("typed inbound quiet-auth timeout");
            assert!(quiet_client.load(Ordering::Acquire), "typed outbound quiet-auth timeout");
            let (alias_requests, unexpected) = relay.quiet_alias_observations();
            assert!(alias_requests.into_iter().all(|attempts| attempts > 0));
            assert_eq!(unexpected, 0, "only exact alias REQ and cleanup CLOSE");
            assert_eq!(baseline, [relay.publication_attempts(1059), relay.publication_attempts(445)]);
            assert_eq!(cleanup.sockets().count(), 0, "quiet AUTH allocated no tmux socket");
            eprintln!("terminal_relay_quiet_cli_ms={:.1} ordinary_command_ms={:.1}",
                elapsed.as_secs_f64() * 1000.0, before.as_secs_f64() * 1000.0);
            return Measurements { unloaded: before, ..Measurements::default() };
        }
        relay
            .subscription(server_alias, 1059, "#p", json!(server_alias))
            .await;
        if matches!(
            workload,
            Workload::Observe | Workload::ObserveBenchmark | Workload::Takeover
        ) {
            let mut sample = observe::prove(&socket, start, &cleanup, &turns, workload).await;
            sample.unloaded = before;
            return sample;
        }
        if matches!(workload, Workload::Foreground) {
            let (cold, echo) =
                foreground::prove(temp.path(), &daemon, remote, &relay, &cleanup).await;
            return Measurements {
                cold,
                echo,
                unloaded: before,
                after: command(&turns).await,
                ..Measurements::default()
            };
        }
        if matches!(workload, Workload::Cancel) {
            cancel_held_probe(
                &relay,
                server_alias,
                &socket,
                start(handshake::Attach::Default),
                &cleanup,
            )
            .await;
        }
        let began = Instant::now();
        let request = start(handshake::Attach::Default);
        let mut terminal = if matches!(workload, Workload::ProbeRetry) {
            open_through_delayed_eose(&relay, server_alias, client_alias, &socket, request).await
        } else {
            open_terminal(&socket, request).await.expect("cold Default writer")
        };
        let cold = began.elapsed();
        if matches!(workload, Workload::Cancel) {
            assert_eq!(cleanup.sockets().count(), 1, "retry allocated one tmux socket");
        }
        predictable_shell(&mut terminal).await;
        // A bare key exercises the PTY's actual line echo. Ctrl-U clears
        // the pending line before printing the UTF-8 and punctuation marker.
        let echo = terminal.echo(b"~", b"~", 32 * 1024).await;
        let (_, during) = tokio::join!(
            terminal.echo(
                "\x15printf '\\033[2J\\033[H[%s]\\n' 'two words; % 🐾'; printf '%4096s' '' | tr ' ' z; printf '<%s>' overlap\r".as_bytes(),
                b"<overlap>",
                32 * 1024,
            ),
            command(&turns),
        );
        assert!(terminal.data.windows("[two words; % 🐾]".len())
            .any(|bytes| bytes == "[two words; % 🐾]".as_bytes()));

        let mut sample = Measurements {
            cold,
            unloaded: before,
            loaded: during,
            echo,
            ..Measurements::default()
        };
        if matches!(workload, Workload::Benchmark { throughput: true }) {
            measure_output(&mut terminal, &mut sample).await;
        }

        terminal.resize(30, 90).await;
        terminal
            .echo(b"stty size | tr ' ' :\r", b"30:90", 32 * 1024)
            .await;
        terminal
            .echo(
                b"MARMISH_KEEP=YES; printf '<%s>' ready\r",
                b"<ready>",
                32 * 1024,
            )
            .await;
        if matches!(workload, Workload::Verify) {
            terminal
                .echo(
                    b"printf '<%s>' interrupt-ready; /bin/sleep 30\r",
                    b"<interrupt-ready>",
                    32 * 1024,
                )
                .await;
            terminal.send(Record::Input(b"\x03")).await;
            terminal
                .echo(b"printf '<%s>' interrupted\r", b"<interrupted>", 32 * 1024)
                .await;
            assert_eq!(cleanup.sockets().count(), 1, "one tmux socket for the Default slot");
            assert!(matches!(
                terminal
                    .while_attached(open_terminal(&socket, start(handshake::Attach::Default)))
                    .await,
                Err(Error::Invalid("terminal_busy"))
            ));
            assert_eq!(cleanup.sockets().count(), 1, "Busy allocated no new tmux socket");
            terminal
                .echo(b"printf '<%s>' stable\r", b"<stable>", 32 * 1024)
                .await;
            let mut isolated = terminal
                .while_attached(open_terminal(&socket, start(handshake::Attach::New)))
                .await
                .expect("separate New writer");
            assert_eq!(cleanup.sockets().count(), 2, "New allocated exactly one tmux socket");
            predictable_shell(&mut isolated).await;
            isolated
                .echo(
                    b"printf '<%s>' \"${MARMISH_KEEP:-EMPTY}\"\r",
                    b"<EMPTY>",
                    32 * 1024,
                )
                .await;
            // Native EOF, without sending the application Detach record.
            isolated.close(End::Eof).await;
        }
        terminal.close(End::Detach).await;
        if matches!(workload, Workload::Restart) {
            assert_eq!(cleanup.sockets().count(), 1, "one tmux socket before owner restart");
            server_runtime.take().unwrap().join().await.unwrap();
            assert_eq!(cleanup.sockets().count(), 1, "owner shutdown preserved the tmux socket");
            let previous = relay.subscription_counts(server_alias, 1059, "#p", &json!(server_alias)).0;
            assert!(relay.stored_gift_wraps_to(server_alias) > 0, "relay retains the first flight");
            let prior_replies = relay.stored_gift_wraps_to(client_alias);
            let prepared = prepare(&server_lease, &server.material, Some(config.clone()));
            let (runtime, control) = Runtime::start(prepared, server_lease.clone()).unwrap();
            server_runtime = Some(runtime);
            turns.send(CommandTurn::ReplaceServer(control)).await.unwrap();
            renewed_subscription(&relay, server_alias, previous).await;
            let unsolicited = timeout(Duration::from_secs(2), async {
                while relay.stored_gift_wraps_to(client_alias) == prior_replies {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }).await;
            assert!(unsolicited.is_err(), "restarted owner replied to stored bootstrap without a live client");
            assert_eq!(cleanup.sockets().count(), 1, "stored relay traffic allocated no tmux socket");
        }
        let began = Instant::now();
        let (mut resumed, retries) = resume_default(&socket, start).await.expect("warm Default writer");
        sample.warm_busy_retries = retries;
        if matches!(workload, Workload::Restart) {
            assert_eq!(cleanup.sockets().count(), 1, "restarted owner reused the tmux socket");
        }
        sample.warm = began.elapsed();
        resumed
            .echo(b"printf '<%s>' \"$MARMISH_KEEP\"\r", b"<YES>", 32 * 1024)
            .await;
        resumed
            .close(if matches!(workload, Workload::ShellExit) {
                End::ShellExit
            } else {
                End::Detach
            })
            .await;
        sample.after = command(&turns).await;
        sample
    })
    .catch_unwind()
    .await;
    ipc.abort();
    let ipc_result = ipc.await;
    let join_server = async {
        match server_runtime {
            Some(runtime) => runtime.join().await,
            None => Ok(()),
        }
    };
    let (client_result, server_result) = tokio::join!(client_runtime.join(), join_server);
    let _ = turns.send(CommandTurn::Stop).await;
    let actor_result = actor.await;
    drop(cleanup);
    let client_cleanup = client_lease.cleanup_runtime();
    let server_cleanup = server_lease.cleanup_runtime();
    relay.assert_healthy();
    let sample = verdict.unwrap_or_else(|panic| resume_unwind(panic));
    match ipc_result {
        Err(error) if error.is_cancelled() => (),
        result => panic!("test IPC listener terminated early: {result:?}"),
    }
    actor_result.unwrap();
    client_result.unwrap();
    server_result.unwrap();
    client_cleanup.unwrap();
    server_cleanup.unwrap();
    relay.assert_no_plaintext(MARKER);
    assert_stores_omit_marker(&client_lease, &server_lease);
    if let Some(benchmark) = &sample.observer {
        benchmark.report(relay.accepted_publications());
    }
    sample
}
