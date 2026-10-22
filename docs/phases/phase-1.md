# Phase 1: durable asynchronous command engine

This document records the original phase-1 contract and its tests. The later [SQLCipher storage increment](sqlcipher-storage.md) supersedes its plaintext SQLite/spool choices, dependency pins and Rust version. Current storage uses encrypted databases and the pinned Rust 1.97.1 toolchain.

## Scope

Build a Linux-first, same-user command engine with an offline Rust API and local `marmish-worker` executable. Commands survive client/frontend disconnection and frontend restart because a detached worker owns each child. No MDK, network transport, remote CLI, PTY, tmux, user switching, or root execution is introduced. Phase 2's relay-policy blocker does not block this phase.

Follow [the build plan](../build-plan.md): Claude reviews this detailed plan before agents implement; Claude reviews the integrated code in auto mode afterward. Beads holds assignments, findings, and dispositions. Never execute strings received as ordinary chat text. The future transport must authenticate peers before constructing the engine's authorized context.

## Minimal API and ownership

Add one `marmish-engine` crate with a library and `marmish-worker` binary. Reuse the existing core limits/config where suitable; agree shared types before agent edits. Do not expose a network listener or generic shell management service.

| Owner | Files | Responsibility |
| --- | --- | --- |
| Agent A | `marmish-engine/src/store.rs`, `model.rs`, `admission.rs`, store/admission tests | SQLite schema, authorized operation types, freshness tokens, invocation state, quotas, durable stdin and cancellation requests |
| Agent B | `marmish-engine/src/worker.rs`, `launcher.rs`, `spool.rs`, `bin/marmish-worker.rs`, worker/spool tests | Detached worker, same-user launcher, subprocess pipes/process groups, bounded output and recovery identity |
| Integrator | `marmish-engine/src/lib.rs`, end-to-end tests, root manifests/lockfile, docs | Public facade, ownership handshake, integration, fault tests, checks, review dispositions |

The public API, completed by the frozen appendix below, exposes `Engine::open(policy)`, `issue_execute_token(context)`, `submit(context, token, invocation_id, request)`, `status(context, id)`, `write_stdin(context, id, generation, offset, bytes, deadline)`, `stdin_eof(context, id, generation, final_offset)`, `read_output(context, id, stream, cursor, limit)`, and `cancel(context, id)`. Return structured bounded errors; never return secrets or SQL internals. Context contains an opaque already-authorized principal, endpoint, server and same-user target identifier; authorization policy checks happen on every operation, including status and output. Invocation IDs alone grant no access. `spawn_child` accepts only the validated immutable ChildSpec, while `spawn_worker` accepts the server-generated job location; their phase-1 implementations are same-user only.

## Durable state and admission

Use SQLite with schema versioning, foreign keys, WAL and `synchronous=FULL`; configure a bounded busy timeout. A single engine-owner file lock excludes simultaneous frontends; workers use their own SQLite connections. A server-generated random epoch changes on each engine startup under the owner lock. Tokens issued under older epochs cannot launch new work; existing jobs remain accessible. Workers validate their recorded job credentials, not the current admission epoch.

Represent `accepted -> launching -> running -> exited`, with terminal `unknown-launch` and `lost`. Store the immutable canonical request digest, scoped invocation ID, authorized owner, timestamps, worker identity, stream metadata, and outcome. All state transitions use short `BEGIN IMMEDIATE` transactional compare-and-swap; different bytes for an existing ID always conflict. Persist accepted state before starting a worker and persist launching before spawning a command. A request replay never launches a second worker/child merely because an acknowledgement was lost.

Execute tokens are 256-bit random values, stored as hashes and bound to principal, endpoint, target, server and startup epoch. Default launch deadline is 60 seconds; phase 1 allows no extension beyond a hard 300 seconds. Consuming a token and inserting the invocation happen in one transaction; bind the consumed token to that invocation/digest. An exact duplicate returns existing status without granting another launch, including after token expiry; conflicting reuse fails. For new admission, reject expired tokens, unknown tokens, epoch mismatch, and a monotonic deadline that has elapsed even if the wall clock moved backward. Token issuance/deadline checking is owned by the running frontend; startup invalidation avoids resurrecting tokens after reboot or uncertain clock recovery. Clock anomalies may invalidate all outstanding tokens rather than extend authority.

Keep terminal invocation records/tombstones for at least 48 hours after completion and beyond the final token authorization window plus bounded delivery/skew margin. Never delete active jobs or live token replay protection to make room. After tombstone cleanup an old token remains expired, consumed, unknown or wrong-epoch and cannot authorize a new launch; the client must use a new invocation ID for new work. Output expiry does not delete digest/outcome records. Terminal records may outlive their minimum retention while their output is retained; tombstone deletion never implicitly shortens configured output retention. Defaults: 32 active jobs, 10,000 retained invocation records, 1,024 outstanding tokens, 4 concurrent workers starting; admission refuses at quota. Garbage collection is explicit/periodic bounded engine work, never whole-database unbounded scans.

## Worker lifecycle and crash semantics

After committing acceptance, the launcher starts the installed/configured worker executable through structured arguments containing only state path and server-generated job identifier. Do not place command contents, private credentials, or environment values in argv. The worker starts in a new session with stdin/stdout/stderr disconnected from frontend pipes; all nonstandard descriptors are CLOEXEC and none of the frontend lock/SQLite handles survive exec; it must not receive a parent-death signal. The engine constructor does not own a kill-on-drop handle. Do not deploy under a service configuration that kills all workers on frontend restart; document this operational constraint.

The worker opens a per-job exclusive lifetime lock, validates same-UID state and its immutable job record, and claims accepted state transactionally. Concurrent workers cannot both claim/spawn. That same Accepted -> Launching claim transaction records Linux boot ID, PID and `/proc` start-time identity. It spawns exactly one child in a separate process group/session with piped stdin/stdout/stderr, records child identity and running status durably, and owns all reaping. It drains output and reads bounded database input/cancellation work until terminal. The worker retains its job lock until final outcome commit and resource cleanup.

Recovery first checks lock ownership and recorded process start identity, with bounded retry for a worker that is legitimately initializing. A healthy worker continues while a new frontend reconnects to the database. A pre-launch accepted job whose worker demonstrably never claimed may be retried by launching a worker; transactional claiming still prevents duplicates. A launching job with no verifiable live worker becomes unknown-launch; it is never automatically spawned again. A running job with a lost worker becomes lost unless a terminal worker record proves completion. Do not infer completion or safe cancellation from a PID alone. A command may have started in the spawn/persistence crash window: report uncertainty and retain replay protection. This phase does not promise job survival after worker death, system reboot, or exactly-once execution across OS/database boundaries.

## Process policy and cancellation

Both library entrypoints and worker refuse effective UID 0. Resolve same-user UID/GID/home/shell from trusted local account data. Reject target account switching. Default cwd is that account's home; a server-configured absolute existing directory may replace it. Clients cannot choose arbitrary cwd/environment in this phase. Use a small server-controlled environment (`HOME`, `USER`, `LOGNAME`, deterministic `PATH`, locale), never inherit frontend secrets or loader/hook environment variables. Explicit shell mode can still execute account-controlled startup files (for example zsh `.zshenv`); this is intentional same-user shell behavior, not a sandbox. Set umask 077 before file creation/child launch. Direct execution preserves argument bytes where the API supports them, rejects NULs and empty executable names, and uses a deterministic server PATH or absolute executable path. Explicit shell-string mode requires server permission and an absolute trusted account shell; execute it with `-c`, without a login-shell mode. Bound argv count/bytes before persistence.

Only the worker signals its child process group. `cancel` records an idempotent owner-authorized request; it never sends signals from a recovered numeric PID. Worker sends TERM then KILL after a default 5-second monotonic grace, while retaining the unreaped group-leader child so that its PID/PGID cannot be recycled. Use non-reaping exit observation (Linux `waitid` with `WNOWAIT`, or equivalent) until final group cleanup; do not reap the leader before later killpg operations. Before each signal verify worker ownership, saved identity and process-group relationship; a mismatch makes cancellation unavailable/lost rather than risking another process. External cancellation after worker loss is unsupported. Document that descendants can intentionally escape a process group; full containment/cgroup enforcement is deferred and not claimed.

A naturally exited leader may leave pipe-holding descendants. Allow a bounded default 2-second drain grace, terminate remaining owned-group descendants while retaining the leader identity, drain again with a bound, then close pipes and record any unavailable tail explicitly. Report real exit code or signal separately from cancellation request status; a raced cancellation must not replace an already committed natural outcome. No unbounded wait on inherited pipes.

## Stdin ordering and disconnected behavior

Client loss never closes stdin implicitly. A job awaiting more input remains running until explicit EOF, cancellation, process exit, or configured job policy. Each stdin write carries an attachment/input generation, byte offset and fresh bounded deadline. Reattachment rotates input generation; late writes from the previous attachment fail. Phase 1 provides this API without implementing network attachment leases. Startup invalidates old pending input generations and queued undelivered stdin; report an explicit input gap rather than silently replay it.

Default queued stdin window is 64 KiB per job and frame size is at most the core payload limit. Writes must be nonempty and start at the next accepted offset; reject gaps, stale deadlines and over-cap writes with the next expected offset/available credit. An identical retry within the retained bounded deduplication window is acknowledged without rewriting; conflicting overlaps fail and retries outside that window require status/resynchronization, never guessed acceptance. Store hashes/metadata only for that bounded recent window. EOF specifies final offset and applies only once all preceding accepted bytes were delivered; a duplicate matching EOF is idempotent and later input is rejected.

