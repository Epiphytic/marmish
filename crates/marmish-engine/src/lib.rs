//! Offline, same-user durable command engine.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("marmish-engine supports Linux and macOS only");
mod admission;
mod cipher;
mod cleanup;
mod fault;
mod launcher;
mod model;
mod platform;
mod policy;
mod spool;
mod storage;
mod store;
mod worker;
use admission::{context_owner, random_bytes, sha256};
pub use cleanup::CleanupGuard;
use launcher::{FrontendGuard, LaunchSpec, StateRoot, WorkerPresence};
pub use model::*;
pub use policy::{CommandPolicy, KeyRestrictions, Restrictions};
use std::collections::HashMap;
use std::time::Duration;
pub use storage::{NativeBinding, ProvidedMaster};
use store::{AccountSpec, BootTime, InputOwner, StartDecision, Store, WorkerCredential};

/// A single local frontend. Detached workers continue after this value is dropped.
pub struct Engine {
    store: Store,
    keys: cipher::StorageKeys,
    storage: storage::StorageMode,
    root: StateRoot,
    _guard: FrontendGuard,
    policy: EnginePolicy,
    account: AccountSpec,
    epoch: [u8; 16],
    deadlines: HashMap<[u8; 32], BootTime>,
    orphan_cursor: Option<JobId>,
}

enum Readiness {
    Ready,
    Pending,
    Initializing,
}
enum ReconcileFailure {
    Job(EngineError),
    Global(EngineError),
}
impl From<EngineError> for ReconcileFailure {
    fn from(error: EngineError) -> Self {
        Self::Global(error)
    }
}

impl Engine {
    /// Consume a stopped frontend and prove that its private state can be removed.
    /// Keep the returned guard alive until removal finishes. Failure leaves state intact.
    pub fn into_cleanup(mut self) -> Result<CleanupGuard> {
        self.check_uid()?;
        let locks = cleanup::acquire(&self.root, &mut self.store)?;
        let Self {
            store,
            keys,
            root,
            _guard,
            ..
        } = self;
        drop(store);
        drop(keys);
        Ok(CleanupGuard::new(root, _guard, locks))
    }

    pub fn open(policy: EnginePolicy) -> Result<Self> {
        Self::open_storage(policy, None)
    }

    pub fn open_with_master(
        policy: EnginePolicy,
        binding: NativeBinding,
        master: ProvidedMaster,
    ) -> Result<Self> {
        Self::open_storage(policy, Some((binding, master)))
    }

    pub fn prepare_native_migration(
        policy: EnginePolicy,
        binding: NativeBinding,
        master: ProvidedMaster,
    ) -> Result<CleanupGuard> {
        admission::validate_policy(&policy)?;
        let root = StateRoot::open_existing(&policy.state_root)?;
        let guard = root.lock_frontend()?;
        root.validate_migration(&binding, &master.0)?;
        let keys = cipher::StorageKeys(master.0);
        let fingerprint = sha256(keys.0.as_ref());
        let mut store = Store::open_worker(&root, &keys)?;
        let locks = cleanup::acquire(&root, &mut store)?;
        drop(store);
        drop(keys);
        Ok(CleanupGuard::migration(
            root,
            guard,
            locks,
            binding,
            fingerprint,
        ))
    }

    fn open_storage(
        mut policy: EnginePolicy,
        provided: Option<(NativeBinding, ProvidedMaster)>,
    ) -> Result<Self> {
        let account = admission::validate_policy(&policy)?;
        policy.worker_executable = launcher::worker_preconditions(&policy.worker_executable)?;
        let root = StateRoot::open(&policy.state_root)?;
        let guard = root.lock_frontend()?;
        let (keys, storage) = match provided {
            None => (
                root.initialize_storage(&guard)?,
                storage::StorageMode::Files,
            ),
            Some((binding, master)) => {
                root.initialize_native(&guard, &binding)?;
                (
                    cipher::StorageKeys(master.0),
                    storage::StorageMode::Native(binding),
                )
            }
        };
        let epoch = random_bytes()?;
        let store = Store::create_frontend(&root, &keys, &policy, &account, epoch)?;
        let mut engine = Self {
            store,
            keys,
            storage,
            root,
            _guard: guard,
            policy,
            account,
            epoch,
            deadlines: HashMap::new(),
            orphan_cursor: None,
        };
        for pending in engine.store.pending_jobs(128)? {
            if matches!(
                pending.state,
                InvocationState::Accepted | InvocationState::Launching | InvocationState::Running
            ) {
                engine.reconcile_background(pending.job)?;
            }
        }
        Ok(engine)
    }

