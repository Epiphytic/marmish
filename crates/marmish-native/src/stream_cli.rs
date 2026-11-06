//! Binary-clean stream frontend. Socket waits never prevent explicit cancellation.
use crate::{
    Error, Result,
    command_cli::Context,
    ipc,
    stream_service::{StreamLocalRequest, StreamLocalResult},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use marmish_stream::{
    codec::{self, Codec},
    wire::{self, Binding, Request, RequestBody, ResponseBody, Terminal},
};
use std::{cell::Cell, ffi::OsString, os::unix::ffi::OsStringExt, time::Duration};
use tokio::time::Instant;

fn diagnostic(message: std::fmt::Arguments<'_>) {
    use std::io::Write;
    // A full or closed diagnostic pipe must never panic during cancellation.
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

fn invalid(why: &'static str) -> Error {
    Error::Invalid(why)
}
fn protocol<T>(result: marmish_stream::Result<T>) -> Result<T> {
    result.map_err(|_| invalid("stream_protocol"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignalPolicy {
    Interactive,
    Rsync,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExecutionSignal {
    Interrupt,
    Terminate,
    Hangup,
    User,
}

/// Registered before any frontend await, retained through cancel confirmation.
pub(crate) struct ExecutionSignals {
    policy: SignalPolicy,
    interrupt: tokio::signal::unix::Signal,
    terminate: Option<tokio::signal::unix::Signal>,
    hangup: Option<tokio::signal::unix::Signal>,
    user: Option<tokio::signal::unix::Signal>,
    last: Option<ExecutionSignal>,
}

async fn optional_signal(signal: &mut Option<tokio::signal::unix::Signal>) -> Option<()> {
    match signal {
        Some(signal) => signal.recv().await,
        None => std::future::pending().await,
    }
}

impl ExecutionSignals {
    pub(crate) fn register(policy: SignalPolicy) -> Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        let rsync = policy == SignalPolicy::Rsync;
        Ok(Self {
            policy,
            interrupt: signal(SignalKind::interrupt())?,
            terminate: rsync.then(|| signal(SignalKind::terminate())).transpose()?,
            hangup: rsync.then(|| signal(SignalKind::hangup())).transpose()?,
            user: rsync
                .then(|| signal(SignalKind::user_defined1()))
                .transpose()?,
            last: None,
        })
    }

    pub(crate) async fn interrupted(&mut self) -> Result<()> {
        let (received, kind) = tokio::select! {
            biased;
            value = self.interrupt.recv() => (value, ExecutionSignal::Interrupt),
            value = optional_signal(&mut self.terminate) => (value, ExecutionSignal::Terminate),
            value = optional_signal(&mut self.hangup) => (value, ExecutionSignal::Hangup),
            value = optional_signal(&mut self.user) => (value, ExecutionSignal::User),
        };
        received.ok_or(invalid("stream_signal"))?;
        self.last = Some(kind);
        Ok(())
    }

    async fn pending(&mut self) -> Result<bool> {
        tokio::select! {
            biased;
            signal = self.interrupted() => { signal?; Ok(true) }
            () = std::future::ready(()) => Ok(false),
        }
    }

    // OS handlers wake Tokio's signal driver before recv becomes ready. A
    // bounded wait gives that driver a real scheduling opportunity even when
    // this frontend runs on the multi-thread runtime's block_on thread.
    async fn pending_after_dispatch(&mut self) -> Result<bool> {
        if self.pending().await? {
            return Ok(true);
        }
        match tokio::time::timeout(Duration::from_millis(50), self.interrupted()).await {
            Ok(result) => {
                result?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn coalesces(&self, first: Instant, now: Instant) -> bool {
        self.policy == SignalPolicy::Rsync
            && (self.last == Some(ExecutionSignal::User)
                || now.duration_since(first) < Duration::from_secs(2))
    }
}

async fn confirm_cancel(peer: &Peer<'_>, signals: &mut ExecutionSignals) -> Result<u8> {
    diagnostic(format_args!(
        "marmish stream: requesting remote cancellation"
    ));
    let first = Instant::now();
    let confirmation = peer.cancel();
    tokio::pin!(confirmation);
    loop {
        tokio::select! {
            biased;
            signal = signals.interrupted() => {
                signal?;
                if !signals.coalesces(first, Instant::now()) {
                    return Err(invalid("stream_cancellation_uncertain"));
                }
            }
            result = &mut confirmation => return result,
        }
    }
}

async fn attachment_error(
    peer: &Peer<'_>,
    signals: &mut ExecutionSignals,
    open_attempted: bool,
    error: Error,
) -> Result<u8> {
    if signals.pending_after_dispatch().await? {
        if open_attempted {
            confirm_cancel(peer, signals).await
        } else {
            interrupted_before_open()
        }
    } else {
        Err(error)
    }
}

fn interrupted_before_open<T>() -> Result<T> {
    diagnostic(format_args!(
        "marmish stream: interrupted before remote Open; no remote child was launched"
    ));
    Err(invalid("stream_interrupted_before_open"))
}

struct Peer<'a> {
    context: &'a Context,
    relays: Vec<String>,
    binding: Binding,
    idle: Duration,
}
impl Peer<'_> {
    async fn call(&self, sequence: u64, body: RequestBody) -> Result<wire::Response> {
        let request = Request {
            binding: self.binding.clone(),
            sequence,
            body,
        };
        let bytes = protocol(wire::encode_request(&request))?;
        let digest = wire::request_digest(&bytes);
        let local = StreamLocalRequest::Frame {
            frame_base64: STANDARD.encode(bytes),
            relays: self.relays.clone(),
        };
        let deadline = Instant::now() + self.idle;
        loop {
            let result = tokio::time::timeout_at(
                deadline,
                ipc::stream_request(&self.context.socket, local.clone()),
            )
            .await
            .map_err(|_| Error::Timeout)??;
            match result {
                StreamLocalResult::Pending => (),
                StreamLocalResult::Frame { frame_base64 } => {
                    if frame_base64.len() > 44_000 {
                        return Err(invalid("stream_response_size"));
                    }
                    let bytes = STANDARD
                        .decode(&frame_base64)
                        .map_err(|_| invalid("stream_response_base64"))?;
                    let response = protocol(wire::decode_response(&bytes))?;
                    if response.binding != self.binding
                        || response.sequence != sequence
                        || response.request_sha256 != digest
                    {
                        return Err(invalid("stream_response_binding"));
                    }
                    if let ResponseBody::Error { code } = response.body {
                        diagnostic(format_args!("marmish stream: remote error {code:?}"));
                        return Err(invalid("stream_remote_error"));
                    }
                    return Ok(response);
                }
                _ => return Err(invalid("stream_local_response")),
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_millis(100)).min(deadline))
                .await;
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
        }
    }
    async fn cancel(&self) -> Result<u8> {
        let deadline = Instant::now() + self.idle.min(Duration::from_secs(60));
        loop {
            let response = tokio::time::timeout_at(deadline, self.call(0, RequestBody::Cancel))
                .await
                .map_err(|_| Error::Timeout)??;
            if let ResponseBody::Exchange {
                terminal: Some(terminal),
                ..
            } = response.body
            {
                let code = cancellation_result(terminal)?;
                diagnostic(format_args!(
                    "marmish stream {}: remote terminal state confirmed",
                    hex::encode(self.binding.stream)
                ));
                return Ok(code);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
        }
    }
}
fn cancellation_result(terminal: Terminal) -> Result<u8> {
    if terminal
        == (Terminal::Failed {
            code: wire::WireErrorCode::Uncertain,
        })
    {
        return Err(invalid("stream_cancellation_uncertain"));
    }
    Ok(exit_code(terminal))
}

fn exit_code(terminal: Terminal) -> u8 {
    match terminal {
        Terminal::Exited { code } => code,
        Terminal::Signaled { signal } => 128u8.saturating_add(signal),
        Terminal::Cancelled => 130,
        Terminal::Failed { code } => {
            diagnostic(format_args!("marmish stream: remote failure {code:?}"));
            255
        }
    }
}

/// Restore shared file-description flags on normal unwinding, including cancellation.
/// Abrupt termination that skips Drop can leave inherited descriptors nonblocking.
struct Nonblocking {
    flags: Vec<(i32, i32)>,
}
impl Nonblocking {
    fn stdio() -> Result<Self> {
        Self::descriptors(&[0, 1, 2])
    }
    fn descriptors(descriptors: &[i32]) -> Result<Self> {
        // Read every original flag before changing any: dup'd descriptors can
        // share one open file description (a terminal or shell's 2>&1).
        let mut flags = Vec::new();
        for &fd in descriptors {
            let original = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if original < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            flags.push((fd, original));
        }
        let guard = Self { flags };
        for &(fd, original) in &guard.flags {
            if unsafe { libc::fcntl(fd, libc::F_SETFL, original | libc::O_NONBLOCK) } < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(guard)
    }
}
impl Drop for Nonblocking {
    fn drop(&mut self) {
        for &(fd, flags) in self.flags.iter().rev() {
            unsafe {
                libc::fcntl(fd, libc::F_SETFL, flags);
            }
        }
    }
}
fn try_read() -> Result<Option<Vec<u8>>> {
    let mut bytes = vec![0; marmish_stream::MAX_BLOCK];
    let count = unsafe { libc::read(0, bytes.as_mut_ptr().cast(), bytes.len()) };
    if count < 0 {
        let error = std::io::Error::last_os_error();
        return if matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        ) {
            Ok(None)
        } else {
            Err(error.into())
        };
    }
    bytes.truncate(count as usize);
    Ok(Some(bytes))
}
async fn write_bytes(fd: i32, bytes: &[u8]) -> Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        let count =
            unsafe { libc::write(fd, bytes[written..].as_ptr().cast(), bytes.len() - written) };
        if count > 0 {
            written += count as usize;
        } else if count == 0 {
            return Err(invalid("stream_output_closed"));
        } else {
            let error = std::io::Error::last_os_error();
            if !matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) {
                return Err(error.into());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    Ok(())
}

struct Progress {
    stdin: u64,
    accepted: u64,
    delivered: u64,
    stdout: u64,
    stderr: u64,
    eof: bool,
    stdin_closed: bool,
}
impl Progress {
    async fn receive(
        &mut self,
        response: ResponseBody,
        codec: Codec,
    ) -> Result<(bool, Option<u8>)> {
        let ResponseBody::Exchange {
            stdin_accepted,
            stdin_delivered,
            stdin_closed,
            stdout,
            stderr,
            stdout_eof,
            stderr_eof,
            terminal,
            ..
        } = response
        else {
            return Err(invalid("stream_exchange_response"));
        };
        if stdin_delivered < self.delivered
            || stdin_delivered > stdin_accepted
            || stdin_accepted < self.accepted
            || stdin_accepted > self.stdin
            || (!stdin_closed && stdin_accepted != self.stdin)
        {
            return Err(invalid("stream_stdin_ack"));
        }
        self.delivered = stdin_delivered;
        self.accepted = stdin_accepted;
        if stdin_closed {
            // The child may exit or close its pipe before reading all local
            // input. Discard only the unsent/unaccepted tail, never claim it
            // was delivered, and continue draining output and natural status.
            self.stdin = stdin_accepted;
            self.eof = true;
            self.stdin_closed = true;
        }
        let mut progress = false;
        for (fd, block, cursor) in [(1, stdout, &mut self.stdout), (2, stderr, &mut self.stderr)] {
            if let Some(block) = block {
                if block.offset != *cursor || (block.codec != Codec::None && block.codec != codec) {
                    return Err(invalid("stream_output_offset"));
                }
                let bytes = protocol(codec::decode(&block, codec))?;
                write_bytes(fd, &bytes).await?;
                *cursor = cursor
                    .checked_add(bytes.len() as u64)
                    .ok_or(invalid("stream_offset"))?;
                progress = true;
            }
        }
        if let Some(terminal) = terminal {
            if terminal
                == (Terminal::Failed {
                    code: wire::WireErrorCode::Uncertain,
                })
            {
                return Err(invalid("stream_execution_uncertain"));
            }
            if stdout_eof != Some(self.stdout) || stderr_eof != Some(self.stderr) {
                return Ok((progress, None));
            }
            return Ok((true, Some(exit_code(terminal))));
        }
        Ok((progress, None))
    }
}

async fn pipeline(
    peer: &Peer<'_>,
    argv: Vec<Vec<u8>>,
    codec: Codec,
    keepalive: Duration,
    open_attempted: &Cell<bool>,
) -> Result<u8> {
    let hello = peer
        .call(
            0,
            RequestBody::Hello {
                codecs: if codec == Codec::None {
                    vec![Codec::None]
                } else {
                    vec![Codec::None, codec]
                },
            },
        )
        .await?;
    if !matches!(hello.body, ResponseBody::Hello { ref codecs, .. } if codecs.contains(&codec)) {
        return Err(invalid("stream_unsupported"));
    }
    open_attempted.set(true);
    if !matches!(peer.call(0, RequestBody::Open { argv, codec }).await?.body, ResponseBody::Opened { codec: selected, .. } if selected==codec)
    {
        return Err(invalid("stream_open"));
    }
    let mut state = Progress {
        stdin: 0,
        accepted: 0,
        delivered: 0,
        stdout: 0,
        stderr: 0,
        eof: false,
        stdin_closed: false,
    };
    let mut sequence = 1u64;
    let mut backoff = Duration::ZERO;
    loop {
        let turn_started = Instant::now();
        let input = if state.eof
            || state.stdin - state.delivered
                > (marmish_stream::WINDOW_BYTES - marmish_stream::MAX_BLOCK) as u64
        {
            None
        } else {
            try_read()?
        };
        let block = match input {
            Some(bytes) if bytes.is_empty() => {
                state.eof = true;
                None
            }
            Some(bytes) => {
                let block = protocol(codec::encode(state.stdin, &bytes, codec))?;
                state.stdin += bytes.len() as u64;
                Some(block)
            }
            None => None,
        };
        let active = block.is_some();
        let response = peer
            .call(
                sequence,
                RequestBody::Exchange {
                    stdin: block,
                    stdin_eof: (state.eof && !state.stdin_closed).then_some(state.stdin),
                    stdout_ack: state.stdout,
                    stderr_ack: state.stderr,
                },
            )
            .await?;
        let (progress, terminal) = state.receive(response.body, codec).await?;
        if let Some(code) = terminal {
            let final_sequence = sequence.checked_add(1).ok_or(invalid("stream_sequence"))?;
            let final_response = peer
                .call(
                    final_sequence,
                    RequestBody::Exchange {
                        stdin: None,
                        stdin_eof: None,
                        stdout_ack: state.stdout,
                        stderr_ack: state.stderr,
                    },
                )
                .await?;
            let (_, confirmed) = state.receive(final_response.body, codec).await?;
            if confirmed != Some(code) {
                return Err(invalid("stream_terminal_changed"));
            }
            return Ok(code);
        }
        sequence = sequence.checked_add(1).ok_or(invalid("stream_sequence"))?;
        backoff = if progress || active {
            Duration::ZERO
        } else if backoff.is_zero() {
            Duration::from_millis(250)
        } else {
            (backoff * 2).min(Duration::from_secs(2))
        };
        // Short polls notice stdin readiness without blocking the independent Ctrl-C lane.
        let until = (Instant::now() + backoff.min(keepalive))
            .max(turn_started + Duration::from_millis(125));
        while Instant::now() < until {
            let mut descriptor = libc::pollfd {
                fd: 0,
                events: libc::POLLIN,
                revents: 0,
            };
            if Instant::now() >= turn_started + Duration::from_millis(125)
                && !state.eof
                && state.stdin - state.delivered
                    <= (marmish_stream::WINDOW_BYTES - marmish_stream::MAX_BLOCK) as u64
                && unsafe { libc::poll(&mut descriptor, 1, 0) } > 0
            {
                break;
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_millis(20)).min(until)).await;
        }
    }
}

pub(crate) struct ExecuteOptions {
    pub compress: bool,
    pub new_session: bool,
    pub idle: Duration,
    pub keepalive: Duration,
    pub signals: ExecutionSignals,
}
pub(crate) async fn execute(
    context: Context,
    remote: [u8; 32],
    relays: Vec<String>,
    argv: Vec<OsString>,
    options: ExecuteOptions,
) -> Result<u8> {
    let mut signals = options.signals;
    // Reject unusable local pipes before room preparation or remote Open. Keep
    // the restoration guard outside the cancellable pipeline future.
    let _flags = Nonblocking::stdio()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid("stream_clock"))?
        .as_secs();
    let mut stream = [0u8; 16];
    stream[..8].copy_from_slice(&now.to_be_bytes());
    getrandom::fill(&mut stream[8..]).map_err(|_| Error::Identity("random"))?;
    let (session, client) = tokio::select! {
        biased;
        signal = signals.interrupted() => {
            signal?;
            return interrupted_before_open();
        }
        result = context.prepare(remote, relays.clone(), options.new_session) => result?,
    };
    let peer = Peer {
        context: &context,
        relays,
        binding: Binding {
            stream,
            session,
            client,
            server: remote,
        },
        idle: options.idle,
    };
    let codec = if options.compress {
        Codec::Zlib
    } else {
        Codec::None
    };
    let argv = argv.into_iter().map(OsString::into_vec).collect();
    diagnostic(format_args!("marmish stream: {}", hex::encode(stream)));
    let open_attempted = Cell::new(false);
    let result = tokio::select! {
        biased;
        signal = signals.interrupted() => {
            signal?;
            if open_attempted.get() {
                confirm_cancel(&peer, &mut signals).await
            } else {
                interrupted_before_open()
            }
        }
        result = pipeline(&peer, argv, codec, options.keepalive, &open_attempted) => {
            match result {
                Err(error) => attachment_error(&peer, &mut signals, open_attempted.get(), error).await,
                Ok(code) => Ok(code),
            }
        }
    };
    if result.is_err()
        && !matches!(
            result,
            Err(Error::Invalid("stream_interrupted_before_open"))
        )
    {
        diagnostic(format_args!(
            "marmish stream {}: attachment ended; remote execution may continue",
            hex::encode(stream)
        ));
    }
    result
}
fn parse_id<const N: usize>(text: &str) -> Result<[u8; N]> {
    if text.len() != N * 2
        || text
            .bytes()
            .any(|b| !b.is_ascii_digit() && !(b'a'..=b'f').contains(&b))
    {
        return Err(invalid("stream_id"));
    }
    hex::decode(text)
        .map_err(|_| invalid("stream_id"))?
        .try_into()
        .map_err(|_| invalid("stream_id"))
}
pub(crate) async fn list(context: &Context, remote: Option<[u8; 32]>) -> Result<u8> {
    let result = ipc::stream_request(
        &context.socket,
        StreamLocalRequest::List {
            remote: remote.map(hex::encode),
        },
    )
    .await?;
    let StreamLocalResult::Entries { entries } = result else {
        return Err(invalid("stream_list"));
    };
    println!(
        "{}",
        serde_json::to_string(&entries).map_err(|_| invalid("stream_list"))?
    );
    Ok(0)
}
pub(crate) async fn cancel_selected(
    context: &Context,
    stream: &str,
    remote: Option<[u8; 32]>,
    local: bool,
    relays: Vec<String>,
) -> Result<u8> {
    let id = parse_id::<16>(stream)?;
    if local {
        ipc::stream_request(
            &context.socket,
            StreamLocalRequest::AdminCancel {
                stream: hex::encode(id),
            },
        )
        .await?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let result =
                ipc::stream_request(&context.socket, StreamLocalRequest::List { remote: None })
                    .await?;
            let StreamLocalResult::Entries { entries } = result else {
                return Err(invalid("stream_list"));
            };
            let entry = entries
                .into_iter()
                .find(|entry| entry.stream == stream && entry.direction == "server")
                .ok_or(invalid("stream_not_found"))?;
            if let Some(terminal) = entry.terminal {
                let code = cancellation_result(terminal)?;
                diagnostic(format_args!(
                    "marmish stream: server process terminal state confirmed"
                ));
                return Ok(code);
            }
            if Instant::now() >= deadline {
                return Err(invalid("stream_cancellation_uncertain"));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let result = ipc::stream_request(
        &context.socket,
        StreamLocalRequest::List {
            remote: remote.map(hex::encode),
        },
    )
    .await?;
    let StreamLocalResult::Entries { entries } = result else {
        return Err(invalid("stream_list"));
    };
    let entry = entries
        .into_iter()
        .find(|e| e.stream == stream && e.direction == "client")
        .ok_or(invalid("stream_not_found"))?;
    let peer = Peer {
        context,
        relays,
        binding: Binding {
            stream: id,
            session: parse_id(&entry.session)?,
            client: parse_id(&entry.client)?,
            server: parse_id(&entry.server)?,
        },
        idle: Duration::from_secs(60),
    };
    peer.cancel().await
}
pub(crate) async fn reconcile(
    context: &Context,
    stream: &str,
    acknowledge_possible_live: bool,
) -> Result<u8> {
    parse_id::<16>(stream)?;
    ipc::stream_request(
        &context.socket,
        StreamLocalRequest::AdminReconcile {
            stream: stream.to_owned(),
            acknowledge_possible_live,
        },
    )
    .await?;
    diagnostic(format_args!(
        "marmish stream: reconciliation recorded; this does not confirm termination"
    ));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn rsync_signal_and_epipe_priority_are_isolated() {
        signal_epipe_subprocess("stream_cli::tests::rsync_signal_and_epipe_priority_are_isolated")
            .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rsync_signal_and_epipe_priority_on_worker_threads() {
        signal_epipe_subprocess(
            "stream_cli::tests::rsync_signal_and_epipe_priority_on_worker_threads",
        )
        .await;
    }

    async fn signal_epipe_subprocess(test_name: &str) {
        const CHILD: &str = "MARMISH_TEST_RSYNC_SIGNAL_SOCKET";
        const MODE: &str = "MARMISH_TEST_RSYNC_SIGNAL_MODE";
        if let Some(socket) = std::env::var_os(CHILD) {
            let mut signals = ExecutionSignals::register(SignalPolicy::Rsync).unwrap();
            let mode = std::env::var(MODE).unwrap();
            let signaled = mode != "detach";
            if signaled {
                assert_eq!(unsafe { libc::raise(libc::SIGUSR1) }, 0);
            }
            let mut pipe = [0; 2];
            assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
            unsafe {
                libc::close(pipe[0]);
            }
            let error = write_bytes(pipe[1], b"closed").await.unwrap_err();
            unsafe {
                libc::close(pipe[1]);
            }
            let context = Context {
                socket: socket.into(),
                profile: "test".into(),
            };
            let peer = Peer {
                context: &context,
                relays: Vec::new(),
                binding: Binding {
                    stream: [1; 16],
                    session: [2; 16],
                    client: [3; 32],
                    server: [4; 32],
                },
                idle: Duration::from_secs(3),
            };
            let result = attachment_error(&peer, &mut signals, mode != "preopen", error).await;
            if mode == "preopen" {
                assert!(matches!(
                    result,
                    Err(Error::Invalid("stream_interrupted_before_open"))
                ));
            } else if signaled {
                assert_eq!(result.unwrap(), 130);
            } else {
                assert!(
                    matches!(result, Err(Error::Io(ref error)) if error.kind() == std::io::ErrorKind::BrokenPipe)
                );
            }
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for mode in ["signal", "detach", "preopen"] {
            let directory = tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let path = directory.path().join("s");
            let listener = ipc::bind(&path).unwrap();
            let cancellations = Arc::new(AtomicUsize::new(0));
            let count = cancellations.clone();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let length = socket.read_u32().await.unwrap() as usize;
                assert!(length <= 65_536);
                let mut bytes = vec![0; length];
                socket.read_exact(&mut bytes).await.unwrap();
                let ipc::Request::Stream {
                    request: StreamLocalRequest::Frame { frame_base64, .. },
                } = serde_json::from_slice(&bytes).unwrap()
                else {
                    panic!("expected stream");
                };
                let bytes = STANDARD.decode(frame_base64).unwrap();
                let request = wire::decode_request(&bytes).unwrap();
                assert!(matches!(request.body, RequestBody::Cancel));
                count.fetch_add(1, Ordering::SeqCst);
                let response = wire::Response {
                    binding: request.binding,
                    sequence: 0,
                    request_sha256: wire::request_digest(&bytes),
                    body: ResponseBody::Exchange {
                        stdin_accepted: 0,
                        stdin_delivered: 0,
                        stdin_closed: true,
                        stdout: None,
                        stderr: None,
                        stdout_eof: Some(0),
                        stderr_eof: Some(0),
                        cancel_requested: true,
                        terminal: Some(Terminal::Cancelled),
                    },
                };
                let response = ipc::Response::stream(StreamLocalResult::Frame {
                    frame_base64: STANDARD.encode(wire::encode_response(&response).unwrap()),
                });
                let bytes = serde_json::to_vec(&response).unwrap();
                socket.write_u32(bytes.len() as u32).await.unwrap();
                socket.write_all(&bytes).await.unwrap();
            });
            let child = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test_name, "--nocapture"])
                .env(CHILD, &path)
                .env(MODE, mode)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
                .await
                .expect("signal test child timed out")
                .unwrap();
            assert!(output.status.success(), "mode={mode} child={output:?}");
            assert_eq!(
                cancellations.load(Ordering::SeqCst),
                usize::from(mode == "signal")
            );
            server.abort();
            let _ = server.await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_confirmation_handles_late_signals_without_extending_deadline() {
        const CHILD: &str = "MARMISH_TEST_CANCEL_CONFIRM_SOCKET";
        const MODE: &str = "MARMISH_TEST_CANCEL_CONFIRM_MODE";
        if let Some(socket) = std::env::var_os(CHILD) {
            let mode = std::env::var(MODE).unwrap();
            let mut signals = ExecutionSignals::register(SignalPolicy::Rsync).unwrap();
            let context = Context {
                socket: socket.into(),
                profile: "test".into(),
            };
            let peer = Peer {
                context: &context,
                relays: Vec::new(),
                binding: Binding {
                    stream: [1; 16],
                    session: [2; 16],
                    client: [3; 32],
                    server: [4; 32],
                },
                idle: Duration::from_secs(3),
            };
            let late_interrupt = mode == "interrupt";
            let repeat = mode == "deadline";
            let sender = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(2150)).await;
                let signal = if late_interrupt {
                    libc::SIGINT
                } else {
                    libc::SIGUSR1
                };
                assert_eq!(unsafe { libc::raise(signal) }, 0);
                if repeat {
                    for _ in 0..2 {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        assert_eq!(unsafe { libc::raise(libc::SIGUSR1) }, 0);
                    }
                }
            });
            let started = Instant::now();
            let result = confirm_cancel(&peer, &mut signals).await;
            match mode.as_str() {
                "interrupt" => assert!(matches!(
                    result,
                    Err(Error::Invalid("stream_cancellation_uncertain"))
                )),
                "confirm" => assert_eq!(result.unwrap(), 130),
                "deadline" => {
                    assert!(matches!(result, Err(Error::Timeout)));
                    // Forwarded USR1 notifications must not restart the original deadline.
                    assert!(started.elapsed() >= Duration::from_millis(2900));
                    assert!(started.elapsed() < Duration::from_millis(3900));
                }
                _ => panic!("unknown signal mode"),
            }
            assert!(started.elapsed() >= Duration::from_secs(2));
            sender.await.unwrap();
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        for mode in ["interrupt", "confirm", "deadline"] {
            let directory = tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let path = directory.path().join("s");
            let listener = ipc::bind(&path).unwrap();
            let started = Instant::now();
            let server = tokio::spawn(async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let length = socket.read_u32().await.unwrap() as usize;
                    assert!(length <= 65_536);
                    let mut bytes = vec![0; length];
                    socket.read_exact(&mut bytes).await.unwrap();
                    let ipc::Request::Stream {
                        request: StreamLocalRequest::Frame { frame_base64, .. },
                    } = serde_json::from_slice(&bytes).unwrap()
                    else {
                        panic!("expected Cancel")
                    };
                    let bytes = STANDARD.decode(frame_base64).unwrap();
                    let request = wire::decode_request(&bytes).unwrap();
                    assert!(matches!(request.body, RequestBody::Cancel));
                    let terminal = (mode == "confirm"
                        && started.elapsed() >= Duration::from_millis(2350))
                    .then_some(Terminal::Cancelled);
                    let response = wire::Response {
                        binding: request.binding,
                        sequence: 0,
                        request_sha256: wire::request_digest(&bytes),
                        body: ResponseBody::Exchange {
                            stdin_accepted: 0,
                            stdin_delivered: 0,
                            stdin_closed: true,
                            stdout: None,
                            stderr: None,
                            stdout_eof: terminal.map(|_| 0),
                            stderr_eof: terminal.map(|_| 0),
                            cancel_requested: true,
                            terminal,
                        },
                    };
                    let response = ipc::Response::stream(StreamLocalResult::Frame {
                        frame_base64: STANDARD.encode(wire::encode_response(&response).unwrap()),
                    });
                    let bytes = serde_json::to_vec(&response).unwrap();
                    socket.write_u32(bytes.len() as u32).await.unwrap();
                    socket.write_all(&bytes).await.unwrap();
                }
            });
            let child = tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "stream_cli::tests::cancellation_confirmation_handles_late_signals_without_extending_deadline", "--nocapture"])
                .env(CHILD, &path).env(MODE, mode)
                .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped()).kill_on_drop(true).spawn().unwrap();
            let output = tokio::time::timeout(Duration::from_secs(6), child.wait_with_output())
                .await
                .expect("confirmation child timed out")
                .unwrap();
            server.abort();
            let _ = server.await;
            assert!(output.status.success(), "mode={mode} child={output:?}");
        }
    }

    #[tokio::test]
    async fn rsync_bursts_and_forwarded_usr1_do_not_abandon_confirmation() {
        let mut signals = ExecutionSignals::register(SignalPolicy::Rsync).unwrap();
        let first = Instant::now();
        for kind in [
            ExecutionSignal::Interrupt,
            ExecutionSignal::Terminate,
            ExecutionSignal::Hangup,
            ExecutionSignal::User,
        ] {
            signals.last = Some(kind);
            assert!(signals.coalesces(first, first + Duration::from_millis(1999)));
            assert_eq!(
                signals.coalesces(first, first + Duration::from_secs(2)),
                kind == ExecutionSignal::User
            );
            assert_eq!(
                signals.coalesces(first, first + Duration::from_secs(59)),
                kind == ExecutionSignal::User
            );
        }
        signals.policy = SignalPolicy::Interactive;
        signals.last = Some(ExecutionSignal::Interrupt);
        assert!(!signals.coalesces(first, first));
    }

    #[tokio::test]
    async fn interrupt_during_preparation_restores_stdio_without_open() {
        const CHILD_SOCKET: &str = "MARMISH_TEST_PREPARE_INTERRUPT_SOCKET";
        const STAGE: &str = "MARMISH_TEST_PREPARE_INTERRUPT_STAGE";
        if let Some(socket) = std::env::var_os(CHILD_SOCKET) {
            let flags: Vec<_> = (0..=2)
                .map(|fd| unsafe { libc::fcntl(fd, libc::F_GETFL) })
                .collect();
            let result = execute(
                Context {
                    socket: socket.into(),
                    profile: "test".into(),
                },
                [4; 32],
                Vec::new(),
                vec![OsString::from("true")],
                ExecuteOptions {
                    compress: false,
                    new_session: false,
                    idle: Duration::from_secs(3),
                    keepalive: Duration::from_secs(1),
                    signals: ExecutionSignals::register(SignalPolicy::Interactive).unwrap(),
                },
            )
            .await;
            assert!(matches!(
                result,
                Err(Error::Invalid("stream_interrupted_before_open"))
            ));
            for (fd, original) in (0..=2).zip(flags) {
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFL) }, original);
            }
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        for stage in ["prepare", "hello"] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let path = directory.path().join("owner.sock");
            let listener = ipc::bind(&path).unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "stream_cli::tests::interrupt_during_preparation_restores_stdio_without_open",
                    "--nocapture",
                ])
                .env(CHILD_SOCKET, &path)
                .env(STAGE, stage)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let observed = tokio::time::timeout(Duration::from_secs(10), async {
                let (mut connection, _) = listener.accept().await?;
                let length = connection.read_u32().await? as usize;
                assert!(length <= 65_536);
                let mut bytes = vec![0; length];
                connection.read_exact(&mut bytes).await?;
                let request: ipc::Request = serde_json::from_slice(&bytes).unwrap();
                assert!(matches!(request, ipc::Request::CommandPrepare { .. }));
                if stage == "hello" {
                    let response =
                        ipc::Response::command(crate::command_local::LocalResult::Ready {
                            session: hex::encode([2; 16]),
                            local_account: hex::encode([3; 32]),
                        });
                    let bytes = serde_json::to_vec(&response).unwrap();
                    connection.write_u32(bytes.len() as u32).await?;
                    connection.write_all(&bytes).await?;
                    drop(connection);
                    let (mut hello, _) = listener.accept().await?;
                    let length = hello.read_u32().await? as usize;
                    assert!(length <= 65_536);
                    let mut bytes = vec![0; length];
                    hello.read_exact(&mut bytes).await?;
                    let ipc::Request::Stream {
                        request: StreamLocalRequest::Frame { frame_base64, .. },
                    } = serde_json::from_slice(&bytes).unwrap()
                    else {
                        panic!("expected Hello")
                    };
                    let request =
                        wire::decode_request(&STANDARD.decode(frame_base64).unwrap()).unwrap();
                    assert!(matches!(request.body, RequestBody::Hello { .. }));
                    return std::io::Result::Ok(hello);
                }
                std::io::Result::Ok(connection)
            })
            .await;
            let connection = match observed {
                Ok(Ok(connection)) => connection,
                other => {
                    let _ = child.kill();
                    let output = child.wait_with_output().unwrap();
                    panic!("prepare not observed: {other:?}, child={output:?}");
                }
            };
            // Signal only the still-owned live child, never a persisted PID.
            assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    let output = child.wait_with_output().unwrap();
                    assert!(status.success(), "child={output:?}");
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    assert!(stderr.contains("no remote child was launched"), "{stderr}");
                    assert!(
                        !stderr.contains("remote execution may continue"),
                        "{stderr}"
                    );
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let output = child.wait_with_output().unwrap();
                    panic!("interrupt did not stop preparation: {output:?}");
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            drop(connection);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn cancellation_uses_control_lane_while_exchange_is_pending() {
        let directory = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.path().join("owner.sock");
        let listener = ipc::bind(&socket).unwrap();
        let (seen, mut observations) = tokio::sync::mpsc::channel(16);
        let server = tokio::spawn(async move {
            let mut cancellations = 0;
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let size = socket.read_u32().await.unwrap() as usize;
                assert!(size <= 65_536);
                let mut bytes = vec![0; size];
                socket.read_exact(&mut bytes).await.unwrap();
                let ipc::Request::Stream {
                    request: StreamLocalRequest::Frame { frame_base64, .. },
                } = serde_json::from_slice(&bytes).unwrap()
                else {
                    panic!("stream request");
                };
                let bytes = STANDARD.decode(frame_base64).unwrap();
                let request = wire::decode_request(&bytes).unwrap();
                let result = match request.body {
                    RequestBody::Exchange { .. } => {
                        assert_eq!(request.sequence, 1);
                        seen.send(false).await.unwrap();
                        StreamLocalResult::Pending
                    }
                    RequestBody::Cancel => {
                        assert_eq!(request.sequence, 0);
                        seen.send(true).await.unwrap();
                        cancellations += 1;
                        let response = wire::Response {
                            binding: request.binding,
                            sequence: 0,
                            request_sha256: wire::request_digest(&bytes),
                            body: ResponseBody::Exchange {
                                stdin_accepted: 0,
                                stdin_delivered: 0,
                                stdin_closed: true,
                                stdout: None,
                                stderr: None,
                                stdout_eof: Some(0),
                                stderr_eof: Some(0),
                                cancel_requested: true,
                                terminal: (cancellations >= 2).then_some(Terminal::Cancelled),
                            },
                        };
                        StreamLocalResult::Frame {
                            frame_base64: STANDARD
                                .encode(wire::encode_response(&response).unwrap()),
                        }
                    }
                    _ => panic!("unexpected operation"),
                };
                let response = serde_json::to_vec(&ipc::Response::stream(result)).unwrap();
                socket.write_u32(response.len() as u32).await.unwrap();
                socket.write_all(&response).await.unwrap();
            }
        });
        let context = Context {
            socket,
            profile: "test".into(),
        };
        let peer = Peer {
            context: &context,
            relays: Vec::new(),
            binding: Binding {
                stream: [1; 16],
                session: [2; 16],
                client: [3; 32],
                server: [4; 32],
            },
            idle: Duration::from_secs(3),
        };
        let exchange = peer.call(
            1,
            RequestBody::Exchange {
                stdin: None,
                stdin_eof: None,
                stdout_ack: 0,
                stderr_ack: 0,
            },
        );
        tokio::pin!(exchange);
        tokio::select! { result=&mut exchange => panic!("data unexpectedly completed: {}",result.is_ok()), observed=observations.recv()=>assert_eq!(observed,Some(false)) }
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), peer.cancel())
                .await
                .unwrap()
                .unwrap(),
            130
        );
        assert_eq!(observations.recv().await, Some(true));
        assert_eq!(observations.recv().await, Some(true));
        server.abort();
        let _ = server.await;
    }
    #[test]
    fn descriptor_flags_restore_after_success_and_partial_failure() {
        use std::os::fd::AsRawFd;
        let file = tempfile::tempfile().unwrap();
        let fd = file.as_raw_fd();
        let original = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        assert!(original >= 0);
        {
            let _guard = Nonblocking::descriptors(&[fd]).unwrap();
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_NONBLOCK,
                0
            );
        }
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFL) }, original);
        let alias = file.try_clone().unwrap();
        {
            let _guard = Nonblocking::descriptors(&[fd, alias.as_raw_fd()]).unwrap();
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_NONBLOCK,
                0
            );
        }
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFL) }, original);
        assert_eq!(
            unsafe { libc::fcntl(alias.as_raw_fd(), libc::F_GETFL) },
            original
        );
        assert!(Nonblocking::descriptors(&[fd, -1]).is_err());
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFL) }, original);
    }
    #[tokio::test]
    async fn early_stdin_close_preserves_truthful_cursors_and_natural_status() {
        let mut progress = Progress {
            stdin: 4096,
            accepted: 1024,
            delivered: 512,
            stdout: 0,
            stderr: 0,
            eof: false,
            stdin_closed: false,
        };
        let response = ResponseBody::Exchange {
            stdin_accepted: 1024,
            stdin_delivered: 512,
            stdin_closed: true,
            stdout: None,
            stderr: None,
            stdout_eof: Some(0),
            stderr_eof: Some(0),
            cancel_requested: false,
            terminal: Some(Terminal::Exited { code: 7 }),
        };
        assert_eq!(
            progress
                .receive(response.clone(), Codec::None)
                .await
                .unwrap(),
            (true, Some(7))
        );
        assert_eq!(
            (progress.stdin, progress.accepted, progress.delivered),
            (1024, 1024, 512)
        );
        assert!(progress.eof && progress.stdin_closed);
        let mut regression = response.clone();
        if let ResponseBody::Exchange {
            stdin_delivered, ..
        } = &mut regression
        {
            *stdin_delivered = 511;
        }
        assert!(progress.receive(regression, Codec::None).await.is_err());
    }
    #[test]
    fn ids_are_closed() {
        assert!(parse_id::<16>(&"00".repeat(16)).is_ok());
        for text in ["AA".repeat(16), "0".repeat(31), "/".repeat(32)] {
            assert!(parse_id::<16>(&text).is_err());
        }
    }
    #[test]
    fn uncertain_execution_never_confirms_cancellation() {
        assert!(
            cancellation_result(Terminal::Failed {
                code: wire::WireErrorCode::Uncertain
            })
            .is_err()
        );
        assert_eq!(
            cancellation_result(Terminal::Exited { code: 7 }).unwrap(),
            7
        );
        assert_eq!(cancellation_result(Terminal::Cancelled).unwrap(), 130);
    }
    #[test]
    fn terminal_status_mapping() {
        assert_eq!(exit_code(Terminal::Cancelled), 130);
        assert_eq!(exit_code(Terminal::Signaled { signal: 9 }), 137);
        assert_eq!(exit_code(Terminal::Exited { code: 42 }), 42);
    }
}