The worker marks an input record delivering before writing to the pipe, then records delivered progress. Crash between pipe write and progress persistence is ambiguous: do not redeliver after restart. A lost worker yields lost/uncertain input status. Partial writes use nonblocking IO and retain a bounded local remainder; input waiting past its freshness deadline is discarded with a visible gap and input stream failure, not skipped while pretending a contiguous stream. The client must explicitly establish a new generation or cancel. EOF cannot overtake queued data. Worker IO must continue draining stdout/stderr while stdin is backpressured.

## Output storage and cursors

Use distinct monotonic byte cursors for stdout and stderr; no cross-stream total order is promised. Default spool cap is 8 MiB per stream per job, configurable within an aggregate server disk quota. Retain the first cap bytes, continue draining/discarding excess, and track total produced bytes plus a gap range. This avoids a ring-buffer implementation in phase 1 while making truncation explicit. Read results contain retained bytes, next cursor, produced watermark, missing ranges and terminal/EOF state; never concatenate retained and missing bytes as if complete.

Use server-generated paths, private bounded append files, and transactional metadata. Read order is IoGuard, transactional metadata snapshot, then spool read; never load metadata before acquiring the guard. Synchronize spool data before advancing the durable committed watermark. Recovery ignores/truncates an uncommitted tail, and never returns bytes beyond committed metadata. Spool errors/full disk cause explicit lost-output metadata while continuing bounded draining if the database remains usable; if durable state cannot be recorded, stop accepting work and surface an operational failure rather than claim durable output. Unrecoverable worker storage failure may terminate its owned process group under the documented failure policy.

Output retention defaults to 24 hours and is independent of invocation/tombstone retention. Cleanup only deletes inactive reader-safe spool content through engine-controlled paths; reads return explicit expired gaps afterward. At phase 1's scale, serialize cleanup and reads under a per-job guard rather than build a general lease system. Rate/byte-bound read responses. Job completion/exit status remains queryable after spool expiry. State directories are 0700; database, WAL/SHM, locks and spool files are 0600. Reject symlink/non-owned/permissive state roots; use exclusive no-follow file creation and directory-relative operations for generated paths. These controls protect from other Unix users, not malicious processes sharing the same UID.

## Verification and acceptance

Run workspace fmt, clippy with warnings denied, build and tests. Integration tests use a temporary private state directory, the actual worker binary, and deterministic local commands; never invoke MDK or a network service. Include tests for client-handle drop and frontend process death while output continues; new frontend status/output reads; distinct stdout/stderr byte streams, NUL/non-UTF-8 payloads; delayed stdin, explicit ordered EOF and backpressure; output cap/expiry gaps; signal exit and idempotent cancellation; authorization cross-owner denial; conflicting/duplicate invocations; quotas; root refusal; file modes and unsafe paths.

Inject deterministic crashes before/after accepted commit, worker claim, launching commit, command spawn, running commit, stdin write/progress commit, spool sync/metadata commit and exit commit. Use the off-default feature-gated file-based fault controls specified below; production builds cannot activate them through files, arguments or ambient environment. Prove recovery never automatically repeats an ambiguous command. Exercise stale startup epochs, expired tokens after tombstone deletion, backward/forward clock changes, worker loss, forged/reused process identities, cancellation/exit races, leader exit with pipe-holding descendants and aggregate storage limits. Use bounded timeouts; assert worker cleanup for each test, including failed tests.

Before implementation, agree exact public types and the small store methods worker needs, and resolve any Claude blocking finding. Completion requires independent Claude auto-mode code review, incorporated useful changes, passing checks and honest documented limits. No networking or interactive-shell capability is claimed. The remaining phase 2 integration must supply authenticated context, room/device validation, relay enforcement and transport credits without bypassing this engine's admission checks.

## Frozen interface appendix (implementation contract)

This appendix resolves the earlier illustrative signatures. It is the contract for parallel implementation and overrides ambiguous choices above. The integrator creates `crates/marmish-engine/Cargo.toml`, `src/lib.rs`, and module declarations before agents start. The paths in the ownership table above mean `crates/marmish-engine/...`; agents do not edit another owner's files. Agent A owns in-module `src/admission/tests.rs` and `src/store/tests.rs`; Agent B owns in-module `src/worker/tests.rs` and `src/spool/tests.rs`; the integrator owns other integration tests and any test frontend helper. These files are included with `#[cfg(test)] mod tests` from their owner module so they can exercise crate-private APIs; no private Store/launcher/spool surface is made public for tests. Both agents may also add public-API-only integration tests within their assigned test responsibility, coordinated with the integrator. Shared type changes go through Agent A and the integrator before dependent edits.

Use edition 2021/MSRV 1.93 inherited from the workspace. Dependencies are `marmish-core` (workspace path), `libc`, `rusqlite` with bundled SQLite, `sha2`, `getrandom`, and `thiserror`; use exact new pins `rusqlite = "=0.40.2"` (bundled), `sha2 = "=0.10.9"`, `getrandom = "=0.3.4"`, `zeroize = "=1.8.2"`, and existing workspace `thiserror = "2"`/`libc`. Existing workspace `serde`/`serde_json` encode private versioned database records; tokens and worker credentials have no serde implementations. The integrator updates the lockfile once and records any necessary `cargo fetch`. A C compiler is required for bundled SQLite. Confine `unsafe` to `launcher.rs` and `worker.rs`; no unsafe code in admission/store/spool/facade. Use standard blocking threads/nonblocking file descriptors and a bounded poll loop; no async runtime or generic plugin interface. `tempfile` is test-only. Schema and immutable request serialization are Agent A's private versioned encoding, not a new network protocol. Command vectors use owned Unix byte strings in the public model so arbitrary non-NUL bytes survive storage.

### Public model (`model.rs`, Agent A)

All exposed structs/enums implement `Debug`, with custom redacted implementations for command requests, tokens, credentials and stdin delivery payloads; IDs derive `Clone, Copy, Eq, PartialEq, Hash, Debug`, other data types derive `Clone, Debug` where practical. Tokens have explicit byte import/export below, but no serde serialization; transport authentication/encryption is phase 2's responsibility. These are concrete type shapes, not method bodies:

```rust
pub struct PrincipalId(pub [u8; 32]);
pub struct EndpointId(pub [u8; 32]);
pub struct ServerId(pub [u8; 32]);
pub struct InvocationId(pub [u8; 16]); // client random ID, scoped by principal/server/target
pub struct JobId(pub [u8; 16]);        // independent server-generated random path identity
pub struct InputGeneration(pub [u8; 16]);
pub struct AuthorizedContext { /* private principal, endpoint, server, target_uid */ }
pub struct ExecuteToken { /* private zeroizing 32 random bytes; redacted Debug */ }
impl ExecuteToken {
    pub fn expose_bytes(&self) -> [u8; 32]; // caller must zeroize its copy; never log
    pub fn from_bytes(bytes: [u8; 32]) -> Self; // importing bytes grants no authority by itself
}

pub struct EnginePolicy {
    pub state_root: std::path::PathBuf, // explicit absolute path
    pub worker_executable: std::path::PathBuf, // explicit absolute trusted executable
    pub server: ServerId,
    pub allowed_principals: Vec<PrincipalId>, // nonempty; exact allowlist
    pub allow_shell_command: bool,           // default false
    pub cwd: Option<std::path::PathBuf>,      // None = account home
    pub limits: EngineLimits,
}
pub struct EngineLimits {
    pub active_jobs: u32,             // default 32; hard max 128
    pub invocation_records: u32,      // default 10_000; hard max 100_000
    pub outstanding_tokens: u32,      // default 1_024; hard max 4_096
    pub starting_workers: u32,        // default 4; hard max 16
    pub token_lifetime: std::time::Duration, // default 60s; 1..=300s
    pub spool_bytes_per_stream: u64,  // default 8 MiB; 1 byte..=64 MiB
    pub aggregate_spool_bytes: u64,   // default 512 MiB; hard max 8 GiB
    pub output_retention: marmish_core::Retention, // default seconds() == Some(86400)
    pub tombstone_retention: std::time::Duration, // default/min 48h; max 30d
    pub cancel_grace: std::time::Duration, // default 5s; 1ms..=30s
    pub drain_grace: std::time::Duration,  // default 2s; 1ms..=30s
}
impl Default for EngineLimits { /* values above */ }
pub enum CommandRequest {
    Exec { argv: Vec<Vec<u8>> }, // argv[0] executable, at least one element
    Shell { command: Vec<u8> }, // explicit permission; trusted account shell -c
}
pub enum InvocationState { Accepted, Launching, Running, Exited, UnknownLaunch, Lost }
pub enum ExitOutcome { Code(i32), Signal(i32), NotStarted }
pub struct InvocationStatus {
    pub invocation: InvocationId,
    pub state: InvocationState,
    pub outcome: Option<ExitOutcome>,
    pub cancel_requested: bool,
    pub input: InputStatus,
    pub stdout: OutputStatus,
    pub stderr: OutputStatus,
}
pub enum InputHealth { Open, EofPending, Closed, Gap, Uncertain }
pub struct InputStatus {
    pub generation: Option<InputGeneration>, // None after frontend restart until explicit attach
    pub accepted: u64, // offsets within this generation
    pub delivered: u64,
    pub available_credit: u32,
    pub health: InputHealth,
    pub gap_count: u64, // sticky cumulative gaps across rotations; never reset by begin_input
}
pub enum Stream { Stdout, Stderr }
pub enum MissingReason { Limit, StorageFailure, Expired, UncertainTail }
pub struct MissingRange {
    pub start: u64,
    pub end: Option<u64>, // exclusive; None only for unknowable tail after worker loss
    pub reason: MissingReason,
}
pub struct OutputStatus {
    pub retained: u64, // committed prefix length, 0 if expired
    pub produced: u64, // last durably known watermark
    pub closed: bool,
    pub gaps: Vec<MissingRange>, // at most 4 normalized ranges
}
pub struct OutputChunk {
    pub bytes: Vec<u8>,
    pub next_cursor: u64,
    pub status: OutputStatus,
}
pub enum SubmitDisposition { Accepted, Existing }
pub struct Submission { pub disposition: SubmitDisposition, pub status: InvocationStatus }
pub struct InputReceipt { pub accepted: u64, pub available_credit: u32, pub duplicate: bool }
pub struct MaintenanceReport { pub records_removed: u32, pub spool_bytes_removed: u64 }

#[derive(thiserror::Error, Debug)]
pub enum EngineError {
    // Each variant has a fixed sanitized display string; no path, key, argv, SQL or OS text.
    Unauthorized, RootForbidden, InvalidPolicy, UnsafePath, AlreadyOpen,
    InvalidRequest, NotFound, InvocationConflict, InvalidToken, ExpiredToken,
    QuotaExceeded, StaleGeneration, InputExpired, InputConflict, InputClosed,
    InputGap, WorkerUnavailable, RecoveryUncertain, StorageFailure, Busy,
    InputBackpressure { next_offset: u64, available_credit: u32 },
}
pub type Result<T> = std::result::Result<T, EngineError>;
```