    pub fn authorize(
        &self,
        principal: PrincipalId,
        endpoint: EndpointId,
        server: ServerId,
        target_uid: u32,
    ) -> Result<AuthorizedContext> {
        self.check_uid()?;
        admission::authorize(
            &self.policy,
            &self.account,
            principal,
            endpoint,
            server,
            target_uid,
        )
    }

    fn check_uid(&self) -> Result<()> {
        launcher::ensure_uid(self.account.uid)
    }

    fn owner(&self, ctx: &AuthorizedContext) -> Result<InputOwner> {
        self.check_uid()?;
        context_owner(&self.policy, &self.account, ctx)
    }

    pub fn issue_execute_token(&mut self, ctx: &AuthorizedContext) -> Result<ExecuteToken> {
        let owner = self.owner(ctx)?;
        let now = launcher::boot_time()?;
        self.deadlines
            .retain(|_, d| d.boot_id == now.boot_id && d.millis > now.millis);
        self.store.reap_tokens(now, 128)?;
        let token = ExecuteToken::from_bytes(random_bytes()?);
        let hash = token_hash(&token);
        let expires = deadline(now, self.policy.limits.token_lifetime)?;
        match self
            .store
            .issue_token(&owner, self.epoch, hash, now, expires)
        {
            Err(EngineError::QuotaExceeded) => {
                self.maintain(128)?;
                self.store
                    .issue_token(&owner, self.epoch, hash, now, expires)?;
            }
            other => other?,
        }
        self.deadlines.insert(hash, expires);
        Ok(token)
    }

    pub fn submit(
        &mut self,
        ctx: &AuthorizedContext,
        token: &ExecuteToken,
        id: InvocationId,
        request: &CommandRequest,
    ) -> Result<Submission> {
        let owner = self.owner(ctx)?;
        let hash = token_hash(token);
        admission::validate_request(request, true)?;
        if let Some(existing) = self.store.check_existing(&owner, hash, id, request)? {
            let job = self.store.job_for(&owner.owner, id)?;
            self.await_worker(job)?;
            return Ok(Submission {
                status: self.store.status(&owner.owner, job)?,
                ..existing
            });
        }
        if policy::check_command(&self.policy, ctx.principal, request)? {
            policy::require_canonical_exec(request)?;
        }
        admission::validate_request(request, self.policy.allow_shell_command)?;
        launcher::worker_preconditions(&self.policy.worker_executable)?;
        let now = launcher::boot_time()?;
        let expires = self.deadlines.get(&hash).ok_or(EngineError::InvalidToken)?;
        if expires.boot_id != now.boot_id || expires.millis <= now.millis {
            return Err(EngineError::ExpiredToken);
        }
        match self.store.preflight_admit(&owner, self.epoch, hash, now) {
            Err(EngineError::QuotaExceeded) => {
                self.maintain(128)?;
                self.store
                    .preflight_admit(&owner, self.epoch, hash, launcher::boot_time()?)?;
            }
            other => other?,
        }
        // Bounded orphan inspection prevents accumulating directories after crashes.
        self.reap_orphans(128)?;
        let job = JobId(random_bytes()?);
        let credential = WorkerCredential(random_bytes()?);
        if let Err(error) = self.root.prepare_job(job, &credential) {
            if self.root.job_directory_exists(job)? {
                self.remove_orphan(job)?;
            }
            return Err(error);
        }
        let admission = (|| {
            self.store.admit(
                &owner,
                self.epoch,
                hash,
                launcher::boot_time()?,
                id,
                request,
                job,
                sha256(&credential.0),
            )
        })();
        match admission {
            Ok(submission) if submission.disposition == SubmitDisposition::Accepted => {
                self.deadlines.remove(&hash);
                self.await_worker(job)?;
                Ok(Submission {
                    status: self.store.status(&owner.owner, job)?,
                    ..submission
                })
            }
            other => {
                self.remove_orphan(job)?;
                other
            }
        }
    }

