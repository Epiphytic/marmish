# Native macOS platform and confined transport

This is the detailed design for increment B of the command-policy/platform
initiative (`marmish-b33`). Increment A's additive command restrictions remain
unchanged. Increment C supplies the final native CI and cross-platform Marmot
execution evidence. This document describes intended behavior; native support
is not established until the acceptance gates below pass on the actual runner.

## Platform contract

Support Linux and macOS as native clients and servers with the same versioned
Marmish messages, authenticated principals, durable invocation IDs, command
policy semantics, byte-preserving argv/output and asynchronous command lifetime.
An accepted command continues when its client, Python frontend or MDK transport
disconnects. Transport cleanup must not terminate detached command workers.
Root execution remains refused; macOS support does not implement privileged
identity-to-local-user mapping, interactive sessions or tmux.

Replace the Linux-only crate gate in `crates/marmish-engine/src/lib.rs` with
explicit Linux/macOS platform implementations and an explicit unsupported-target
error. Keep common engine/store semantics in shared code. Introduce a small
private platform boundary beneath `launcher.rs` for descriptor enumeration,
exclusive publication, lifetime locks, boot/monotonic time, process identity
and child descriptor preparation. Preserve the current Linux implementations.
The worker binary and bridge compile against the same public engine contract.

The host executing a command determines its executable paths and policy matches.
There is no cross-platform path translation. A client on either OS can address
a server on either OS, including literal argv with non-UTF-8 Unix bytes.

## Durable engine primitives

State and job traversal remains descriptor-relative: `openat`, `O_NOFOLLOW`,
ownership and private-mode checks apply on both systems. Adapt libc mode types
explicitly; do not assume macOS `mode_t` or `stat.st_mode` has Linux's width.
Canonicalize the existing trusted parent when preparing a state path on macOS,
where `/tmp` and `/var` are symlink aliases, then perform the existing secure
walk and leaf checks. This does not authorize symlinks inside the private state
tree or weaken command-policy canonical-executable checks.

Replace `/proc/self/fd/<jobsfd>` directory iteration with a shared bounded
descriptor-relative helper: open `.` relative to the held directory descriptor
to obtain an independent directory offset, then use `fdopendir`/`readdir` with
RAII ownership. Retain existing entry/page bounds and validate entry names and
types through directory-relative operations. This covers storage discovery,
pristine-state detection and job listing without reopening a mutable pathname.

For publishing `engine.key`, retain Linux `renameat2(RENAME_NOREPLACE)` and use
macOS `renameatx_np(RENAME_EXCL)`. A preexisting destination is an error; no
check-then-overwrite emulation is permitted. Preserve file synchronization,
exclusive publication, directory synchronization and pending-file recovery.

macOS lifetime locks use `flock(LOCK_EX | LOCK_NB)` on separately opened file
descriptions, retained by the existing guard objects. Worker-presence probing
opens an independent description, takes `LOCK_SH | LOCK_NB`, and immediately
unlocks on success. A busy probe requires matching persisted process identity
before reporting live; otherwise the state is initializing. A successful probe
is only a momentary observation, never authority to change durable job state.
Since a shared probe can collide with worker acquisition, replace the worker's
immediate-success-on-`Busy` path with state-aware acquisition: retry with bounded
backoff for five continuous-clock seconds, checking durable claim and holder
identity. Only another verified live worker with the matching active claim
permits duplicate-launch success. Pending, initializing or unverifiable holders
eventually produce an explicit retryable startup error, never silent success.
The frontend uses an eight-second continuous readiness deadline, exceeding the
five-second worker acquisition budget, and retries only still-unclaimed jobs
within that deadline before reporting failure. Durable admission remains recoverable.
Fault-inject a shared probe across worker startup and require execution or that
bounded error, never a job stalled behind a worker that reported success and
exited. Do not substitute POSIX process-owned record locks:
their close behavior would violate the existing guard/presence contract.
Linux retains its open-file-description locks. Runtime state is local to one
host and must not be shared across these lock implementations over network FS.

macOS boot identity is `kern.bootsessionuuid` via `sysctlbyname`; validate and
parse the UUID into the existing fixed-width field. Its elapsed clock is
`mach_continuous_time` converted using `mach_timebase_info` with checked wide
arithmetic. This clock includes suspend, preserving timeout and retention
semantics without relying on wall-clock adjustment. Linux retains boot UUID
and `CLOCK_BOOTTIME`. Unavailable or malformed identity/clock data fails closed.

macOS process identity uses `proc_pidinfo(PROC_PIDTBSDINFO)`, requiring the exact
returned structure size and matching PID. Encode its validated start seconds
and microseconds into the existing opaque start-identity value with checked
arithmetic. Combined with boot UUID and PID, this detects PID reuse. The value
is local platform data, never a cross-host timestamp. Cancellation still checks
worker and child identity and verifies that the child owns its process group
before signaling the group.