`OutputStatus.retained` describes the physical committed prefix; expiration sets it to zero while preserving `produced` and returning the expired interval. Reads at a cursor inside a known gap return no bytes, that gap, and `next_cursor` advanced to its known end. An unknown tail cannot be skipped: return its start and terminal status. Clients must display gaps. Arithmetic uses checked additions; no unbounded gap list.

`CommandRequest` is immutable once accepted. Limit argv to 256 entries and a total of 64 KiB including separators; executable maximum 4 KiB, shell string maximum 64 KiB, reject all NULs. Limits apply before hashing/encoding. Stdout/stderr frame reads and stdin writes are at most the core default raw payload ceiling (16 KiB); stdin queued bytes are hard-bounded to 64 KiB per job. Read `limit=0` or over-limit is invalid. Jobs reserve two stream caps against aggregate spool capacity at admission. Each stream atomically shrinks its reservation to committed retained bytes when closed; expiry releases the remainder, while tombstones remain. Validate aggregate capacity >= two stream caps. Sum of open-stream caps plus closed-stream retained bytes must never exceed aggregate capacity; zero-output completed jobs release their whole reservation immediately. Retention disabled disables age expiry only; it does not disable quotas. Numeric/cap relationships are validated at open.

### Facade and authorization (`lib.rs`, integrator; checks in `admission.rs`)

```rust
pub struct Engine { /* Store, owner lock, policy, in-memory token deadlines */ }
impl Engine {
    pub fn open(policy: EnginePolicy) -> Result<Self>;
    pub fn authorize(&self, principal: PrincipalId, endpoint: EndpointId,
                     server: ServerId, target_uid: u32) -> Result<AuthorizedContext>;
    pub fn issue_execute_token(&mut self, ctx: &AuthorizedContext) -> Result<ExecuteToken>;
    pub fn submit(&mut self, ctx: &AuthorizedContext, token: &ExecuteToken,
                  id: InvocationId, request: &CommandRequest) -> Result<Submission>;
    pub fn status(&mut self, ctx: &AuthorizedContext, id: InvocationId)
        -> Result<InvocationStatus>;
    // private facade helper, never exposed as a remote operation:
    fn reconcile(&mut self, job: JobId) -> Result<()>;
    pub fn begin_input(&mut self, ctx: &AuthorizedContext, id: InvocationId)
        -> Result<InputStatus>;
    pub fn write_stdin(&mut self, ctx: &AuthorizedContext, id: InvocationId,
        generation: InputGeneration, offset: u64, bytes: &[u8],
        fresh_for: std::time::Duration) -> Result<InputReceipt>;
    pub fn stdin_eof(&mut self, ctx: &AuthorizedContext, id: InvocationId,
        generation: InputGeneration, final_offset: u64) -> Result<InputStatus>;
    pub fn read_output(&mut self, ctx: &AuthorizedContext, id: InvocationId,
        stream: Stream, cursor: u64, limit: usize) -> Result<OutputChunk>;
    pub fn cancel(&mut self, ctx: &AuthorizedContext, id: InvocationId)
        -> Result<InvocationStatus>;
    pub fn maintain(&mut self, max_jobs: u32) -> Result<MaintenanceReport>;
}
```

`authorize` is a local trusted adapter boundary, not cryptographic peer authentication. It requires exact configured server, allowed principal, and the actual effective UID; endpoint is provided by the trusted caller. Context fields are `pub(crate)` for the admission module but private to external callers, and each operation rechecks the tuple against the currently held policy; server/policy tuple authorization is rechecked; a context from another server or disallowed tuple is rejected, while the same allowed tuple remains authorized at this local trusted boundary. All methods reject root, including worker open. Status/output ownership is principal/server/target, so a new authenticated endpoint for the same principal can resume. Execute tokens and input generations additionally bind to endpoint. A generation issued by `begin_input` replaces the old generation, restarts its offsets at zero, increments a durable sticky gap counter for abandoned input, and never silently replays it. Repeating `begin_input` rotates again; it is deliberately explicit, not automatic on `status`.

There is no public account selector/environment/cwd override on submissions, no public `Store`, and no method to manufacture an unchecked context. Phase 2 must validate identity/device/room membership before calling `authorize`; callers of a local same-UID library already possess local execution authority. `maintain` is local operator maintenance and accepts 1..=128 jobs per call. It does not need remote context and is never exposed directly to a room.

`fresh_for` is 1ms..=60s, measured from local receipt; the future adapter must first reject stale transport requests and subtract their authenticated age. Record an absolute `CLOCK_BOOTTIME` deadline with boot identity, so workers share the clock and suspend time counts. Expired undelivered input fails that generation with a gap. Token issuance also keeps an in-memory boot-clock deadline; tokens from previous frontend epochs are invalid. Time seams are private injectable test dependencies, never production environment variables.

### Internal storage boundary (`store.rs` and `admission.rs`, Agent A)

Everything below is `pub(crate)`, preventing external code from bypassing authorization. The worker uses the same schema/encoding through `Store`; Agent B writes no SQL. Method calls use individual short transactions, a 2-second busy timeout, and checked compare-and-swap. Transient BUSY/contended short locks return `Busy`; workers retry with bounded 10..250ms backoff for up to 30 seconds, continuing bounded pipe draining/backpressure without repeating child spawn or pipe writes. Only exhaustion or nonretryable storage failure triggers owned-group failure cleanup. Frontend returns Busy for caller retry. Never treat one busy timeout as durable storage loss. Public facade methods authorize before looking up a job. Store admission independently rechecks owner/token binding atomically.

