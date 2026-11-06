//! Owned, off-actor live pipes. Network detachment never invokes cancellation.
use crate::{Error, Result};
use marmish_engine::{ExitOutcome, LiveChild, LiveProcessRecord, LiveSpawnError, PreparedLiveExec};
use marmish_stream::{
    MAX_BLOCK,
    windows::{ReceiveWindow, SendWindow},
    wire::{Terminal, WireErrorCode},
};
use std::{
    fs::File,
    io::{Read, Write},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) struct RunnerLimits {
    pub drain_grace: Duration,
    pub cancel_grace: Duration,
    pub poll_interval: Duration,
}
impl Default for RunnerLimits {
    fn default() -> Self {
        Self {
            drain_grace: Duration::from_secs(2),
            cancel_grace: Duration::from_secs(1),
            poll_interval: Duration::from_millis(10),
        }
    }
}
#[derive(Clone)]
pub(crate) struct RunnerBlock {
    pub offset: u64,
    pub data: Vec<u8>,
}
#[derive(Clone)]
pub(crate) struct RunnerSnapshot {
    pub identity: Option<LiveProcessRecord>,
    pub stdin_accepted: u64,
    pub stdin_delivered: u64,
    pub stdin_closed: bool,
    pub stdout: Option<RunnerBlock>,
    pub stderr: Option<RunnerBlock>,
    pub stdout_eof: Option<u64>,
    pub stderr_eof: Option<u64>,
    pub cancel_requested: bool,
    pub terminal: Option<Terminal>,
    pub uncertain: bool,
}
#[derive(Default)]
struct State {
    identity: Option<LiveProcessRecord>,
    stdin: ReceiveWindow,
    stdin_closed: bool,
    stdout: SendWindow,
    stderr: SendWindow,
    terminal: Option<Terminal>,
    uncertain: bool,
}
struct Shared {
    state: Mutex<State>,
    cancel: AtomicBool,
    input_permitted: AtomicBool,
    input_suspended: AtomicBool,
    finished: AtomicBool,
}
impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn snapshot(&self, state: &State) -> RunnerSnapshot {
        fn block(window: &SendWindow) -> Option<RunnerBlock> {
            let data = window.read(window.acknowledged(), MAX_BLOCK).ok()?;
            (!data.is_empty()).then(|| RunnerBlock {
                offset: window.acknowledged(),
                data,
            })
        }
        let cancel_requested = self.cancel.load(Ordering::Acquire);
        RunnerSnapshot {
            identity: state.identity.clone(),
            stdin_accepted: state.stdin.accepted(),
            stdin_delivered: state.stdin.delivered(),
            stdin_closed: state.stdin_closed || state.terminal.is_some() || cancel_requested,
            stdout: block(&state.stdout),
            stderr: block(&state.stderr),
            stdout_eof: state.stdout.eof(),
            stderr_eof: state.stderr.eof(),
            cancel_requested,
            terminal: state.terminal,
            uncertain: state.uncertain,
        }
    }
    fn failed(&self, uncertain: bool) {
        let mut state = self.state();
        let stdout_end = state.stdout.produced();
        let stderr_end = state.stderr.produced();
        let _ = state.stdout.finish(stdout_end);
        let _ = state.stderr.finish(stderr_end);
        state.uncertain = uncertain;
        state.terminal = Some(Terminal::Failed {
            code: if uncertain {
                WireErrorCode::Uncertain
            } else {
                WireErrorCode::Invalid
            },
        });
    }
}
struct Control {
    shared: Arc<Shared>,
    wake: thread::Thread,
}
impl Drop for Control {
    fn drop(&mut self) {
        // Only release of the service's LAST handle is administrative shutdown.
        // Dropping a network attachment must leave its service-owned handle alive.
        self.shared.cancel.store(true, Ordering::Release);
        self.wake.unpark();
    }
}
#[derive(Clone)]
pub(crate) struct RunnerHandle {
    control: Arc<Control>,
}
impl RunnerHandle {
    pub fn start(prepared: PreparedLiveExec, limits: RunnerLimits) -> Result<Self> {
        if limits.poll_interval.is_zero()
            || limits.poll_interval > Duration::from_millis(100)
            || limits.drain_grace > Duration::from_secs(60)
            || limits.cancel_grace > Duration::from_secs(60)
        {
            return Err(Error::Invalid("stream_runner_limits"));
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cancel: AtomicBool::new(false),
            input_permitted: AtomicBool::new(false),
            input_suspended: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        let owned = shared.clone();
        let worker = thread::Builder::new()
            .name("marmish-stream".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(prepared, &owned, limits)
                }));
                if result.is_err() {
                    owned.failed(true);
                }
                // run() has returned or fully unwound: its child owner and every
                // pipe have been dropped before the service can release a lease.
                owned.finished.store(true, Ordering::Release);
            })?;
        let wake = worker.thread().clone();
        // The service owns the handle and the thread owns the child. Joining is not
        // an actor operation; completion is observed through bounded snapshots.
        drop(worker);
        Ok(Self {
            control: Arc::new(Control { shared, wake }),
        })
    }
    /// True only after the worker has released all child and pipe ownership.
    pub fn finished(&self) -> bool {
        self.control.shared.finished.load(Ordering::Acquire)
    }
    /// The owner calls this only AFTER the diagnostic child identity is committed.
    pub fn permit_input(&self) {
        if !self.control.shared.input_suspended.load(Ordering::Acquire) {
            self.control
                .shared
                .input_permitted
                .store(true, Ordering::Release);
        }
        self.control.wake.unpark();
    }
    /// Irreversibly suspend delivery after authority withdrawal, without closing
    /// stdin or cancelling the process. An already in-flight nonblocking write
    /// may complete; its actual delivery is still reflected in the cursor.
    pub fn suspend_input(&self) {
        self.control
            .shared
            .input_suspended
            .store(true, Ordering::Release);
        self.control
            .shared
            .input_permitted
            .store(false, Ordering::Release);
        self.control.wake.unpark();
    }
    pub fn cancel(&self) {
        self.control.shared.cancel.store(true, Ordering::Release);
        self.control.wake.unpark();
    }
    pub fn snapshot(&self) -> RunnerSnapshot {
        let state = self.control.shared.state();
        self.control.shared.snapshot(&state)
    }
    pub fn exchange(
        &self,
        input: Option<(u64, &[u8])>,
        stdin_eof: Option<u64>,
        stdout_ack: u64,
        stderr_ack: u64,
    ) -> marmish_stream::Result<RunnerSnapshot> {
        let shared = &self.control.shared;
        let mut state = shared.state();
        // Validate every independent cursor before changing any window.
        if stdout_ack < state.stdout.acknowledged()
            || stdout_ack > state.stdout.produced()
            || stderr_ack < state.stderr.acknowledged()
            || stderr_ack > state.stderr.produced()
        {
            return Err(marmish_stream::Error::Ack);
        }
        // A closed pipe forbids new bytes, but retained exact receipts remain
        // valid retries. accept() checks their length/hash without mutating data.
        if input.is_some_and(|(offset, _)| offset >= state.stdin.accepted())
            && (state.stdin_closed
                || state.terminal.is_some()
                || shared.cancel.load(Ordering::Acquire))
        {
            return Err(marmish_stream::Error::Eof);
        }
        let end = if let Some((offset, bytes)) = input {
            offset
                .checked_add(bytes.len() as u64)
                .ok_or(marmish_stream::Error::Offset)?
                .max(state.stdin.accepted())
        } else {
            state.stdin.accepted()
        };
        if stdin_eof
            .is_some_and(|eof| eof != end || state.stdin.eof().is_some_and(|old| old != eof))
        {
            return Err(marmish_stream::Error::Eof);
        }
        if let Some((offset, bytes)) = input {
            state.stdin.accept(offset, bytes)?;
        }
        if let Some(eof) = stdin_eof {
            state.stdin.finish(eof)?;
        }
        state.stdout.acknowledge(stdout_ack)?;
        state.stderr.acknowledge(stderr_ack)?;
        let snapshot = shared.snapshot(&state);
        drop(state);
        self.control.wake.unpark();
        Ok(snapshot)
    }
}