For SQLCipher descriptor verification, enumerate macOS descriptors with
`proc_pidinfo(PROC_PIDLISTFDS)` using bounded allocation/retry. Ignore explicitly
non-vnode types; every vnode requires successful `fstat`, `F_GETPATH` and
`F_GETFD`. An uninspectable vnode fails verification, including deleted files;
inspection failure never means irrelevant. Preserve Linux's scope of every
descriptor under the root, not merely DB/WAL/SHM files. Compare `(st_dev, st_ino)`
against held root/jobs and known state-file identities. Establish the kernel's
canonical root spelling and verify it against the held inode. Walk each vnode
path's directory ancestry using held directory descriptors, comparing directory
device/inode identities to the root so aliases and case differences cannot hide
membership. All relevant descriptors require `FD_CLOEXEC`. Fail closed on
unresolved or changing ancestry, nonlocal filesystems or unsupported case
semantics. The native spike must establish APFS alternate-case and `/var` alias
behavior; path-prefix matching alone is forbidden. Directory opens are allowed;
do not reopen any regular file. Do not open or close an extra descriptor for
any SQLite file while verifying it: closing such a descriptor can release
process-owned SQLite locks. Keep the existing Linux `/proc`
inspection behind the platform boundary.

Before spawning, require `RLIMIT_NOFILE` soft limit at most 65,536 and enumerate
all descriptors with bounded `PROC_PIDLISTFDS`, failing if any is at or above
that ceiling (including files opened before a limit was lowered). Marmish must
never raise its soft limit for the remaining engine process lifetime. Audit
engine and linked-library limit changes to maintain this invariant; concurrent
opens cannot allocate above that unchanged soft limit. Do not call `getrlimit`
in `pre_exec`; the parent checks and rejects an excessive limit before each
spawn. Validate the executable
using the existing trusted-executable checks. The macOS `pre_exec` path uses
only async-signal-safe operations: `setsid`, `umask` and `fcntl` to set
`FD_CLOEXEC` on every open descriptor from 3 through 65,535, including numbers
above the current soft limit. Cost is at most 65,533 `fcntl` calls per spawn;
record native latency. Ignore only `EBADF`; other errors
fail the spawn. Do not enumerate proc structures, allocate, log or acquire
Rust/Python locks after fork. Keep Linux `close_range(CLOSE_RANGE_CLOEXEC)`.
Retain detached workers, child process groups and owned-child reaping; no
parent-death termination is introduced.

## Authenticated native RPC

`tools/marmish_rpc.py` must accept the daemon's execution-visible home and
socket paths explicitly. Linux keeps `/state` and `/state/s` inside its mount
sandbox. macOS passes its actual canonical private home and Unix socket path;
the current hardcoded Linux paths must not reach native macOS RPC requests.
Keep private-parent ownership/mode checks and Unix socket length bounds.

Authenticate the connected peer on macOS through libc `getpeereid`, with pinned
ctypes argument/result types, checked return value and expected effective UID.
Also require `LOCAL_PEERPID` to equal the supervised daemon PID and verify its
boot/start identity against the run manifest before issuing RPC. Linux keeps
`SO_PEERCRED`. Socket-path ownership alone is insufficient. Missing
credential support, wrong UID, stale socket or malformed replies fail closed.
The native transport implements the same `ConfinedDaemon` context-manager,
socket, readiness and idempotent-close contract as the Linux implementation.

## Reproducible MDK artifact and exact relay transport

Build macOS `wnd` from a clean checkout of
`https://github.com/liamhelmer-bel/mdk.git` at
`37c248a11239e108afc9bd4ee62b8f60542012e3`, with a maintained, reviewed patch
stored in this repository. Never build from, edit or copy a sibling working
tree. Record source URL/revision, patch digest, Cargo.lock digest, toolchain,
target, deployment target, clean-checkout assertion and executable SHA-256 in a
build manifest. Build with `cargo build --locked --release -p wn-cli --bin wnd`;
ad-hoc sign the Apple Silicon artifact before calculating its final digest.
The launcher verifies the manifest, exclusively copies the artifact into a
fresh private owned directory, hashes the opened copy and executes only that
verified private copy. Reject no-follow/ownership/mode violations and mismatches.
An existing locally installed binary's hash does not establish
this source/patch provenance.

The pinned lock resolves `nostr-sdk` 0.44.1, `nostr-relay-pool` 0.44.3 and
`async-wsocket` 0.13.2. The patch provides one shared SDK-client factory and
injects a custom transport into **every production builder**, including:

* `marmot-app/src/relay_plane/mod.rs`: primary `from_sdk` client.
* `marmot-app/src/relay_plane/directory.rs`: anonymous directory client.
* The same directory module's scoped inspection client, retaining its existing
  optional signer and automatic-authentication configuration.
* `marmot-app/src/lib.rs`: `relay_client_for_account_id`, the signed per-account
  publish client.

The actual SDK contract is `ClientBuilder::websocket_transport(...)` and
`WebSocketTransport: Debug + Send + Sync`, with `support_ping() -> bool` and
`connect(&self, &Url, &ConnectionMode, Duration) -> BoxedFuture<Result<
(WebSocketSink, WebSocketStream), TransportError>>`. Implement this contract
against the pinned crates, preserving ping support. Audit every builder literal
after patching and test all four production paths. A CI source guard rejects
every production SDK builder outside the shared factory, with an explicit
path allowlist and expected count of one. Include aliases in the audit and use
clippy disallowed-methods where supported; test the guard with a forbidden
builder fixture. Non-SDK reqwest clients in audit logging, media, collector
host safety and `directory/open_ranking.rs` remain outside the factory. Disable those optional functions in
restricted mode and test that configuration cannot trigger them; Seatbelt is
the backstop against direct egress if a raw client is reached.