```rust
pub(crate) struct OwnerKey {
    pub principal: PrincipalId, pub server: ServerId, pub target_uid: u32,
}
pub(crate) struct InputOwner { pub owner: OwnerKey, pub endpoint: EndpointId }
pub(crate) struct BootTime { pub boot_id: [u8; 16], pub millis: u64 }
pub(crate) struct ProcessIdentity {
    pub boot_id: [u8; 16], pub pid: i32, pub start_ticks: u64,
}
pub(crate) struct WorkerCredential(pub [u8; 32]); // redacted Debug
pub(crate) struct WorkerClaim {
    pub job: JobId,
    pub request: CommandRequest,
    pub account: AccountSpec,
    pub limits: EngineLimits,
}
pub(crate) struct AccountSpec {
    pub uid: u32, pub gid: u32, pub home: std::path::PathBuf,
    pub shell: std::path::PathBuf, pub cwd: std::path::PathBuf,
    pub user: Vec<u8>, // environment assembled only by launcher, deterministic PATH
}
pub(crate) struct PendingJob { pub job: JobId, pub state: InvocationState }
pub(crate) enum StartDecision { Reserved, Deferred, FinishedNotStarted }
pub(crate) struct OrphanPage { pub jobs: Vec<JobId>, pub next: Option<JobId> }
pub(crate) enum ClaimResult { Claimed(WorkerClaim), AlreadyClaimed }
pub(crate) struct Delivery {
    pub record: i64, pub generation: InputGeneration, pub offset: u64,
    pub bytes: Vec<u8>, pub deadline: BootTime,
}
pub(crate) enum InputWork { Idle, Deliver(Delivery), Eof, Failed }
pub(crate) struct Controls { pub cancel_requested: bool, pub generation: Option<InputGeneration> }
pub(crate) struct OutputCommit {
    pub retained: u64, pub produced: u64, pub closed: bool,
    pub gaps: Vec<MissingRange>,
}
pub(crate) struct CleanupCandidate { pub job: JobId, pub delete_record: bool }
pub(crate) struct Store { /* connection, schema */ }
impl Store {
    pub fn create_frontend(root: &StateRoot, policy: &EnginePolicy,
        account: &AccountSpec, epoch: [u8; 16]) -> Result<Self>;
    pub fn open_worker(root: &StateRoot) -> Result<Self>;
    pub fn issue_token(&mut self, owner: &InputOwner, epoch: [u8; 16],
        token_hash: [u8; 32], issued: BootTime, expires: BootTime) -> Result<()>;
    pub fn check_existing(&mut self, owner: &InputOwner, token_hash: [u8; 32],
        id: InvocationId, request: &CommandRequest) -> Result<Option<Submission>>;
    pub fn preflight_admit(&mut self, owner: &InputOwner, epoch: [u8; 16],
        token_hash: [u8; 32], now: BootTime) -> Result<()>;
    pub fn reap_tokens(&mut self, now: BootTime, limit: u32) -> Result<u32>;
    pub fn admit(&mut self, owner: &InputOwner, epoch: [u8; 16],
        token_hash: [u8; 32], now: BootTime, id: InvocationId,
        request: &CommandRequest, job: JobId, credential_hash: [u8; 32])
        -> Result<Submission>;
    pub fn job_for(&mut self, owner: &OwnerKey, id: InvocationId) -> Result<JobId>;
    pub fn status(&mut self, owner: &OwnerKey, job: JobId) -> Result<InvocationStatus>;
    pub fn job_state(&mut self, job: JobId) -> Result<InvocationState>;
    pub fn job_exists(&mut self, job: JobId) -> Result<bool>;
    pub fn reserve_worker_start(&mut self, job: JobId, now: BootTime)
        -> Result<StartDecision>;
    pub fn release_worker_start(&mut self, job: JobId) -> Result<()>;
    pub fn pending_jobs(&mut self, limit: u32) -> Result<Vec<PendingJob>>;
    pub fn worker_identity(&mut self, job: JobId) -> Result<Option<ProcessIdentity>>;
    pub fn claim_worker(&mut self, job: JobId, credential: &WorkerCredential,
        identity: &ProcessIdentity) -> Result<ClaimResult>;
    pub fn mark_running(&mut self, job: JobId, worker: &ProcessIdentity,
        child: &ProcessIdentity, pgid: i32) -> Result<()>;
    pub fn finish(&mut self, job: JobId, worker: &ProcessIdentity,
        outcome: ExitOutcome) -> Result<()>;
    pub fn mark_unrecoverable(&mut self, job: JobId,
        expected: InvocationState, state: InvocationState) -> Result<()>;
    pub fn begin_input(&mut self, job: JobId, owner: &InputOwner,
        generation: InputGeneration) -> Result<InputStatus>;
    pub fn queue_input(&mut self, job: JobId, owner: &InputOwner,
        generation: InputGeneration, offset: u64, bytes: &[u8],
        now: BootTime, deadline: BootTime) -> Result<InputReceipt>;
    pub fn queue_eof(&mut self, job: JobId, owner: &InputOwner,
        generation: InputGeneration, final_offset: u64) -> Result<InputStatus>;
    pub fn controls(&mut self, job: JobId, worker: &ProcessIdentity) -> Result<Controls>;
    pub fn take_input(&mut self, job: JobId, worker: &ProcessIdentity,
        now: BootTime) -> Result<InputWork>; // marks one record Delivering atomically
    pub fn advance_input(&mut self, job: JobId, worker: &ProcessIdentity,
        record: i64, written_total: u32) -> Result<()>; // cumulative within record
    pub fn fail_input(&mut self, job: JobId, worker: &ProcessIdentity,
        generation: InputGeneration, uncertain: bool) -> Result<()>;
    pub fn close_input(&mut self, job: JobId, worker: &ProcessIdentity) -> Result<()>;
    pub fn request_cancel(&mut self, owner: &OwnerKey, job: JobId) -> Result<()>;
    pub fn output_status(&mut self, owner: &OwnerKey, job: JobId,
        stream: Stream) -> Result<OutputStatus>;
    pub fn worker_output_status(&mut self, job: JobId, worker: &ProcessIdentity,
        stream: Stream) -> Result<OutputStatus>;
    pub fn commit_output(&mut self, job: JobId, worker: &ProcessIdentity,
        stream: Stream, commit: &OutputCommit) -> Result<()>;
    pub fn retention_now(&mut self, clock: BootTime, wall_seconds: u64) -> Result<u64>;
    pub fn cleanup_candidates(&mut self, now_seconds: u64,
        limit: u32) -> Result<Vec<CleanupCandidate>>;
    pub fn expire_output(&mut self, job: JobId) -> Result<u64>;
    pub fn delete_tombstone(&mut self, job: JobId) -> Result<()>;
}
```

Agent A also provides these admission helpers: `validate_policy(&EnginePolicy) -> Result<AccountSpec>`, `authorize(&EnginePolicy, &AccountSpec, PrincipalId, EndpointId, ServerId, u32) -> Result<AuthorizedContext>`, `context_owner(&EnginePolicy, &AccountSpec, &AuthorizedContext) -> Result<InputOwner>`, `random_bytes<const N: usize>() -> Result<[u8; N]>`, and `sha256(&[u8]) -> [u8; 32]`. `AccountSpec`, IDs and policy persistence use only validated server-owned values. To keep unsafe confined to the launcher, admission validation uses launcher helpers `local_account(cwd: Option<&std::path::Path>) -> Result<AccountSpec>` and `boot_time() -> Result<BootTime>`; both are `pub(crate)`, owned by Agent B, with stub declarations established by the integrator before parallel edits. Account lookup/root/real-effective UID checks and Linux clock/proc syscalls belong to launcher, not admission. `create_frontend` atomically installs the new epoch, expires all previous unconsumed tokens, rotates all input generations to None, and fails queued undelivered old input; it does not modify the running/terminal job state or steal worker ownership. A worker checks `controls` before every bounded nonblocking stdin write; a generation change discards its local remainder and records the previous generation's gap. This fences queued input; a nonblocking write already in progress at the generation transition may complete, an explicit OS-boundary uncertainty rather than an exactly-once/fence guarantee.

`admit` computes the canonical request digest itself. Exact duplicate resolution is scoped to the owner and original token hash and occurs before the new-token epoch/expiry checks, but never bypasses current authorization. A duplicate with another token is rejected; callers recover status rather than consume fresh authority on an old ID. The facade must still call `admit` for expired in-memory tokens so exact retries can return Existing; only genuinely new admissions require a live in-memory deadline. `admit` returns Accepted only for the insertion winner. The facade gets the job through `job_for`; `Submission` never exposes internal paths/credentials.

Stdin dedup metadata covers at most the last 64 KiB of accepted writes and 256 records per job, including queued records; enforce both bounds. Identical retries require exact generation/offset/length/hash and are acknowledged even if their original delivery deadline elapsed. New writes require fresh deadline. `take_input` never returns a Delivering record again; its owner continues through partial writes using `advance_input`. `controls`/`take_input` expire old records before delivery; `controls` may obtain the current boot-clock value through launcher::boot_time internally, while take_input uses its explicit now argument. Exact progress after worker death is reported uncertain, never retried by another worker. Unknown/lost transitions close output with explicit uncertain tail. Every worker-mutating method verifies exact saved worker identity, and refuses stale or terminal ownership. `mark_unrecoverable` accepts only Accepted/Launching -> UnknownLaunch or Running -> Lost; it is called only after launcher recovery proves lock/process mismatch. It never runs a command.

### Launch, paths and output boundary (Agent B)

```rust
pub(crate) struct StateRoot { /* verified root directory handle */ }
pub(crate) struct FrontendGuard { /* exclusive nonblocking OFD lock */ }
pub(crate) struct JobGuard { /* exclusive nonblocking lifetime OFD lock */ }
pub(crate) struct IoGuard { /* exclusive short per-job OFD lock */ }
pub(crate) struct LaunchSpec { pub root: std::path::PathBuf, pub job: JobId }
pub(crate) enum WorkerPresence { Initializing, Live(ProcessIdentity), Absent }
pub(crate) struct ChildSpec {
    pub account: AccountSpec, pub request: CommandRequest,
    pub environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    pub cwd: std::path::PathBuf, pub umask: u32,
}
pub(crate) struct SpawnedChild {
    pub child: std::process::Child, pub identity: ProcessIdentity, pub pgid: i32,
    pub stdin: std::process::ChildStdin,
    pub stdout: std::process::ChildStdout, pub stderr: std::process::ChildStderr,
}
pub(crate) enum ChildSpawnError {
    NotStarted, // OS spawn/exec error proves no command executed
    Uncertain,  // child may exist, identity/setup could not be durably established
}
pub(crate) fn spawn_child(spec: &ChildSpec)
    -> std::result::Result<SpawnedChild, ChildSpawnError>;
impl StateRoot {
    pub fn open(path: &std::path::Path) -> Result<Self>;
    pub fn open_existing(path: &std::path::Path) -> Result<Self>; // worker: no root/DB creation
    pub fn path(&self) -> &std::path::Path;
    #[cfg(feature = "fault-injection")]
    pub fn read_fault(&self, job: JobId) -> Result<Option<Vec<u8>>>; // at most 256 bytes
    #[cfg(feature = "fault-injection")]
    pub fn read_frontend_fault(&self) -> Result<Option<Vec<u8>>>; // root/fault, at most 256 bytes
    pub fn lock_frontend(&self) -> Result<FrontendGuard>;
    pub fn prepare_db(&self, create: bool) -> Result<std::path::PathBuf>;
    pub fn verify_db_files(&self) -> Result<()>;
    pub fn list_job_dirs(&self, limit: u32, cursor: Option<JobId>) -> Result<OrphanPage>;
    pub fn prepare_job(&self, job: JobId, credential: &WorkerCredential) -> Result<()>;
    pub fn read_credential(&self, job: JobId) -> Result<WorkerCredential>;
    pub fn lock_job(&self, job: JobId) -> Result<JobGuard>;
    pub fn lock_io(&self, job: JobId) -> Result<IoGuard>;
    pub fn presence(&self, job: JobId,
        recorded: Option<&ProcessIdentity>) -> Result<WorkerPresence>;
    pub fn expire_spool(&self, job: JobId, guard: &IoGuard) -> Result<()>;
    pub fn remove_job(&self, job: JobId, guard: &IoGuard) -> Result<()>;
}
pub(crate) fn trusted_worker_executable(path: &std::path::Path) -> Result<std::path::PathBuf>;
pub(crate) fn spawn_worker(executable: &std::path::Path, spec: &LaunchSpec) -> Result<()>;
pub(crate) fn process_identity(pid: i32) -> Result<ProcessIdentity>;
pub(crate) struct Spool { /* one stream file, bounded scratch */ }
impl Spool {
    pub fn open(root: &StateRoot, job: JobId, stream: Stream,
        committed: &OutputStatus, cap: u64, guard: &IoGuard) -> Result<Self>;
    pub fn append(&mut self, bytes: &[u8], guard: &IoGuard) -> Result<OutputCommit>;
    pub fn close(&mut self, guard: &IoGuard) -> Result<OutputCommit>;
    pub fn read(root: &StateRoot, job: JobId, stream: Stream,
        status: &OutputStatus, cursor: u64, limit: usize,
        guard: &IoGuard) -> Result<OutputChunk>;
}
// lib.rs exposes only this worker entry shim for the separate binary target:
pub fn worker_main(args: Vec<std::ffi::OsString>) -> std::process::ExitCode;
```

