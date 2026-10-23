use crate::cipher::db_error;
use crate::{
    admission::{sha256, validate_request},
    fault::{fault_point, FaultPoint},
    launcher::{self, StateRoot},
    model::*,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
#[cfg(test)]
use std::time::Duration;
use std::{fmt, path::PathBuf};
use zeroize::Zeroize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OwnerKey {
    pub principal: PrincipalId,
    pub server: ServerId,
    pub target_uid: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct InputOwner {
    pub owner: OwnerKey,
    pub endpoint: EndpointId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BootTime {
    pub boot_id: [u8; 16],
    pub millis: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProcessIdentity {
    pub boot_id: [u8; 16],
    pub pid: i32,
    pub start_ticks: u64,
}
#[derive(Clone)]
pub(crate) struct WorkerCredential(pub [u8; 32]);
impl Drop for WorkerCredential {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl fmt::Debug for WorkerCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WorkerCredential([REDACTED])")
    }
}
#[derive(Clone, Debug)]
pub(crate) struct WorkerClaim {
    pub job: JobId,
    pub request: CommandRequest,
    pub account: AccountSpec,
    pub limits: EngineLimits,
    pub canonical_exec_required: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AccountSpec {
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
    pub shell: PathBuf,
    pub cwd: PathBuf,
    pub user: Vec<u8>,
}
#[derive(Clone, Debug)]
pub(crate) struct PendingJob {
    pub job: JobId,
    pub state: InvocationState,
}
#[derive(Clone, Debug)]
pub(crate) enum StartDecision {
    Reserved,
    Deferred,
    FinishedNotStarted,
}
#[derive(Clone, Debug)]
pub(crate) struct OrphanPage {
    pub jobs: Vec<JobId>,
    pub next: Option<JobId>,
}
#[allow(clippy::large_enum_variant)] // Frozen private contract transfers one claim without indirection.
#[derive(Clone, Debug)]
pub(crate) enum ClaimResult {
    Claimed(WorkerClaim),
    AlreadyClaimed,
}
#[derive(Clone)]
pub(crate) struct Delivery {
    pub record: i64,
    pub generation: InputGeneration,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub deadline: BootTime,
}
impl fmt::Debug for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delivery")
            .field("record", &self.record)
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}
#[derive(Clone, Debug)]
pub(crate) enum InputWork {
    Idle,
    Deliver(Delivery),
    Eof,
    Failed,
}
#[derive(Clone, Debug)]
pub(crate) struct Controls {
    pub cancel_requested: bool,
    pub generation: Option<InputGeneration>,
}
#[derive(Clone, Debug)]
pub(crate) struct OutputCommit {
    pub retained: u64,
    pub produced: u64,
    pub closed: bool,
    pub gaps: Vec<MissingRange>,
}
#[derive(Clone, Debug)]
pub(crate) struct CleanupCandidate {
    pub job: JobId,
    pub delete_record: bool,
}

#[derive(Serialize, Deserialize)]
struct Config {
    version: u32,
    policy: EnginePolicy,
    account: AccountSpec,
    epoch: [u8; 16],
}
impl Config {
    fn validate(&self) -> Result<()> {
        if !matches!(self.version, 1 | 2)
            || (self.version == 1 && self.policy.command_policy.is_some())
        {
            return Err(EngineError::StorageFailure);
        }
        if let Some(policy) = &self.policy.command_policy {
            policy
                .validate(&self.policy.allowed_principals)
                .map_err(|_| EngineError::StorageFailure)?;
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize, Default)]
struct Clock {
    boot: [u8; 16],
    millis: u64,
    seconds: u64,
    remainder: u64,
}
#[derive(Serialize, Deserialize)]
struct Token {
    owner: InputOwner,
    epoch: [u8; 16],
    issued: BootTime,
    expires: BootTime,
    consumed: bool,
}
#[derive(Serialize, Deserialize)]
struct InputRecord {
    id: i64,
    offset: u64,
    length: u32,
    hash: [u8; 32],
    bytes: Option<Vec<u8>>,
    deadline: BootTime,
    written: u32,
    delivering: bool,
}
#[derive(Serialize, Deserialize)]
struct Input {
    generation: Option<InputGeneration>,
    endpoint: Option<EndpointId>,
    accepted: u64,
    delivered: u64,
    health: InputHealth,
    gaps: u64,
    eof: Option<u64>,
    next_record: i64,
    records: Vec<InputRecord>,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            generation: None,
            endpoint: None,
            accepted: 0,
            delivered: 0,
            health: InputHealth::Open,
            gaps: 0,
            eof: None,
            next_record: 1,
            records: Vec::new(),
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Job {
    id: JobId,
    owner: OwnerKey,
    endpoint: EndpointId,
    invocation: InvocationId,
    digest: [u8; 32],
    token: [u8; 32],
    credential: [u8; 32],
    limits: EngineLimits,
    account: AccountSpec,
    request: Option<CommandRequest>,
    #[serde(default)]
    canonical_exec_required: bool,
    state: InvocationState,
    outcome: Option<ExitOutcome>,
    worker: Option<ProcessIdentity>,
    child: Option<ProcessIdentity>,
    pgid: Option<i32>,
    cancel: bool,
    attempts: u32,
    reserved: Option<BootTime>,
    input: Input,
    stdout: OutputStatus,
    stderr: OutputStatus,
    completed: Option<u64>,
}
impl Job {
    fn output(&self, s: Stream) -> &OutputStatus {
        match s {
            Stream::Stdout => &self.stdout,
            Stream::Stderr => &self.stderr,
        }
    }
    fn output_mut(&mut self, s: Stream) -> &mut OutputStatus {
        match s {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
        }
    }
    fn terminal(&self) -> bool {
        matches!(
            self.state,
            InvocationState::Exited | InvocationState::UnknownLaunch | InvocationState::Lost
        )
    }
    fn input_status(&self) -> InputStatus {
        InputStatus {
            generation: self.input.generation,
            accepted: self.input.accepted,
            delivered: self.input.delivered,
            available_credit: credit(&self.input),
            health: self.input.health,
            gap_count: self.input.gaps,
        }
    }
    fn status(&self) -> InvocationStatus {
        InvocationStatus {
            invocation: self.invocation,
            state: self.state,
            outcome: self.outcome.clone(),
            cancel_requested: self.cancel,
            input: self.input_status(),
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        }
    }
    fn verify_owner(&self, owner: &OwnerKey) -> Result<()> {
        if &self.owner != owner {
            Err(EngineError::NotFound)
        } else {
            Ok(())
        }
    }
    fn verify_worker(&self, worker: &ProcessIdentity) -> Result<()> {
        if self.worker.as_ref() != Some(worker) || self.terminal() {
            Err(EngineError::WorkerUnavailable)
        } else {
            Ok(())
        }
    }
    fn purge(&mut self) {
        self.request = None;
        for r in &mut self.input.records {
            r.bytes = None;
        }
        self.reserved = None;
    }
}

pub(crate) struct Store {
    conn: Connection,
    root: StateRoot,
    config: Config,
}
fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|_| EngineError::StorageFailure)
}
fn decode<T: DeserializeOwned>(v: &[u8]) -> Result<T> {
    if v.len() > 2 * 1024 * 1024 {
        return Err(EngineError::StorageFailure);
    }
    serde_json::from_slice(v).map_err(|_| EngineError::StorageFailure)
}
fn load(conn: &Connection, id: JobId) -> Result<Job> {
    let bytes: Option<Vec<u8>> = conn
        .query_row(
            "SELECT data FROM jobs WHERE job=? AND length(data)<=2097152",
            params![id.0.as_slice()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    decode(&bytes.ok_or(EngineError::NotFound)?)
}
fn save(conn: &Connection, j: &Job) -> Result<()> {
    let reserved_boot = j.reserved.map(|d| d.boot_id.to_vec());
    let reserved_ms = j.reserved.map(|d| sql_integer(d.millis)).transpose()?;
    let used = [&j.stdout, &j.stderr]
        .into_iter()
        .try_fold(0u64, |sum, out| {
            sum.checked_add(if out.closed {
                out.retained
            } else {
                j.limits.spool_bytes_per_stream
            })
            .ok_or(EngineError::StorageFailure)
        })?;
    conn.execute("UPDATE jobs SET state=?,data=?,reserved_boot=?,reserved_ms=?,spool_used=?,retained=?,completed=?,ttl=?,tombstone=? WHERE job=?",
      params![state_num(j.state),encode(j)?,reserved_boot,reserved_ms,sql_integer(used)?,sql_integer(j.stdout.retained+j.stderr.retained)?,j.completed.map(sql_integer).transpose()?,j.limits.output_retention.seconds().map(sql_integer).transpose()?,sql_integer(j.limits.tombstone_retention.as_secs())?,j.id.0.as_slice()]).map_err(db_error)?;
    Ok(())
}
fn state_num(s: InvocationState) -> i32 {
    match s {
        InvocationState::Accepted => 0,
        InvocationState::Launching => 1,
        InvocationState::Running => 2,
        InvocationState::Exited => 3,
        InvocationState::UnknownLaunch => 4,
        InvocationState::Lost => 5,
    }
}
fn same_boot_before(now: BootTime, deadline: BootTime) -> bool {
    now.boot_id == deadline.boot_id && now.millis < deadline.millis
}
fn credit(i: &Input) -> u32 {
    let count = i.records.len();
    if count >= 256 || !matches!(i.health, InputHealth::Open) {
        return 0;
    }
    let bytes: u64 = i
        .records
        .iter()
        .filter(|r| r.bytes.is_some())
        .map(|r| u64::from(r.length - r.written))
        .sum();
    (65536u64.saturating_sub(bytes)) as u32
}
fn input_gap(i: &mut Input, uncertain: bool) {
    i.health = if uncertain {
        InputHealth::Uncertain
    } else {
        InputHealth::Gap
    };
    i.gaps = i.gaps.saturating_add(1);
    for r in &mut i.records {
        r.bytes = None;
    }
}
// A declared EOF still closes the OS pipe when preceding bytes were lost.
// Ordinary gaps remain rotatable and must not close the pipe.
fn failed_input_work(input: &Input) -> InputWork {
    if input.eof.is_some() {
        InputWork::Eof
    } else {
        InputWork::Failed
    }
}
fn input_expired(i: &Input, now: BootTime) -> bool {
    i.records
        .iter()
        .any(|r| r.bytes.is_some() && !same_boot_before(now, r.deadline))
}
fn expire_input(i: &mut Input, now: BootTime) {
    if input_expired(i, now) {
        input_gap(i, false);
    }
}
fn terminal_job(j: &mut Job, outcome: Option<ExitOutcome>, state: InvocationState, now: u64) {
    j.state = state;
    j.outcome = outcome;
    j.completed = Some(now);
    j.purge();
    j.stdout.closed = true;
    j.stderr.closed = true;
    if !matches!(j.input.health, InputHealth::Gap | InputHealth::Uncertain) {
        j.input.health = InputHealth::Closed;
    }
}
fn clock_now(conn: &Connection, clock: BootTime, wall: u64) -> Result<u64> {
    let bytes: Vec<u8> = conn
        .query_row("SELECT data FROM meta WHERE name='clock'", [], |r| r.get(0))
        .map_err(db_error)?;
    let mut anchor: Clock = decode(&bytes)?;
    if anchor.boot == clock.boot_id {
        if clock.millis < anchor.millis {
            return Err(EngineError::RecoveryUncertain);
        }
        let elapsed = clock.millis - anchor.millis;
        let total = elapsed
            .checked_add(anchor.remainder)
            .ok_or(EngineError::StorageFailure)?;
        anchor.seconds = anchor
            .seconds
            .checked_add(total / 1000)
            .ok_or(EngineError::StorageFailure)?;
        anchor.remainder = total % 1000;
    } else {
        anchor.seconds = anchor.seconds.max(wall);
        anchor.remainder = 0;
        anchor.boot = clock.boot_id;
    }
    anchor.millis = clock.millis;
    conn.execute(
        "UPDATE meta SET data=? WHERE name='clock'",
        params![encode(&anchor)?],
    )
    .map_err(db_error)?;
    Ok(anchor.seconds)
}
fn logical_now(conn: &Connection) -> Result<u64> {
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| EngineError::RecoveryUncertain)?
        .as_secs();
    clock_now(conn, launcher::boot_time()?, wall)
}

// Inspect existing authenticated state before any schema or orphan recovery mutation.
fn validate_frontend_schema(conn: &Connection, root: &StateRoot) -> Result<()> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_error)?;
    if version != 0 && version != 1 {
        return Err(EngineError::StorageFailure);
    }
    if version == 0 {
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
            .map_err(db_error)?;
        if tables != 0 || !root.storage_is_pristine()? {
            return Err(EngineError::StorageFailure);
        }
    } else {
        // A keyed but damaged schema must never become a new empty engine:
        // that would make surviving job directories look like orphans.
        let config: Vec<u8> = conn
            .query_row("SELECT data FROM meta WHERE name='config'", [], |r| {
                r.get(0)
            })
            .map_err(db_error)?;
        let config: Config = decode(&config)?;
        config.validate()?;
        for table in ["jobs", "tokens"] {
            let present: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
                    [table],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if present != 1 {
                return Err(EngineError::StorageFailure);
            }
        }
    }
    Ok(())
}

impl Store {
    #[cfg(test)]
    pub(crate) fn test_total_changes(&self) -> u64 {
        self.conn.total_changes()
    }
    fn connection(
        root: &StateRoot,
        keys: &crate::cipher::StorageKeys,
        create: bool,
    ) -> Result<Connection> {
        let database_path = root.prepare_db(create)?;
        let conn = crate::cipher::open(
            &database_path,
            keys,
            crate::cipher::Context::State,
            crate::cipher::Journal::Wal,
        )?;
        root.verify_db_files()?;
        Ok(conn)
    }
    pub fn create_frontend(
        root: &StateRoot,
        keys: &crate::cipher::StorageKeys,
        policy: &EnginePolicy,
        account: &AccountSpec,
        epoch: [u8; 16],
    ) -> Result<Self> {
        if account.uid == 0 {
            return Err(EngineError::RootForbidden);
        }
        let real = launcher::local_account(None)?;
        if real.uid != account.uid {
            return Err(EngineError::Unauthorized);
        }
        let mut conn = Self::connection(root, keys, true)?;
        validate_frontend_schema(&conn, root)?;
        fault_point(root, JobId([0; 16]), FaultPoint::BeforeSchemaCreate);
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS meta(name TEXT PRIMARY KEY,data BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS jobs(job BLOB PRIMARY KEY,principal BLOB NOT NULL,server BLOB NOT NULL,target INTEGER NOT NULL,invocation BLOB NOT NULL,state INTEGER NOT NULL,data BLOB NOT NULL,reserved_boot BLOB,reserved_ms INTEGER,spool_used INTEGER NOT NULL DEFAULT 0,retained INTEGER NOT NULL DEFAULT 0,completed INTEGER,ttl INTEGER,tombstone INTEGER NOT NULL DEFAULT 172800,UNIQUE(principal,server,target,invocation));
            CREATE INDEX IF NOT EXISTS jobs_state ON jobs(state,job);
            CREATE TABLE IF NOT EXISTS tokens(hash BLOB PRIMARY KEY,data BLOB NOT NULL,principal BLOB NOT NULL,epoch BLOB NOT NULL,boot BLOB NOT NULL,expires INTEGER NOT NULL,consumed INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS tokens_expiry ON tokens(consumed,boot,expires);
            PRAGMA user_version=1;").map_err(db_error)?;
        let old: Option<Vec<u8>> = tx
            .query_row("SELECT data FROM meta WHERE name='config'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(db_error)?;
        if let Some(bytes) = old {
            let config: Config = decode(&bytes)?;
            config.validate()?;
            if config.policy.server != policy.server || config.account.uid != account.uid {
                return Err(EngineError::Unauthorized);
            }
        }
        let config = Config {
            version: 2,
            policy: policy.clone(),
            account: account.clone(),
            epoch,
        };
        tx.execute(
            "INSERT OR REPLACE INTO meta(name,data) VALUES('config',?)",
            params![encode(&config)?],
        )
        .map_err(db_error)?;
        tx.execute(
            "INSERT OR IGNORE INTO meta(name,data) VALUES('clock',?)",
            params![encode(&Clock::default())?],
        )
        .map_err(db_error)?;
        for cursor in ["pending_cursor", "cleanup_cursor"] {
            tx.execute(
                "INSERT OR REPLACE INTO meta(name,data) VALUES(?,?)",
                params![cursor, Vec::<u8>::new()],
            )
            .map_err(db_error)?;
        }
        // Restart revokes unused authority. Consumed replay bindings stay in jobs.
        tx.execute("DELETE FROM tokens", []).map_err(db_error)?;
        let mut stmt = tx
            .prepare("SELECT job FROM jobs WHERE state<3 ORDER BY job LIMIT 129")
            .map_err(db_error)?;
        let rows: Vec<Vec<u8>> = stmt
            .query_map([], |r| r.get(0))
            .map_err(db_error)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_error)?;
        drop(stmt);
        if rows.len() > 128 {
            return Err(EngineError::StorageFailure);
        }
        for id in rows {
            let mut j = load(&tx, job_id(&id)?)?;
            if j.input.records.iter().any(|r| r.bytes.is_some()) {
                input_gap(&mut j.input, true);
            }
            j.input.generation = None;
            j.input.endpoint = None;
            j.input.records.clear();
            j.input.accepted = 0;
            j.input.delivered = 0;
            // An explicit EOF is irreversible even when restart revokes pending input.
            if j.input.eof.is_some() {
                j.input.eof = Some(0);
            }
            save(&tx, &j)?;
        }
        logical_now(&tx)?;
        tx.commit().map_err(db_error)?;
        fault_point(root, JobId([0; 16]), FaultPoint::AfterSchemaCreate);
        root.verify_db_files()?;
        Ok(Self {
            conn,
            root: root.try_clone()?,
            config,
        })
    }
    pub fn open_worker(root: &StateRoot, keys: &crate::cipher::StorageKeys) -> Result<Self> {
        let account = launcher::local_account(None)?;
        let conn = Self::connection(root, keys, false)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_error)?;
        if version != 1 {
            return Err(EngineError::StorageFailure);
        }
        let bytes: Vec<u8> = conn
            .query_row("SELECT data FROM meta WHERE name='config'", [], |r| {
                r.get(0)
            })
            .map_err(db_error)?;
        let config: Config = decode(&bytes)?;
        config.validate()?;
        if config.account.uid != account.uid || account.uid == 0 {
            return Err(EngineError::Unauthorized);
        }
        root.verify_db_files()?;
        Ok(Self {
            conn,
            root: root.try_clone()?,
            config,
        })
    }
    fn transaction<T>(&mut self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let result = f(&tx)?;
        tx.commit().map_err(db_error)?;
        Ok(result)
    }
    fn mutate<T>(
        &mut self,
        id: JobId,
        f: impl FnOnce(&mut Job, &Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        self.transaction(|tx| {
            let mut j = load(tx, id)?;
            let result = f(&mut j, tx)?;
            save(tx, &j)?;
            Ok(result)
        })
    }
    pub fn issue_token(
        &mut self,
        owner: &InputOwner,
        epoch: [u8; 16],
        token_hash: [u8; 32],
        issued: BootTime,
        expires: BootTime,
    ) -> Result<()> {
        self.reap_tokens(issued, 128)?;
        let config = &self.config;
        if epoch != config.epoch
            || owner.owner.server != config.policy.server
            || owner.owner.target_uid != config.account.uid
            || !config
                .policy
                .allowed_principals
                .contains(&owner.owner.principal)
            || !same_boot_before(issued, expires)
            || expires.millis - issued.millis > 300_000
        {
            return Err(EngineError::InvalidToken);
        }
        let limits = config.policy.limits.clone();
        self.transaction(|tx| {
            let (total,mine):(u32,u32)=tx.query_row("SELECT count(*),coalesce(sum(principal=?),0) FROM tokens WHERE consumed=0 AND boot=? AND expires>?",params![owner.owner.principal.0.as_slice(),issued.boot_id.as_slice(),sql_integer(issued.millis)?],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            let stored:u32=tx.query_row("SELECT count(*) FROM tokens",[],|r|r.get(0)).map_err(db_error)?;
            if stored>=4096 || total>=limits.outstanding_tokens || mine>=limits.outstanding_tokens.min(128) {return Err(EngineError::QuotaExceeded)};
            let t = Token {
                owner: owner.clone(),
                epoch,
                issued,
                expires,
                consumed: false,
            };
            tx.execute(
                "INSERT INTO tokens(hash,data,principal,epoch,boot,expires) VALUES(?,?,?,?,?,?)",
                params![token_hash.as_slice(), encode(&t)?,owner.owner.principal.0.as_slice(),epoch.as_slice(),expires.boot_id.as_slice(),sql_integer(expires.millis)?],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
    pub fn reap_tokens(&mut self, now: BootTime, limit: u32) -> Result<u32> {
        if !(1..=128).contains(&limit) {
            return Err(EngineError::InvalidRequest);
        }
        let epoch = self.config.epoch;
        self.transaction(|tx| {
            let count=tx.execute("DELETE FROM tokens WHERE hash IN (SELECT hash FROM tokens WHERE consumed=1 OR epoch!=? OR boot!=? OR expires<=? ORDER BY hash LIMIT ?)",params![epoch.as_slice(),now.boot_id.as_slice(),sql_integer(now.millis)?,limit]).map_err(db_error)?;
            Ok(count as u32)
        })
    }
    pub fn check_existing(
        &mut self,
        owner: &InputOwner,
        token_hash: [u8; 32],
        id: InvocationId,
        request: &CommandRequest,
    ) -> Result<Option<Submission>> {
        validate_request(request, true)?;
        existing(&self.conn, owner, token_hash, id, request)
    }
    pub fn preflight_admit(
        &mut self,
        owner: &InputOwner,
        epoch: [u8; 16],
        hash: [u8; 32],
        now: BootTime,
    ) -> Result<()> {
        let config = &self.config;
        check_admission(&self.conn, config, owner, epoch, hash, now)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn admit(
        &mut self,
        owner: &InputOwner,
        epoch: [u8; 16],
        hash: [u8; 32],
        now: BootTime,
        id: InvocationId,
        request: &CommandRequest,
        job: JobId,
        credential_hash: [u8; 32],
    ) -> Result<Submission> {
        validate_request(request, true)?;
        let config = Config {
            version: 2,
            policy: self.config.policy.clone(),
            account: self.config.account.clone(),
            epoch: self.config.epoch,
        };
        if let Some(existing) = existing(&self.conn, owner, hash, id, request)? {
            return Ok(existing);
        }
        let canonical_exec_required =
            crate::policy::check_command(&self.config.policy, owner.owner.principal, request)?;
        validate_request(request, self.config.policy.allow_shell_command)?;
        fault_point(&self.root, job, FaultPoint::BeforeAccepted);
        let result=self.transaction(|tx|{
            if let Some(existing)=existing(tx,owner,hash,id,request)? {return Ok(existing)}
            check_admission(tx,&config,owner,epoch,hash,now)?;
            let empty=OutputStatus {retained:0,produced:0,closed:false,gaps:Vec::new()};
            let j=Job {id:job,owner:owner.owner.clone(),endpoint:owner.endpoint,invocation:id,digest:request_digest(request)?,token:hash,credential:credential_hash,limits:config.policy.limits.clone(),account:config.account.clone(),request:Some(request.clone()),canonical_exec_required,state:InvocationState::Accepted,outcome:None,worker:None,child:None,pgid:None,cancel:false,attempts:0,reserved:None,input:Input::default(),stdout:empty.clone(),stderr:empty,completed:None};
            tx.execute("INSERT INTO jobs(job,principal,server,target,invocation,state,data) VALUES(?,?,?,?,?,0,?)",params![job.0.as_slice(),owner.owner.principal.0.as_slice(),owner.owner.server.0.as_slice(),owner.owner.target_uid,id.0.as_slice(),encode(&j)?]).map_err(db_error)?;
            save(tx, &j)?;
            let mut token=load_token(tx,hash)?;token.consumed=true;
            tx.execute("UPDATE tokens SET data=?,consumed=1 WHERE hash=?",params![encode(&token)?,hash.as_slice()]).map_err(db_error)?;
            Ok(Submission {disposition:SubmitDisposition::Accepted,status:j.status()})
        })?;
        if matches!(result.disposition, SubmitDisposition::Accepted) {
            fault_point(&self.root, job, FaultPoint::AfterAccepted);
        }
        Ok(result)
    }
    pub fn job_for(&mut self, owner: &OwnerKey, id: InvocationId) -> Result<JobId> {
        find_job(&self.conn, owner, id)?
            .map(|j| j.id)
            .ok_or(EngineError::NotFound)
    }
    pub fn status(&mut self, owner: &OwnerKey, id: JobId) -> Result<InvocationStatus> {
        let j = load(&self.conn, id)?;
        j.verify_owner(owner)?;
        Ok(j.status())
    }
    pub fn job_state(&mut self, id: JobId) -> Result<InvocationState> {
        Ok(load(&self.conn, id)?.state)
    }
    pub(crate) fn cleanup_jobs(&mut self) -> Result<Vec<JobId>> {
        self.conn
            .busy_timeout(std::time::Duration::ZERO)
            .map_err(db_error)?;
        let mut statement = self
            .conn
            .prepare("SELECT job FROM jobs ORDER BY job LIMIT 257")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(db_error)?;
        let jobs = rows
            .map(|row| job_id(&row.map_err(db_error)?))
            .collect::<Result<Vec<_>>>()?;
        if jobs.len() > 256 {
            return Err(EngineError::QuotaExceeded);
        }
        Ok(jobs)
    }
    pub fn job_exists(&mut self, id: JobId) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE job=?)",
                params![id.0.as_slice()],
                |r| r.get(0),
            )
            .map_err(db_error)
    }
    pub fn reserve_worker_start(&mut self, id: JobId, now: BootTime) -> Result<StartDecision> {
        let max = self.config.policy.limits.starting_workers;
        self.mutate(id, |j, tx| {
            if j.state != InvocationState::Accepted {
                return Ok(StartDecision::Deferred);
            }
            if j.reserved
                .is_some_and(|deadline| same_boot_before(now, deadline))
            {
                return Ok(StartDecision::Deferred);
            }
            if j.attempts >= 3 {
                terminal_job(
                    j,
                    Some(ExitOutcome::NotStarted),
                    InvocationState::Exited,
                    logical_now(tx)?,
                );
                return Ok(StartDecision::FinishedNotStarted);
            }
            let starting:u32=tx.query_row("SELECT count(*) FROM jobs WHERE state=1 OR (state=0 AND reserved_boot=? AND reserved_ms>?)",params![now.boot_id.as_slice(),sql_integer(now.millis)?],|r|r.get(0)).map_err(db_error)?;
            if starting >= max {
                return Ok(StartDecision::Deferred);
            }
            j.attempts += 1;
            j.reserved = Some(BootTime {
                boot_id: now.boot_id,
                millis: now
                    .millis
                    .checked_add(2000)
                    .ok_or(EngineError::StorageFailure)?,
            });
            Ok(StartDecision::Reserved)
        })
    }
    pub fn release_worker_start(&mut self, id: JobId) -> Result<()> {
        self.mutate(id, |j, _| {
            j.reserved = None;
            Ok(())
        })
    }
    /// Refund only a reservation which has not been claimed by a worker.
    pub(crate) fn refund_worker_start(&mut self, id: JobId) -> Result<()> {
        self.mutate(id, |job, _| {
            if job.state == InvocationState::Accepted && job.reserved.is_some() {
                job.attempts = job
                    .attempts
                    .checked_sub(1)
                    .ok_or(EngineError::StorageFailure)?;
                job.reserved = None;
            }
            Ok(())
        })
    }
    pub fn pending_jobs(&mut self, limit: u32) -> Result<Vec<PendingJob>> {
        if !(1..=128).contains(&limit) {
            return Err(EngineError::InvalidRequest);
        }
        self.transaction(|tx| {
            let cursor: Vec<u8> = tx
                .query_row(
                    "SELECT data FROM meta WHERE name='pending_cursor'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            let mut stmt = tx
                .prepare("SELECT job,state FROM jobs WHERE state<3 AND job>? ORDER BY job LIMIT ?")
                .map_err(db_error)?;
            let rows = stmt
                .query_map(params![cursor, limit], |r| {
                    Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i32>(1)?))
                })
                .map_err(db_error)?;
            let mut jobs = Vec::new();
            for row in rows {
                let (id, state) = row.map_err(db_error)?;
                jobs.push(PendingJob {
                    job: job_id(&id)?,
                    state: match state {
                        0 => InvocationState::Accepted,
                        1 => InvocationState::Launching,
                        2 => InvocationState::Running,
                        _ => return Err(EngineError::StorageFailure),
                    },
                });
            }
            drop(stmt);
            let next = jobs.last().map(|j| j.job.0.to_vec()).unwrap_or_default();
            tx.execute(
                "UPDATE meta SET data=? WHERE name='pending_cursor'",
                params![next],
            )
            .map_err(db_error)?;
            Ok(jobs)
        })
    }
    pub fn worker_identity(&mut self, id: JobId) -> Result<Option<ProcessIdentity>> {
        Ok(load(&self.conn, id)?.worker)
    }
    pub fn claim_worker(
        &mut self,
        id: JobId,
        credential: &WorkerCredential,
        identity: &ProcessIdentity,
    ) -> Result<ClaimResult> {
        let config = &self.config;
        let account = config.account.clone();
        let server = config.policy.server;
        fault_point(&self.root, id, FaultPoint::BeforeClaim);
        let result = self.mutate(id, |j, _| {
            if j.credential != sha256(&credential.0) {
                return Err(EngineError::Unauthorized);
            }
            if j.owner.server != server || j.owner.target_uid != account.uid {
                return Err(EngineError::Unauthorized);
            }
            if j.state != InvocationState::Accepted {
                return Ok(ClaimResult::AlreadyClaimed);
            }
            if identity.pid <= 0 || identity.start_ticks == 0 {
                return Err(EngineError::WorkerUnavailable);
            }
            let request = j.request.take().ok_or(EngineError::StorageFailure)?;
            // Accepted work retains its original authority across policy changes.
            validate_request(&request, true)?;
            if j.canonical_exec_required && !matches!(request, CommandRequest::Exec { .. }) {
                return Err(EngineError::StorageFailure);
            }
            j.state = InvocationState::Launching;
            j.worker = Some(identity.clone());
            j.reserved = None;
            Ok(ClaimResult::Claimed(WorkerClaim {
                job: id,
                request,
                account: j.account.clone(),
                limits: j.limits.clone(),
                canonical_exec_required: j.canonical_exec_required,
            }))
        })?;
        fault_point(&self.root, id, FaultPoint::AfterClaim);
        Ok(result)
    }
    pub fn mark_running(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        child: &ProcessIdentity,
        pgid: i32,
    ) -> Result<()> {
        fault_point(&self.root, id, FaultPoint::BeforeRunning);
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            if j.state != InvocationState::Launching
                || child.pid <= 0
                || child.pid != pgid
                || child.start_ticks == 0
                || child.boot_id != worker.boot_id
            {
                return Err(EngineError::WorkerUnavailable);
            }
            j.child = Some(child.clone());
            j.pgid = Some(pgid);
            j.state = InvocationState::Running;
            Ok(())
        })?;
        fault_point(&self.root, id, FaultPoint::AfterRunning);
        Ok(())
    }
    pub fn finish(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        outcome: ExitOutcome,
    ) -> Result<()> {
        fault_point(&self.root, id, FaultPoint::BeforeExitCommit);
        self.mutate(id, |j, tx| {
            j.verify_worker(worker)?;
            if !(j.state == InvocationState::Running && !matches!(outcome, ExitOutcome::NotStarted)
                || j.state == InvocationState::Launching
                    && matches!(outcome, ExitOutcome::NotStarted))
            {
                return Err(EngineError::WorkerUnavailable);
            }
            terminal_job(j, Some(outcome), InvocationState::Exited, logical_now(tx)?);
            Ok(())
        })?;
        fault_point(&self.root, id, FaultPoint::AfterExitCommit);
        Ok(())
    }
    pub fn mark_unrecoverable(
        &mut self,
        id: JobId,
        expected: InvocationState,
        state: InvocationState,
    ) -> Result<()> {
        self.mutate(id, |j, tx| {
            if j.state != expected {
                return Err(EngineError::Busy);
            }
            if !matches!(
                (expected, state),
                (
                    InvocationState::Accepted | InvocationState::Launching,
                    InvocationState::UnknownLaunch
                ) | (InvocationState::Running, InvocationState::Lost)
            ) {
                return Err(EngineError::InvalidRequest);
            }
            for output in [&mut j.stdout, &mut j.stderr] {
                if !output.closed {
                    crate::spool::add_gap(
                        &mut output.gaps,
                        output.produced,
                        None,
                        MissingReason::UncertainTail,
                    );
                    // At the four-range bound the shared helper coarsens the
                    // final range. Keep its unbounded tail explicitly uncertain.
                    if let Some(tail) = output.gaps.last_mut() {
                        tail.reason = MissingReason::UncertainTail;
                    }
                }
            }
            input_gap(&mut j.input, true);
            terminal_job(j, None, state, logical_now(tx)?);
            Ok(())
        })
    }
    pub fn begin_input(
        &mut self,
        id: JobId,
        owner: &InputOwner,
        generation: InputGeneration,
    ) -> Result<InputStatus> {
        self.mutate(id, |j, _| {
            j.verify_owner(&owner.owner)?;
            if j.terminal()
                || j.input.eof.is_some()
                || matches!(
                    j.input.health,
                    InputHealth::EofPending | InputHealth::Closed
                )
            {
                return Err(EngineError::InputClosed);
            }
            if j.input.records.iter().any(|r| r.bytes.is_some()) {
                j.input.gaps = j.input.gaps.saturating_add(1);
            }
            let gaps = j.input.gaps;
            let next_record = j.input.next_record;
            j.input = Input::default();
            j.input.gaps = gaps;
            j.input.next_record = next_record;
            j.input.generation = Some(generation);
            j.input.endpoint = Some(owner.endpoint);
            Ok(j.input_status())
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn queue_input(
        &mut self,
        id: JobId,
        owner: &InputOwner,
        generation: InputGeneration,
        offset: u64,
        bytes: &[u8],
        now: BootTime,
        deadline: BootTime,
    ) -> Result<InputReceipt> {
        if bytes.is_empty() || bytes.len() > marmish_core::Limits::RAW_BYTES {
            return Err(EngineError::InvalidRequest);
        }
        self.mutate(id, |j, _| {
            j.verify_owner(&owner.owner)?;
            verify_input_owner(j, owner, generation)?;
            let hash = sha256(bytes);
            if let Some(receipt) = retry_input(&j.input, offset, bytes.len(), hash)? {
                return Ok(receipt);
            }
            validate_new_input(j, offset, now, deadline)?;
            reserve_input_capacity(&mut j.input, bytes.len())?;
            let next = offset
                .checked_add(bytes.len() as u64)
                .ok_or(EngineError::InvalidRequest)?;
            let record = InputRecord {
                id: j.input.next_record,
                offset,
                length: bytes.len() as u32,
                hash,
                bytes: Some(bytes.to_vec()),
                deadline,
                written: 0,
                delivering: false,
            };
            j.input.next_record = j
                .input
                .next_record
                .checked_add(1)
                .ok_or(EngineError::StorageFailure)?;
            j.input.records.push(record);
            j.input.accepted = next;
            Ok(InputReceipt {
                accepted: next,
                available_credit: credit(&j.input),
                duplicate: false,
            })
        })
    }
    pub fn queue_eof(
        &mut self,
        id: JobId,
        owner: &InputOwner,
        generation: InputGeneration,
        final_offset: u64,
    ) -> Result<InputStatus> {
        self.mutate(id, |j, _| {
            j.verify_owner(&owner.owner)?;
            verify_input_owner(j, owner, generation)?;
            if final_offset != j.input.accepted {
                return Err(EngineError::InputConflict);
            }
            if j.input.eof == Some(final_offset) {
                return Ok(j.input_status());
            }
            if j.terminal() {
                return Err(EngineError::InputClosed);
            }
            if j.input.health != InputHealth::Open {
                return Err(EngineError::InputClosed);
            }
            j.input.eof = Some(final_offset);
            j.input.health = InputHealth::EofPending;
            Ok(j.input_status())
        })
    }
    pub fn controls(&mut self, id: JobId, worker: &ProcessIdentity) -> Result<Controls> {
        let now = launcher::boot_time()?;
        let snapshot = load(&self.conn, id)?;
        snapshot.verify_worker(worker)?;
        if !input_expired(&snapshot.input, now) {
            return Ok(Controls {
                cancel_requested: snapshot.cancel,
                generation: snapshot.input.generation,
            });
        }
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            expire_input(&mut j.input, now);
            Ok(Controls {
                cancel_requested: j.cancel,
                generation: j.input.generation,
            })
        })
    }
    pub fn take_input(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        now: BootTime,
    ) -> Result<InputWork> {
        let snapshot = load(&self.conn, id)?;
        snapshot.verify_worker(worker)?;
        if !input_expired(&snapshot.input, now) {
            if matches!(
                snapshot.input.health,
                InputHealth::Gap | InputHealth::Uncertain
            ) {
                return Ok(failed_input_work(&snapshot.input));
            }
            if snapshot
                .input
                .records
                .iter()
                .any(|r| r.delivering && r.bytes.is_some())
            {
                return Ok(InputWork::Idle);
            }
            if !snapshot.input.records.iter().any(|r| r.bytes.is_some()) {
                return Ok(if snapshot.input.eof == Some(snapshot.input.delivered) {
                    InputWork::Eof
                } else {
                    InputWork::Idle
                });
            }
        }
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            expire_input(&mut j.input, now);
            if matches!(j.input.health, InputHealth::Gap | InputHealth::Uncertain) {
                return Ok(failed_input_work(&j.input));
            }
            if j.input
                .records
                .iter()
                .any(|r| r.delivering && r.bytes.is_some())
            {
                return Ok(InputWork::Idle);
            }
            if let Some(record) = j.input.records.iter_mut().find(|r| r.bytes.is_some()) {
                record.delivering = true;
                return Ok(InputWork::Deliver(Delivery {
                    record: record.id,
                    generation: j.input.generation.ok_or(EngineError::StaleGeneration)?,
                    offset: record.offset,
                    bytes: record.bytes.clone().ok_or(EngineError::StorageFailure)?,
                    deadline: record.deadline,
                }));
            }
            if j.input.eof == Some(j.input.delivered) {
                return Ok(InputWork::Eof);
            }
            Ok(InputWork::Idle)
        })
    }
    pub fn advance_input(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        record: i64,
        written_total: u32,
    ) -> Result<()> {
        fault_point(&self.root, id, FaultPoint::BeforeInputProgress);
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            let r = j
                .input
                .records
                .iter_mut()
                .find(|r| r.id == record)
                .ok_or(EngineError::StaleGeneration)?;
            if !r.delivering || written_total < r.written || written_total > r.length {
                return Err(EngineError::InputConflict);
            }
            j.input.delivered = j
                .input
                .delivered
                .checked_add(u64::from(written_total - r.written))
                .ok_or(EngineError::StorageFailure)?;
            r.written = written_total;
            if r.written == r.length {
                r.bytes = None;
            }
            Ok(())
        })?;
        fault_point(&self.root, id, FaultPoint::AfterInputProgress);
        Ok(())
    }
    pub fn fail_input(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        generation: InputGeneration,
        uncertain: bool,
    ) -> Result<()> {
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            if j.input.generation == Some(generation) {
                input_gap(&mut j.input, uncertain);
            }
            Ok(())
        })
    }
    pub fn close_input(&mut self, id: JobId, worker: &ProcessIdentity) -> Result<()> {
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            if j.input.records.iter().any(|r| r.bytes.is_some()) {
                input_gap(&mut j.input, false);
            } else if !matches!(j.input.health, InputHealth::Gap | InputHealth::Uncertain) {
                j.input.health = InputHealth::Closed;
            }
            Ok(())
        })
    }
    pub fn request_cancel(&mut self, owner: &OwnerKey, id: JobId) -> Result<()> {
        self.mutate(id, |j, tx| {
            j.verify_owner(owner)?;
            if j.terminal() {
                return Ok(());
            }
            j.cancel = true;
            if j.state == InvocationState::Accepted {
                terminal_job(
                    j,
                    Some(ExitOutcome::NotStarted),
                    InvocationState::Exited,
                    logical_now(tx)?,
                );
            }
            Ok(())
        })
    }
    pub fn output_status(
        &mut self,
        owner: &OwnerKey,
        id: JobId,
        stream: Stream,
    ) -> Result<OutputStatus> {
        let j = load(&self.conn, id)?;
        j.verify_owner(owner)?;
        Ok(j.output(stream).clone())
    }
    pub fn worker_output_status(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        stream: Stream,
    ) -> Result<OutputStatus> {
        let j = load(&self.conn, id)?;
        j.verify_worker(worker)?;
        Ok(j.output(stream).clone())
    }
    pub fn commit_output(
        &mut self,
        id: JobId,
        worker: &ProcessIdentity,
        stream: Stream,
        commit: &OutputCommit,
    ) -> Result<()> {
        fault_point(&self.root, id, FaultPoint::BeforeOutputCommit);
        self.mutate(id, |j, _| {
            j.verify_worker(worker)?;
            let cap = j.limits.spool_bytes_per_stream;
            let old = j.output_mut(stream);
            if commit.retained < old.retained
                || commit.retained > cap
                || commit.produced < old.produced
                || commit.retained > commit.produced
                || old.closed && !commit.closed
                || commit.gaps.len() > 4
            {
                return Err(EngineError::StorageFailure);
            }
            let mut end = commit.retained;
            for gap in &commit.gaps {
                if gap.start < end
                    || gap
                        .end
                        .is_some_and(|v| v < gap.start || v > commit.produced)
                {
                    return Err(EngineError::StorageFailure);
                }
                end = gap.end.unwrap_or(u64::MAX);
            }
            *old = OutputStatus {
                retained: commit.retained,
                produced: commit.produced,
                closed: commit.closed,
                gaps: commit.gaps.clone(),
            };
            Ok(())
        })?;
        fault_point(&self.root, id, FaultPoint::AfterOutputCommit);
        Ok(())
    }
    pub fn retention_now_current(&mut self) -> Result<u64> {
        // Sample the production clock after acquiring the write transaction;
        // a worker may otherwise advance the anchor while this caller waits.
        self.transaction(|tx| logical_now(tx))
    }
    #[cfg(test)]
    pub fn retention_now(&mut self, clock: BootTime, wall_seconds: u64) -> Result<u64> {
        self.transaction(|tx| clock_now(tx, clock, wall_seconds))
    }
    pub fn cleanup_candidates(
        &mut self,
        now_seconds: u64,
        limit: u32,
    ) -> Result<Vec<CleanupCandidate>> {
        if !(1..=128).contains(&limit) {
            return Err(EngineError::InvalidRequest);
        }
        self.transaction(|tx| {
            let cursor: Vec<u8> = tx
                .query_row(
                    "SELECT data FROM meta WHERE name='cleanup_cursor'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            let mut stmt=tx.prepare("SELECT job,completed,ttl,tombstone,retained FROM jobs WHERE state>=3 AND job>? ORDER BY job LIMIT ?").map_err(db_error)?;
            let jobs=stmt.query_map(params![cursor,limit],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?))).map_err(db_error)?.collect::<std::result::Result<Vec<_>,_>>().map_err(db_error)?;
            drop(stmt);
            let next = jobs.last().map(|j| j.0.clone()).unwrap_or_default();
            tx.execute(
                "UPDATE meta SET data=? WHERE name='cleanup_cursor'",
                params![next],
            )
            .map_err(db_error)?;
            let mut result = Vec::new();
            for (id,completed,ttl,tombstone,retained) in jobs {
                let completed=u64::try_from(completed).map_err(|_|EngineError::StorageFailure)?;
                let tombstone=u64::try_from(tombstone).map_err(|_|EngineError::StorageFailure)?;
                let ttl=ttl.map(u64::try_from).transpose().map_err(|_|EngineError::StorageFailure)?;
                let age=now_seconds.saturating_sub(completed);
                let expire=retained>0 && ttl.is_some_and(|ttl|age>=ttl);
                if expire || retained == 0 && age >= tombstone {
                    result.push(CleanupCandidate {
                        job: job_id(&id)?,
                        delete_record: age >= tombstone && (retained == 0 || expire),
                    });
                }
            }
            Ok(result)
        })
    }
    pub fn expire_output(&mut self, id: JobId) -> Result<u64> {
        self.mutate(id, |j, _| {
            if !j.terminal() {
                return Err(EngineError::Busy);
            }
            let mut removed = 0u64;
            for s in [&mut j.stdout, &mut j.stderr] {
                removed = removed
                    .checked_add(s.retained)
                    .ok_or(EngineError::StorageFailure)?;
                if s.retained > 0 {
                    let prefix = MissingRange {
                        start: 0,
                        end: Some(s.retained),
                        reason: MissingReason::Expired,
                    };
                    s.retained = 0;
                    s.gaps.insert(0, prefix);
                    // Bounded statuses retain terminal unknown-tail semantics.
                    if s.gaps.len() > 4 {
                        return Err(EngineError::StorageFailure);
                    }
                }
            }
            Ok(removed)
        })
    }
    pub fn delete_tombstone(&mut self, id: JobId) -> Result<()> {
        self.transaction(|tx| {
            let j = load(tx, id)?;
            let minimum = j.limits.tombstone_retention.as_secs();
            if !j.terminal()
                || j.stdout.retained != 0
                || j.stderr.retained != 0
                || logical_now(tx)?.saturating_sub(j.completed.ok_or(EngineError::StorageFailure)?)
                    < minimum
            {
                return Err(EngineError::Busy);
            }
            tx.execute("DELETE FROM jobs WHERE job=?", params![id.0.as_slice()])
                .map_err(db_error)?;
            Ok(())
        })
    }
}