The private launch configuration carries only explicit canonical relay URLs
and the local broker address. It has strict size/count/version validation. The
custom transport first checks the complete canonical URL against that set,
then delegates to the default transport with `ConnectionMode::Proxy` forced to
the broker address, disregarding the caller's requested connection mode. Verify
the transitive `async-wsocket` SOCKS feature in the locked relay-pool build;
`nostr-sdk` 0.44.1 itself has no SOCKS feature. The pinned native proxy implementation uses
SOCKS5 domain requests and preserves the original WebSocket URL for TLS/SNI.
Wrap the entire delegate future, including SOCKS negotiation, in the trait's
timeout: the pinned proxy helper does not bound that whole operation itself.
No direct-connection fallback is allowed. Reject unselected URLs before any
connection, including URLs learned through peer metadata or directory lookup.

Accept the same bounded root `wss://` DNS URLs as Marmish's relay parser: no
userinfo, query, fragment, non-root path or IP literal. Rust patch validation
must agree with Python canonicalization, including ports and host spelling;
share fixtures across both sides. Restricted launch configuration is mandatory
for the Marmish artifact and must be acknowledged before readiness. Pass it in
a private 0600 configuration file with a fresh 256-bit launch nonce. The patch
installs immutable restricted factory state before building any client, then
atomically writes a private acknowledgement containing nonce, configuration
digest and patch-protocol version. The supervisor reads it no-follow, verifies
size/owner/mode/nonce/digest and only then accepts RPC readiness. Remove stale
acknowledgements under the transport lock. Never emit this channel on stdout
or upload it as an artifact. Missing,
invalid or unsupported configuration stops startup. Neither a digest alone nor
a successful relay connection proves that the transport patch is active.

## macOS confinement and broker

Use native `/usr/bin/sandbox-exec` with a generated private deny-default Seatbelt
profile. Use a static profile and `sandbox-exec -D` path parameters; never
interpolate paths into Scheme text. Use canonical `/private/...` temporary
paths; reject NUL/overlong values and test spaces, quotes, parentheses and Unicode.
Do not assume arbitrary remote-IP filters are supported: the practical
network boundary is one localhost SOCKS broker port. Bind that broker on
`127.0.0.1` before generating the profile; do not listen on all interfaces.

The daemon profile permits outbound TCP only to that localhost port, its own
private Unix RPC socket bind/listen/accept, runtime reads needed by the pinned
binary, read access to its explicit configuration, and writes within its
canonical private home. Permit only necessary device/entropy and runtime
operations established by native probes. It does not grant arbitrary outbound
networking, DNS/mDNS services, other Unix socket connections, broad Mach service
lookup, arbitrary process signaling or writes outside that home. Do not use an
unrestricted `network*` rule or an unsafe outbound local-address rule. The final
profile and required runtime paths are reviewed after the empirical runner
spike, before general implementation relies on them.

Run the broker outside Seatbelt as the supervising local user. Its SOCKS5 parser
accepts CONNECT with domain-name addresses only and checks the canonical domain
and exact selected port against the explicit relay set. Reject IPv4/IPv6 address
forms, BIND, UDP, malformed/oversized negotiation, unselected names and ports
before DNS or socket creation. This is a second enforcement layer, not a
replacement for the daemon's exact URL check. TLS remains end to end between
MDK and the selected relay; the broker does not terminate or inspect it.

Apply finite limits: 32 global connections, 8 per destination, 16 KiB buffers
per direction, 5-second negotiation/connect/DNS deadlines and 660-second idle
timeout. Cap parsed greeting/request bytes at 1024 and DNS names at 253 bytes.
Use the bounded resolver helper with the pinned absolute `sys.executable -I`,
not a presumed macOS `/usr/bin/python3`. Resolve only admitted hostnames. The
configured relays remain operator-trusted destinations; do not invent a new
public-address-only contract. Errors and logs contain fixed categories and
public destination information, never credentials, command payloads or output.
Other local processes can reach the loopback broker: bounded connection
exhaustion and constrained relay use are accepted local denial-of-service risks.

Retain the private transport lock, verified stale-socket cleanup, environment
allowlist and private state directory. Use dedicated owned process groups for
the daemon and resolver helpers; shutdown closes listeners/streams, terminates
only owned children with bounded TERM/KILL waits and joins owned threads.
Unexpected daemon exit causes transport failure and cleanup. Preserve durable
identity/MLS state for restart. No broad process-name kills, detached orphan
brokers or deletion of accepted command state is permitted.

The daemon is owned by a small native lifetime shim. The supervisor opens a
private pipe, retains its sole write end with `FD_CLOEXEC`, and gives the shim
only the read end. The shim launches the verified daemon in its owned process
group, reaps it, and polls pipe EOF plus daemon exit. EOF terminates that group
with two seconds TERM then two seconds KILL and reaps it, even if Python was
SIGKILLed. Daemon, broker and command workers must not inherit the write end.
Also watch the recorded parent with kqueue `EVFILT_PROC/NOTE_EXIT`; failure to
install the watch aborts launch. EOF is authoritative independently of PID reuse.

Before readiness, atomically persist a private run manifest with boot UUID,
PID/start identity and executable digest for shim/daemon, owned process-group
IDs, launch nonce and configuration digest. The shim records startup identities
before acknowledging launch. Under the exclusive transport lock, next startup
and CI cleanup reap an orphan only after boot/PID/start/executable identity and
group leadership match. Mismatch fails diagnostically, never signals a reused
PID. Test SIGKILL-supervisor cleanup and SIGKILL-shim verified orphan recovery.