fn run(prepared: PreparedLiveExec, shared: &Shared, limits: RunnerLimits) {
    if shared.cancel.load(Ordering::Acquire) {
        let mut state = shared.state();
        let _ = state.stdout.finish(0);
        let _ = state.stderr.finish(0);
        state.terminal = Some(Terminal::Cancelled);
        return;
    }
    let mut child = match prepared.spawn() {
        Ok(child) => child,
        Err(error) => {
            shared.failed(matches!(error, LiveSpawnError::Uncertain));
            return;
        }
    };
    shared.state().identity = Some(child.identity());
    let pipes = match child.take_pipes() {
        Ok(pipes) => pipes,
        Err(_) => {
            shared.failed(true);
            return;
        }
    };
    let mut stdin = Some(pipes.stdin);
    let mut stdout = Some(pipes.stdout);
    let mut stderr = Some(pipes.stderr);
    let mut lifecycle = Lifecycle::default();
    loop {
        // Check the cancellation latch on EVERY iteration, including immediately
        // after spawn, before journal permission can admit a single input byte.
        if !lifecycle.step(&mut child, shared, limits) {
            return;
        }
        if lifecycle.cancel_started.is_some() || lifecycle.exit_seen.is_some() {
            stdin.take();
        }
        let input_progress = if shared.input_permitted.load(Ordering::Acquire)
            && !shared.input_suspended.load(Ordering::Acquire)
            && lifecycle.cancel_started.is_none()
        {
            match pump_stdin(&mut stdin, shared) {
                Ok(progress) => progress,
                Err(_) => {
                    shared.failed(true);
                    return;
                }
            }
        } else {
            false
        };
        // One bounded read from EACH pipe per turn; a noisy channel cannot starve
        // its sibling or the independent cancellation control lane.
        let progress = match (
            pump_output(&mut stdout, shared, true),
            pump_output(&mut stderr, shared, false),
        ) {
            (Ok(stdout), Ok(stderr)) => (stdout, stderr),
            _ => {
                shared.failed(true);
                return;
            }
        };
        if lifecycle.finish(
            &mut child,
            shared,
            &mut stdout,
            &mut stderr,
            progress,
            limits,
        ) {
            return;
        }
        if !input_progress && !progress.0 && !progress.1 {
            thread::park_timeout(limits.poll_interval);
        }
    }
}
#[derive(Default)]
struct Lifecycle {
    exit_seen: Option<Instant>,
    cancel_started: Option<Instant>,
    cancelled_while_running: bool,
    killed: Option<Instant>,
}
impl Lifecycle {
    fn step(&mut self, child: &mut LiveChild, shared: &Shared, limits: RunnerLimits) -> bool {
        let now = Instant::now();
        let exited = match child.try_exit() {
            Ok(exited) => exited,
            Err(_) => {
                shared.failed(true);
                return false;
            }
        };
        if exited && self.exit_seen.is_none() {
            self.exit_seen = Some(now);
            // Leader exit fences input even while output remains backpressured
            // and terminal cannot be exposed until owned cleanup/reaping.
            shared.state().stdin_closed = true;
        }
        let cancellation = shared.cancel.load(Ordering::Acquire);
        // A late user request must not relabel an already-observed natural exit.
        // Drain cleanup and cancellation share signals, but not terminal meaning.
        if cancellation && !exited {
            self.cancelled_while_running = true;
        }
        let drain_expired = self
            .exit_seen
            .is_some_and(|at| now.duration_since(at) >= limits.drain_grace);
        if self.cancel_started.is_none() && (cancellation || drain_expired) {
            if child.signal_term().is_err() {
                shared.failed(true);
                return false;
            }
            self.cancel_started = Some(now);
        }
        if self.killed.is_none()
            && self
                .cancel_started
                .is_some_and(|at| now.duration_since(at) >= limits.cancel_grace)
        {
            if child.signal_kill().is_err() {
                shared.failed(true);
                return false;
            }
            self.killed = Some(now);
        }
        true
    }
    fn finish(
        &mut self,
        child: &mut LiveChild,
        shared: &Shared,
        stdout: &mut Option<File>,
        stderr: &mut Option<File>,
        progress: (bool, bool),
        limits: RunnerLimits,
    ) -> bool {
        let now = Instant::now();
        if self.exit_seen.is_none() {
            if self
                .killed
                .is_some_and(|at| now.duration_since(at) >= limits.drain_grace)
            {
                shared.failed(true);
                return true;
            }
            return false;
        }
        let cancellation = self.cancelled_while_running;
        let abandon_output = shared.cancel.load(Ordering::Acquire);
        if abandon_output
            && self
                .killed
                .is_some_and(|at| now.duration_since(at) >= limits.drain_grace)
        {
            // Explicit cancellation, including administrative stop, abandons
            // unread pipe output after owned group cleanup even when the leader
            // already exited. Its actual exit status remains authoritative.
            finish_output(stdout, shared, true);
            finish_output(stderr, shared, false);
        }
        if !abandon_output
            && self
                .killed
                .is_some_and(|at| now.duration_since(at) >= limits.drain_grace)
        {
            // If cleanup cannot produce EOF despite available output credit, an
            // escaped descendant may still hold a pipe. Do not claim full output
            // or normal completion. Full windows remain ordinary backpressure.
            let state = shared.state();
            let stalled_stdout = stdout.is_some() && state.stdout.credit() > 0 && !progress.0;
            let stalled_stderr = stderr.is_some() && state.stderr.credit() > 0 && !progress.1;
            drop(state);
            if stalled_stdout || stalled_stderr {
                finish_output(stdout, shared, true);
                finish_output(stderr, shared, false);
                let _ = child.reap_last();
                shared.failed(true);
                return true;
            }
        }
        if stdout.is_some() || stderr.is_some() {
            return false;
        }
        if self.killed.is_none() {
            if child.signal_kill().is_err() {
                shared.failed(true);
                return true;
            }
            self.killed = Some(now);
        }
        let outcome = match child.reap_last() {
            Ok(outcome) => outcome,
            Err(_) => {
                shared.failed(true);
                return true;
            }
        };
        let terminal = if cancellation && self.cancel_started.is_some() {
            Terminal::Cancelled
        } else {
            match outcome {
                ExitOutcome::Code(code) => Terminal::Exited {
                    code: code.clamp(0, 255) as u8,
                },
                ExitOutcome::Signal(signal) => Terminal::Signaled {
                    signal: signal.clamp(1, 255) as u8,
                },
                _ => Terminal::Failed {
                    code: WireErrorCode::Uncertain,
                },
            }
        };
        let mut state = shared.state();
        state.uncertain = matches!(terminal, Terminal::Failed { .. });
        state.terminal = Some(terminal);
        true
    }
}
fn pump_stdin(pipe: &mut Option<File>, shared: &Shared) -> std::io::Result<bool> {
    let Some(file) = pipe.as_mut() else {
        return Ok(false);
    };
    let mut progressed = false;
    let pending = shared
        .state()
        .stdin
        .pending(MAX_BLOCK)
        .map_err(|_| std::io::Error::other("stream_window"))?;
    if !pending.is_empty() {
        if shared.input_suspended.load(Ordering::Acquire) || shared.cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        match file.write(&pending) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(count) => {
                progressed = true;
                let mut state = shared.state();
                let delivered = state.stdin.delivered() + count as u64;
                state
                    .stdin
                    .delivered_through(delivered)
                    .map_err(|_| std::io::Error::other("stream_window"))?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                pipe.take();
                // The peer closed its read end. Keep accepted/delivered truthful;
                // output and natural process completion remain independently live.
                shared.state().stdin_closed = true;
                return Ok(true);
            }
            Err(error) => return Err(error),
        }
    }
    if shared.state().stdin.drained() {
        pipe.take();
        shared.state().stdin_closed = true;
        progressed = true;
    }
    Ok(progressed)
}
fn pump_output(pipe: &mut Option<File>, shared: &Shared, is_stdout: bool) -> std::io::Result<bool> {
    let Some(file) = pipe.as_mut() else {
        return Ok(false);
    };
    let credit = {
        let state = shared.state();
        if is_stdout {
            state.stdout.credit()
        } else {
            state.stderr.credit()
        }
    };
    if credit == 0 {
        return Ok(false);
    }
    let mut bytes = [0; MAX_BLOCK];
    match file.read(&mut bytes[..credit.min(MAX_BLOCK)]) {
        Ok(0) => {
            finish_output(pipe, shared, is_stdout);
            return Ok(true);
        }
        Ok(count) => {
            let mut state = shared.state();
            let window = if is_stdout {
                &mut state.stdout
            } else {
                &mut state.stderr
            };
            window
                .push(&bytes[..count])
                .map_err(|_| std::io::Error::other("stream_window"))?;
            return Ok(true);
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) => {}
        Err(error) => return Err(error),
    }
    Ok(false)
}
fn finish_output(pipe: &mut Option<File>, shared: &Shared, is_stdout: bool) {
    pipe.take();
    let mut state = shared.state();
    let window = if is_stdout {
        &mut state.stdout
    } else {
        &mut state.stderr
    };
    let _ = window.finish(window.produced());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use marmish_engine::{EndpointId, Engine, EngineLimits, EnginePolicy, PrincipalId, ServerId};
    use std::os::unix::ffi::OsStrExt;

    fn limits() -> RunnerLimits {
        RunnerLimits {
            drain_grace: Duration::from_millis(80),
            cancel_grace: Duration::from_millis(40),
            poll_interval: Duration::from_millis(2),
        }
    }
    pub(crate) fn prepare(script: &str) -> (tempfile::TempDir, PreparedLiveExec) {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("worker");
        std::fs::copy(std::env::current_exe().unwrap(), &worker).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        let engine = Engine::open_with_master(
            EnginePolicy {
                state_root: directory.path().join("engine"),
                worker_executable: worker,
                server: ServerId([1; 32]),
                allowed_principals: vec![PrincipalId([2; 32])],
                allow_shell_command: false,
                command_policy: None,
                cwd: Some(directory.path().to_owned()),
                limits: EngineLimits::default(),
            },
            marmish_engine::NativeBinding::new([8; 32], [9; 32]),
            marmish_engine::ProvidedMaster::new([10; 32]),
        )
        .unwrap();
        let context = engine
            .authorize(
                PrincipalId([2; 32]),
                EndpointId([3; 32]),
                ServerId([1; 32]),
                unsafe { libc::geteuid() },
            )
            .unwrap();
        let prepared = engine
            .prepare_live_exec(
                &context,
                vec![
                    b"/bin/sh".to_vec(),
                    b"-c".to_vec(),
                    script.as_bytes().to_vec(),
                ],
                Some(std::ffi::OsStr::from_bytes(b"/usr/bin:/bin")),
            )
            .unwrap();
        (directory, prepared)
    }
    fn wait_for(
        handle: &RunnerHandle,
        predicate: impl Fn(&RunnerSnapshot) -> bool,
    ) -> RunnerSnapshot {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let snapshot = handle.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "runner condition timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn drain(handle: &RunnerHandle) -> (Vec<u8>, Vec<u8>, RunnerSnapshot) {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut out_ack = 0;
        let mut err_ack = 0;
        loop {
            let snapshot = handle.exchange(None, None, out_ack, err_ack).unwrap();
            if let Some(block) = &snapshot.stdout {
                assert_eq!(block.offset, out_ack);
                stdout.extend_from_slice(&block.data);
                out_ack += block.data.len() as u64;
            }
            if let Some(block) = &snapshot.stderr {
                assert_eq!(block.offset, err_ack);
                stderr.extend_from_slice(&block.data);
                err_ack += block.data.len() as u64;
            }
            if snapshot.terminal.is_some()
                && snapshot.stdout_eof == Some(out_ack)
                && snapshot.stderr_eof == Some(err_ack)
            {
                handle.exchange(None, None, out_ack, err_ack).unwrap();
                return (stdout, stderr, snapshot);
            }
            assert!(Instant::now() < deadline, "runner drain timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }
    /// Repeatable, offset-addressable pseudo-random bytes with guaranteed NULs.
    /// Allocates only one negotiated block, even for the large qualification.
    fn qualification_block(offset: u64, length: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(length);
        while bytes.len() < length {
            let position = offset + bytes.len() as u64;
            let mut word = (position / 8).wrapping_add(0x9e3779b97f4a7c15);
            word = (word ^ (word >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            word = (word ^ (word >> 27)).wrapping_mul(0x94d049bb133111eb);
            word ^= word >> 31;
            let encoded = word.to_le_bytes();
            let start = (position % 8) as usize;
            let count = (8 - start).min(length - bytes.len());
            for (index, byte) in encoded[start..start + count].iter().enumerate() {
                bytes.push(if (position + index as u64).is_multiple_of(257) {
                    0
                } else {
                    *byte
                });
            }
        }
        bytes
    }

    #[test]
    #[ignore = "large 65 MiB duplex qualification; run explicitly"]
    fn large_duplex_integrity_with_slow_consumers_and_bounded_windows() {
        use sha2::{Digest, Sha256};
        const TOTAL: u64 = 65 * 1024 * 1024 + 17;
        let (_dir, prepared) = prepare("exec tee /dev/stderr");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        handle.permit_input();
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut sent = 0;
        let mut stdout_ack = 0;
        let mut stderr_ack = 0;
        let mut source_hash = Sha256::new();
        let mut stdout_hash = Sha256::new();
        let mut stderr_hash = Sha256::new();
        let mut withheld = true;
        let mut turns = 0_u64;
        loop {
            assert!(
                Instant::now() < deadline,
                "large duplex qualification timed out"
            );
            let credit = {
                let state = handle.control.shared.state();
                for buffered in [
                    state.stdin.buffered(),
                    state.stdout.buffered(),
                    state.stderr.buffered(),
                ] {
                    assert!(buffered <= marmish_stream::WINDOW_BYTES);
                }
                if state.stdout.credit() == 0 && state.stderr.credit() == 0 {
                    withheld = false;
                }
                state.stdin.credit()
            };
            let length = credit.min(MAX_BLOCK).min((TOTAL - sent) as usize);
            let input = (length > 0).then(|| qualification_block(sent, length));
            let eof = (sent + length as u64 == TOTAL).then_some(TOTAL);
            let snapshot = handle
                .exchange(
                    input.as_deref().map(|bytes| (sent, bytes)),
                    eof,
                    stdout_ack,
                    stderr_ack,
                )
                .unwrap();
            if let Some(bytes) = input {
                source_hash.update(&bytes);
                sent += bytes.len() as u64;
            }
            assert!(snapshot.stdin_delivered <= snapshot.stdin_accepted);
            assert_eq!(snapshot.stdin_accepted, sent);
            if !withheld {
                if let Some(block) = snapshot.stdout {
                    assert_eq!(block.offset, stdout_ack);
                    assert_eq!(
                        block.data,
                        qualification_block(stdout_ack, block.data.len())
                    );
                    stdout_hash.update(&block.data);
                    stdout_ack += block.data.len() as u64;
                }
                if let Some(block) = snapshot.stderr {
                    assert_eq!(block.offset, stderr_ack);
                    assert_eq!(
                        block.data,
                        qualification_block(stderr_ack, block.data.len())
                    );
                    stderr_hash.update(&block.data);
                    stderr_ack += block.data.len() as u64;
                }
            }
            if snapshot.terminal.is_some()
                && snapshot.stdout_eof == Some(stdout_ack)
                && snapshot.stderr_eof == Some(stderr_ack)
            {
                assert!(!withheld, "qualification never filled both output windows");
                assert_eq!(sent, TOTAL);
                assert_eq!(snapshot.stdin_delivered, TOTAL);
                assert_eq!(stdout_ack, TOTAL);
                assert_eq!(stderr_ack, TOTAL);
                assert_eq!(snapshot.terminal, Some(Terminal::Exited { code: 0 }));
                assert!(!snapshot.uncertain);
                handle.exchange(None, None, stdout_ack, stderr_ack).unwrap();
                break;
            }
            turns += 1;
            // Deliberately stall each consumer periodically after proving full
            // backpressure, while the producer and sibling remain independent.
            if withheld || turns.is_multiple_of(16) {
                thread::sleep(Duration::from_millis(4));
            } else {
                thread::yield_now();
            }
        }
        let expected = source_hash.finalize();
        assert_eq!(stdout_hash.finalize(), expected);
        assert_eq!(stderr_hash.finalize(), expected);
    }

    #[test]
    fn identity_commit_gates_stdin_then_preserves_binary_and_exit() {
        let (_dir, prepared) = prepare("cat; printf stderr >&2; exit 7");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |s| s.identity.is_some());
        let bytes = b"a\0b\xff\n";
        handle
            .exchange(Some((0, bytes)), Some(bytes.len() as u64), 0, 0)
            .unwrap();
        thread::sleep(Duration::from_millis(40));
        let gated = handle.snapshot();
        assert_eq!(gated.stdin_delivered, 0);
        assert!(gated.stdout.is_none());
        handle.permit_input();
        let (stdout, stderr, snapshot) = drain(&handle);
        assert_eq!(stdout, bytes);
        assert_eq!(stderr, b"stderr");
        assert_eq!(snapshot.terminal, Some(Terminal::Exited { code: 7 }));
        assert!(!snapshot.uncertain);
    }
    #[test]
    fn disconnect_preserves_backpressured_process_and_independent_cancel() {
        let (_dir, prepared) = prepare("trap '' TERM; printf marker >&2; exec yes x");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        handle.permit_input();
        wait_for(&handle, |s| s.stdout.is_some() && s.stderr.is_some());
        wait_for(&handle, |_| {
            handle.control.shared.state().stdout.credit() == 0
        });
        // A frontend clone detaches; the service owner remains alive.
        drop(handle.clone());
        thread::sleep(Duration::from_millis(100));
        assert!(!handle.snapshot().cancel_requested);
        assert!(handle.snapshot().terminal.is_none());
        let shared = &handle.control.shared;
        {
            let state = shared.state();
            assert!(state.stdout.buffered() <= marmish_stream::WINDOW_BYTES);
            assert!(state.stderr.buffered() <= marmish_stream::WINDOW_BYTES);
        }
        handle.cancel();
        let done = wait_for(&handle, |s| s.terminal.is_some());
        assert_eq!(done.terminal, Some(Terminal::Cancelled));
        assert!(!done.uncertain);
    }
    #[test]
    fn leader_exit_cleans_descendant_held_pipes_before_reaping() {
        let (_dir, prepared) = prepare("sleep 30 & printf leader; exit 0");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        handle.permit_input();
        let (stdout, _, snapshot) = drain(&handle);
        assert_eq!(stdout, b"leader");
        assert_eq!(snapshot.terminal, Some(Terminal::Exited { code: 0 }));
        assert!(!snapshot.uncertain);
    }
    #[test]
    fn cancellation_before_identity_grant_never_delivers_input() {
        let (_dir, prepared) = prepare("trap '' TERM; cat");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.identity.is_some());
        handle.exchange(Some((0, b"secret")), None, 0, 0).unwrap();
        handle.cancel();
        let snapshot = wait_for(&handle, |s| s.terminal.is_some());
        assert_eq!(snapshot.stdin_delivered, 0);
        assert_eq!(snapshot.terminal, Some(Terminal::Cancelled));
    }
    #[test]
    fn authority_withdrawal_halts_queued_input_irreversibly_without_cancel() {
        let (_dir, prepared) = prepare(
            "printf ready; sleep 1; dd bs=1 count=1 >/dev/null 2>/dev/null; printf reading; cat >/dev/null",
        );
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.stdout.is_some());
        handle.permit_input();
        let block = vec![b'x'; MAX_BLOCK];
        handle.exchange(Some((0, &block)), None, 0, 0).unwrap();
        wait_for(&handle, |snapshot| {
            snapshot.stdin_delivered == MAX_BLOCK as u64
        });
        for index in 1..=32 {
            handle
                .exchange(Some((index * MAX_BLOCK as u64, &block)), None, 0, 0)
                .unwrap();
        }
        handle.suspend_input();
        // Seeing the child's marker proves that any write already in flight
        // before suspension has completed on this same worker thread, and that
        // the child has reached its stdin-reading phase.
        wait_for(&handle, |snapshot| {
            snapshot.stdout.as_ref().is_some_and(|block| {
                block
                    .data
                    .windows(b"reading".len())
                    .any(|bytes| bytes == b"reading")
            })
        });
        let halted = handle.snapshot();
        assert!(halted.stdin_accepted > halted.stdin_delivered);
        handle.permit_input(); // Revocation wins over every subsequent permit.
        thread::sleep(Duration::from_millis(30));
        let alive = handle.snapshot();
        assert_eq!(alive.stdin_delivered, halted.stdin_delivered);
        assert!(!alive.cancel_requested);
        assert!(alive.terminal.is_none());
        handle.cancel();
        let done = wait_for(&handle, |snapshot| snapshot.terminal.is_some());
        assert_eq!(done.terminal, Some(Terminal::Cancelled));
        assert!(!done.uncertain);
    }

    #[test]
    fn final_service_handle_drop_finishes_owned_cleanup() {
        let (_dir, prepared) = prepare("trap '' TERM; exec sleep 30");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.identity.is_some());
        let shared = handle.control.shared.clone();
        assert!(!handle.finished());
        drop(handle);
        let deadline = Instant::now() + Duration::from_secs(8);
        while !shared.finished.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "last-handle cleanup timed out");
            thread::sleep(Duration::from_millis(2));
        }
        let state = shared.state();
        assert_eq!(state.terminal, Some(Terminal::Cancelled));
        assert!(!state.uncertain);
    }

    #[test]
    fn cancellation_before_spawn_never_creates_child() {
        let (dir, prepared) = prepare("printf launched > marker");
        let shared = Shared {
            state: Mutex::new(State::default()),
            cancel: AtomicBool::new(true),
            input_permitted: AtomicBool::new(false),
            input_suspended: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        };
        run(prepared, &shared, limits());
        let state = shared.state();
        assert!(state.identity.is_none());
        assert_eq!(state.terminal, Some(Terminal::Cancelled));
        assert!(!dir.path().join("marker").exists());
    }

    #[test]
    fn cancellation_after_observed_leader_exit_preserves_natural_status() {
        let (_dir, prepared) = prepare("sleep 30 & exit 7");
        let mut child = prepared.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        while !child.try_exit().unwrap() {
            assert!(Instant::now() < deadline, "leader exit timed out");
            thread::sleep(Duration::from_millis(2));
        }
        // The leader is already wait-observable, but descendants still hold the
        // pipes and the leader has deliberately not been reaped.
        let shared = Shared {
            state: Mutex::new(State::default()),
            cancel: AtomicBool::new(true),
            input_permitted: AtomicBool::new(false),
            input_suspended: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        };
        let pipes = child.take_pipes().unwrap();
        drop(pipes.stdin);
        let mut stdout = Some(pipes.stdout);
        let mut stderr = Some(pipes.stderr);
        let mut lifecycle = Lifecycle::default();
        loop {
            assert!(Instant::now() < deadline, "late cancel cleanup timed out");
            assert!(lifecycle.step(&mut child, &shared, limits()));
            let progress = (
                pump_output(&mut stdout, &shared, true).unwrap(),
                pump_output(&mut stderr, &shared, false).unwrap(),
            );
            if lifecycle.finish(
                &mut child,
                &shared,
                &mut stdout,
                &mut stderr,
                progress,
                limits(),
            ) {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!lifecycle.cancelled_while_running);
        let state = shared.state();
        assert_eq!(state.terminal, Some(Terminal::Exited { code: 7 }));
        assert!(!state.uncertain);
    }

    #[test]
    fn late_cancel_releases_full_output_window_after_natural_leader_exit() {
        let script = format!(
            "dd if=/dev/zero bs=1024 count={} 2>/dev/null; printf tail; exit 7",
            marmish_stream::WINDOW_BYTES / 1024
        );
        let (_dir, prepared) = prepare(&script);
        let mut child = prepared.spawn().unwrap();
        let pipes = child.take_pipes().unwrap();
        drop(pipes.stdin);
        let mut stdout = Some(pipes.stdout);
        let mut stderr = Some(pipes.stderr);
        let shared = Shared {
            state: Mutex::new(State::default()),
            cancel: AtomicBool::new(false),
            input_permitted: AtomicBool::new(false),
            input_suspended: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        };
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(Instant::now() < deadline, "full-window child did not exit");
            pump_output(&mut stdout, &shared, true).unwrap();
            pump_output(&mut stderr, &shared, false).unwrap();
            if shared.state().stdout.credit() == 0 && child.try_exit().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        let mut lifecycle = Lifecycle::default();
        assert!(lifecycle.step(&mut child, &shared, limits()));
        assert!(!lifecycle.finish(
            &mut child,
            &shared,
            &mut stdout,
            &mut stderr,
            (false, false),
            limits()
        ));
        // Identical latch is used by remote Cancel and administrative stop.
        shared.cancel.store(true, Ordering::Release);
        loop {
            assert!(
                Instant::now() < deadline,
                "full-window cancellation did not finish"
            );
            assert!(lifecycle.step(&mut child, &shared, limits()));
            let progress = (
                pump_output(&mut stdout, &shared, true).unwrap(),
                pump_output(&mut stderr, &shared, false).unwrap(),
            );
            if lifecycle.finish(
                &mut child,
                &shared,
                &mut stdout,
                &mut stderr,
                progress,
                limits(),
            ) {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!lifecycle.cancelled_while_running);
        let state = shared.state();
        assert_eq!(state.terminal, Some(Terminal::Exited { code: 7 }));
        assert_eq!(
            state.stdout.eof(),
            Some(marmish_stream::WINDOW_BYTES as u64)
        );
        assert!(!state.uncertain);
    }

    #[test]
    fn final_handle_drop_releases_full_unacked_output_after_exit() {
        let script = format!(
            "dd if=/dev/zero bs=1024 count={} 2>/dev/null; printf tail; exit 7",
            marmish_stream::WINDOW_BYTES / 1024
        );
        let (_dir, prepared) = prepare(&script);
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |_| {
            handle.control.shared.state().stdout.credit() == 0
        });
        let shared = handle.control.shared.clone();
        drop(handle);
        let deadline = Instant::now() + Duration::from_secs(8);
        while !shared.finished.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "full-window last-handle cleanup timed out"
            );
            thread::sleep(Duration::from_millis(2));
        }
        assert!(shared.state().terminal.is_some());
        assert!(!shared.state().uncertain);
    }

    #[test]
    fn closed_stdin_retains_delivery_cursors_and_natural_exit() {
        let (_dir, prepared) = prepare("exec 0<&-; printf closed; sleep 1; exit 9");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.stdout.is_some());
        handle
            .exchange(Some((0, b"not delivered")), None, 0, 0)
            .unwrap();
        handle.permit_input();
        let closed = wait_for(&handle, |snapshot| snapshot.stdin_closed);
        assert_eq!(closed.stdin_accepted, 13);
        assert_eq!(closed.stdin_delivered, 0);
        assert!(closed.terminal.is_none());
        assert!(!closed.cancel_requested);
        let retry = handle
            .exchange(Some((0, b"not delivered")), None, 0, 0)
            .unwrap();
        assert!(retry.stdin_closed);
        assert_eq!(retry.stdin_accepted, 13);
        assert_eq!(retry.stdin_delivered, 0);
        assert!(matches!(
            handle.exchange(Some((0, b"wrong payload")), None, 0, 0),
            Err(marmish_stream::Error::Conflict)
        ));
        assert!(matches!(
            handle.exchange(Some((13, b"more")), None, 0, 0),
            Err(marmish_stream::Error::Eof)
        ));
        let (stdout, stderr, done) = drain(&handle);
        assert_eq!(stdout, b"closed");
        assert!(stderr.is_empty());
        assert!(done.stdin_closed);
        assert_eq!(done.stdin_accepted, 13);
        assert_eq!(done.stdin_delivered, 0);
        assert_eq!(done.terminal, Some(Terminal::Exited { code: 9 }));
        assert!(!done.uncertain);
        let retry = handle
            .exchange(Some((0, b"not delivered")), None, stdout.len() as u64, 0)
            .unwrap();
        assert!(retry.stdin_closed);
        assert_eq!(retry.stdin_accepted, 13);
        assert_eq!(retry.stdin_delivered, 0);
        assert_eq!(retry.terminal, Some(Terminal::Exited { code: 9 }));
    }

    #[test]
    fn natural_exit_closes_input_without_delivering_queued_bytes() {
        let (dir, prepared) =
            prepare("printf ready; while [ ! -e finish ]; do sleep 0.01; done; exit 0");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.stdout.is_some());
        let accepted = handle.exchange(Some((0, b"queued")), None, 0, 0).unwrap();
        assert!(!accepted.stdin_closed);
        std::fs::write(dir.path().join("finish"), b"exit").unwrap();
        // No identity-commit permit: the queued bytes must remain undelivered.
        let (stdout, _, done) = drain(&handle);
        assert_eq!(stdout, b"ready");
        assert!(done.stdin_closed);
        assert_eq!(done.stdin_accepted, 6);
        assert_eq!(done.stdin_delivered, 0);
        assert_eq!(done.terminal, Some(Terminal::Exited { code: 0 }));
        assert!(matches!(
            handle.exchange(Some((6, b"more")), None, 5, 0),
            Err(marmish_stream::Error::Eof)
        ));
    }

    #[test]
    fn cancellation_snapshot_closes_input_before_cleanup_finishes() {
        let (_dir, prepared) = prepare("trap '' TERM; exec sleep 30");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.identity.is_some());
        assert!(!handle.snapshot().stdin_closed);
        handle.cancel();
        let cancelled = handle.snapshot();
        assert!(cancelled.stdin_closed);
        assert!(cancelled.cancel_requested);
        wait_for(&handle, |snapshot| snapshot.terminal.is_some());
    }

    #[test]
    fn cancellation_after_terminal_preserves_actual_exit() {
        let (_dir, prepared) = prepare("exit 7");
        let handle = RunnerHandle::start(prepared, limits()).unwrap();
        wait_for(&handle, |snapshot| snapshot.terminal.is_some());
        handle.cancel();
        let snapshot = wait_for(&handle, |_| handle.finished());
        assert_eq!(snapshot.terminal, Some(Terminal::Exited { code: 7 }));
        assert!(snapshot.cancel_requested);
        assert!(!snapshot.uncertain);
    }

    #[test]
    fn failed_exchange_does_not_apply_other_channel_ack() {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cancel: AtomicBool::new(false),
            input_permitted: AtomicBool::new(false),
            input_suspended: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        shared.state().stdout.push(b"abc").unwrap();
        let handle = RunnerHandle {
            control: Arc::new(Control {
                shared: shared.clone(),
                wake: thread::current(),
            }),
        };
        assert!(handle.exchange(Some((1, b"gap")), None, 3, 0).is_err());
        assert_eq!(shared.state().stdout.acknowledged(), 0);
        assert!(handle.exchange(Some((0, b"abc")), Some(2), 3, 0).is_err());
        assert_eq!(shared.state().stdin.accepted(), 0);
        assert_eq!(shared.state().stdout.acknowledged(), 0);
    }
}