`StateRoot`, guards, path operations, `spawn_worker`, `spawn_child` and process identity live in `launcher.rs`; `Spool` lives in `spool.rs`; launcher supplies private safe directory-relative file open/read/remove helpers for spool so spool contains no unsafe (Agent B owns both modules and may choose their private signatures); `worker_main` delegates to `worker.rs`. The integrator reexports only the worker entry shim (`#[doc(hidden)]`); binary calls it with `args_os().skip(1).collect()`. Exactly two worker arguments are accepted: absolute state-root path and canonical lowercase 32-hex JobId. No configurable command, UID, file path, credential or environment is accepted by the worker CLI.

`Engine::open` refuses root, validates policy/account, opens the safe root and takes `FrontendGuard` **before** changing database epoch or recovering jobs. Hold that guard until Engine drop. Worker executable must be absolute, regular, executable, owned by root or effective UID, and neither it nor its parent directories writable by another user/group; do not inherit environment for spawning it. No implicit PATH search or current-executable fallback. The child command environment is exactly `HOME`, `USER`, `LOGNAME`, `PATH=/usr/bin:/bin`, and `LANG=C`; no other variables are inherited. Resolve and store the canonical trusted executable path at open. The trusted installation is not a defence against code already running under the same UID.

Private generated layout: `engine.sqlite3`, `frontend.lock`, `jobs/<jobhex>/{credential,worker.lock,io.lock,stdout,stderr}`. Job directory and credential file exist durably **before** `admit`; credentials are generated by frontend, written via exclusive no-follow create, synced with directory, and their SHA-256 stored in the invocation. Unadmitted directories are harmless bounded orphans and cleaned during maintenance using the same locks. Worker obtains `JobGuard`, reads credential without following links, verifies it against the immutable job record, and atomically claims accepted -> launching with its process identity. The claim is its durable readiness handshake. Frontend polling observes readiness for at most 2 seconds; Accepted still initializing is returned truthfully and retried only through bounded recovery, never marked Running based on spawn success. `spawn_worker` uses setsid and null stdio; it reaps its worker child asynchronously without kill-on-drop. Worker lifetime is independent of frontend, including its pipe handles.

`presence` uses Linux `F_OFD_GETLK` to inspect the existing lifetime lock without acquiring it; locks are obtained with `F_OFD_SETLK`. Never probe by acquiring an otherwise free lock. A busy lock and verified recorded identity yields Live; a busy lock without recorded identity yields Initializing for at most 2 seconds of a recovery attempt. A free lock yields Absent even if a numeric PID exists; an unverifiable/mismatched recorded identity never yields Live. Reconcile under short transactions and retry changed states rather than overwriting them. Never spawn from Launching/Running; Accepted can spawn another worker only after verified absence (the job-lock/CAS still excludes duplicate claim). Do not hold an SQLite transaction while waiting on process/file locks. Initializing timeout is an operational uncertainty, not permission to steal a lock or duplicate work.

All state paths use verified directory-relative opens with `O_NOFOLLOW`; root ancestry must not be writable by other users except sticky system ancestors such as `/tmp`, while the final state root is owned 0700. SQLite receives a verified absolute database path under that root; same-UID adversaries are out of scope. WAL/SHM mode checks happen after connection setup. Temporary roots owned by integration tests are supported. `IoGuard` is held during spool sync plus `commit_output`, reads, and expiry. `Spool::open` truncates to the committed retained watermark under that guard. `append` syncs new prefix bytes before returning a commit, continues counting/discarding after cap, and makes a permanent storage-failure tail after an append/sync error. Ordinary spool I/O failure is represented in OutputCommit, not an error that stops pipe draining. `Result::Err` is for unusable state/invalid metadata where a safe durable commit is impossible. The worker records the commit before releasing the guard. Final spool bytes/stream metadata are committed before `finish`, which atomically verifies or finalizes both stream closures; cleanup holds `IoGuard`, deletes content before `expire_output`, and interrupted expiry is recovered as an explicit expired/storage gap, never returned as retained bytes. Tombstone deletion acquires the job lifetime lock before the I/O lock, rechecks retention/state, and never unlinks a lock that a live process holds. Global job lock order is JobGuard before IoGuard; acquisitions are nonblocking with bounded retry and no transaction held. Read-only frontend operations acquire only IoGuard, never subsequently JobGuard. Filesystem orphan cleanup uses a bounded candidate list and never scans arbitrary external paths.

Cancellation machinery stays private to `worker.rs`: save child identity and PGID at spawn, observe exit with `waitid(P_PID,...,WEXITED|WNOHANG|WNOWAIT)`, TERM group on request, retain unreaped leader through the monotonic grace, KILL remaining group, bound drain, then reap and commit its actual exit outcome. If group cleanup is required after natural leader exit, use the same identity-verified sequence. A failed identity/group check records lost/uncertain state and never signals another PID. A worker never adopts another worker's child or replays a Delivering stdin record. Explicit termination on unrecoverable storage failure uses this same owned-group cleanup; it is not inferred to be user cancellation.

These interfaces are the initial freeze. If implementing a safety invariant requires a changed signature, agents stop that affected edit and coordinate a documented additive correction with the integrator; neither invents an incompatible second representation. The integrator resolves any review blocker before parallel implementation begins.

### Review corrections: exact lifecycle and operational rules

The rules in this section complete the frozen contract; they are part of the implementation acceptance criteria.

**Admission and bounded launch.** `submit` validates current context/request, then calls `check_existing` before creating a directory. That method compares the canonical request digest first (`InvocationConflict` on mismatch), then the original token hash and endpoint (`InvalidToken` on mismatch); an exact match returns Existing despite expired token/epoch. An ID owned by another principal is indistinguishable from absence; status/output/cancel return `NotFound` for both. Context authorization itself can return `Unauthorized`. `job_for` is only a scoped lookup and cannot stand in for replay validation. If no existing invocation matches, the frontend calls `preflight_admit` (token, epoch, freshness, quotas), checks its in-memory token deadline, creates/syncs the random credential directory, and calls atomic `admit` which repeats all checks. On every error or Existing result it immediately removes its unadmitted directory using JobGuard -> IoGuard; failures are retained as bounded operator-visible orphan work, and further admission pauses after 128 such orphans until maintenance succeeds. At most one unadmitted directory is being prepared per Engine because submission requires `&mut self`. A crash leaves at most one such directory per frontend epoch; startup bounded orphan cleanup must free capacity or refuse new admission. Never use an existing invocation's directory for loser cleanup.

Every successful `spawn_worker` invocation, including a spawn that returns before readiness, consumes an attempt; definite spawn failures release the short starting reservation. `reserve_worker_start` atomically allocates a boot-clock 2-second starting slot, increments `launch_attempts`, and returns Reserved only for Accepted jobs within the configured starting quota. Live reservations count with Launching workers against that quota; a claim consumes its reservation and a running/terminal transition releases it. `release_worker_start` releases only the reservation, never rolls back the attempt. Expiry permits a new attempt only after reconciliation proves absence; it does not prove a worker dead. After 3 attempted worker starts, a still-Accepted job with no claimant becomes Exited/NotStarted atomically, both streams closed and reservations released. The CAS prevents a late worker from claiming it. A worker that claims and then fails is never retried. Interrupted launch where the command may exist remains UnknownLaunch, not NotStarted. A spawn failure proven to precede command exec is NotStarted; ambiguous post-spawn identity/setup errors are `ChildSpawnError::Uncertain`, trigger safe owned-child cleanup where identity is known, and transition Launching to UnknownLaunch.