Preserve validated host-only `EPIPHYTIC_EXECUTION_ID` in supervisors, daemon and
detached workers for trusted runner cleanup. Read only host environment, bound
length/characters, and never accept it from remote payloads. Commands keep their
existing fixed environment; cancellation cleanup explicitly reaps verified
worker groups and tests that no command descendants survive. Normal frontend
disconnect continues to leave accepted commands running.

## Timers and suspend

Use a shared Python continuous elapsed-clock helper for broker idle, resolver,
connect, RPC and readiness deadlines (macOS continuous time; Linux boottime).
Provide an explicit `extern` binding for `mach_continuous_time` where pinned
libc lacks it. Rust acquisition/readiness loops check the platform elapsed
clock rather than relying solely on `Instant`. Short sleeps recheck deadlines
on wake; expired broker connections close before forwarding more bytes.

CI holds an owned `/usr/bin/caffeinate -i -s` assertion for its finite window.
Compare continuous and runtime-monotonic elapsed time; divergence over five
seconds fails interop as suspended and triggers cleanup. Each host enforces its
own continuous deadline, so wall-clock skew never extends test lifetime.

## Empirical native gate and fail-closed probes

The first native macOS runner spike verifies the compiler/linker, exact patched
SDK API, daemon startup, Seatbelt syntax, socket operations, minimum runtime
permissions and process/clock APIs. Build with Rust 1.97.1, Python 3.11+, Xcode
command-line tools, C build tools and Perl for vendored OpenSSL/SQLCipher.
Use the same vendored SQLCipher configuration as Linux, not system SQLite.
For Apple ARM64 set `MACOSX_DEPLOYMENT_TARGET=15.0` and matching target-specific
C/C++ minimum-version flags, following the pinned MDK macOS build settings.
Record the actual OS, SDK, architecture and toolchain in sanitized evidence.
macOS AMD64 remains unvalidated until an actual native runner is available.

Use preinstalled or user-local tools; assume neither apt sudo nor ownership of
the macOS Homebrew prefix. Missing tools are runner prerequisite failures.
The spike verifies APFS exclusive-publish races and actual Rust `sync_all`,
`F_FULLFSYNC`, directory synchronization and interrupted-publication recovery.
Unsupported filesystem, rename/sync `ENOTSUP` or inadequate directory durability
returns a distinct unsupported-storage error, never a weaker fallback. Record
sync latency. HFS+ is unsupported until equivalent native evidence exists.

Startup preflight and native tests must exercise confinement rather than merely
parse the profile. Use a small owned native probe under the same restrictions
to verify allowed private RPC/broker access and denied direct TCP, an unselected
localhost listener, another Unix socket and an outside-home sentinel file.
Include same-port `::1`, UDP, non-loopback local-interface destinations and
launch under an already-sandboxed parent. Measure `localhost` semantics: if
the profile permits unrelated same-port IPv6 services, it fails this design's
single-broker boundary and requires reviewed revision. Inherited sandbox
restrictions causing failed probes are startup errors, never fallback triggers.
Verify unapproved name resolution cannot use a system service to bypass the
broker. A production preflight uses controlled local sentinels with no public
scan or arbitrary external side effect; native integration tests additionally
verify real TLS via a selected relay. Probe permissions must not broaden the
production daemon profile: substitute the explicitly permitted executable for
the probe run and otherwise use identical rules.

Broker tests assert malformed/IP/unselected requests produce zero resolver and
connect attempts. Patched MDK tests exercise main, anonymous and inspection
clients with selected and peer-advertised decoy relay URLs; decoys must produce
zero broker requests, including reconnect paths. A missing sandbox executable,
denied necessary capability, malformed profile, inactive transport patch,
hash/manifest mismatch or failed probe stops startup and cleans owned processes.
There is no automatic unconfined or direct-network fallback. If native runtime
requirements cannot be bounded by this profile, report the concrete blocker
and revise this reviewed design before weakening confinement.

## Native and cross-platform acceptance

Replace Linux-only test gates with platform-native coverage of secure traversal,
exclusive publication races, lock lifetime/presence, PID reuse detection,
suspend-inclusive clock conversion, FD inheritance, SQLCipher encryption and
lock safety, cancellation, restart/recovery and detached command completion.
Test wrong-peer UID rejection where runner privileges permit it; otherwise
record that limitation and test the native credential wrapper's failure paths.
Linux coverage must continue to pass, including existing confinement tests.

Native runners are `[self-hosted, macOS, ARM64, laptop-ci]`,
`[self-hosted, Linux, ARM64, laptop-ci]` and
`[self-hosted, Linux, X64, bel-ci]`. The two laptop configurations share one FIFO
execution slot, so they cannot host concurrent cross-platform endpoints. Run
native laptop jobs sequentially. For actual interoperability, one macOS job
and one Bel Linux X64 job run concurrently, each holding its own server and
client, and exercise both command directions during the same bounded window.
Do not schedule a second Bel job that must run concurrently on the same slot.

Use per-run disposable identities and private 0700/0600 state on each runner.
Exchange only public npubs, explicit relay URLs, a run nonce, protocol version
and readiness/deadline metadata through a narrowly scoped CI rendezvous. Never
upload secret keys, SQLCipher databases/keys, MLS state, journals, command output
containing secrets, home directories or environment dumps. Artifacts contain
only schema-validated public rendezvous data or sanitized final results. Bind
rendezvous records to the workflow run/attempt and nonce, with finite polling
and expiry; stale records cannot select an identity or extend a test deadline.

