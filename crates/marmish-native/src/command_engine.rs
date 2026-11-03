//! Serialized command engine boundary. No journal, MLS runtime, or profile lease
//! crosses this thread. Completions remain registered until the durable actor ACKs.
use crate::command_wire as wire;
use crate::journal::{
    CommitAck, CommittedDecision, EngineCompletion, EngineObservation, EngineOperation,
    FeederState, InvocationKey, InvocationRecord, OperationRecord, OperationalFailure,
    RecoveryRecord, WorkerState,
};
use crate::{Error, Result};
use marmish_engine as engine;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const CAPACITY: usize = 64;
#[cfg(test)]
const OPEN_OBSERVER: Duration = Duration::from_secs(30);
#[cfg(test)]
const OP_OBSERVER: Duration = Duration::from_secs(15);
type Key = ([u8; 32], [u8; 32], [u8; 16]);
fn key(k: &InvocationKey) -> Key {
    (k.server, k.principal, k.invocation)
}

/// Never printable: ownership of the master moves directly to the engine thread.
pub(crate) struct EngineSettings {
    pub policy: engine::EnginePolicy,
    pub binding: engine::NativeBinding,
    pub master: Zeroizing<[u8; 32]>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Readiness {
    Opening,
    Ready,
    Failed,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanupState {
    Open,
    Draining,
    Quiesced,
    Failed,
}
#[derive(Clone, Copy)]
pub(crate) struct StopReport {
    pub complete: bool,
    pub quiesced: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueBusy {
    Full,
    InvocationBusy,
    Unavailable,
}
struct Registration {
    id: [u8; 16],
    revision: u64,
    #[cfg(test)]
    started: Instant,
    completed: bool,
}
enum Control {
    Ack(CommitAck),
    Cleanup,
    FailCleanup,
    Stop,
}
enum Event {
    Ready,
    OpenFailed,
    Completion(Box<EngineCompletion>),
    Cleanup(CleanupState),
}

pub(crate) struct CommandEngine {
    operations: SyncSender<OperationRecord>,
    controls: SyncSender<Control>,
    events: Receiver<Event>,
    completions: VecDeque<EngineCompletion>,
    registered: BTreeMap<Key, Registration>,
    thread: Option<JoinHandle<()>>,
    readiness: Readiness,
    cleanup: CleanupState,
    #[cfg(test)]
    opened: Instant,
    stopping: bool,
}
impl CommandEngine {
    /// Returns after spawning, never after opening storage. The actor retains
    /// this exact handle until its thread joins; observation never permits reopening.
    pub fn start(settings: EngineSettings, recovery: Vec<RecoveryRecord>) -> Result<Self> {
        if recovery.len() > 32 {
            return Err(Error::Invalid("engine_recovery_limit"));
        }
        let (operations, rx) = mpsc::sync_channel(CAPACITY);
        let (controls, control_rx) = mpsc::sync_channel(CAPACITY);
        let (tx, events) = mpsc::sync_channel(CAPACITY);
        let thread = thread::Builder::new()
            .name("marmish-engine".into())
            .spawn(move || {
                let EngineSettings {
                    policy,
                    binding,
                    master,
                } = settings;
                let opened = engine::Engine::open_with_master(
                    policy,
                    binding,
                    engine::ProvidedMaster::new(*master),
                );
                drop(master);
                match opened {
                    Ok(engine) => {
                        if tx.send(Event::Ready).is_ok() {
                            Worker::new(engine, recovery).run(rx, control_rx, tx);
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(Event::OpenFailed);
                    }
                }
            })?;
        Ok(Self {
            operations,
            controls,
            events,
            completions: VecDeque::new(),
            registered: BTreeMap::new(),
            thread: Some(thread),
            readiness: Readiness::Opening,
            cleanup: CleanupState::Open,
            #[cfg(test)]
            opened: Instant::now(),
            stopping: false,
        })
    }
    fn collect(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Ready => self.readiness = Readiness::Ready,
                Event::OpenFailed => self.readiness = Readiness::Failed,
                Event::Completion(c) => {
                    if let Some(r) = self.registered.get_mut(&key(&c.key))
                        && r.id == c.operation.0
                        && r.revision == c.revision
                    {
                        r.completed = true;
                    }
                    self.completions.push_back(*c)
                }
                Event::Cleanup(state) => self.cleanup = state,
            }
        }
    }
    pub fn readiness(&mut self) -> Readiness {
        self.collect();
        self.readiness
    }
    #[cfg(test)]
    pub fn open_observer_expired(&self) -> bool {
        self.readiness == Readiness::Opening && self.opened.elapsed() >= OPEN_OBSERVER
    }
    pub fn observation_pending(&self) -> bool {
        !self.registered.is_empty()
    }
    #[cfg(test)]
    pub fn observer_expired(&self, k: &InvocationKey) -> bool {
        self.registered
            .get(&key(k))
            .is_some_and(|r| r.started.elapsed() >= OP_OBSERVER)
    }
    pub fn cleanup_state(&mut self) -> CleanupState {
        self.collect();
        self.cleanup
    }
    pub fn try_enqueue(&mut self, op: OperationRecord) -> std::result::Result<(), QueueBusy> {
        self.collect();
        if self.stopping
            || self.cleanup != CleanupState::Open
            || self.readiness == Readiness::Failed
        {
            return Err(QueueBusy::Unavailable);
        }
        let k = key(&op.key);
        if self.registered.contains_key(&k) {
            return Err(QueueBusy::InvocationBusy);
        }
        if self.registered.len() >= CAPACITY {
            return Err(QueueBusy::Full);
        }
        let reg = Registration {
            id: op.id.0,
            revision: op.revision,
            #[cfg(test)]
            started: Instant::now(),
            completed: false,
        };
        self.operations.try_send(op).map_err(|e| match e {
            TrySendError::Full(_) => QueueBusy::Full,
            TrySendError::Disconnected(_) => QueueBusy::Unavailable,
        })?;
        self.registered.insert(k, reg);
        Ok(())
    }
    pub fn try_completion(&mut self) -> Option<EngineCompletion> {
        self.collect();
        self.completions.pop_front()
    }
    pub fn ack_committed(&mut self, ack: CommitAck) -> Result<()> {
        let k = key(&ack.key);
        if !self
            .registered
            .get(&k)
            .is_some_and(|r| r.completed && r.id == ack.operation.0 && r.revision == ack.revision)
        {
            return Err(Error::Invalid("engine_ack_mismatch"));
        }
        self.controls
            .try_send(Control::Ack(ack))
            .map_err(|_| Error::Storage("engine_ack_pending"))?;
        self.registered.remove(&k);
        Ok(())
    }
    /// Actor has closed admission and committed every final read-only refresh.
    /// Any failure is terminal for this instance; no implicit reopen is possible.
    pub fn try_prepare_cleanup(&mut self) -> Result<()> {
        self.collect();
        if self.cleanup != CleanupState::Open
            || !self.registered.is_empty()
            || self.readiness != Readiness::Ready
        {
            return Err(Error::Invalid("engine_cleanup_busy"));
        }
        self.controls
            .try_send(Control::Cleanup)
            .map_err(|_| Error::Storage("engine_control_busy"))?;
        self.cleanup = CleanupState::Draining;
        Ok(())
    }
    pub fn fail_cleanup(&mut self) -> Result<()> {
        self.controls
            .try_send(Control::FailCleanup)
            .map_err(|_| Error::Storage("engine_control_busy"))?;
        self.cleanup = CleanupState::Failed;
        Ok(())
    }
    pub fn shutdown(&mut self, deadline: Instant) -> Result<StopReport> {
        self.stopping = true;
        // A full control queue is retried on the same owner, never detached.
        loop {
            match self.controls.try_send(Control::Stop) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(_)) if Instant::now() < deadline => {
                    self.collect();
                    thread::sleep(Duration::from_millis(1))
                }
                Err(_) => {
                    return Ok(StopReport {
                        complete: false,
                        quiesced: self.cleanup == CleanupState::Quiesced,
                    });
                }
            }
        }
        while self.thread.as_ref().is_some_and(|t| !t.is_finished()) && Instant::now() < deadline {
            self.collect();
            thread::sleep(Duration::from_millis(1));
        }
        self.collect();
        let complete = self.thread.as_ref().is_none_or(|t| t.is_finished());
        if complete && let Some(t) = self.thread.take() {
            t.join()
                .map_err(|_| Error::Storage("engine_thread_failed"))?;
        }
        Ok(StopReport {
            complete,
            quiesced: self.cleanup == CleanupState::Quiesced,
        })
    }
}

