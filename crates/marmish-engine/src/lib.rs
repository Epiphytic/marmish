//! Offline, same-user durable command engine.
#![cfg(target_os = "linux")]
mod admission;
mod cipher;
mod fault;
mod launcher;
mod model;
mod spool;
mod store;
mod worker;
use admission::{context_owner, random_bytes, sha256};
use launcher::{FrontendGuard, LaunchSpec, StateRoot, WorkerPresence};
pub use model::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use store::{AccountSpec, BootTime, InputOwner, StartDecision, Store, WorkerCredential};

/// A single local frontend. Detached workers continue after this value is dropped.
pub struct Engine {
    store: Store,
    keys: cipher::StorageKeys,
    root: StateRoot,
    _guard: FrontendGuard,
    policy: EnginePolicy,
    account: AccountSpec,
    epoch: [u8; 16],
    deadlines: HashMap<[u8; 32], BootTime>,
    orphan_cursor: Option<JobId>,
}

impl Engine {
    pub fn open(mut policy: EnginePolicy) -> Result<Self> {
        let account = admission::validate_policy(&policy)?;
        policy.worker_executable = launcher::trusted_worker_executable(&policy.worker_executable)?;
        let root = StateRoot::open(&policy.state_root)?;
        let guard = root.lock_frontend()?;
        let keys = root.initialize_storage(&guard)?;
        let epoch = random_bytes()?;
        let store = Store::create_frontend(&root, &keys, &policy, &account, epoch)?;
        let mut engine = Self {
            store,
            keys,
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
                engine.reconcile(pending.job)?;
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
        admission::validate_request(request, self.policy.allow_shell_command)?;
        if let Some(existing) = self.store.check_existing(&owner, hash, id, request)? {
            let job = self.store.job_for(&owner.owner, id)?;
            self.reconcile(job)?;
            return Ok(Submission {
                status: self.store.status(&owner.owner, job)?,
                ..existing
            });
        }
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
                self.reconcile(job)?;
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
        self.reconcile(job)?;
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
        let (owner, job) = self.authorized_job(ctx, id)?;
        self.store.request_cancel(&owner.owner, job)?;
        self.store.status(&owner.owner, job)
    }

    fn reconcile(&mut self, job: JobId) -> Result<()> {
        let until = Instant::now() + Duration::from_secs(2);
        let mut started = false;
        loop {
            let state = self.store.job_state(job)?;
            if matches!(
                state,
                InvocationState::Exited | InvocationState::UnknownLaunch | InvocationState::Lost
            ) {
                return Ok(());
            }
            let identity = self.store.worker_identity(job)?;
            match self.root.presence(job, identity.as_ref())? {
                WorkerPresence::Live(live) => {
                    if identity.as_ref() != Some(&live) {
                        return Err(EngineError::RecoveryUncertain);
                    }
                    return Ok(());
                }
                WorkerPresence::Initializing => {
                    if Instant::now() >= until {
                        return Err(EngineError::RecoveryUncertain);
                    }
                }
                WorkerPresence::Absent => {
                    if state == InvocationState::Accepted && (started || Instant::now() >= until) {
                        return Ok(());
                    }
                    match self.recover_absent(job, state) {
                        Ok(false) => return Ok(()),
                        Ok(true) => started = true,
                        // A worker may have committed a legitimate transition
                        // between our snapshot and the recovery CAS. Reread it.
                        Err(EngineError::Busy) if Instant::now() < until => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
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
                self.store.mark_unrecoverable(job, state, terminal)?;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn start_accepted(&mut self, job: JobId) -> Result<bool> {
        match self
            .store
            .reserve_worker_start(job, launcher::boot_time()?)?
        {
            StartDecision::Reserved => {
                let spec = LaunchSpec {
                    root: self.root.path().to_owned(),
                    job,
                };
                if launcher::spawn_worker(&self.policy.worker_executable, &spec).is_err() {
                    self.store.release_worker_start(job)?;
                    return Ok(false);
                }
                Ok(true)
            }
            StartDecision::Deferred | StartDecision::FinishedNotStarted => Ok(false),
        }
    }

    pub fn maintain(&mut self, max_jobs: u32) -> Result<MaintenanceReport> {
        self.check_uid()?;
        if !(1..=128).contains(&max_jobs) {
            return Err(EngineError::InvalidRequest);
        }
        let now = launcher::boot_time()?;
        self.store.reap_tokens(now, max_jobs)?;
        for pending in self.store.pending_jobs(max_jobs)? {
            if matches!(
                pending.state,
                InvocationState::Accepted | InvocationState::Launching | InvocationState::Running
            ) {
                self.reconcile(pending.job)?;
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