The increment C design is normative for exact CI artifact schemas, two-stage
ready/paired barriers, runner queue budgets and all live-test deadlines. It is
reviewed independently before those workflows are implemented. Use same-run
immutable Actions artifacts with exact commit/run/attempt/role binding and both
random role nonces. Actual endpoint readiness precedes the paired barrier; no
job-output dependency or cross-run lookup is permitted. Grant `contents: read`
and `actions: read` only; artifact uploads use job-scoped Actions runtime tokens.
No PAT, repository-write permission or keys enter artifacts.

CI triggers only on `push` to `master`: no manual dispatch, PR events, inputs
or fork code. Master write authority is the trusted self-hosted code-execution
boundary; do not claim branch protection that is not configured. Pin actions
and check out the same trusted SHA. Use a repository-wide constant workflow
concurrency group with `cancel-in-progress: false`; replacement/skipping of
pending pushes is accepted. C's pairing budget must accommodate the shared
runner queue; a peer that never appears fails as a runner prerequisite within
its finite deadline and cleans up. No wall-clock or artifact timestamp extends
the continuous local budget. Reuse C's offline build artifacts for live tests.

The interop job uses explicitly selected test relay URLs and restricted known
command fixtures. Validate Mac client to Linux server and Linux client to Mac
server with byte-exact output/exit status, quotes, whitespace, dollar signs,
backslashes, newlines, Unicode and supported non-UTF-8 bytes. Include additive
default/per-key denials without side effects, duplicate retry/digest conflict,
client disconnect with continuing command and later result retrieval, server
restart recovery, and decoy-relay confinement. Allow enough time for actual
Marmot delivery but bound every wait, command runtime and total test window.
Use only the explicitly user-authorized fixed test relays
`wss://relay.eu.whitenoise.chat/` and `wss://relay.us.whitenoise.chat/` for bounded
live TLS/Marmot tests. Add no SRV-derived destinations. Record the exact selected
URLs; these are never production defaults. Increment B's TLS
probe has a five-second total connect deadline and performs no command workload.
Increment C uses disposable keys and the bounded fixture commands above.

Port Linux `/proc`-based test helpers to native identity/descriptor inspectors
instead of dropping their coverage. Verify the SQLCipher `RLIMIT_MEMLOCK`
negative test on macOS and document platform-specific resource semantics while
preserving the production secret-memory failure contract.

Use cancellation traps and an always-run cleanup path on both runners. Report
assertions, OS/architecture, artifact provenance, relay selection, time window
and failures without key material. A relay publication or a passing local mock
does not count as interoperable remote command execution. Increment B closes
only after native engine/RPC/confinement checks and independent Claude code
review pass; increment C closes after the actual two-direction Marmot tests,
reviewed sanitized evidence, documentation updates and commit/push verification.

## Owned test cleanup refinement

### Engine review refinement: readiness and recovery isolation

The following replaces the increment B frontend reconciliation loop. It does
not change admission, worker claims, replay binding, or the durable attempt cap.

* Run the host spawn preconditions at `Engine::open`, before storage recovery
  touches any job: trusted canonical worker executable, supported FD limit and
  inherited-descriptor ceiling, and valid host execution marker. Failure returns
  the existing typed policy/path/worker error immediately, even for an empty root,
  with zero job attempts consumed. Recheck these preconditions before each real
  start; a changed global prerequisite must never exhaust Accepted jobs.
* Prepare the worker command, existing-root/SQLite descriptor audit, and global
  checks before reserving a durable attempt. Separate preparation errors from
  actual spawn errors. Actual fork/thread resource errors (`EAGAIN`/`ENOMEM`)
  count as transient attempts and release the reservation. Other spawn errors,
  including executable disappearance, permission/format errors and failed
  `pre_exec`, return a typed global error and refund only this still-Accepted
  reservation's increment. A worker already claimed concurrently is never reset.
  Exactly three counted failed starts still end in Exited/NotStarted; quotas and
  environment/preparation failures do not count. A successfully spawned worker
  that never claims retains the existing bounded reservation/attempt behavior.
* One reconciliation step reads state/identity, observes presence and performs
  at most one reserve/start or uncertainty CAS. It never sleeps. Open and each
  maintenance call visit at most their existing 128-job page, preserving the
  persisted pending cursor; a deferred head cannot monopolize later pages.
  Status/output/input use one step for their requested job. Cancel first resolves
  the authenticated job without starting it and applies the existing Accepted
  cancellation transaction: Accepted becomes Exited/NotStarted without a spawn.
  Claimed jobs retain ordinary cancellation and verified worker signaling.
