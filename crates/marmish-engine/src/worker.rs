//! Detached, same-user command supervision. No transport connection owns the child.
use crate::fault::{fault_point, FaultPoint};
use crate::launcher::{self, ChildSpawnError, ChildSpec, SpawnedChild, StateRoot};
use crate::model::*;
use crate::spool::Spool;
use crate::store::{ClaimResult, Delivery, InputWork, OutputCommit, ProcessIdentity, Store};
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const BUFFER: usize = 64 * 1024;
const RETRY_LIMIT: Duration = Duration::from_secs(30);
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_handler(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

fn retry<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    let start = Instant::now();
    let mut delay = Duration::from_millis(10);
    loop {
        match operation() {
            Err(EngineError::Busy) if start.elapsed() < RETRY_LIMIT => {
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_millis(250));
            }
            result => return result,
        }
    }
}
fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(EngineError::WorkerUnavailable);
    }
    Ok(())
}
fn pipe_file(pipe: impl Into<OwnedFd>) -> Result<File> {
    let file = File::from(pipe.into());
    nonblocking(file.as_raw_fd())?;
    Ok(file)
}

/// The leader remains unreaped until the last group signal. Its reserved PID
/// prevents PGID reuse. Descendants deliberately calling setsid can escape;
/// Phase 1 is same-user execution, not a hostile-process containment sandbox.
struct ChildOwner {
    child: Child,
    identity: ProcessIdentity,
    worker: ProcessIdentity,
    pgid: i32,
    reaped: bool,
}
impl ChildOwner {
    fn signal(&self, signal: i32) -> Result<()> {
        if self.reaped
            || launcher::process_identity(self.worker.pid)? != self.worker
            || launcher::process_identity(self.identity.pid)? != self.identity
            || unsafe { libc::getpgid(self.identity.pid) } != self.pgid
            || self.pgid != self.identity.pid
        {
            return Err(EngineError::RecoveryUncertain);
        }
        if unsafe { libc::kill(-self.pgid, signal) } < 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        {
            return Err(EngineError::RecoveryUncertain);
        }
        Ok(())
    }
    fn exited(&self) -> Result<bool> {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                self.identity.pid as u32,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } < 0
        {
            return Err(EngineError::RecoveryUncertain);
        }
        Ok(unsafe { info.si_pid() } != 0)
    }
    fn reap(&mut self) -> Result<ExitOutcome> {
        if !self.exited()? {
            return Err(EngineError::RecoveryUncertain);
        }
        let status = self
            .child
            .wait()
            .map_err(|_| EngineError::RecoveryUncertain)?;
        self.reaped = true;
        status
            .code()
            .map(ExitOutcome::Code)
            .or_else(|| status.signal().map(ExitOutcome::Signal))
            .ok_or(EngineError::RecoveryUncertain)
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if self.signal(libc::SIGKILL).is_err() {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.exited().unwrap_or(false) {
                let _ = self.reap();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // An uninterruptible kernel wait must not hang supervisor shutdown.
    }
}

struct OutputPump {
    stream: Stream,
    pipe: Option<File>,
    bytes: Vec<u8>,
    spool: Spool,
    pending: Option<OutputCommit>,
    closed: bool,
    uncertain: bool,
    last_flush: Instant,
    busy_since: Option<Instant>,
}
impl OutputPump {
    #[allow(clippy::too_many_arguments)]
    fn open(
        root: &StateRoot,
        keys: &crate::cipher::StorageKeys,
        job: JobId,
        store: &mut Store,
        worker: &ProcessIdentity,
        stream: Stream,
        pipe: File,
        cap: u64,
    ) -> Result<Self> {
        let status = retry(|| store.worker_output_status(job, worker, stream))?;
        let guard = retry(|| root.lock_io(job))?;
        let spool = Spool::open(root, keys, job, stream, &status, cap, &guard)?;
        Ok(Self {
            stream,
            pipe: Some(pipe),
            bytes: Vec::with_capacity(BUFFER),
            spool,
            pending: None,
            closed: false,
            uncertain: false,
            last_flush: Instant::now(),
            busy_since: None,
        })
    }
    fn drain(&mut self) -> Result<usize> {
        let Some(pipe) = self.pipe.as_mut() else {
            return Ok(0);
        };
        let initial = self.bytes.len();
        let started = Instant::now();
        let mut bytes = [0u8; 16 * 1024];
        // Bound both bytes and CPU time before serving the other stream/control.
        while self.bytes.len() < BUFFER && started.elapsed() < Duration::from_millis(2) {
            let amount = bytes.len().min(BUFFER - self.bytes.len());
            match pipe.read(&mut bytes[..amount]) {
                Ok(0) => {
                    self.pipe = None;
                    break;
                }
                Ok(n) => self.bytes.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.pipe = None;
                    self.uncertain = true;
                    break;
                }
            }
        }
        Ok(self.bytes.len() - initial)
    }
    fn abandon(&mut self) {
        if self.pipe.take().is_some() {
            self.uncertain = true;
        }
    }
    fn flush(
        &mut self,
        root: &StateRoot,
        job: JobId,
        store: &mut Store,
        worker: &ProcessIdentity,
    ) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        if self.pending.is_none()
            && self.pipe.is_some()
            && (self.bytes.is_empty()
                || (self.bytes.len() < BUFFER
                    && self.last_flush.elapsed() < Duration::from_millis(100)))
        {
            return Ok(());
        }
        let result = self.flush_once(root, job, store, worker);
        match result {
            Err(EngineError::Busy) => {
                if self.busy_since.get_or_insert_with(Instant::now).elapsed() >= RETRY_LIMIT {
                    Err(EngineError::Busy)
                } else {
                    Ok(())
                }
            }
            Ok(()) => {
                self.busy_since = None;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    fn flush_once(
        &mut self,
        root: &StateRoot,
        job: JobId,
        store: &mut Store,
        worker: &ProcessIdentity,
    ) -> Result<()> {
        let guard = root.lock_io(job)?;
        if self.pending.is_none() {
            let mut commit = self.spool.append(&self.bytes, &guard)?;
            self.bytes.clear();
            if self.pipe.is_none() {
                commit = self.spool.close(&guard)?;
                if self.uncertain && commit.gaps.last().is_none_or(|g| g.end.is_some()) {
                    if commit.gaps.len() == 4 {
                        // Coarsen the final range instead of losing its known gap.
                        let last = commit.gaps.last_mut().ok_or(EngineError::StorageFailure)?;
                        last.end = None;
                        last.reason = MissingReason::UncertainTail;
                    } else {
                        commit.gaps.push(MissingRange {
                            start: commit.produced,
                            end: None,
                            reason: MissingReason::UncertainTail,
                        });
                    }
                }
            }
            self.pending = Some(commit);
        }
        // Retain this exact metadata on Busy; never append the bytes twice.
        let commit = self.pending.as_ref().ok_or(EngineError::StorageFailure)?;
        store.commit_output(job, worker, self.stream, commit)?;
        self.closed = commit.closed;
        self.pending = None;
        self.last_flush = Instant::now();
        Ok(())
    }
}

/// Retries one state operation across loop iterations so pipe drainage continues.
struct Deferred {
    since: Option<Instant>,
    next: Instant,
    delay: Duration,
}
impl Deferred {
    fn new() -> Self {
        Self {
            since: None,
            next: Instant::now(),
            delay: Duration::from_millis(10),
        }
    }
    fn call<T>(&mut self, operation: impl FnOnce() -> Result<T>) -> Result<Option<T>> {
        if self.since.is_some_and(|t| t.elapsed() >= RETRY_LIMIT) {
            return Err(EngineError::Busy);
        }
        if Instant::now() < self.next {
            return Ok(None);
        }
        match operation() {
            Err(EngineError::Busy) => {
                self.since.get_or_insert_with(Instant::now);
                self.next = Instant::now() + self.delay;
                self.delay = (self.delay * 2).min(Duration::from_millis(250));
                Ok(None)
            }
            Ok(value) => {
                self.since = None;
                self.delay = Duration::from_millis(10);
                Ok(Some(value))
            }
            Err(error) => Err(error),
        }
    }
}
struct InputPump {
    pipe: Option<ChildStdin>,
    delivery: Option<Delivery>,
    written: usize,
    progress_pending: bool,
    failure: Option<(InputGeneration, bool)>,
    close_pending: bool,
    deferred: Deferred,
}
impl InputPump {
    /// Finish state bookkeeping before allowing another pipe write.
    fn persist_progress(
        &mut self,
        job: JobId,
        store: &mut Store,
        worker: &ProcessIdentity,
    ) -> Result<bool> {
        if let Some((generation, uncertain)) = self.failure {
            if self
                .deferred
                .call(|| store.fail_input(job, worker, generation, uncertain))?
                .is_none()
            {
                return Ok(false);
            }
            self.failure = None;
            self.delivery = None;
            self.written = 0;
            self.progress_pending = false;
        }
        if self.close_pending {
            if self
                .deferred
                .call(|| store.close_input(job, worker))?
                .is_some()
            {
                self.close_pending = false;
            }
            return Ok(false);
        }
        if self.pipe.is_none() {
            return Ok(false);
        }
        if self.progress_pending {
            let d = self.delivery.as_ref().ok_or(EngineError::StorageFailure)?;
            match self
                .deferred
                .call(|| store.advance_input(job, worker, d.record, self.written as u32))
            {
                Ok(Some(())) => (),
                Ok(None) => return Ok(false),
                Err(error @ (EngineError::StaleGeneration | EngineError::InputConflict)) => {
                    // Rotation can delete an old delivery between pipe write and
                    // progress commit. Never mask a same-generation invariant error.
                    let Some(controls) = self.deferred.call(|| store.controls(job, worker))? else {
                        return Ok(false);
                    };
                    if controls.generation == Some(d.generation) {
                        return Err(error);
                    }
                    self.failure = Some((d.generation, true));
                    return Ok(false);
                }
                Err(error) => return Err(error),
            }
            self.progress_pending = false;
            if self.written == d.bytes.len() {
                self.delivery = None;
                self.written = 0;
            }
        }
        Ok(true)
    }
    fn tick(
        &mut self,
        root: &StateRoot,
        job: JobId,
        store: &mut Store,
        worker: &ProcessIdentity,
        generation: Option<InputGeneration>,
    ) -> Result<()> {
        if !self.persist_progress(job, store, worker)? {
            return Ok(());
        }
        if let Some(d) = self.delivery.as_ref() {
            let now = launcher::boot_time()?;
            if generation != Some(d.generation)
                || now.boot_id != d.deadline.boot_id
                || now.millis >= d.deadline.millis
            {
                self.failure = Some((d.generation, false));
                return Ok(());
            }
        }
        if self.delivery.is_none() {
            match self
                .deferred
                .call(|| store.take_input(job, worker, launcher::boot_time()?))?
            {
                Some(InputWork::Deliver(d)) => {
                    d.offset
                        .checked_add(d.bytes.len() as u64)
                        .ok_or(EngineError::StorageFailure)?;
                    self.delivery = Some(d);
                    self.written = 0;
                }
                Some(InputWork::Eof) => {
                    self.pipe = None;
                    self.close_pending = true;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        self.write_delivery(root, job, store, worker)
    }
    fn write_delivery(
        &mut self,
        root: &StateRoot,
        job: JobId,
        store: &mut Store,
        worker: &ProcessIdentity,
    ) -> Result<()> {
        let d = self.delivery.as_ref().ok_or(EngineError::StorageFailure)?;
        // Recheck controls and deadline immediately before every bounded write.
        let Some(controls) = self.deferred.call(|| store.controls(job, worker))? else {
            return Ok(());
        };
        let now = launcher::boot_time()?;
        if controls.cancel_requested
            || controls.generation != Some(d.generation)
            || now.boot_id != d.deadline.boot_id
            || now.millis >= d.deadline.millis
        {
            self.failure = Some((d.generation, false));
            return Ok(());
        }
        fault_point(root, job, FaultPoint::BeforeInputWrite);
        let result = self
            .pipe
            .as_mut()
            .ok_or(EngineError::InputClosed)?
            .write(&d.bytes[self.written..d.bytes.len().min(self.written + 16 * 1024)]);
        fault_point(root, job, FaultPoint::AfterInputWrite);
        match result {
            Ok(0) => {
                self.pipe = None;
                self.failure = Some((d.generation, false));
            }
            Ok(n) => {
                self.written += n;
                self.progress_pending = true;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => {
                self.pipe = None;
                self.failure = Some((d.generation, false));
            }
        }
        Ok(())
    }
}

/// Cancellation and natural-exit drainage share one ordered escalation policy.
#[derive(Default)]
struct Shutdown {
    leader_exit: Option<Instant>,
    term: Option<Instant>,
    killed: Option<Instant>,
}
impl Shutdown {
    fn update(&mut self, owner: &ChildOwner, limits: &EngineLimits, cancel: bool) -> Result<()> {
        if cancel && self.term.is_none() {
            owner.signal(libc::SIGTERM)?;
            self.term = Some(Instant::now());
        }
        if self.leader_exit.is_none() && owner.exited()? {
            self.leader_exit = Some(Instant::now());
        }
        if self.term.is_none()
            && self
                .leader_exit
                .is_some_and(|t| t.elapsed() >= limits.drain_grace)
        {
            owner.signal(libc::SIGTERM)?;
            self.term = Some(Instant::now());
        }
        if self.killed.is_none()
            && self
                .term
                .is_some_and(|t| t.elapsed() >= limits.cancel_grace)
        {
            owner.signal(libc::SIGKILL)?;
            self.killed = Some(Instant::now());
        }
        Ok(())
    }
    fn input_stopped(&self) -> bool {
        self.leader_exit.is_some() || self.term.is_some()
    }
    fn drain_expired(&self, limits: &EngineLimits) -> bool {
        self.killed
            .is_some_and(|t| t.elapsed() >= limits.drain_grace)
    }
}

fn supervise(
    root: &StateRoot,
    keys: &crate::cipher::StorageKeys,
    job: JobId,
    store: &mut Store,
    worker: &ProcessIdentity,
    claim: crate::store::WorkerClaim,
    spawned: SpawnedChild,
) -> Result<()> {
    let SpawnedChild {
        child,
        identity,
        pgid,
        stdin,
        stdout,
        stderr,
    } = spawned;
    let mut owner = ChildOwner {
        child,
        identity,
        worker: worker.clone(),
        pgid,
        reaped: false,
    };
    fault_point(root, job, FaultPoint::AfterSpawn);
    retry(|| store.mark_running(job, worker, &owner.identity, pgid))?;
    nonblocking(stdin.as_raw_fd())?;
    let mut input = InputPump {
        pipe: Some(stdin),
        delivery: None,
        written: 0,
        progress_pending: false,
        failure: None,
        close_pending: false,
        deferred: Deferred::new(),
    };
    let mut out = OutputPump::open(
        root,
        keys,
        job,
        store,
        worker,
        Stream::Stdout,
        pipe_file(stdout)?,
        claim.limits.spool_bytes_per_stream,
    )?;
    let mut err = OutputPump::open(
        root,
        keys,
        job,
        store,
        worker,
        Stream::Stderr,
        pipe_file(stderr)?,
        claim.limits.spool_bytes_per_stream,
    )?;
    let mut control_retry = Deferred::new();
    let mut cadence = ControlCadence::new();
    let mut shutdown = Shutdown::default();
    loop {
        if STOP.load(Ordering::Relaxed) {
            return Err(EngineError::RecoveryUncertain);
        }
        let controls = if cadence.due() {
            let controls = control_retry.call(|| store.controls(job, worker))?;
            if controls.is_some() {
                cadence.polled();
            }
            controls
        } else {
            None
        };
        shutdown.update(
            &owner,
            &claim.limits,
            controls.as_ref().is_some_and(|c| c.cancel_requested),
        )?;
        if shutdown.input_stopped() {
            input.pipe = None;
        }
        if shutdown.drain_expired(&claim.limits) {
            out.abandon();
            err.abandon();
        }
        let output_bytes = out.drain()? + err.drain()?;
        if output_bytes != 0 {
            cadence.active();
        }
        out.flush(root, job, store, worker)?;
        err.flush(root, job, store, worker)?;
        if !shutdown.input_stopped() {
            if let Some(controls) = controls {
                input.tick(root, job, store, worker, controls.generation)?;
            } else if input.delivery.is_some() || input.failure.is_some() || input.close_pending {
                // Active delivery fences each write using a fresh controls read.
                input.tick(
                    root,
                    job,
                    store,
                    worker,
                    input.delivery.as_ref().map(|d| d.generation),
                )?;
            }
        }
        if shutdown.leader_exit.is_some() && out.closed && err.closed {
            // Completion also ends descendants in this command's owned group.
            // The unreaped leader reserves its identity through this last signal.
            owner.signal(libc::SIGKILL)?;
            let outcome = owner.reap()?;
            retry(|| store.finish(job, worker, outcome.clone()))?;
            return Ok(());
        }
        wait_for_io(&out, &err, &input, cadence.until_poll())?;
    }
}

/// Quiet commands back off state reads; activity restores responsive controls.
struct ControlCadence {
    next: Instant,
    delay: Duration,
}
impl ControlCadence {
    fn new() -> Self {
        Self {
            next: Instant::now(),
            delay: Duration::from_millis(50),
        }
    }
    fn due(&self) -> bool {
        Instant::now() >= self.next
    }
    fn polled(&mut self) {
        self.next = Instant::now() + self.delay;
        self.delay = (self.delay * 2).min(Duration::from_millis(250));
    }
    fn active(&mut self) {
        self.delay = Duration::from_millis(50);
        self.next = self.next.min(Instant::now() + self.delay);
    }
    fn until_poll(&self) -> Duration {
        self.next
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(50))
    }
}

fn input_write_ready(input: &InputPump) -> bool {
    input.delivery.is_some()
        && !input.progress_pending
        && input.failure.is_none()
        && Instant::now() >= input.deferred.next
}

fn wait_for_io(
    out: &OutputPump,
    err: &OutputPump,
    input: &InputPump,
    timeout: Duration,
) -> Result<()> {
    let mut descriptors = Vec::with_capacity(3);
    for pump in [out, err] {
        if pump.bytes.len() < BUFFER && pump.pending.is_none() {
            if let Some(pipe) = &pump.pipe {
                descriptors.push(libc::pollfd {
                    fd: pipe.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
            }
        }
    }
    if input_write_ready(input) {
        if let Some(pipe) = &input.pipe {
            descriptors.push(libc::pollfd {
                fd: pipe.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            });
        }
    }
    // Cap sleeps even with silent pipes so exit, signal and flush deadlines advance.
    let millis = timeout.as_millis().clamp(1, 50) as i32;
    let result = unsafe {
        libc::poll(
            descriptors.as_mut_ptr(),
            descriptors.len() as libc::nfds_t,
            millis,
        )
    };
    if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
        return Err(EngineError::WorkerUnavailable);
    }
    Ok(())
}

/// A presence probe can transiently hold a shared Darwin flock. Busy is
/// duplicate success only when the durable active claimant is demonstrably live.
pub(super) fn acquire_job(
    root: &StateRoot,
    store: &mut Store,
    job: JobId,
) -> Result<Option<launcher::JobGuard>> {
    let deadline = crate::platform::Deadline::after_millis(5_000)?;
    loop {
        match root.lock_job(job) {
            Ok(guard) => return Ok(Some(guard)),
            Err(EngineError::Busy) => {
                let active = matches!(
                    store.job_state(job)?,
                    InvocationState::Launching | InvocationState::Running
                );
                if active {
                    if let Some(identity) = store.worker_identity(job)? {
                        if matches!(root.presence(job, Some(&identity))?, launcher::WorkerPresence::Live(actual) if actual == identity)
                        {
                            return Ok(None);
                        }
                    }
                }
                if deadline.expired()? {
                    return Err(EngineError::Busy);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }
}

fn run(args: Vec<OsString>) -> Result<()> {
    if args.len() != 2 {
        return Err(EngineError::InvalidRequest);
    }
    let root = StateRoot::open_existing(&PathBuf::from(&args[0]))?;
    root.prepare_db(false)?;
    let job = launcher::parse_job(&args[1])?;
    let credential = root.read_credential(job)?;
    let keys = crate::storage::receive(&root, job)?;
    // Declaration order keeps the lifetime lock held through Store's drop.
    let _job_guard;
    let mut store = Store::open_worker(&root, &keys)?;
    _job_guard = match acquire_job(&root, &mut store, job)? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    let worker = launcher::process_identity(std::process::id() as i32)?;
    let claim = match retry(|| store.claim_worker(job, &credential, &worker))? {
        ClaimResult::AlreadyClaimed => return Ok(()),
        ClaimResult::Claimed(claim) => claim,
    };
    if claim.job != job {
        return Err(EngineError::WorkerUnavailable);
    }
    if retry(|| store.controls(job, &worker))?.cancel_requested {
        return retry(|| store.finish(job, &worker, ExitOutcome::NotStarted));
    }
    launch_claimed(&root, &keys, job, &mut store, &worker, claim)
}
fn launch_claimed(
    root: &StateRoot,
    keys: &crate::cipher::StorageKeys,
    job: JobId,
    store: &mut Store,
    worker: &ProcessIdentity,
    claim: crate::store::WorkerClaim,
) -> Result<()> {
    let spec = ChildSpec {
        environment: launcher::child_environment(&claim.account),
        cwd: claim.account.cwd.clone(),
        account: claim.account.clone(),
        request: claim.request.clone(),
        canonical_exec_required: claim.canonical_exec_required,
        umask: 0o077,
    };
    fault_point(root, job, FaultPoint::BeforeSpawn);
    let spawned = match launcher::spawn_child(&spec) {
        Ok(child) => child,
        Err(ChildSpawnError::NotStarted) => {
            return retry(|| store.finish(job, worker, ExitOutcome::NotStarted))
        }
        Err(ChildSpawnError::Uncertain) => {
            return retry(|| {
                store.mark_unrecoverable(
                    job,
                    InvocationState::Launching,
                    InvocationState::UnknownLaunch,
                )
            })
        }
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        supervise(root, keys, job, store, worker, claim, spawned)
    }));
    match result {
        Ok(Ok(())) => Ok(()),
        _ => {
            // ChildOwner cleaned up first, while the job lock still proves exclusive ownership.
            let _ = mark_worker_failure(store, job);
            Err(EngineError::RecoveryUncertain)
        }
    }
}
fn mark_worker_failure(store: &mut Store, job: JobId) -> Result<()> {
    retry(|| {
        // A failure before mark_running leaves Launching. Choose from a fresh
        // state each attempt instead of retrying a mismatched CAS for 30 seconds.
        let expected = store.job_state(job)?;
        let terminal = match expected {
            InvocationState::Launching => InvocationState::UnknownLaunch,
            InvocationState::Running => InvocationState::Lost,
            InvocationState::Exited | InvocationState::UnknownLaunch | InvocationState::Lost => {
                return Ok(())
            }
            InvocationState::Accepted => return Err(EngineError::RecoveryUncertain),
        };
        store.mark_unrecoverable(job, expected, terminal)
    })
}

pub(crate) fn worker_main(args: Vec<OsString>) -> ExitCode {
    STOP.store(false, Ordering::Relaxed);
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = stop_handler as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        for signal in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            libc::sigaction(signal, &action, std::ptr::null_mut());
        }
    }
    if run(args).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests;