`request_cancel` performs Accepted -> Exited/NotStarted atomically if no worker has claimed; both output streams close in that transaction. Otherwise it records cancellation for the worker. After committing Launching and before `spawn_child`, the worker checks cancellation again and calls `finish(..., NotStarted)` if requested. `finish` permits Launching -> Exited only for NotStarted and Running -> Exited for actual Code/Signal; it atomically closes both stream metadata and releases reservations. Normal workers first commit their final output so finish does not hide a buffered tail. Startup/launch failures produce NotStarted only when no command could have run. Natural outcomes already committed cannot be overwritten by cancel.

`reconcile` is one bounded internal facade path: inspect current state, ask `presence`, retry changed CAS state, and for absent Accepted workers reserve/start, absent Launching mark UnknownLaunch, absent Running mark Lost. Never retry child launch. For Initializing, wait at most 2 seconds in 10..50ms steps; return Busy/uncertain status without stealing a lock. Call reconcile at open for all active jobs (bounded by hard active limit), before nonterminal status/read_output/cancel responses, and from maintain for its bounded active-job batch. Thus worker death and deferred starts are detected without a frontend restart. `cancel` rereads terminal status after reconciliation before setting a request. `maintain` advances a persistent bounded cursor through active jobs and cleanup candidates so early long-lived jobs cannot starve later entries; implement that cursor privately in Store, no second public API. `pending_jobs(limit)` supplies a bounded page; at frontend open its private cursor is reset and the caller enumerates all active jobs (at most 128), while maintain resumes its persistent cursor for fairness. A frontend still alive must report Lost on the next status after its worker is killed.

**Token limits and clocks.** Outstanding means unexpired, unconsumed execute tokens; default per-principal cap is `min(128, outstanding_tokens)` in addition to the global cap. `issue_token` reaps up to 128 stale tokens first; consumed token hashes and owner/endpoint bindings live with invocation replay records, even when token rows expire. `reap_tokens(now, limit)` accepts 1..=128 and only removes expired/old-epoch unused authority or consumed issuance rows whose replay binding remains in the invocation. `maintain` reaps a bounded token batch, reconciles jobs, and cleans output/tombstones/orphans. On quota error the facade performs one maintenance batch (up to 32 jobs/tokens) and retries admission once, before creating a directory. The engine has no background maintenance thread: callers must invoke maintain at least once a minute while serving, plus lazy work on operations. Losing the frontend stops maintenance, not commands.

`retention_now` persists a logical seconds anchor with boot ID, wall timestamp and boot-clock milliseconds. Within the same boot advance it by elapsed CLOCK_BOOTTIME; ignore wall jumps. On boot change set the new anchor to max(last logical seconds, supplied wall seconds), never decreasing. Output/tombstone ages and persisted completion timestamps use only that logical clock, not direct `SystemTime` comparisons. `cleanup_candidates(now_seconds, limit)` receives the current logical value returned by `retention_now`; persisted policy supplies both output retention (`Option<u64>`, including None for disabled expiry) and tombstone TTL, so now_seconds is not a TTL/duration. No retention-duration parameter is passed to this method. Document that elapsed time across reboot depends on the operator's wall clock; within-boot forward/backward changes cannot cause premature pruning. Tokens/input use boot-clock deadlines and boot ID, never this retention clock. Tests cover both directions of clock change and reboot identity changes.

**Private data and retention.** `claim_worker` reads/validates the immutable request and NULLs stored argv/shell bytes in the same successful claim transaction; the worker thereafter owns the validated request in private memory. Accepted work retains request bytes only until claim, cancel, or exhausted-launch finalization. `advance_input` NULLs each completely delivered payload, retaining only bounded hash/length/offset metadata; partial-record bytes remain until complete or failed. `finish`, NotStarted finalization, and `mark_unrecoverable` NULL every remaining queued/delivering stdin payload and command payload. No raw request/input appears in Debug/errors/logs. This is logical deletion only: SQLite pages, WAL, OS caches and backups may retain bytes. No forensic secure-erasure guarantee; private state permissions and operator disk encryption/backup policy remain necessary.

Output expiry and tombstone expiry are separate. A retained output stream prevents tombstone deletion even when its minimum 48-hour age passed; disabled output expiry may retain the associated record indefinitely and eventually hit the record quota. `cleanup_candidates` sets delete_record only when terminal, old enough, and no retained output remains. With JobGuard -> IoGuard held, delete spool files, call `expire_output`, then remove the remaining generated directory, and only then call `delete_tombstone`. Crash after file removal but before metadata update recovers an explicit missing interval; crash before row deletion retries idempotently. Never delete a row while its files remain. `list_job_dirs(limit, cursor)` paginates only canonical server-generated JobIds in lexical order (at most 128); ignores unrelated names with a bounded diagnostic, and refuses unsafe/symlink entries. `job_exists` plus JobGuard -> IoGuard permits bounded orphan cleanup without deleting another invocation. Missing already-deleted paths are idempotent success; no recursive traversal outside a verified job dir. `list_job_dirs` tolerates a root-level fault sentinel and recognizes canonical job directories containing a fault sentinel; `remove_job` explicitly removes a regular no-follow job/fault file along with known generated files. Sentinels never prevent cleanup and production builds never interpret their contents.

**Descriptor/identity and filesystem rules.** Open every private descriptor with `O_CLOEXEC` or equivalent: state/lock/spool/proc, SQLite and any helper pipes. SQLite connections enable/verify close-on-exec behavior; spawned commands receive only stdio descriptors. In each worker/command pre-exec path mark every inherited fd >=3 close-on-exec using Linux close_range(CLOSE_RANGE_CLOEXEC), or an equivalent bounded verified descriptor sweep, so a host embedding the library cannot leak its unrelated non-CLOEXEC descriptors into a worker/command. Preserve the standard spawn error-reporting pipe until exec; CLOEXEC marking preserves its pre-exec error reporting. `spawn_child` is the single same-user launch seam that phase 5 can replace. It validates the immutable ChildSpec account, cwd, fixed environment and umask 077; no plugin trait or privileged implementation is added now. Both worker and launcher require euid == ruid == account.uid == owner of the state root and reject zero. Match persisted server and UID at every frontend/worker store open. `/proc/<pid>/stat` parsing handles a command name containing spaces and parentheses by parsing fields after the final closing parenthesis, with strict field count/numeric validation.

`StateRoot::prepare_db(true)` securely precreates the database 0600 with no-follow exclusive creation when absent, or verifies an existing regular owned 0600 file before SQLite opens it. It also checks for preexisting unsafe WAL/SHM paths before connection setup. `Store::create_frontend`/`open_worker` never open an unverified root/database. Frontend compares persisted server/UID with policy/account; worker compares persisted UID with its real/effective UID and rejects job rows whose server/target disagrees with the stored policy before claim. Before worker SQLite open, worker constructs `StateRoot::open_existing`; `Store::open_worker(&StateRoot)` uses launcher `local_account(None)` to reject root/UID mismatches without unsafe in store. Both Store constructors call `root.prepare_db(create)` before SQLite open: true for frontend, false for worker. That launcher-owned method performs directory-relative no-follow DB creation/validation and pre-open WAL/SHM checks and returns the verified database path. Both constructors then call `root.verify_db_files()` immediately after connection setup before returning; facade/worker may additionally recheck. Store never constructs a raw unverified database path. Worker open never creates a missing database/schema. The integrator calls `verify_db_files` immediately after frontend setup; worker does the same after its connection opens; verify DB/WAL/SHM ownership, regular-file/no-symlink type, 0600 mode and noninheritance. WAL/SHM may be absent when SQLite has legitimately removed them. Reject insecure files rather than silently chmod an attacker-controlled path.

Nonacquiring OFD presence probes prevent lock-probe interference with a worker's initialization. Every multi-lock operation follows JobGuard -> IoGuard. Reads take only IoGuard, then load output metadata, then read file bytes under the same guard. No lock wait occurs while holding a SQLite transaction, and a busy IoGuard returns Busy for bounded caller/worker retry. Tests kill the frontend while workers run and immediately reopen the engine; inspect command descriptors to prove no frontend/worker lifetime lock survived exec (the inspection helper may temporarily open its own `/proc` directory FD and must exclude that helper-created FD).

**Worker service loop.** Poll control/input state every 50ms under activity and back off to 250ms when idle. Drain pipes concurrently using bounded scratch buffers; batch spool sync/metadata at 64 KiB or 100ms, whichever occurs first, and always before closing a stream/finishing. These batching buffers are bounded; uncommitted output may be lost after worker death and is reported as uncertain tail. On transient database BUSY preserve the latest pending metadata and bounded output/input buffers, retry within the 30-second budget, and allow OS pipe backpressure rather than unbounded allocation. Never replay a partial write when retrying a failed progress commit; keep its written_total in memory and retry only that idempotent metadata update.

Install minimal SIGTERM/SIGHUP handlers that set an atomic shutdown flag (no SQLite/locking in signal handlers); the loop performs owned-group cleanup. `catch_unwind` around the worker loop and an ownership guard run the same bounded cleanup on Rust panic before exiting nonzero. Use safe unwinding rather than abort for ordinary production panics. SIGKILL, abort, reboot and catastrophic process failure cannot run cleanup and may leave descendants; report Lost/UnknownLaunch and never infer safe resubmission. Stdin EPIPE closes the pipe and marks Closed if no undelivered bytes remain, otherwise sticky Gap; it does not kill the command. Failure cleanup never fabricates user cancellation or a normal exit.