#[cfg(test)]
impl CommandEngine {
    pub(crate) fn gated_stop_for_test(gate: Receiver<()>) -> Self {
        let (operations, rx) = mpsc::sync_channel(CAPACITY);
        let (controls, control_rx) = mpsc::sync_channel(CAPACITY);
        let (tx, events) = mpsc::sync_channel(CAPACITY);
        let worker = thread::spawn(move || {
            let _ = gate.recv();
            drop((rx, control_rx, tx));
        });
        Self {
            operations,
            controls,
            events,
            completions: VecDeque::new(),
            registered: BTreeMap::new(),
            thread: Some(worker),
            readiness: Readiness::Ready,
            cleanup: CleanupState::Open,
            #[cfg(test)]
            opened: Instant::now(),
            stopping: false,
        }
    }
}

struct Awaiting {
    id: [u8; 16],
    revision: u64,
    fact: Fact,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fact {
    Found,
    Missing,
    Refused,
    Uncertain,
}
struct Worker {
    engine: Option<engine::Engine>,
    guard: Option<engine::CleanupGuard>,
    unresolved: BTreeSet<Key>,
    known: BTreeSet<Key>,
    awaiting: BTreeMap<Key, Awaiting>,
    pending: VecDeque<OperationRecord>,
    closed: bool,
}
impl Worker {
    fn new(engine: engine::Engine, recovery: Vec<RecoveryRecord>) -> Self {
        let mut unresolved = BTreeSet::new();
        let mut known = BTreeSet::new();
        for r in recovery {
            if r.engine_known {
                known.insert(key(&r.key));
            }
            unresolved.insert(key(&r.key));
        }
        Self {
            engine: Some(engine),
            guard: None,
            unresolved,
            known,
            awaiting: BTreeMap::new(),
            pending: VecDeque::new(),
            closed: false,
        }
    }
    fn can_dispatch(&self, k: &Key) -> bool {
        self.awaiting.is_empty() && self.unresolved.iter().all(|other| other == k)
    }
    fn ack(&mut self, ack: CommitAck) {
        let k = key(&ack.key);
        let Some(wait) = self.awaiting.get(&k) else {
            return;
        };
        if wait.id != ack.operation.0 || wait.revision != ack.revision {
            return;
        }
        let resolves = matches!(
            (wait.fact, ack.decision),
            (Fact::Found, CommittedDecision::KnownFound)
                | (Fact::Missing, CommittedDecision::DefinitelyUnsubmitted)
                | (Fact::Refused, CommittedDecision::DefinitivelyRefused)
        );
        let resolves = resolves
            || (wait.fact == Fact::Missing
                && ack.decision == CommittedDecision::KnownFound
                && self.known.contains(&k));
        if resolves {
            self.unresolved.remove(&k);
        }
        self.awaiting.remove(&k);
    }
    fn run(
        mut self,
        rx: Receiver<OperationRecord>,
        controls: Receiver<Control>,
        tx: SyncSender<Event>,
    ) {
        loop {
            loop {
                match controls.try_recv() {
                    Ok(Control::Stop) => return,
                    Ok(Control::Ack(ack)) => self.ack(ack),
                    Ok(Control::FailCleanup) => {
                        self.closed = true;
                        if tx.send(Event::Cleanup(CleanupState::Failed)).is_err() {
                            return;
                        }
                    }
                    Ok(Control::Cleanup) => {
                        self.closed = true;
                        let state = if self.pending.is_empty()
                            && self.awaiting.is_empty()
                            && self.unresolved.is_empty()
                        {
                            match self.engine.take().and_then(|e| e.into_cleanup().ok()) {
                                Some(guard) => {
                                    self.guard = Some(guard);
                                    CleanupState::Quiesced
                                }
                                None => CleanupState::Failed,
                            }
                        } else {
                            CleanupState::Failed
                        };
                        if tx.send(Event::Cleanup(state)).is_err() {
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            // A single bounded queue remains observable while dispatch is fenced.
            while self.pending.len() < CAPACITY {
                match rx.try_recv() {
                    Ok(op) => self.pending.push_back(op),
                    Err(_) => break,
                }
            }
            let selected = self.pending.iter().position(|op| {
                !matches!(op.operation, EngineOperation::Dispatch { .. })
                    || self.can_dispatch(&key(&op.key))
            });
            if let Some(index) = selected {
                let op = self.pending.remove(index).unwrap();
                let k = key(&op.key);
                let result = if self.closed {
                    EngineObservation::Failed(OperationalFailure::Unavailable)
                } else {
                    self.execute(&op)
                };
                let fact = match &result {
                    EngineObservation::Missing => Fact::Missing,
                    EngineObservation::Refused(_) => Fact::Refused,
                    EngineObservation::Failed(_) => Fact::Uncertain,
                    _ => Fact::Found,
                };
                if fact == Fact::Found {
                    self.known.insert(k);
                }
                // No maintenance until this exact result is durably committed.
                self.unresolved.insert(k);
                self.awaiting.insert(
                    k,
                    Awaiting {
                        id: op.id.0,
                        revision: op.revision,
                        fact,
                    },
                );
                if tx
                    .send(Event::Completion(Box::new(EngineCompletion {
                        operation: op.id,
                        key: op.key,
                        revision: op.revision,
                        result,
                    })))
                    .is_err()
                {
                    return;
                }
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
    fn execute(&mut self, op: &OperationRecord) -> EngineObservation {
        if op.engine_known {
            self.known.insert(key(&op.key));
        }
        let known = self.known.contains(&key(&op.key));
        let Some(e) = self.engine.as_mut() else {
            return EngineObservation::Failed(OperationalFailure::Unavailable);
        };
        let b = op.owner.binding();
        if !op.owner.server_side() || op.key.server != b.server || op.key.principal != b.client {
            return EngineObservation::Failed(OperationalFailure::Uncertain);
        }
        let ctx = match e.authorize(
            engine::PrincipalId(b.client),
            engine::EndpointId(Sha256::digest(op.owner.group()).into()),
            engine::ServerId(b.server),
            op.owner.uid(),
        ) {
            Ok(c) => c,
            Err(err) => return failed(err),
        };
        let id = engine::InvocationId(op.key.invocation);
        execute_operation(e, &ctx, id, op, known)
    }
}

fn failed(error: engine::EngineError) -> EngineObservation {
    use engine::EngineError as E;
    EngineObservation::Failed(match error {
        E::StorageFailure | E::UnsupportedStorage | E::UnsafePath => OperationalFailure::Storage,
        E::RecoveryUncertain | E::WorkerUnavailable => OperationalFailure::Uncertain,
        _ => OperationalFailure::Unavailable,
    })
}
fn worker(
    e: &mut engine::Engine,
    ctx: &engine::AuthorizedContext,
    id: engine::InvocationId,
    state: engine::InvocationState,
) -> WorkerState {
    match e.owned_worker_presence(ctx, id) {
        Ok(engine::OwnedWorkerPresence::Present) => WorkerState::Running,
        Ok(engine::OwnedWorkerPresence::Absent) if state == engine::InvocationState::Exited => {
            WorkerState::Exited
        }
        _ => WorkerState::Unknown,
    }
}
fn error(code: wire::ErrorCode, detail: Option<String>) -> EngineObservation {
    EngineObservation::Refused(wire::WireError { code, detail })
}
fn execute_operation(
    e: &mut engine::Engine,
    ctx: &engine::AuthorizedContext,
    id: engine::InvocationId,
    op: &OperationRecord,
    known: bool,
) -> EngineObservation {
    let initial = if matches!(op.operation, EngineOperation::FinalObserve) {
        e.terminal_status(ctx, id)
    } else {
        e.status(ctx, id)
    };
    let mut status = match initial {
        Ok(s) => s,
        Err(engine::EngineError::NotFound) => {
            if known || !matches!(op.operation, EngineOperation::Dispatch { .. }) {
                return EngineObservation::Missing;
            }
            if !op.ledger.authority_valid {
                return error(wire::ErrorCode::AuthorityRevoked, None);
            }
            let EngineOperation::Dispatch { submit } = &op.operation else {
                unreachable!()
            };
            let request = engine::CommandRequest::Exec {
                argv: submit.argv.clone(),
            };
            match engine::inspect_canonical_exec(&request) {
                engine::CanonicalExecOutcome::Allowed => {}
                engine::CanonicalExecOutcome::Unresolvable => {
                    return error(wire::ErrorCode::CommandDenied, None);
                }
                engine::CanonicalExecOutcome::Noncanonical { canonical } => {
                    let detail = String::from_utf8(canonical)
                        .ok()
                        .filter(|s| serde_json::to_vec(s).is_ok_and(|v| v.len() <= 1024));
                    return error(wire::ErrorCode::NoncanonicalExecutable, detail);
                }
            }
            let submitted = match e.issue_execute_token(ctx) {
                Ok(token) => e.submit(ctx, &token, id, &request),
                Err(err) => Err(err),
            };
            match e.status(ctx, id) {
                Ok(s) => {
                    let w = worker(e, ctx, id, s.state);
                    return observed(s, op, w, ObservationKind::Dispatched);
                }
                Err(engine::EngineError::NotFound) => {
                    return match submitted {
                        Err(engine::EngineError::CommandDenied) => {
                            error(wire::ErrorCode::CommandDenied, None)
                        }
                        Err(engine::EngineError::InvalidRequest) => {
                            error(wire::ErrorCode::InvalidRequest, None)
                        }
                        Err(engine::EngineError::QuotaExceeded) => {
                            error(wire::ErrorCode::QuotaExceeded, None)
                        }
                        _ => EngineObservation::Missing,
                    };
                }
                Err(err) => return failed(err),
            }
        }
        Err(err) => return failed(err),
    };
    let fallback = status.clone();
    // Found bypasses canonicalization, token issuance and submit even on changed
    // filesystem, stale policy, or revoked remote authority. Facts must commit.
    let kind = match &op.operation {
        EngineOperation::Observe
            if matches!(
                op.ledger.feeder,
                FeederState::Beginning
                    | FeederState::WriteIntended { .. }
                    | FeederState::EofIntended { .. }
                    | FeederState::Failed
            ) =>
        {
            ObservationKind::Input
        }
        EngineOperation::Dispatch { .. }
        | EngineOperation::Observe
        | EngineOperation::FinalObserve => ObservationKind::Found,
        EngineOperation::BeginInput => {
            match e.begin_input(ctx, id) {
                Ok(input) => status.input = input,
                Err(err) => return input_failure(e, ctx, id, op, &fallback, err),
            }
            ObservationKind::Input
        }
        EngineOperation::WriteInput {
            generation,
            offset,
            bytes,
        } => {
            if let Err(err) = e.write_stdin(
                ctx,
                id,
                engine::InputGeneration(*generation),
                *offset,
                bytes,
                Duration::from_secs(60),
            ) {
                return input_failure(e, ctx, id, op, &fallback, err);
            }
            status = match e.status(ctx, id) {
                Ok(s) => s,
                Err(_) => {
                    return observed(fallback, op, WorkerState::Unknown, ObservationKind::Found);
                }
            };
            ObservationKind::Input
        }
        EngineOperation::Eof { generation, offset } => {
            if status.input.delivered != u64::from(op.ledger.stdin_length)
                || matches!(
                    status.input.health,
                    engine::InputHealth::Gap | engine::InputHealth::Uncertain
                )
            {
                return observed(status, op, WorkerState::Unknown, ObservationKind::Input);
            }
            match e.stdin_eof(ctx, id, engine::InputGeneration(*generation), *offset) {
                Ok(input) => status.input = input,
                Err(err) => return input_failure(e, ctx, id, op, &fallback, err),
            }
            ObservationKind::EofConfirmed
        }
        EngineOperation::Cancel => {
            status = match e.cancel(ctx, id) {
                Ok(s) => s,
                Err(_) => {
                    return observed(fallback, op, WorkerState::Unknown, ObservationKind::Found);
                }
            };
            ObservationKind::Found
        }
        EngineOperation::Poll {
            stdout_cursor,
            stderr_cursor,
            max_bytes,
        } => {
            if !(1..=16384).contains(max_bytes) {
                return observed(fallback, op, WorkerState::Unknown, ObservationKind::Found);
            }
            let (stdout, stderr) = match poll_outputs(
                *stdout_cursor,
                *stderr_cursor,
                *max_bytes as usize,
                status.stdout.clone(),
                status.stderr.clone(),
                |stream, cursor, limit| e.read_output(ctx, id, stream, cursor, limit),
            ) {
                Ok(chunks) => chunks,
                Err(_) => {
                    return observed(fallback, op, WorkerState::Unknown, ObservationKind::Found);
                }
            };
            status.stdout = stdout.status;
            status.stderr = stderr.status;
            let w = worker(e, ctx, id, status.state);
            return match wire_status(&status, op) {
                Some(status) => EngineObservation::Polled {
                    status,
                    stdout: wire::Chunk {
                        bytes: stdout.bytes,
                        next_cursor: stdout.next_cursor,
                    },
                    stderr: wire::Chunk {
                        bytes: stderr.bytes,
                        next_cursor: stderr.next_cursor,
                    },
                    worker: w,
                },
                None => EngineObservation::Failed(OperationalFailure::Uncertain),
            };
        }
    };
    let w = worker(e, ctx, id, status.state);
    observed(status, op, w, kind)
}
/// Give both streams an initial share before either consumes unused capacity.
fn poll_outputs(
    stdout_cursor: u64,
    stderr_cursor: u64,
    max: usize,
    stdout_status: engine::OutputStatus,
    stderr_status: engine::OutputStatus,
    mut read: impl FnMut(engine::Stream, u64, usize) -> engine::Result<engine::OutputChunk>,
) -> engine::Result<(engine::OutputChunk, engine::OutputChunk)> {
    let mut stdout = engine::OutputChunk {
        bytes: vec![],
        next_cursor: stdout_cursor,
        status: stdout_status,
    };
    let mut stderr = engine::OutputChunk {
        bytes: vec![],
        next_cursor: stderr_cursor,
        status: stderr_status,
    };
    for (stream, chunk, limit) in [
        (
            engine::Stream::Stdout,
            &mut stdout,
            max.div_ceil(2).min(8192),
        ),
        (engine::Stream::Stderr, &mut stderr, (max / 2).min(8192)),
    ] {
        if limit > 0 {
            let value = read(stream, chunk.next_cursor, limit)?;
            if value.bytes.len() > limit {
                return Err(engine::EngineError::RecoveryUncertain);
            }
            *chunk = value;
        }
    }
    let mut remaining = max
        .checked_sub(stdout.bytes.len())
        .and_then(|value| value.checked_sub(stderr.bytes.len()))
        .ok_or(engine::EngineError::RecoveryUncertain)?;
    for (stream, chunk) in [
        (engine::Stream::Stdout, &mut stdout),
        (engine::Stream::Stderr, &mut stderr),
    ] {
        if remaining == 0 {
            break;
        }
        let more = read(stream, chunk.next_cursor, remaining)?;
        remaining = remaining
            .checked_sub(more.bytes.len())
            .ok_or(engine::EngineError::RecoveryUncertain)?;
        chunk.bytes.extend(more.bytes);
        chunk.next_cursor = more.next_cursor;
        chunk.status = more.status;
    }
    Ok((stdout, stderr))
}
fn input_failure(
    e: &mut engine::Engine,
    ctx: &engine::AuthorizedContext,
    id: engine::InvocationId,
    op: &OperationRecord,
    fallback: &engine::InvocationStatus,
    _error: engine::EngineError,
) -> EngineObservation {
    // Never infer absence or retry bytes from an input error. The next committed
    // status supplies accepted/delivered/health/generation and preserves ambiguity.
    match e.status(ctx, id) {
        Ok(s) => {
            let w = worker(e, ctx, id, s.state);
            observed(s, op, w, ObservationKind::Input)
        }
        Err(_) => observed(
            fallback.clone(),
            op,
            WorkerState::Unknown,
            ObservationKind::Found,
        ),
    }
}
enum ObservationKind {
    Found,
    Dispatched,
    Input,
    EofConfirmed,
}
fn observed(
    s: engine::InvocationStatus,
    op: &OperationRecord,
    worker: WorkerState,
    kind: ObservationKind,
) -> EngineObservation {
    let Some(mut status) = wire_status(&s, op) else {
        return EngineObservation::Failed(OperationalFailure::Uncertain);
    };
    // Closed is also set on natural exit without EOF. Only EofPending or the
    // actual successful stdin_eof call proves acceptance; recovery never guesses.
    if matches!(kind, ObservationKind::Input | ObservationKind::EofConfirmed) {
        let intended = match op.ledger.feeder {
            FeederState::EofIntended { generation, offset } => Some((generation, offset)),
            _ => None,
        };
        status.input.eof_sent |= (matches!(op.ledger.feeder, FeederState::Failed)
            && s.input.generation.is_some()
            && s.input.accepted == u64::from(op.ledger.stdin_length)
            && s.input.delivered == u64::from(op.ledger.stdin_length)
            && s.input.health == engine::InputHealth::EofPending)
            || intended.is_some_and(|(generation, offset)| {
                s.input.generation == Some(engine::InputGeneration(generation))
                    && offset == u64::from(op.ledger.stdin_length)
                    && s.input.accepted == offset
                    && s.input.delivered == offset
                    && (s.input.health == engine::InputHealth::EofPending
                        || matches!(kind, ObservationKind::EofConfirmed))
            });
    }
    if !matches!(op.ledger.feeder, FeederState::Complete)
        && s.input.delivered < u64::from(op.ledger.stdin_length)
        && s.state == engine::InvocationState::Exited
        && !s.cancel_requested
    {
        status.warning = Some(wire::Warning::StdinUnconsumed);
    } else if matches!(
        s.input.health,
        engine::InputHealth::Gap | engine::InputHealth::Uncertain
    ) && !matches!(op.ledger.feeder, FeederState::Complete)
        && !(s.state == engine::InvocationState::Exited
            && op.ledger.stdin_length == 0
            && s.input.accepted == 0
            && s.input.delivered == 0)
    {
        status.warning = Some(wire::Warning::InputFailed);
    }
    match kind {
        ObservationKind::Found => EngineObservation::Found { status, worker },
        ObservationKind::Dispatched => EngineObservation::Dispatched { status, worker },
        ObservationKind::Input | ObservationKind::EofConfirmed => EngineObservation::Input {
            status,
            generation: s.input.generation.map(|g| g.0),
            credit: u64::from(s.input.available_credit),
            worker,
        },
    }
}
fn wire_status(s: &engine::InvocationStatus, op: &OperationRecord) -> Option<wire::Status> {
    use engine::InvocationState as E;
    use wire::State as W;
    let outcome = match &s.outcome {
        None => None,
        Some(engine::ExitOutcome::NotStarted) => Some(wire::Outcome {
            kind: wire::OutcomeKind::NotStarted,
            value: None,
        }),
        Some(engine::ExitOutcome::Code(v)) => Some(wire::Outcome {
            kind: wire::OutcomeKind::Code,
            value: Some(u8::try_from(*v).ok()?),
        }),
        Some(engine::ExitOutcome::Signal(v)) => Some(wire::Outcome {
            kind: wire::OutcomeKind::Signal,
            value: Some(u8::try_from(*v).ok()?),
        }),
    };
    let output = |s: &engine::OutputStatus| wire::OutputStatus {
        closed: s.closed,
        produced: s.produced,
        retained: s.retained,
        gaps: s
            .gaps
            .iter()
            .map(|g| wire::Gap {
                start: g.start,
                end: g.end,
                reason: match g.reason {
                    engine::MissingReason::Limit => wire::GapReason::Limit,
                    engine::MissingReason::StorageFailure => wire::GapReason::StorageFailure,
                    engine::MissingReason::Expired => wire::GapReason::Expired,
                    engine::MissingReason::UncertainTail => wire::GapReason::UncertainTail,
                },
            })
            .collect(),
    };
    Some(wire::Status {
        admission: wire::Admission::Engine,
        authority_valid: op.ledger.authority_valid,
        cancel_requested: s.cancel_requested,
        engine_known: true,
        input: wire::InputStatus {
            accepted: s.input.accepted.try_into().ok()?,
            delivered: s.input.delivered.try_into().ok()?,
            eof_sent: matches!(op.ledger.feeder, FeederState::Complete),
            health: match s.input.health {
                engine::InputHealth::Open => wire::InputHealth::Open,
                engine::InputHealth::EofPending => wire::InputHealth::EofPending,
                engine::InputHealth::Closed => wire::InputHealth::Closed,
                engine::InputHealth::Gap => wire::InputHealth::Gap,
                engine::InputHealth::Uncertain => wire::InputHealth::Uncertain,
            },
            length: op.ledger.stdin_length,
        },
        invocation: s.invocation.0,
        outcome,
        state: match s.state {
            E::Accepted => W::Accepted,
            E::Launching => W::Launching,
            E::Running => W::Running,
            E::Exited => W::Exited,
            E::UnknownLaunch => W::UnknownLaunch,
            E::Lost => W::Lost,
        },
        stdout: output(&s.stdout),
        stderr: output(&s.stderr),
        warning: op.ledger.warning,
    })
}

/// Select one next local feeder intention. The actor must durably register it
/// before enqueueing. Ambiguous begin/write/EOF never repeats an input side effect.
pub(crate) fn next_feeder_operation(
    row: &InvocationRecord,
) -> crate::Result<Option<EngineOperation>> {
    let Some(status) = row.status.as_ref() else {
        return Ok(None);
    };
    if !row.engine_known {
        return Ok(None);
    }
    if status.state == wire::State::Exited {
        return Ok(None);
    }
    if row.warning == Some(wire::Warning::InputFailed)
        || matches!(
            status.input.health,
            wire::InputHealth::Gap | wire::InputHealth::Uncertain
        )
    {
        return Ok((!status.cancel_requested).then_some(EngineOperation::Cancel));
    }
    Ok(match &row.feeder {
        FeederState::NeverBegun => Some(EngineOperation::BeginInput),
        FeederState::Ready { generation, credit } => {
            let submit = row
                .submit
                .as_ref()
                .ok_or(crate::Error::Storage("command_payload_expired"))?;
            if submit.stdin.is_empty() {
                Some(EngineOperation::Eof {
                    generation: *generation,
                    offset: 0,
                })
            } else if *credit >= submit.stdin.len() as u64 {
                Some(EngineOperation::WriteInput {
                    generation: *generation,
                    offset: 0,
                    bytes: submit.stdin.clone(),
                })
            } else {
                Some(EngineOperation::Observe)
            }
        }
        FeederState::Accepted { generation, .. }
            if status.input.delivered == status.input.length =>
        {
            Some(EngineOperation::Eof {
                generation: *generation,
                offset: u64::from(status.input.length),
            })
        }
        FeederState::Beginning
        | FeederState::WriteIntended { .. }
        | FeederState::EofIntended { .. }
        | FeederState::Accepted { .. } => Some(EngineOperation::Observe),
        FeederState::Failed | FeederState::Complete => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{OperationId, OperationLedgerSnapshot, Owner};
    use crate::mdk_adapter::{AuthenticatedCommandContext, CommandDirection};
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn operation(n: u8, operation: EngineOperation) -> OperationRecord {
        let context = AuthenticatedCommandContext::test_fixture(
            [1; 32],
            [2; 32],
            vec![3; 32],
            [4; 16],
            CommandDirection::Server,
        );
        OperationRecord {
            id: OperationId([n; 16]),
            request_id: None,
            key: InvocationKey {
                server: [1; 32],
                principal: [2; 32],
                invocation: [n; 16],
            },
            owner: Owner::from_context(&context),
            revision: 7,
            engine_known: false,
            ledger: OperationLedgerSnapshot {
                stdin_length: 0,
                feeder: FeederState::NeverBegun,
                warning: None,
                authority_valid: true,
            },
            operation,
        }
    }
    fn scheduler() -> Worker {
        Worker {
            engine: None,
            guard: None,
            unresolved: BTreeSet::new(),
            known: BTreeSet::new(),
            awaiting: BTreeMap::new(),
            pending: VecDeque::new(),
            closed: false,
        }
    }
    fn handle() -> (
        CommandEngine,
        Receiver<OperationRecord>,
        Receiver<Control>,
        SyncSender<Event>,
    ) {
        let (operations, rx) = mpsc::sync_channel(CAPACITY);
        let (controls, crx) = mpsc::sync_channel(CAPACITY);
        let (tx, events) = mpsc::sync_channel(CAPACITY);
        (
            CommandEngine {
                operations,
                controls,
                events,
                completions: VecDeque::new(),
                registered: BTreeMap::new(),
                thread: None,
                readiness: Readiness::Ready,
                cleanup: CleanupState::Open,
                #[cfg(test)]
                opened: Instant::now(),
                stopping: false,
            },
            rx,
            crx,
            tx,
        )
    }
    fn ack(n: u8, decision: CommittedDecision) -> CommitAck {
        let op = operation(n, EngineOperation::Observe);
        CommitAck {
            operation: op.id,
            key: op.key,
            revision: op.revision,
            decision,
            answered: vec![],
            suppressed: vec![],
        }
    }
    #[test]
    fn dispatch_waits_for_every_other_unknown_and_commit_ack() {
        let mut w = scheduler();
        let a = key(&operation(1, EngineOperation::Observe).key);
        let b = key(&operation(2, EngineOperation::Observe).key);
        w.unresolved.extend([a, b]);
        assert!(!w.can_dispatch(&a));
        w.awaiting.insert(
            b,
            Awaiting {
                id: [2; 16],
                revision: 7,
                fact: Fact::Found,
            },
        );
        let mut stale = ack(2, CommittedDecision::KnownFound);
        stale.revision = 6;
        w.ack(stale);
        assert!(!w.can_dispatch(&a));
        w.ack(ack(2, CommittedDecision::KnownFound));
        assert!(w.can_dispatch(&a));
        w.awaiting.insert(
            a,
            Awaiting {
                id: [1; 16],
                revision: 7,
                fact: Fact::Found,
            },
        );
        assert!(!w.can_dispatch(&a));
        w.ack(ack(1, CommittedDecision::KnownFound));
        assert!(w.can_dispatch(&a));
    }
    #[test]
    fn mismatched_decision_and_uncertainty_do_not_release_recovery_barrier() {
        for (fact, decision) in [
            (Fact::Uncertain, CommittedDecision::DefinitelyUnsubmitted),
            (Fact::Found, CommittedDecision::DefinitelyUnsubmitted),
            (Fact::Missing, CommittedDecision::KnownFound),
            (Fact::Found, CommittedDecision::StillUncertain),
        ] {
            let mut w = scheduler();
            let a = key(&operation(1, EngineOperation::Observe).key);
            let b = key(&operation(2, EngineOperation::Observe).key);
            w.unresolved.insert(a);
            w.awaiting.insert(
                a,
                Awaiting {
                    id: [1; 16],
                    revision: 7,
                    fact,
                },
            );
            w.ack(ack(1, decision));
            assert!(!w.can_dispatch(&b));
        }
    }
    #[test]
    fn observer_expiry_never_releases_registration_or_allows_duplicate() {
        let (mut h, _rx, _crx, tx) = handle();
        h.try_enqueue(operation(1, EngineOperation::Observe))
            .ok()
            .unwrap();
        h.registered.values_mut().next().unwrap().started =
            Instant::now() - Duration::from_secs(16);
        assert!(h.observer_expired(&operation(1, EngineOperation::Observe).key));
        assert!(h.observation_pending());
        assert!(matches!(
            h.try_enqueue(operation(1, EngineOperation::Observe)),
            Err(QueueBusy::InvocationBusy)
        ));
        let mut wrong = ack(1, CommittedDecision::KnownFound);
        wrong.operation = OperationId([9; 16]);
        assert!(h.ack_committed(wrong).is_err());
        assert!(h.observation_pending());
        tx.send(Event::Completion(Box::new(EngineCompletion {
            operation: OperationId([1; 16]),
            key: operation(1, EngineOperation::Observe).key,
            revision: 7,
            result: EngineObservation::Missing,
        })))
        .unwrap();
        assert!(h.try_completion().is_some());
        h.ack_committed(ack(1, CommittedDecision::KnownFound))
            .unwrap();
        assert!(!h.observation_pending());
    }
    #[test]
    fn bounded_queue_and_open_timeout_retain_same_handle() {
        let (mut h, _rx, _crx, _tx) = handle();
        h.readiness = Readiness::Opening;
        h.opened = Instant::now() - Duration::from_secs(31);
        assert!(h.open_observer_expired());
        for n in 0..64 {
            h.try_enqueue(operation(n, EngineOperation::Observe))
                .ok()
                .unwrap();
        }
        assert!(matches!(
            h.try_enqueue(operation(65, EngineOperation::Observe)),
            Err(QueueBusy::Full)
        ));
        assert_eq!(h.registered.len(), 64);
        assert!(h.try_prepare_cleanup().is_err());
    }
    #[test]
    fn stop_drains_events_while_control_queue_is_full() {
        let (mut handle, _operations, controls, events) = handle();
        for _ in 0..CAPACITY {
            handle
                .controls
                .try_send(Control::Ack(ack(1, CommittedDecision::KnownFound)))
                .ok()
                .unwrap();
            events.try_send(Event::Ready).ok().unwrap();
        }
        handle.thread = Some(thread::spawn(move || {
            // This send cannot finish until production shutdown drains events.
            events.send(Event::Ready).ok().unwrap();
            while let Ok(control) = controls.recv() {
                if matches!(control, Control::Stop) {
                    return;
                }
            }
        }));
        assert!(
            handle
                .shutdown(Instant::now() + Duration::from_secs(2))
                .unwrap()
                .complete
        );
    }
    fn engine_settings(d: &tempfile::TempDir) -> EngineSettings {
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let worker = d.path().join("worker");
        std::fs::copy(std::env::current_exe().unwrap(), &worker).unwrap();
        std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
        let policy = engine::EnginePolicy {
            state_root: d.path().join("engine"),
            worker_executable: worker,
            server: engine::ServerId([1; 32]),
            allowed_principals: vec![engine::PrincipalId([2; 32])],
            allow_shell_command: false,
            command_policy: None,
            cwd: Some(d.path().to_owned()),
            limits: engine::EngineLimits::default(),
        };
        EngineSettings {
            policy,
            binding: engine::NativeBinding::new([8; 32], [9; 32]),
            master: Zeroizing::new([10; 32]),
        }
    }
    fn engine_fixture() -> (tempfile::TempDir, engine::Engine) {
        let d = tempfile::tempdir().unwrap();
        let settings = engine_settings(&d);
        let e = engine::Engine::open_with_master(
            settings.policy,
            settings.binding,
            engine::ProvidedMaster::new(*settings.master),
        )
        .unwrap();
        (d, e)
    }
    #[test]
    fn real_thread_opens_observes_commits_and_retains_cleanup_guard_until_shutdown() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let mut h = CommandEngine::start(engine_settings(&d), vec![]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.readiness() == Readiness::Opening && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(h.readiness() == Readiness::Ready);
        h.try_enqueue(operation(1, EngineOperation::Observe))
            .ok()
            .unwrap();
        let completion = loop {
            if let Some(c) = h.try_completion() {
                break c;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        };
        assert!(matches!(completion.result, EngineObservation::Missing));
        assert!(h.try_prepare_cleanup().is_err());
        h.ack_committed(ack(1, CommittedDecision::DefinitelyUnsubmitted))
            .unwrap();
        h.try_prepare_cleanup().unwrap();
        while h.cleanup_state() == CleanupState::Draining && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(h.cleanup_state() == CleanupState::Quiesced);
        assert!(matches!(
            h.try_enqueue(operation(2, EngineOperation::Observe)),
            Err(QueueBusy::Unavailable)
        ));
        // Protected locks remain held by this exact thread's CleanupGuard.
        let other = engine_settings(&d);
        assert!(
            engine::Engine::open_with_master(
                other.policy,
                other.binding,
                engine::ProvidedMaster::new(*other.master)
            )
            .is_err()
        );
        let stopped = h.shutdown(deadline).unwrap();
        assert!(stopped.complete && stopped.quiesced);
        let reopened = engine_settings(&d);
        assert!(
            engine::Engine::open_with_master(
                reopened.policy,
                reopened.binding,
                engine::ProvidedMaster::new(*reopened.master)
            )
            .is_ok()
        );
    }
    #[test]
    fn recovery_known_rows_also_require_first_status_commit_before_new_dispatch() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (_d, e) = engine_fixture();
        let op = operation(1, EngineOperation::Observe);
        let own = key(&op.key);
        let other = key(&operation(2, EngineOperation::Observe).key);
        let mut w = Worker::new(
            e,
            vec![RecoveryRecord {
                key: op.key,
                engine_known: true,
            }],
        );
        assert!(!w.can_dispatch(&other));
        assert!(w.known.contains(&own));
        w.awaiting.insert(
            own,
            Awaiting {
                id: [1; 16],
                revision: 7,
                fact: Fact::Missing,
            },
        );
        w.ack(ack(1, CommittedDecision::KnownFound));
        assert!(w.can_dispatch(&other));
        assert!(w.known.contains(&own));
    }
    #[test]
    fn real_engine_canonical_refusal_creates_no_job_and_known_missing_never_dispatches() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let (d, mut e) = engine_fixture();
        let ctx = e
            .authorize(
                engine::PrincipalId([2; 32]),
                engine::EndpointId(Sha256::digest([3; 32]).into()),
                engine::ServerId([1; 32]),
                unsafe { libc::geteuid() },
            )
            .unwrap();
        let alias = d.path().join("alias");
        let executable = d.path().join("worker").canonicalize().unwrap();
        assert!(executable.is_file());
        symlink(&executable, &alias).unwrap();
        let op = operation(
            1,
            EngineOperation::Dispatch {
                submit: wire::Submit {
                    invocation: [1; 16],
                    argv: vec![alias.as_os_str().as_encoded_bytes().to_vec()],
                    stdin: vec![],
                },
            },
        );
        assert!(matches!(
            execute_operation(&mut e, &ctx, engine::InvocationId([1; 16]), &op, false),
            EngineObservation::Refused(wire::WireError {
                code: wire::ErrorCode::NoncanonicalExecutable,
                ..
            })
        ));
        assert!(matches!(
            e.status(&ctx, engine::InvocationId([1; 16])),
            Err(engine::EngineError::NotFound)
        ));
        // Even a perfectly executable argv cannot replay a previously known job.
        assert!(matches!(
            execute_operation(&mut e, &ctx, engine::InvocationId([1; 16]), &op, true),
            EngineObservation::Missing
        ));
        assert!(matches!(
            e.status(&ctx, engine::InvocationId([1; 16])),
            Err(engine::EngineError::NotFound)
        ));
        drop(e);
    }
    fn input_status(
        length: u64,
        delivered: u64,
        health: engine::InputHealth,
    ) -> engine::InvocationStatus {
        let output = engine::OutputStatus {
            retained: 0,
            produced: 0,
            closed: false,
            gaps: vec![],
        };
        engine::InvocationStatus {
            invocation: engine::InvocationId([1; 16]),
            state: engine::InvocationState::Running,
            outcome: None,
            cancel_requested: false,
            input: engine::InputStatus {
                generation: Some(engine::InputGeneration([5; 16])),
                accepted: length,
                delivered,
                available_credit: 16384,
                health,
                gap_count: 0,
            },
            stdout: output.clone(),
            stderr: output,
        }
    }
    #[test]
    fn poll_shares_budget_before_stdout_can_consume_stderr_capacity() {
        let output = input_status(0, 0, engine::InputHealth::Open).stdout;
        let mut calls = vec![];
        let (stdout, stderr) = poll_outputs(
            0,
            0,
            16384,
            output.clone(),
            output.clone(),
            |stream, cursor, limit| {
                calls.push((stream, limit));
                let length = if stream == engine::Stream::Stderr {
                    limit.min(3)
                } else {
                    limit
                };
                Ok(engine::OutputChunk {
                    bytes: vec![0; length],
                    next_cursor: cursor + length as u64,
                    status: output.clone(),
                })
            },
        )
        .unwrap();
        assert_eq!(calls[0], (engine::Stream::Stdout, 8192));
        assert_eq!(calls[1], (engine::Stream::Stderr, 8192));
        assert_eq!(stdout.bytes.len() + stderr.bytes.len(), 16384);
        assert_eq!(stderr.bytes.len(), 3);
        assert_eq!(stdout.bytes.len(), 16381);
        let (a, b) = poll_outputs(
            0,
            0,
            1,
            output.clone(),
            output.clone(),
            |_, cursor, limit| {
                assert!(limit > 0);
                Ok(engine::OutputChunk {
                    bytes: vec![0; limit],
                    next_cursor: cursor + limit as u64,
                    status: output.clone(),
                })
            },
        )
        .unwrap();
        assert_eq!(a.bytes.len() + b.bytes.len(), 1);
    }
    #[test]
    fn oversized_poll_reads_fail_finitely_in_initial_and_leftover_passes() {
        let output = input_status(0, 0, engine::InputHealth::Open).stdout;
        for oversized_call in [0, 1, 2, 3] {
            let mut calls = 0;
            let result = poll_outputs(
                0,
                0,
                8,
                output.clone(),
                output.clone(),
                |_, cursor, limit| {
                    let length = if calls == oversized_call {
                        limit + 1
                    } else {
                        0
                    };
                    calls += 1;
                    Ok(engine::OutputChunk {
                        bytes: vec![0; length],
                        next_cursor: cursor + length as u64,
                        status: output.clone(),
                    })
                },
            );
            assert!(matches!(
                result,
                Err(engine::EngineError::RecoveryUncertain)
            ));
            assert_eq!(calls, oversized_call + 1);
        }
    }
    #[test]
    fn exited_empty_input_does_not_warn_or_fabricate_eof() {
        for length in [0, 4] {
            for health in [
                engine::InputHealth::Closed,
                engine::InputHealth::Gap,
                engine::InputHealth::Uncertain,
            ] {
                let mut op = operation(1, EngineOperation::Observe);
                op.ledger.stdin_length = length;
                let mut s = input_status(0, 0, health);
                s.state = engine::InvocationState::Exited;
                let EngineObservation::Found { status, .. } =
                    observed(s, &op, WorkerState::Exited, ObservationKind::Found)
                else {
                    panic!("found")
                };
                assert!(!status.input.eof_sent);
                assert!(status.warning.is_none() == (length == 0));
                if length != 0 {
                    assert!(status.warning == Some(wire::Warning::StdinUnconsumed));
                }
            }
        }
    }
    #[test]
    fn eof_requires_matching_generation_full_delivery_and_engine_confirmation() {
        let mut op = operation(
            1,
            EngineOperation::Eof {
                generation: [5; 16],
                offset: 4,
            },
        );
        op.ledger.stdin_length = 4;
        op.ledger.feeder = FeederState::EofIntended {
            generation: [5; 16],
            offset: 4,
        };
        for (delivered, health, expected) in [
            (4, engine::InputHealth::Open, false),
            (3, engine::InputHealth::EofPending, false),
            (4, engine::InputHealth::EofPending, true),
            (4, engine::InputHealth::Closed, false),
            (4, engine::InputHealth::Gap, false),
        ] {
            let s = input_status(4, delivered, health);
            let EngineObservation::Input { status, .. } =
                observed(s, &op, WorkerState::Running, ObservationKind::Input)
            else {
                panic!("input snapshot")
            };
            assert_eq!(status.input.eof_sent, expected);
        }
        let mut s = input_status(4, 4, engine::InputHealth::Closed);
        s.input.generation = Some(engine::InputGeneration([6; 16]));
        let EngineObservation::Input { status, .. } =
            observed(s, &op, WorkerState::Running, ObservationKind::Input)
        else {
            panic!("input snapshot")
        };
        assert!(!status.input.eof_sent);
        let confirmed = input_status(4, 4, engine::InputHealth::Closed);
        let EngineObservation::Input { status, .. } = observed(
            confirmed,
            &op,
            WorkerState::Running,
            ObservationKind::EofConfirmed,
        ) else {
            panic!("confirmed EOF")
        };
        assert!(status.input.eof_sent);
        op.ledger.feeder = FeederState::Failed;
        let closed = input_status(4, 4, engine::InputHealth::Closed);
        let EngineObservation::Input { status, .. } =
            observed(closed, &op, WorkerState::Running, ObservationKind::Input)
        else {
            panic!("recovery closed")
        };
        assert!(!status.input.eof_sent);
        op.ledger.stdin_length = 0;
        op.ledger.feeder = FeederState::EofIntended {
            generation: [5; 16],
            offset: 0,
        };
        let EngineObservation::Input { status, .. } = observed(
            input_status(0, 0, engine::InputHealth::EofPending),
            &op,
            WorkerState::Running,
            ObservationKind::Input,
        ) else {
            panic!("empty input")
        };
        assert!(status.input.eof_sent);
    }
    #[test]
    fn known_then_collected_ack_releases_barrier_without_replay_permission() {
        let mut w = scheduler();
        let a = key(&operation(1, EngineOperation::Observe).key);
        let b = key(&operation(2, EngineOperation::Observe).key);
        w.known.insert(a);
        w.unresolved.insert(a);
        w.awaiting.insert(
            a,
            Awaiting {
                id: [1; 16],
                revision: 7,
                fact: Fact::Missing,
            },
        );
        w.ack(ack(1, CommittedDecision::KnownFound));
        assert!(w.can_dispatch(&b));
        assert!(w.known.contains(&a));
    }
    #[test]
    fn feeder_waits_for_delivery_and_never_repeats_ambiguous_side_effects() {
        let op = operation(1, EngineOperation::Observe);
        let status = wire_status(&input_status(4, 2, engine::InputHealth::Open), &op).unwrap();
        let mut row = InvocationRecord {
            key: op.key,
            owner: op.owner,
            submit: Some(wire::Submit {
                invocation: [1; 16],
                argv: vec![b"/bin/cat".to_vec()],
                stdin: b"test".to_vec(),
            }),
            stdin_length: 4,
            digest: [0; 32],
            admission: wire::Admission::Engine,
            revision: 7,
            engine_known: true,
            status: Some(status),
            refusal: None,
            feeder: FeederState::Accepted {
                generation: [5; 16],
                offset: 4,
                credit: 16384,
            },
            authority_valid: true,
            warning: None,
            worker: WorkerState::Running,
            observed_seq: 0,
            uncertain: false,
            cursor: 0,
        };
        row.status.as_mut().unwrap().input.length = 4;
        assert!(matches!(
            next_feeder_operation(&row).unwrap(),
            Some(EngineOperation::Observe)
        ));
        row.status.as_mut().unwrap().input.delivered = 4;
        assert!(matches!(
            next_feeder_operation(&row).unwrap(),
            Some(EngineOperation::Eof { offset: 4, .. })
        ));
        for feeder in [
            FeederState::Beginning,
            FeederState::WriteIntended {
                generation: [5; 16],
                offset: 0,
                digest: [0; 32],
            },
            FeederState::EofIntended {
                generation: [5; 16],
                offset: 4,
            },
        ] {
            row.feeder = feeder;
            assert!(matches!(
                next_feeder_operation(&row).unwrap(),
                Some(EngineOperation::Observe)
            ));
        }
        row.status.as_mut().unwrap().input.health = wire::InputHealth::Gap;
        assert!(matches!(
            next_feeder_operation(&row).unwrap(),
            Some(EngineOperation::Cancel)
        ));
        row.status.as_mut().unwrap().cancel_requested = true;
        assert!(next_feeder_operation(&row).unwrap().is_none());
        row.submit = None;
        row.feeder = FeederState::Ready {
            generation: [5; 16],
            credit: 16384,
        };
        row.status.as_mut().unwrap().input.health = wire::InputHealth::Open;
        assert!(matches!(
            next_feeder_operation(&row),
            Err(crate::Error::Storage("command_payload_expired"))
        ));
        row.status.as_mut().unwrap().state = wire::State::Exited;
        assert!(next_feeder_operation(&row).unwrap().is_none());
    }
    #[test]
    fn failure_to_prepare_is_irreversible_and_keeps_same_engine_owner() {
        let (mut h, _rx, crx, _tx) = handle();
        h.fail_cleanup().unwrap();
        assert!(matches!(crx.try_recv(), Ok(Control::FailCleanup)));
        assert!(matches!(
            h.try_enqueue(operation(1, EngineOperation::Observe)),
            Err(QueueBusy::Unavailable)
        ));
        assert!(h.try_prepare_cleanup().is_err());
    }
}