* Source-sensitive error classification: SQLCipher/decode/transaction integrity,
  root-wide storage/path/UID failures and host spawn preconditions are global
  errors and propagate. A malformed **job-local** directory/lock (`UnsafePath`
  from the held jobs directory's child), or a recorded/live worker mismatch, is
  a job anomaly. Open/maintenance skip that job and continue; a direct requested
  operation returns its typed anomaly. Unexpected syscall/storage errors are
  global by default. Recovery-CAS `Busy` is a concurrent-state change: return
  pending immediately, without retrying inside the step. `Initializing` is
  pending rather than an anomaly: status can show Accepted/Launching indefinitely
  without guessing loss. MaintenanceReport gains no new counter in this change.
* Only new submit and duplicate submit repeat the step, with 50 ms sleeps under
  one suspend-inclusive eight-second deadline. Readiness means the persisted job
  is Launching, Running or terminal, or a Live worker matches its persisted
  identity. An Accepted/absent deadline returns WorkerUnavailable; an
  Accepted/Initializing or mismatched-identity deadline returns RecoveryUncertain.
  A per-job mismatch may be retried only inside this bounded requested wait;
  stable structural anomalies return immediately. Claimed-but-absent jobs still
  CAS to UnknownLaunch/Lost and are never relaunched. Background skips do not
  grant command execution or silently turn anomalies into successful claims.
* A submit readiness error occurs **after** atomic admission: its original token
  is consumed and its job remains durable. Retries bind the same owner,
  invocation and command digest to the existing job using its original token
  and endpoint. The existing contract rejects a fresh token with InvalidToken;
  that rejection also cannot admit or execute a second invocation. This change
  does not weaken token/endpoint binding to satisfy a review assumption. A digest
  conflict still fails. A timeout does not itself mark an Accepted job terminal.

Focused regression coverage includes a quota-deferred Accepted job alongside an
unrelated terminal and runnable job; quota release and eventual start; cancellation
before deferred start; global precondition failure before any attempts and after
runtime prerequisite change; exactly three transient failures across reopen;
Initializing status plus requested timeout; UnknownLaunch/Lost without relaunch;
128 pending jobs without per-job sleeps; and persisted cursor fairness. Use
private test seams/stored fixture construction rather than production environment
switches. Verify original consumed-token retry and fresh-token rejection after
timeout, with neither admitting a second job.

The [final engine review](../reviews/macos-engine-code-final.json) approved this
implementation with two nonblocking observations. Keep the unconditional host
spawn precondition at the start of `maintain` deliberately: maintenance is one
operation and returns a global host error before selecting or advancing its
pending page. Consequently a broken executable, unsupported descriptor limit or
invalid host marker also postpones token reaping, retention and quota-recovery
maintenance until the operator restores that prerequisite. Direct cancellation
remains available. This availability tradeoff is explicit; independent retention
under a broken launch configuration would require a separately reviewed change.

Keep the Live identity equality check as a defensive invariant at the engine
boundary. The current presence implementation produces Live only after checking
that equality, so its mismatch branch is currently unreachable; a held lock with
an unverified identity produces Initializing instead. The branch grants no extra
readiness or execution authority and must not be cited as exercised mismatch
coverage. Initializing Accepted work remains pending, while claimed state retains
the approved readiness and no-relaunch rules above.

The cleanup FD test first positively observes canonical `engine.sqlite3` and then
positively observes `frontend.lock` after consuming cleanup. Only frontend,
worker and I/O lock regular files may remain under the canonical root: no database,
WAL/SHM, journal, spool or key descriptor is allowed. Inspectors must fail on an
unreadable descriptor table; an entry that disappears during metadata inspection
is ignored only as a disappeared entry, while other inspection errors fail.

Darwin descriptor auditing deliberately retains fail-closed behavior on changing,
unlinked or unresolvable vnode snapshots. A blanket UnsafePath retry would also
mask stable security failures. A distinct transient-race classification and a
bounded retry, if native evidence requires them, need their own reviewed change.

Terminal status alone does not establish worker teardown. A consuming
`Engine::into_cleanup(self) -> Result<CleanupGuard>` supports owned-test cleanup
without adding a remote operation. Under the frontend lock it sets the consumed
Store's busy timeout to zero and snapshots the union of all persisted job IDs
and descriptor-relative job directories, capped at 256 total. Unrecorded
directories, non-Exited records, malformed lock files, or a still-live matching
recorded worker fail closed. An Exited tombstone without a directory is safe.

Preflight current descriptor count plus two per extant job and 64 spare against
RLIMIT_NOFILE (and Darwin's 65,536 ceiling). Acquire locks in ascending JobId
order, worker before I/O. Retry Busy every 25 ms within one suspend-inclusive
five-second deadline; any failure releases all accumulated guards and returns
explicit incomplete cleanup. Re-read state and worker identity after acquisition.
Drop Store and encryption keys before returning the opaque guard, which retains
the StateRoot, frontend lock, and all job guards. The worker must also drop its
Store before its lifetime lock.

The trusted private bridge consumes its Engine, retains the cleanup guard, and
acknowledges readiness. Duplicate readiness is idempotent; ordinary operations
then have no Engine and fail. The test stops its frontend, cancels all tracked
invocations (tracked before submission, including uncertain submissions), waits
for cleanup readiness, and removes the root while the guard remains alive.
The bridge closes afterward. Failure retains private state and reports incomplete
cleanup. A late initializing worker is fenced by the retained locks and cannot
claim an Exited job after release. No unverified PID scanning or broad signaling
is introduced; this is not a remotely accessible cleanup capability.

## Supervisor control boundary refinement

This refinement addresses review R6–R15 and requires independent approval before
implementation. The data home is daemon-writable; host recovery authority is
not. Export one `control_dir(home)` derivation: canonical home path bytes hashed
with SHA-256, full lowercase hex, named `.marmish-supervisor-<hash>` in
`home.parent`. All readers and cleanup callers use this helper. Different homes
have independent control directories. Walk canonical ancestors with descriptor-
relative `O_DIRECTORY|O_NOFOLLOW` opens and `fstat`: owner must be root/euid;
group/other writes require root ownership plus sticky mode. Create control 0700,
open no-follow, and require exact owner/mode on the held descriptor. Lock and
record operations use that descriptor; pathname operations recheck dev/inode
against it before and after. Precreated wrong-owner/mode/type/symlink paths fail.

ACL decision for this increment: mode checks do not evaluate extended ACLs.
The supported host configuration requires no ACL granting another principal
write/delete/ownership changes to control or user-owned ancestors. Such ACLs are
an explicit accepted host-configuration residual, not a property established by
the mode-bit validator. Root/current-user host processes remain trusted. This
limit must be visible in usage/specification and native evidence; no claim of
protection from an independently hostile same-UID host process is made.

Put persistent lock and atomic run record at control root. Place copied signed
binaries, profile, config and ack in a private per-run subdirectory. Seatbelt
allows exact executable/config read literals, required system runtime reads,
HOME data writes and private RPC socket operations. It grants no broad control
read/write permission. Helpers run by host probes use their own executable
literal. Ack publication uses exactly two allowed write literals, `<ack>.tmp`
and `<ack>`: create-new temporary, write+fsync, then atomic rename. The ack is
ephemeral readiness evidence and does not require parent-directory fsync; the
supervisor record independently requires directory fsync. Supervisor
ignores the temporary name. No third name or broad parent write is allowed.
Stale ack names are removed under the control lock before launch.

Run record publication uses an exclusive private temporary at the held control
fd, file fsync, descriptor-relative atomic rename, then control-directory fsync.
Only after publication does the host grant the shim's blocked child permission
to exec. Incomplete temporaries are not authority; invalid final JSON is a
corruption/tampering error. Record includes a validated per-run basename; after
all recorded identities are gone/obsolete, no-follow cleanup removes that owned
run subtree. Under the same persistent lock, stale validated per-run directories
left by pre-publication crashes are garbage-collected with a bounded count;
unknown entries fail closed. Never unlink the persistent lock while other
callers may open it. Export locked `cleanup_control(home)` for owned-test cleanup:
require no live run record, remove verified stale per-run state, retain lock and
control directory. E2E calls it before deleting its encompassing private test
root, so no old record survives reuse of a home pathname.

Split native inspection into typed `Absent`, `Present` and `Uninspectable`.
Present includes checked pid, uid/ruid, boot UUID and start time even for another
user; the existing public `process_identity` still requires own uid/ruid.
Validate both recorded identities' exact fields, lowercase UUIDs, positive pid
and start, and the same recorded boot before inspecting a target. Read current
boot from this process's native identity. A clean old-boot mismatch skips both
with zero signals. In this boot, Absent or Present with different uid/ruid/start
is obsolete: skip it without signaling. Only Uninspectable fails closed. Only
matching identity, owned group leadership and pinned executable digest authorize
TERM/KILL, with identity rechecked immediately before each signal. Final record
removal requires every entry proved absent/obsolete or verified terminated.

Recovery of non-child processes has an unavoidable identity-check-to-signal
race; only the shim's unreaped-child path closes that race. An absent leader
may leave unrecorded group members after simultaneous abnormal shim/daemon
failure: these are not signaled without verified identities. This is an explicit
residual; normal leader exit and parent death are covered by the shim. Ack/socket
contents remain daemon-controlled acknowledgments, not independent evidence of
a trustworthy daemon; confinement protects supervisor signaling/execution state.

The shim ignores SIGPIPE itself and resets SIGPIPE to default in the forked
child before exec. Before each `waitid(P_PID, child, ..., WEXITED|WNOHANG|WNOWAIT)`
call zero `siginfo_t`; only `si_pid == child` means exited. Natural leader exit:
SIGKILL its group (ESRCH is success), then reap leader. Parent pipe EOF/parent
exit while leader alive: TERM group, observe for at most two continuous seconds
with the same non-reaping waitid loop, then unconditionally KILL group, then reap.
Never return/reap during the grace period before the final group signal. The
unreaped leader reserves pid/pgid; group-emptiness probes before reap are invalid.

Native probes use actual HOME files for allowed data writes. Denied control
probes attempt create, manifest/config/binary write and truncate, rename over,
unlink, and a sibling of ack; exact ack temp/create+rename is allowed. Existing
network/IPC probes remain. Native lifetime tests cover default child SIGPIPE,
natural exit with a surviving group sleeper, parent SIGKILL, shim SIGKILL followed
by identity-verified recovery, and EOF during TERM grace. Unit tests cover every
ancestor/control path rejection, two independent homes, old/mixed/malformed boot,
absent/reused/other-user/uninspectable identities, truncated records, stale
ack/socket and crash-temporary GC. Old boot/absent/reused tests assert zero
signals. Actual Darwin waitid and other-user proc_pidinfo behavior must pass the
runner gate before support is claimed; unreadable other-user inspection fails
closed rather than asserting it was inspected.

## Transport review completion refinement

Resolve F1 by appending `--discovery-relays R --default-account-relays R`,
where `R = ",".join(canonical_relays)`, identical to Linux. Test exact argv and
order after `--home HOME --socket SOCKET --secret-store file`. Resolve F2 with Unix-only bind
and inbound permission restricted to the HOME subpath, covering upstream's
`<home>/.sock.<pid>.<name>/<name>` staging and final socket. No arbitrary Internet
or outside-HOME Unix capability follows. Validate the worst-case decimal pid
staging pathname as `len(home bytes) + len("/.sock.") + 10 + 1 +
len(name bytes) + 1 + len(name bytes) + 1 <= 104`, including NUL and u32 pid
width. One function is used by prepare and spike; the spike stages with its real
child pid.
The native spike binds and hard-links through the same staging/final algorithm;
post-build native E2E exercises the actual pinned MDK helper.

Replace the F5 regex heuristic with a cryptographic approved-source guard: pin
the aggregate SHA-256 of every sorted path and complete Rust file contents below
`crates/marmot-app/src`, including inline tests, plus pinned source revision and
patch digest in a reviewed repository manifest. Build compares the entire tree
after applying the patch. New files and any altered spelling, alias, UFCS,
RelayPool constructor, or production code after test modules fail equally. This
attests that reviewed marmot-app source subtree, including the four factory replacements;
it does not claim to infer Rust semantics. Tests mutate all eight reported
bypasses against a trusted baseline and require rejection. Source/patch updates
require explicit guard-pin updates and independent review. `wnd.rs` initialization
and Cargo manifests are attested by clean pinned revision plus patch digest.
Cargo.lock is checked against LOCK_SHA256 before and after build. Other crates
(including nostr-sdk/transports) are outside this guard; Seatbelt is the backstop
for their network behavior.

F6: keep the committed patch's existing exact-membership/forced-SOCKS/whole-
connect-timeout Rust test; add isolated initialization/ack tests and rejection
cases without racing process-global OnceLock/environment (test subprocess per
configuration). Build tooling runs `cargo test --locked -p marmot-app
marmish_transport::tests --lib` against the clean patched tree before publishing
macOS artifacts. CI must retain sanitized success/failure, rather than treating
a warm build cache as tests. F7: repository JSON fixtures state inputs and
expected canonical string or rejection, including slash, case, explicit 443,
userinfo, IPs, encoded paths, query/fragment, whitespace and noncanonical forms.
Python consumes them directly; build copies the same fixture bytes into the
patched source for Rust `include_str!` tests. This deterministic copied test
input is covered by a separate fixture digest, and introduces no production network default. Sequence is
apply patch, guard complete Rust source tree, release build, then copy fixture
bytes outside the hashed Rust subtree and run patched tests. The release artifact
is already produced from the guarded tree. Guard walks reject symlinks and
nonregular files and hash sorted paths plus raw bytes without newline conversion.
Fixtures have a separate digest. CI retains the exact required patched test
command result. Initialization tests spawn the current test binary with `--exact`
and an env marker per case, with fresh 0700/0600 fixtures; cover ack, pre-existing
ack, bad mode/nlink, oversized config, IPv6/nonloopback proxy and noncanonical
relay. The existing transport test uses a bounded listener accept deadline so
forced-Proxy regression fails rather than hangs.

F9: every denied native probe has an unconfined positive control with the same
arguments and owned listening/file endpoints. Destructive controls use fresh
sentinels reset before each sandbox invocation. A control failure is a failed
prerequisite, never evidence of sandbox denial. Keep actual positive broker TCP,
HOME file read/write, Unix staging bind+publish, and two-name ack publish probes.
Outside-HOME Unix bind is denied with the same unconfined successful control;
connect controls use actual owned listeners and bind controls use fresh paths.
The same production profile text is used. Runtime-only denied probes target
config/binary copies and host control authority, never unrelated host files.

F10 factual disposition: launch already compares the pinned lock hash and the
repository patch digest in `verify_manifest`; these checks remain. Verify copied
native signatures using `/usr/bin/codesign --verify --strict` before launch,
and record the ad-hoc signing scheme in Darwin manifests (Linux null). Expected
artifact SHA remains the trust pin; ad-hoc signing is native executable validity,
not a publisher identity. A daemon descendant that creates a new session escapes
process-group cleanup; it remains Seatbelt-confined. This cleanup residual is
explicit and is not represented as complete descendant eradication.

## Final native code review dispositions

Claude approved the integrated native transport with no blocking findings.
N1: destructive control probes use a sibling `.marmish-probe-record`, never
an authoritative run record; locked control garbage collection removes that
private sentinel after an interrupted probe. N2: recovery re-inspects identity
when a process disappears during group lookup or signaling; uncertain races
retain the record with a typed error. Missing executable data is explicitly
unverified and is never authority to signal. N3: the native builder runs each
of the three expected Rust transport tests by exact name and checks the named
one-test success result before recording passed test provenance.

N4 is a deferred portability limitation: the maintained patch is applied only
to Darwin. Its unused non-Darwin O_NOFOLLOW numeric fallback is not portable to
Linux ARM64. Linux production builds use pristine unpatched MDK; supporting the
restricted patch on Linux would first require native constants and validation.
N5 requires no code change: Seatbelt remains the network boundary for any raw
HTTP or resolver path not explicitly disabled by restricted mode.

The [focused follow-up review](../reviews/macos-hardening-code-final.json)
approved N1–N3. Its remaining availability observation is accepted for this
increment: a hard kill between sentinel creation and its private-mode chmod can
leave a sentinel that strict garbage collection refuses. The enclosing control
directory remains private; an operator must remove that owned stale sentinel
before retrying. The exact-test output gate deliberately fails closed if a future
libtest output format changes.