At the stdin record limit `available_credit` is zero; otherwise it is remaining byte capacity, with each write additionally consuming one of 256 record slots. Reject empty writes and a full record window with InputBackpressure even if byte capacity remains. Sticky gap_count survives begin_input rotation, while generation offsets restart at zero. An identical known retry is resolved before deadline checks; a new endpoint cannot use an old endpoint's execute token/input generation. New input after EOF or a generation's failed stream is rejected until explicit allowed generation rotation; EOF never overtakes data. An already executing bounded pipe write can complete during rotation as documented above.

Request frequency limits, per-command CPU/process/memory rlimits and escaped-descendant containment are deferred; no phase-1 claim covers them. Active-job quota limits tracked nonterminal jobs, not every OS process. Lost/UnknownLaunch jobs can leave untracked orphan processes, which are reported for operator intervention without automatic numeric-PID kills. Shell startup files remain account-controlled even under a sanitized environment; direct Exec avoids implicit shell interpretation.

### Fault-injection build and acceptance matrix

The integrator defines `[features] default = []` and `fault-injection = []`. No self dev-dependency enables the feature. The test frontend helper binary lives at `tests/support/frontend.rs`, is integrator-owned, and has `required-features = ["fault-injection"]`; it is unavailable in default builds. The production worker target remains the same binary name, built in separate target directories for production/default and feature test runs.

The integrator owns `src/fault.rs` and shares its exact `FaultPoint` enum with both agents before edits. Its crate-private `fault_point(root: &StateRoot, job: JobId, point: FaultPoint)` is a no-op by default. Paired enum points cover Before/AfterAccepted, Claim, Spawn, Running, InputWrite, InputProgress, SpoolSync, OutputCommit, and ExitCommit. Claim atomically commits Launching, so Claim and Launching are aliases of the same durable boundary, not two transactions. Under the feature, frontend Before/AfterAccepted points read the fixed state-root `fault` file through `StateRoot::read_frontend_fault`; all worker points read `jobs/<id>/fault` through `StateRoot::read_fault` (each at most 256 bytes, private no-follow file). The root-level sentinel can be placed before the random JobId exists. The test helper aborts immediately after accepted commit and before worker spawn; the harness then lists canonical job dirs to discover the generated ID and place its worker sentinel before restarting/reconciling. It never guesses or supplies the server-generated JobId. Only feature code interprets either sentinel; arbitrary contents never execute code. Fault selectors are fixed enum labels, not scripts/paths/commands, and never come from ambient environment or ordinary production CLI. The feature is off by default; production releases use `--no-default-features` explicitly and are built separately from feature-enabled tests so artifact reuse cannot blur the claim.

Required commands, in addition to workspace default checks:

```sh
cargo build --offline --locked -p marmish-engine --release --no-default-features --target-dir target/phase1-production
cargo build --offline --locked -p marmish-engine --release --features fault-injection --bin marmish-worker --target-dir target/phase1-fault
MARMISH_TEST_PRODUCTION_WORKER="$PWD/target/phase1-production/release/marmish-worker" \
MARMISH_TEST_FAULT_WORKER="$PWD/target/phase1-fault/release/marmish-worker" \
cargo test --offline --locked --workspace --features fault-injection
```

Disable core dumps for intentional crash tests with `ulimit -c 0` before running the matrix.

These MARMISH_TEST_* variables are read only by the test harness to select explicit already-built artifact paths; no production worker/frontend reads them or treats ambient environment as a fault selector. The acceptance harness uses the after-accepted frontend abort procedure above to prepare a job containing the known pre-spawn worker sentinel, then starts the actual production worker: it must ignore that feature-only sentinel and run the command once. The feature worker must abort at the same sentinel with the expected UnknownLaunch/no-respawn result. This is a behavioral check against separate actual binaries, not a string scan or a claim based solely on cfg(test). The helper/frontend-crash tests execute a separate process, not simply drop an Engine handle. Feature tests must not accidentally select the production worker or vice versa. Fault coverage includes every named transaction/spawn/input/output boundary above; individual tests use bounded deadlines and cleanup guards, including failure paths. Also cover all B/S corrections below before phase close.

### Claude plan-review dispositions

| Finding | Disposition / implementation requirement |
| --- | --- |
| B1 no never-started outcome | Added NotStarted, atomic stream-closing finish, Accepted cancellation CAS, pre-spawn cancel check, 3-attempt cap, and spawn uncertainty distinction. |
| B2 no live reconciliation | Added private Engine::reconcile, state/presence paths and open/status/read/cancel/maintain triggers; live-frontend worker-kill test required. |
| B3 spool reservations starve admission | Closed streams shrink reservations to retained bytes; explicit aggregate arithmetic/validation and zero-output release. |
| B4 token not transmissible | Explicit byte expose/import, zeroizing storage, redacted Debug, serde-less; authenticated encrypted delivery deferred to phase 2. |
| B5 impossible cfg(test) crash tests | Off-default fault feature, explicit feature test command, required-feature frontend helper, private sentinel, separate production/fault builds and behavior check; rejected self-dev-dependency feature unification. |
| B6 missing child launcher seam | Added ChildSpec, SpawnedChild, spawn_child and NotStarted/Uncertain spawn errors; same-user only. |
| B7 inherited locks | CLOEXEC on all descriptors, exact child stdio inheritance, frontend-death/reopen and child-FD tests. |
| B8 lock probing/order | Nonacquiring F_OFD_GETLK presence, F_OFD_SETLK ownership, universal JobGuard -> IoGuard and bounded Busy retry. |
| B9 request/stdin retention | Claim-time request removal, delivered-payload removal, terminal purges, redacted request/delivery Debug; logical deletion only, no forensic-erasure claim. |
| B10 orphaned output | Output retention gates row cleanup; files/expiry/directory before row; added bounded list_job_dirs/job_exists and restart-idempotent cleanup. |
| B11 preadmission directory leaks | Added digest/token/endpoint-aware check_existing plus cheap preflight and atomic recheck; immediate loser cleanup, bounded orphan admission gate. Bare job_for lookup rejected as replay check. |
| S1 token maintenance | Added bounded reap_tokens, lazy reaping, per-principal cap, one bounded quota retry and documented periodic maintain requirement. |
| S2 retention clock | Persisted logical wall/boottime anchor; same-boot wall jumps ignored; reboot behavior explicit and tested. |
| S3 transient BUSY | Busy error, BEGIN IMMEDIATE, bounded worker backoff/draining and 30-second budget; no kill on first timeout. |
| S4 fatal worker paths | Panic unwind guard and SIGTERM/SIGHUP flags trigger owned cleanup; EPIPE handling and uncatchable-failure limits explicit. |
| S5 polling/commit cadence | 50..250ms control polling; 64KiB/100ms bounded spool commit batching. |
| S6 read consistency | IoGuard before metadata snapshot before spool read, held throughout. |
| S7 DB ownership | Secure create/preopen checks, verify_db_files, server/UID store pin and CLOEXEC checks. |
| S8 worker identities | Equal nonzero real/effective/account/file UID and robust proc-stat parsing test. |
| S9 stdin edge cases | Empty writes rejected, byte/record credit enforcement, per-generation zero offsets and sticky gap count. |
| S10 precedence/uniformity | Digest then token/endpoint checks; owner-bound status/output/cancel; nonexistent/cross-owner NotFound; new endpoint old token InvalidToken. |
| S11 shell hooks | Removed impossible no-startup-hooks claim; documented same-user shell rc-file execution, with sanitized hook environment. |
| S12 retention/Clone | Opaque Retention default seconds() terminology and Clone for EngineLimits/model types. |
| S13 quota limits | Request-rate/rlimits deferred and tracked-job versus orphan/descendant limits explicit. |
| S14 dependency/unsafe | Exact new dependency versions pinned above; integrator lock/fetch evidence; unsafe confined to launcher/worker. |


### Second Claude plan-review dispositions

| Finding | Disposition |
| --- | --- |
| R1 random job ID and unavailable test artifacts | Root-level frontend sentinel works before acceptance; after-accepted abort exposes the generated job directory for worker sentinel setup. Build both actual worker artifacts before feature tests; pass explicit absolute artifact paths to test harness only. Cleanup tolerates both sentinel locations. |
| R2 private APIs inaccessible to integration tests | Assigned private tests moved to cfg(test) in-module src/<module>/tests.rs; tests/ integration files use only the public Engine API or helper processes. No public storage bypass introduced. |
| R3 ambiguous database ownership/order | Store constructors take &StateRoot and call launcher-owned prepare_db(true frontend / false worker) before SQLite, then verify_db_files after setup; worker root is existing-only. |
| R4 ambiguous retention parameter | cleanup_candidates(now_seconds,limit) takes logical current time; persisted policy supplies independent optional output retention and tombstone TTL. Disabled output expiry is preserved. |
| Guard/finish notes | Spool open/append/close/read require &IoGuard; finish verifies/finalizes stream closure atomically after final spool commit. |

The Store retains its own verified StateRoot handle for admission fault points; its constructors own authoritative pre/post database checks. Additional facade verification is defence in depth. Claude final plan review approved; these three additive clarifications align the introductory prose and helper types with the frozen contract.

### Integration corrections