    fn authorized_job(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
    ) -> Result<(InputOwner, JobId)> {
        let owner = self.owner(ctx)?;
        let job = self.store.job_for(&owner.owner, id)?;
        self.reconcile_step(job).map_err(|error| match error {
            ReconcileFailure::Job(error) | ReconcileFailure::Global(error) => error,
        })?;
        Ok((owner, job))
    }

    pub fn status(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
    ) -> Result<InvocationStatus> {
        let (owner, job) = self.authorized_job(ctx, id)?;
        self.store.status(&owner.owner, job)
    }

    pub fn begin_input(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
    ) -> Result<InputStatus> {
        let (owner, job) = self.authorized_job(ctx, id)?;
        self.store
            .begin_input(job, &owner, InputGeneration(random_bytes()?))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_stdin(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
        generation: InputGeneration,
        offset: u64,
        bytes: &[u8],
        fresh_for: Duration,
    ) -> Result<InputReceipt> {
        let (owner, job) = self.authorized_job(ctx, id)?;
        if fresh_for < Duration::from_millis(1) || fresh_for > Duration::from_secs(60) {
            return Err(EngineError::InvalidRequest);
        }
        let now = launcher::boot_time()?;
        self.store.queue_input(
            job,
            &owner,
            generation,
            offset,
            bytes,
            now,
            deadline(now, fresh_for)?,
        )
    }

    pub fn stdin_eof(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
        generation: InputGeneration,
        final_offset: u64,
    ) -> Result<InputStatus> {
        let (owner, job) = self.authorized_job(ctx, id)?;
        self.store.queue_eof(job, &owner, generation, final_offset)
    }

    pub fn read_output(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
        stream: Stream,
        cursor: u64,
        limit: usize,
    ) -> Result<OutputChunk> {
        let (owner, job) = self.authorized_job(ctx, id)?;
        if limit == 0 || limit > 16 * 1024 {
            return Err(EngineError::InvalidRequest);
        }
        if !self.root.job_directory_exists(job)? {
            let status = self.store.output_status(&owner.owner, job, stream)?;
            if !status.closed || status.retained != 0 {
                return Err(EngineError::RecoveryUncertain);
            }
            let next_cursor = status
                .gaps
                .iter()
                .find(|g| g.start <= cursor && g.end.is_none_or(|end| cursor < end))
                .map_or(cursor, |g| g.end.unwrap_or(g.start));
            return Ok(OutputChunk {
                bytes: Vec::new(),
                next_cursor,
                status,
            });
        }
        let guard = self.root.lock_io(job)?;
        let status = self.store.output_status(&owner.owner, job, stream)?;
        spool::Spool::read(
            &self.root, &self.keys, job, stream, &status, cursor, limit, &guard,
        )
    }

    pub fn cancel(
        &mut self,
        ctx: &AuthorizedContext,
        id: InvocationId,
    ) -> Result<InvocationStatus> {
        // Cancel Accepted work before any reconciliation can start it.
        let owner = self.owner(ctx)?;
        let job = self.store.job_for(&owner.owner, id)?;
        self.store.request_cancel(&owner.owner, job)?;
        self.store.status(&owner.owner, job)
    }

    fn reconcile_background(&mut self, job: JobId) -> Result<()> {
        match self.reconcile_step(job) {
            Ok(_) | Err(ReconcileFailure::Job(_)) => Ok(()),
            Err(ReconcileFailure::Global(error)) => Err(error),
        }
    }

    fn await_worker(&mut self, job: JobId) -> Result<()> {
        let deadline = platform::Deadline::after_millis(8_000)?;
        loop {
            let readiness = match self.reconcile_step(job) {
                Ok(readiness) => readiness,
                Err(ReconcileFailure::Job(EngineError::RecoveryUncertain)) => {
                    Readiness::Initializing
                }
                Err(ReconcileFailure::Job(error) | ReconcileFailure::Global(error)) => {
                    return Err(error)
                }
            };
            match readiness {
                Readiness::Ready => return Ok(()),
                Readiness::Pending if deadline.expired()? => {
                    return Err(EngineError::WorkerUnavailable)
                }
                Readiness::Initializing if deadline.expired()? => {
                    return Err(EngineError::RecoveryUncertain)
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }

    /// One job-local observation/start/CAS, with no readiness sleeps.
    fn reconcile_step(&mut self, job: JobId) -> std::result::Result<Readiness, ReconcileFailure> {
        let state = self.store.job_state(job)?;
        if matches!(
            state,
            InvocationState::Exited | InvocationState::UnknownLaunch | InvocationState::Lost
        ) {
            return Ok(Readiness::Ready);
        }
        let identity = self.store.worker_identity(job)?;
        let presence = self
            .root
            .presence(job, identity.as_ref())
            .map_err(|error| match error {
                EngineError::UnsafePath => ReconcileFailure::Job(error),
                other => ReconcileFailure::Global(other),
            })?;
        match presence {
            WorkerPresence::Live(live) => {
                if identity.as_ref() != Some(&live) {
                    return Err(ReconcileFailure::Job(EngineError::RecoveryUncertain));
                }
                Ok(Readiness::Ready)
            }
            WorkerPresence::Initializing => Ok(if state == InvocationState::Accepted {
                Readiness::Initializing
            } else {
                Readiness::Ready
            }),
            WorkerPresence::Absent => {
                self.recover_absent(job, state)?;
                Ok(if self.store.job_state(job)? == InvocationState::Accepted {
                    Readiness::Pending
                } else {
                    Readiness::Ready
                })
            }
        }
    }

    // Only Accepted jobs may start a worker. A missing worker after claim is
    // terminal uncertainty: the command must never be launched a second time.
    fn recover_absent(&mut self, job: JobId, state: InvocationState) -> Result<bool> {
        match state {
            InvocationState::Accepted => self.start_accepted(job),
            InvocationState::Launching | InvocationState::Running => {
                let terminal = if state == InvocationState::Launching {
                    InvocationState::UnknownLaunch
                } else {
                    InvocationState::Lost
                };
                match self.store.mark_unrecoverable(job, state, terminal) {
                    Ok(()) | Err(EngineError::Busy) => Ok(false),
                    Err(error) => Err(error),
                }
            }
            _ => Ok(false),
        }
    }

    fn start_accepted(&mut self, job: JobId) -> Result<bool> {
        let spec = LaunchSpec {
            root: self.root.path().to_owned(),
            job,
        };
        let prepared = launcher::prepare_worker(&self.policy.worker_executable, &spec)?;
        prepared.verify_root(&self.root)?;
        let decision = match self.store.reserve_worker_start(job, launcher::boot_time()?) {
            Err(EngineError::Busy) => return Ok(false),
            other => other?,
        };
        match decision {
            StartDecision::Reserved => match prepared.spawn(&self.keys, &self.storage) {
                Ok(()) => Ok(true),
                Err(launcher::WorkerSpawnError::Transient) => {
                    self.store.release_worker_start(job)?;
                    Ok(false)
                }
                Err(launcher::WorkerSpawnError::Global(error)) => {
                    self.store.refund_worker_start(job)?;
                    Err(error)
                }
            },
            StartDecision::Deferred | StartDecision::FinishedNotStarted => Ok(false),
        }
    }

    pub fn maintain(&mut self, max_jobs: u32) -> Result<MaintenanceReport> {
        self.check_uid()?;
        if !(1..=128).contains(&max_jobs) {
            return Err(EngineError::InvalidRequest);
        }
        launcher::worker_preconditions(&self.policy.worker_executable)?;
        let now = launcher::boot_time()?;
        self.store.reap_tokens(now, max_jobs)?;
        // The page cursor is committed before processing. A late global error
        // leaves it advanced; the next page proceeds, then the cursor wraps.
        for pending in self.store.pending_jobs(max_jobs)? {
            if matches!(
                pending.state,
                InvocationState::Accepted | InvocationState::Launching | InvocationState::Running
            ) {
                self.reconcile_background(pending.job)?;
            }
        }
        // Sample the clock after taking the transaction: reconciliation or a
        // concurrent worker may have advanced the durable anchor meanwhile.
        let logical = self.store.retention_now_current()?;
        let mut report = MaintenanceReport {
            records_removed: 0,
            spool_bytes_removed: 0,
        };
        for candidate in self.store.cleanup_candidates(logical, max_jobs)? {
            // A crash may have removed the expired directory before deleting its
            // durable tombstone. Never recreate that directory during recovery.
            if candidate.delete_record && !self.root.job_directory_exists(candidate.job)? {
                self.store.delete_tombstone(candidate.job)?;
                report.records_removed += 1;
                continue;
            }
            let _job_guard = match self.root.lock_job(candidate.job) {
                Err(EngineError::Busy) => continue,
                other => other?,
            };
            let guard = match self.root.lock_io(candidate.job) {
                Err(EngineError::Busy) => continue,
                other => other?,
            };
            self.root.expire_spool(candidate.job, &guard)?;
            report.spool_bytes_removed += self.store.expire_output(candidate.job)?;
            if candidate.delete_record {
                self.root.remove_job(candidate.job, &guard)?;
                self.store.delete_tombstone(candidate.job)?;
                report.records_removed += 1;
            }
        }
        self.reap_orphans(max_jobs)?;
        Ok(report)
    }

    fn remove_orphan(&mut self, job: JobId) -> Result<()> {
        // Admission can be ambiguous after a storage error. Never repair locks
        // or remove a directory once its durable job record exists.
        if self.store.job_exists(job)? {
            return Err(EngineError::RecoveryUncertain);
        }
        let (_job_guard, guard) = self.root.lock_orphan_job(job, &self._guard)?;
        self.root.remove_job(job, &guard)
    }

    fn reap_orphans(&mut self, limit: u32) -> Result<()> {
        let page = self.root.list_job_dirs(limit, self.orphan_cursor)?;
        self.orphan_cursor = page.next;
        let mut orphans = 0;
        for job in page.jobs {
            if !self.store.job_exists(job)? {
                orphans += 1;
                match self.remove_orphan(job) {
                    Err(EngineError::Busy) => {}
                    other => other?,
                }
            }
        }
        if orphans >= 128 {
            return Err(EngineError::QuotaExceeded);
        }
        Ok(())
    }
}

fn token_hash(token: &ExecuteToken) -> [u8; 32] {
    let bytes = zeroize::Zeroizing::new(token.expose_bytes());
    sha256(bytes.as_ref())
}

fn deadline(now: BootTime, duration: Duration) -> Result<BootTime> {
    let delta = u64::try_from(duration.as_millis()).map_err(|_| EngineError::InvalidRequest)?;
    Ok(BootTime {
        boot_id: now.boot_id,
        millis: now
            .millis
            .checked_add(delta)
            .ok_or(EngineError::InvalidRequest)?,
    })
}
#[doc(hidden)]
pub fn worker_main(args: Vec<std::ffi::OsString>) -> std::process::ExitCode {
    worker::worker_main(args)
}