/// Exact retries precede freshness/EOF checks because an acknowledged record
/// remains retryable within the bounded recent metadata window.
fn retry_input(
    input: &Input,
    offset: u64,
    length: usize,
    hash: [u8; 32],
) -> Result<Option<InputReceipt>> {
    let Some(record) = input.records.iter().find(|r| r.offset == offset) else {
        return Ok(None);
    };
    if record.length as usize != length || record.hash != hash {
        return Err(EngineError::InputConflict);
    }
    Ok(Some(InputReceipt {
        accepted: input.accepted,
        available_credit: credit(input),
        duplicate: true,
    }))
}
fn validate_new_input(job: &Job, offset: u64, now: BootTime, deadline: BootTime) -> Result<()> {
    if job.terminal() {
        return Err(EngineError::InputClosed);
    }
    match job.input.health {
        InputHealth::Open => {}
        InputHealth::Gap | InputHealth::Uncertain => return Err(EngineError::InputGap),
        _ => return Err(EngineError::InputClosed),
    }
    if !same_boot_before(now, deadline) || deadline.millis - now.millis > 60_000 {
        return Err(EngineError::InputExpired);
    }
    if offset != job.input.accepted {
        return Err(EngineError::InputConflict);
    }
    Ok(())
}
fn input_window_full(input: &Input, additional: usize) -> bool {
    input.records.len() >= 256
        || input
            .records
            .iter()
            .map(|r| u64::from(r.length))
            .sum::<u64>()
            + additional as u64
            > 65536
}
/// Evict delivered hash metadata only; pending payloads cannot be displaced.
fn reserve_input_capacity(input: &mut Input, additional: usize) -> Result<()> {
    while input_window_full(input, additional) {
        if !input
            .records
            .first()
            .is_some_and(|r| r.bytes.is_none() && r.written == r.length)
        {
            break;
        }
        input.records.remove(0);
    }
    let available = credit(input);
    if additional > available as usize || input_window_full(input, additional) {
        return Err(EngineError::InputBackpressure {
            next_offset: input.accepted,
            available_credit: if input.records.len() >= 256 {
                0
            } else {
                available
            },
        });
    }
    Ok(())
}
fn verify_input_owner(j: &Job, owner: &InputOwner, generation: InputGeneration) -> Result<()> {
    if j.input.generation != Some(generation) || j.input.endpoint != Some(owner.endpoint) {
        Err(EngineError::StaleGeneration)
    } else {
        Ok(())
    }
}
fn request_digest(request: &CommandRequest) -> Result<[u8; 32]> {
    Ok(sha256(&encode(request)?))
}
fn find_job(conn: &Connection, owner: &OwnerKey, id: InvocationId) -> Result<Option<Job>> {
    let data:Option<Vec<u8>>=conn.query_row("SELECT data FROM jobs WHERE principal=? AND server=? AND target=? AND invocation=? AND length(data)<=2097152",params![owner.principal.0.as_slice(),owner.server.0.as_slice(),owner.target_uid,id.0.as_slice()],|r|r.get(0)).optional().map_err(db_error)?;
    data.map(|b| decode(&b)).transpose()
}
fn existing(
    conn: &Connection,
    owner: &InputOwner,
    hash: [u8; 32],
    id: InvocationId,
    request: &CommandRequest,
) -> Result<Option<Submission>> {
    let Some(j) = find_job(conn, &owner.owner, id)? else {
        return Ok(None);
    };
    if j.digest != request_digest(request)? {
        return Err(EngineError::InvocationConflict);
    }
    if j.token != hash || j.endpoint != owner.endpoint {
        return Err(EngineError::InvalidToken);
    }
    Ok(Some(Submission {
        disposition: SubmitDisposition::Existing,
        status: j.status(),
    }))
}
fn load_token(conn: &Connection, hash: [u8; 32]) -> Result<Token> {
    let data: Option<Vec<u8>> = conn
        .query_row(
            "SELECT data FROM tokens WHERE hash=? AND length(data)<=2097152",
            params![hash.as_slice()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    decode(&data.ok_or(EngineError::InvalidToken)?)
}
fn check_admission(
    conn: &Connection,
    config: &Config,
    owner: &InputOwner,
    epoch: [u8; 16],
    hash: [u8; 32],
    now: BootTime,
) -> Result<()> {
    if owner.owner.server != config.policy.server
        || owner.owner.target_uid != config.account.uid
        || !config
            .policy
            .allowed_principals
            .contains(&owner.owner.principal)
    {
        return Err(EngineError::Unauthorized);
    }
    let token = load_token(conn, hash)?;
    if token.consumed
        || token.owner.owner != owner.owner
        || token.owner.endpoint != owner.endpoint
        || token.epoch != epoch
        || epoch != config.epoch
    {
        return Err(EngineError::InvalidToken);
    }
    if !same_boot_before(now, token.expires)
        || now.boot_id != token.issued.boot_id
        || now.millis < token.issued.millis
    {
        return Err(EngineError::ExpiredToken);
    }
    let l = &config.policy.limits;
    let (records, active): (u32, u32) = conn
        .query_row(
            "SELECT count(*),coalesce(sum(state<3),0) FROM jobs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db_error)?;
    if records >= l.invocation_records || active >= l.active_jobs {
        return Err(EngineError::QuotaExceeded);
    }
    let used: i64 = conn
        .query_row("SELECT coalesce(sum(spool_used),0) FROM jobs", [], |r| {
            r.get(0)
        })
        .map_err(db_error)?;
    let used = u64::try_from(used).map_err(|_| EngineError::StorageFailure)?;
    if used
        .checked_add(l.spool_bytes_per_stream * 2)
        .ok_or(EngineError::StorageFailure)?
        > l.aggregate_spool_bytes
    {
        return Err(EngineError::QuotaExceeded);
    }
    Ok(())
}
fn sql_integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| EngineError::StorageFailure)
}
fn job_id(bytes: &[u8]) -> Result<JobId> {
    Ok(JobId(
        bytes.try_into().map_err(|_| EngineError::StorageFailure)?,
    ))
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