SQLite permission checks use directory-relative no-follow metadata inspection. They must never independently open/close an existing database, WAL, or SHM file while SQLite owns connections: closing another descriptor can cancel process-wide POSIX locks. Only a missing database is exclusively created before opening SQLite. See [SQLite locking hazards](https://www.sqlite.org/howtocorrupt.html#_posix_advisory_locks_canceled_by_a_separate_thread_doing_close_). Regression tests exercise competing transactions and frontend restart during worker commits.

Cleanup recovers a terminal tombstone whose expired directory was already removed before a crash by deleting the remaining eligible row. A verified absent directory permits a metadata-only output response only for a closed stream with zero retained bytes; no spool bytes are fabricated or read without an I/O guard. Existing directories retain the original guard-before-metadata read order. Prepared-but-unadmitted directories are removed on partial preparation and subsequent admission failures. Quota errors perform one bounded maintenance pass and retry once; they never evict live replay records.

### Implementation verification (2026-10-22)

The detailed plan was approved before implementation by Claude Sonnet 5.5; see [the final plan review](../reviews/phase-1-plan-final.json). Implementation used three agents with the file ownership above and central facade integration.

The frozen implementation passes the default offline workspace suite: 7 core, 42 engine unit, 9 public engine integration, and 9 MDK adapter tests (67 top-level tests). Workspace build, formatting, and Clippy with warnings denied pass. Feature-enabled strict Clippy also passes. A full feature-enabled workspace run with the explicitly selected release workers passes all 73 top-level tests (the default 67 plus 6 crash tests). New dependencies resolved from the existing cache with `cargo generate-lockfile --offline`; no dependency fetch or live MDK/relay probe was used.

The cross-process crash matrix passes 6 tests, with loops covering frontend Before/AfterAccepted, 12 worker claim/spawn/running/spool/output/exit boundaries, and 4 stdin write/progress boundaries. It checks exact-retry no-rerun, bounded certain-not-started worker retries, production ignoring the fault sentinel, and worker survival after frontend SIGKILL. Both actual release workers were independently built offline in separate target directories, one with `--no-default-features` and one with `--features fault-injection`. The rebuilt artifacts were selected explicitly under `target/phase1-production/release/marmish-worker` and `target/phase1-fault/release/marmish-worker`. Core dumps were disabled for intentional aborts (`ulimit -c 0`). The documented commands select those matching release artifact paths explicitly.

Fixture directories require explicit 0700 permissions. BeforeExitCommit may already have fully committed closed output; losing the exit outcome then does not invent an output gap. Earlier crashes with unclosed output retain explicit uncertain tails. A root-mapped frontend probe returned the sanitized root-execution-forbidden error before creating state. The entire engine suite is not run under `unshare -rn`, which maps the caller to forbidden UID 0; mapping only the current nonzero user also leaves trusted root ancestors unmapped. These checks do not relax production UID/ownership policy.

`ripwire --quality-delta` and its cold-parse variant report no preexisting-symbol regressions against the initial empty project HEAD. They also report new-symbol findings, which are not a clean-code verdict: the API parameter shapes follow the reviewed contract, serde/Drop/test entrypoints are not graph-reachable through normal calls, and direct source review is needed for Rust helper complexity. High-complexity admission, reconciliation and worker loops were split into named steps before review. No baseline or acknowledgements were added to suppress findings.

### Initial completed-code review dispositions

Claude Sonnet 5.5 requested changes in [the first code review](../reviews/phase-1-code-initial.json). Its blockers were corrected; another independent completed-code review is required before this phase closes. Idle store polling now uses read-only fast paths. The worker polls controls at 50–250 ms and drains output through readiness polling with bounded per-stream budgets, without empty output commits. A real silent-worker regression observes zero SQLite changes across 800 ms; a public worker test completes 16 MiB of output with a 1 MiB retention cap, a truthful limit gap, and a ten-second test deadline.

Input progress tolerates a conflict only when a fresh snapshot confirms generation rotation; a same-generation conflict remains an error. EOF remains closed across generation changes and frontend restart, while input record identifiers remain monotonic across generations. Dedicated regressions cover these cases. Expiry skips already empty retained output. The crash matrix additionally verifies that spool bytes remain invisible before their metadata commit and committed bytes survive afterward.

The first review's initial crash-suite failures coincided with the fixture permission correction: every helper failed before admission, leaving no token/job path. Explicit fixture 0700 permissions resolved this; the reviewer then passed four consecutive runs. This evidence supports a fixture startup error, rather than its unproven artifact-churn hypothesis. Test sentinels and token writers close before child startup; ordinary file handles are CLOEXEC. No production permission checks were relaxed.

The original M3 claim that start attempts increment before the quota check does not match the implementation. Inspection found a related real bug: a still-live third start reservation must be honored before applying the three-attempt exhaustion rule. Store CAS mismatches remain `Busy` rather than pretending a mutation succeeded; facade reconciliation rereads state and retries within its original two-second bound. Initializing recovery sleeps 50 ms between checks, within the approved range.

Useful optional review guidance is incorporated selectively. Natural completion already retains the unreaped leader until owned-group cleanup; after the leader exits, remaining processes in that verified group are killed before reaping, even if they closed their pipes. Cancellation still uses TERM followed by its configured grace and KILL. Adding an automatic maintenance thread is deferred: the reviewed API deliberately keeps the frontend responsible for scheduling `maintain`. An empty/nonabsolute account shell is currently rejected; no implicit replacement shell is selected. This is an account-configuration requirement, not a command sandbox.

The suggested pre-`setsid` cleanup race does not apply to the current launcher: cleanup runs only after `Command::spawn` succeeds, following the exec-error pipe handshake; its `pre_exec` establishes the session before exec. The worker's unreaped identity checks still apply to all later group signals. No speculative numeric-PID signaling was added.


### Second completed-code review dispositions

[The second Claude review](../reviews/phase-1-code-second.json) accepted the earlier idle polling, output throughput, progress-rotation and EOF fixes, but found an additional blocking interaction: queued EOF followed by expiry of pending input could leave the child pipe open. The correction closes the pipe when a failed input generation already has irrevocable EOF, while preserving the reported Gap/Uncertain health. An ordinary gap without EOF still permits explicit generation rotation.

The maintenance clock is sampled after acquiring the retention transaction, following reconciliation. Sampling before that transaction can race a concurrent worker advancing the durable anchor. Explicit synthetic-clock tests retain their rollback rejection behavior.

A real worker/cat regression confirms EOF delivery, normal completion, and sticky Gap without replaying expired payload. Store tests verify irreversible EOF and ordinary gap rotation separately. The worker excludes writable stdin from readiness polling while its database retry backoff is pending, and schedules input failure immediately upon a failed final control/deadline check. A poll-timing regression covers the former busy spin. Worker failure recovery reloads durable lifecycle state before choosing its conditional transition; early Launching errors no longer retry a Running-only transition for 30 seconds. A regression checks prompt UnknownLaunch and idempotence.


The current implementation source fingerprint is `83b763911b5a8f32339bfb436bfa39414ac7c9d9a2c6ecf7260037df86248a7c` (SHA-256 of workspace manifest, lockfile, engine manifest and engine Rust paths sorted by their relative POSIX strings, each followed by NUL, file contents and NUL). Default workspace verification passes 67 tests. Independently rebuilt production and fault release workers pass the six-test crash matrix after these corrections. Claude approved the production implementation in the final completed-code review below.


### Third completed-code review dispositions

[The third Claude review](../reviews/phase-1-code-third.json) approved the stdin, clock and backoff corrections, then reproduced a partial-preparation crash bug: an orphan directory created before its worker lock caused both maintenance and new submissions to fail. Cleanup now rechecks that no database record exists under the frontend lock, validates the frontend guard belongs to this state root, then creates any missing private worker and I/O locks in order before removing the orphan. Normal admitted-job locking still refuses missing lock files. This also protects an admission storage error whose commit outcome is uncertain: a durable record prevents directory deletion.

Launcher regressions cover empty and credential-only preparation, wrong-root guards, symlink refusal and a busy worker lock. A public integration test reopens with both partial directories, runs maintenance, then proves a new command can be submitted past another partial directory. Crash-tail gap insertion now reuses the existing four-gap bound, preserving an explicit uncertain suffix; a regression verifies both bounded coarsening and repeated-CAS idempotence.


### Final completed-code approval

Claude Sonnet 5.5 returned **APPROVE** with no blocking defects in [the final production code review](../reviews/phase-1-code-final.json). It independently passed default and feature workspace tests, formatting and strict Clippy, and repeated public and launcher suites. This review closes the substantive findings from the earlier three completed-code reviews.

Load testing exposed transient Busy/AlreadyOpen assertions in tests. Test-only changes add bounded retries for frontend lock reacquisition and the output-gap read, retaining the immediate competing-lock denial assertion. Ten full engine unit-suite reruns passed (420 results), and three public-suite runs passed under eight CPU-spinning helper processes. Production code and the independently built production/fault worker artifacts were unchanged by this adjustment. [The focused follow-up review](../reviews/phase-1-code-tests-final.json) also returned **APPROVE**, verified the matching source fingerprint, and independently passed three engine-suite reruns. All runs used nonzero UID 1001; root refusal remains deliberate.

Earlier hash evidence used Python Path-object ordering, which differs from sorting the relative path strings. The current fingerprint uses the explicit procedure below, run from the repository root; it supersedes the earlier hash convention and includes the final test-only adjustments.

```python
from pathlib import Path
import hashlib
paths = {
    Path("Cargo.toml"), Path("Cargo.lock"),
    Path("crates/marmish-engine/Cargo.toml"),
    *Path("crates/marmish-engine").rglob("*.rs"),
}
h = hashlib.sha256()
for p in sorted(paths, key=lambda p: p.as_posix()):
    h.update(p.as_posix().encode() + bytes([0]) + p.read_bytes() + bytes([0]))
print(h.hexdigest())
```
