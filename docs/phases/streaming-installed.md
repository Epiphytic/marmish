# Phase 4: installed streaming and transfer qualification

Design revision2; independent review is recorded in
[the review record](../reviews/streaming-installed-design.md). Beads `marmish-8ka.5`
tracks this phase. This is an implementation and qualification design, not a
platform pass report, release approval, or authorization to publish packages.
Implementation follows the approved rsync and native-copy increments and their
integrated code reviews. Design work can proceed while those dependencies finish.

## Scope and existing building blocks

Qualify actual installed native client and server on Linux AMD64
(`ubuntu-24.04`), Linux ARM64 (`ubuntu-24.04-arm`), and macOS ARM64 (`macos-15`).
Assert OS and machine at runtime; no Rosetta, emulation, Linux namespace/sudo
requirements, Python product runtime, or browser/wasm backend. Hosted runner
availability is a scheduling dependency, never evidence of a product pass.

Reuse `.github/workflows/v02-installed.yml`, the feature-only Rust
`installed_check.rs` driver and `installed_prepare.rs`, `installed_matrix.rs`,
`installed_process.rs`, `installed_records.rs`, and `installed_cleanup.rs`.
`tools/v02-actions/{workflow,supervise,transport,upload-client}.mjs` supplies the
existing Node24 CI artifact rendezvous. The product and installer remain Rust and
POSIX shell; build-only Node coordination is not an installed dependency. Do not
reuse the historical `tools/marmish_installed_*.py` v0.1 harness.

Add bounded private driver modules for streaming/rsync/copy qualification and
closed cohort records. Keep existing finite-command matrix and local cohorts
unchanged. Extend Context's closed cohort table and the Node supervisor's timing,
record grammar, and dispatch together, with tests. Preserve candidate provenance,
artifact run-attempt binding, private workspace ownership and cleanup proofs.
Never infer that existing finite-command evidence covers live streams or copies.

## Candidate and installer provenance

The checked-in `scripts/install.sh` currently pins released **0.2.3**. Running its
public default cannot qualify new streaming features. Do not modify public pins,
bump versions to trigger publication, create tags, upload release assets, publish
to Cargo, or invoke release promotion as part of this phase.

Add a separate manually dispatched **candidate-only** packaging/render workflow,
with `contents:read` and no publication jobs. Reuse the packaging, macOS ad-hoc
signing, native binary inspection, manifests, separate installed-test-tool build,
and bootstrap rendering from `v02-release.yml`; its current version-transition
eligibility/promotion pipeline is not a suitable no-release qualification entry.
Candidate build source is a reviewed full commit on the project default branch,
identical to the dispatch/workflow source. No PR-supplied executable or arbitrary
repository/ref is accepted. Keep the installed workflow's explicit source equality
check and exact successful candidate run/attempt validation. The normal candidate check accepts exactly `.github/workflows/v02-release.yml`
and the new `.github/workflows/streaming-candidate.yml`, both requiring a completed
successful exact run/attempt from the default branch. Reject every other workflow
path and add an explicit allow-list regression. Do not loosen existing release
candidate checks or its separately scoped diagnostic exception. Re-run all jobs for a new attempt.

The candidate `package` job's `Verify native components` step runs the unfiltered
engine, stream, native and vault test suites on all three native runners. It
explicitly refuses UID 0 before testing, because security-sensitive child-spawn
fixtures skip root, and sets `MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS=1` for that
step. The receiver permission fixture must fail qualification if its actual
read-only-parent probe remains writable (for example through elevated
capabilities); nonroot UID alone is insufficient evidence. This schedules the macOS
`only_macos_exact_public_alias_prefixes_are_normalized` unit test and the engine
`context_fd_allocation_and_spawn_survive_closed_stdio_and_original_fd_three`
fixture, including its actual spawn portion, without adding a redundant test
step. The candidate run's per-platform job logs own this evidence; writing the
workflow does not prove those jobs have run.

The candidate may retain its source's package version, including 0.2.3; version
alone is never provenance. Pin source commit, target, manifest digest, artifact
run and attempt, bootstrap digest, product file sizes/hashes, and separate tooling
sizes/hashes before executing any downloaded candidate/tool. The same tuple is
verified against installed `release-info` and installed bytes. Expand the shared
source fingerprint consistently in producer and verifier to include the workspace
Cargo.toml, Cargo.lock, rust-toolchain.toml, `crates/marmish-engine`, `crates/marmish-stream`, `crates/marmish-core`,
`crates/marmish-vault`, native crate, packaging/install sources, candidate/installed
workflows and CI tools. A regression derives the transitive workspace path-dependency
closure of the native/tooling crates from Cargo manifests and requires coverage
in the fingerprint list; missing a newly introduced local crate must fail.

Render a candidate-specific copy of the actual installer from the checked-in
shell template, with all three verified manifest pins and the exact candidate
version. Use the existing Rust release tool; store the resulting bootstrap as an
immutable run artifact, without editing public `scripts/install.sh`. Each platform
runs `/bin/sh candidate-install.sh --asset-dir ABS_CANDIDATE_DIR --prefix OWNED
--bin-dir OWNED --state-dir OWNED --key-storage file` as a non-root user. This
exercises actual installer verification, native install and launchers using
verified local assets. Without --quickstart, setup is a separate installed_prepare
invocation of the installed binary; --key-storage file is passed to that setup,
not assumed to initialize an identity during plain installation. Report it explicitly as **artifact-backed installer**, not
public HTTPS download qualification. A pinned fixture installation must never
fall back to GitHub's released 0.2.3 assets. Existing public-HTTPS installer tests
remain separate and cannot be described as streaming qualification.

Require actual installed `marmish` and `marmishd` launchers, independently verify
installed product hashes/version/commit/target, and use only those absolute paths
for setup and workload. Build-only fixture binaries are separately authenticated
and never substituted for product binaries. Isolate HOME, TMPDIR, state and PATH;
all private roots are canonical, owned and0700. Setup creates fresh file-backed
client/server identities explicitly. Platform keychain testing stays a separate
provider qualification and is not implied by file-backed success.

## Cohorts and bounded orchestration

Use eight separately dispatched waves, never overlapping, each with fresh Context,
nonce, identities, installation and workspace:

| Closed cohort | Directed platform pairs | Required positive work per pair |
| --- | --- | --- |
| `stream-matrix` | all9 | one binary duplex stream with stdout/stderr/exit validation |
| `rsync-matrix` | all9 | one push and one pull using installed `marmish rsh` |
| `copy-matrix` | all9 | one native put and one native get, opposite compression settings |
| `edge-stream`, `edge-rsync`, `edge-copy` | three same-platform pairs | bounded negative/recovery/cancel suite below |
| `copy-interrupt-put` | three same-platform pairs | interrupted1.25MiB put, explicit helper cancel, fresh-stream resume |
| `copy-interrupt-get` | three same-platform pairs | interrupted1.25MiB get, explicit helper cancel, fresh-stream resume |

Each platform job owns one installed server identity and a distinct installed
client identity, and runs both roles concurrently. Reuse the immutable public
identity-ready exchange, then configure trust/grants via installed setup and
strictly validated config edits only where setup cannot express policy/cwd/limits.
Grant only the three fresh client identities for that wave. Clients trust only
the three fresh servers. No unbounded wildcard grants or shell command execution.
Explicit canonical fixture/rsync/native-image allow rules are confined to this
owned test environment. Granting the native image for copy permits its other
subcommands under the same UID; this is not a copy-only security sandbox.

Each role publishes authenticated-provenance-bound server-ready metadata only
after installed daemon Status reports `active`, not `activating`, and its child
remains owned/live. Observe readiness through real IPC rather than fixed sleeps.
No-autostart clients may wait for an existing activating daemon but never spawn
one. Server-ready binds identities, source/tooling/installed-image pins, exact
fixture/rsync binary pins, and a fixed role-relative test-root label where needed. Each fixture resolves
that closed label under its own canonical owned root. Never publish absolute
locators or treat an artifact pathname/PID as execution or cleanup authority.

After the three server-ready records, each local client runs its three server
cells concurrently, at most one active stream per pair. Thus each server has at
most three normal active jobs, within the shared default peer4/global32 limits.
Within a cell push and pull are sequential. Each server records receipts for the
expected `(Context, client role, server role, case)` nonces. A pair completes only
when client byte/status/hash observations and server receipts agree. A client
publishes one finite terminal result including failed/skipped cells; no missing
cell is silently omitted. A server stays alive until all three bound client
terminal outcomes and expected successful receipts are reconciled, or its bounded
uptime expires. Three initial connections are not a completion barrier.

Context lifetime is at most3300s, preparation/identity-ready within900s of common
Context creation. Queue age consumes this budget; no runner remints it. Require
at least1980s remaining before starting the server/traffic phase; otherwise fail
without starting workloads. Each server has explicit `--max-uptime 30m`, not an
artificial default service expiry. The all-server-ready barrier must complete within180s of the earliest server
start, inside the900s preparation window (server start therefore no later than
Context creation+720s). Normal traffic plus receipt reconciliation
is bounded to1200s after that barrier; owned stop/cleanup has180s. Thus earliest
server-start to reconciled stop is at most1560s, leaving240s before its1800s
uptime expires. Require1980s Context remaining at server start:1560s bounded
server work plus420s slack. Node/Rust timing tests measure these bounds from
server start, not from the later barrier. Failure to reach the barrier in180s
stops the owned servers without starting traffic. Matrix
client cell operations each have330s absolute budgets, including their
pre-transfer inventory, receipt collection, handshake and
transfer, with at most two positive operations per matrix cell. Interruption waves instead
use their single1200s family budget:1140s for detach, cancel and resumed completion,
then60s reserved for client/server evidence reconciliation. All bounds are
also clipped to Context expiry minus180s for normal cleanup. No automatic retry
reruns commands or resets identities/room after an ambiguous outcome. Driver
records the failed attempt and uses list/cancel recovery instead.

New rendezvous cohort jobs explicitly use65-minute limits; do not reuse the
existing55-minute matrix timeout unchanged. Actions jobs have65-minute hard limits and a first-step62-minute watchdog. The
Node supervisor must accept3300s only for these named cohorts. For these cohorts
only, require `context.expires_at + 420 <= jobDeadline`, set artifact cleanup
deadline to `context.expires_at + 300`, and preserve the existing +900/+600 policy
for old matrix. Arithmetic from Context creation:3300+420=3720 and3300+300=3600;
the62-minute watchdog is3720s after its first step, leaving at least120s after
the cleanup deadline. The nominal180s headroom before the65-minute hard limit
is a goal, reduced by pre-first-step provisioning; it is not guaranteed. Normal workload still stops180s before expiry.
Test both policies and reject any extension/re-minting. Lost/Unknown cleanup
may remain unresolved; the outer deadline does not manufacture safe cleanup.
Keep existing2220s matrix timing unchanged. Poll artifact service on its existing
bounded cadence with Retry-After handling; remote polling cannot block local
terminal-record upload or owned-child supervision. Report runner jobs not running
within five minutes as infrastructure delay when dispatch is actually performed.

## Workloads and receipts

Stream cells use deterministic96KiB stdin,96KiB stdout and8KiB stderr spanning all
byte values, awkward argument bytes, empty argv, and remote exit7. The fixture
waits for input and records hashes, counts, literal argv digest and nonce in its
owned receipt directory. Client validates exact bytes/stream separation/exit and
final ACK; receipt proves remote execution, not mere relay publication.

Require rsync>=3.2.7 (the captured bootstrap baseline), with current maintained3.x
preferred. Before driver prepare, a distinct prerequisite step verifies the
Linux hosted rsync version/path; no sudo/apt fallback is required or permitted.
A cheap prerequisite-only dispatch verifies hosted Linux rsync availability before
expensive cohorts; a missing ARM64 package is an explicit prerequisite failure.
On macOS set HOMEBREW_NO_AUTO_UPDATE=1 and use Homebrew rsync (`brew install rsync` if absent) and its canonical
absolute path, never Apple's older bundled rsync. This acquisition step consumes
the same900s Context preparation budget; slow acquisition fails rather than
renewing Context. Require a literal remote `--rsync-path` for the qualified
Homebrew image, because daemon PATH need not include Homebrew. Capture exact
version, Homebrew formula/bottle version when applicable, and canonical executable
hash. Homebrew acquisition is recorded as acquired, not a reproducible pin. The
binary path is fixed for the wave and included in server-ready/receipt evidence;
resolve remote executable through daemon PATH, or explicit literal `--rsync-path`
when that platform's qualified path is necessary. Version/package installation
failure is a failed prerequisite, not a skipped successful cell. Rsync cells use
`-as -e 'ABS_INSTALLED_MARMISH ... rsh'`, one push and one pull of a fixed tree
(total32KiB, bounded at16 files), including empty files, nested directories,
spaces, Unicode, newline, leading dash and literal shell metacharacters. Compare
sorted byte-path/type/size/content-digest manifests locally and remotely. Use
only supported filesystem semantics; no symlink transfer security claim. Remote
fixture inventories validate actual destination contents after each direction;
rsync exit0 alone is insufficient. One direction uses `rsh --compress`, one does
not; do not enable rsync `-z` simultaneously.

Native-copy cells put and get deterministic64KiB regular files, including empty
and awkward-name cases in same-platform edge wave. Verify SHA256 and exact byte
length on both ends and final durable receiver commit, with explicit overwrite
only for its intended case. Validate mode and requested modification timestamp at
filesystem-supported precision, without inventing ownership/ACL guarantees.
`--compress` is tested once per direction across each cell's two operations.
No same-platform result substitutes for any of the six off-diagonal pair cells.

A separately authenticated test fixture provides fixed inventory/receipt/check
operations for rsync and copy; it accepts only closed enums and fixed owned paths,
never remote-supplied shell fragments. Remote receipts and inventory may be
collected by subsequent installed Marmish commands after the data transfer, with
their own finite bounded budgets inside the1200s traffic phase. Before each
transfer, require a nonce-scoped empty/absent destination inventory. Bind the
post-transfer receipt to that precondition, server journal stream/launch digest
and terminal-confirmed state; inventory alone is not a remote execution receipt. Pin these extra
operation counts in the final workload table and rollup; do not treat them as
unmeasured or exempt from server lifetime/admission accounting.

## Negative, cancellation and recovery waves

Run fixed sequential cases with a1200s overall traffic ceiling per same-platform
pair; each case has an explicit finite timeout and cleanup before the next. No
more than two active test streams at any moment. Configure peer active limit2 only for the edge-stream wave at setup, since
policy is immutable until restart. For quota saturation, launch exactly two
owned gated jobs and require the third admission to fail Capacity without launch;
explicitly cancel/reap both before continuing. No in-place policy restoration or
restart is part of this wave; later waves have fresh contexts and configuration.

Required installed cases: missing executable versus policy-denied distinct diagnostics;
unsupported rsync bootstrap without `-s` rejected before Open; receiver
existing-file refusal versus explicit overwrite;
symlink/nonregular target refusal; changed resume source/binding refusal; interrupted
put/get retains checkpoint then resumes after explicit helper cancel/owned stop
using the observed-prefix mechanism below; corrupt checkpoint/prefix refusal; and existing destination preserved on failure.
Use existing hermetic protocol/SQLCipher tests for adversarial forged-wire states
that installed CLI cannot generate; report those separately from installed cases.
This includes authenticated duplicate/replayed Open and live authority withdrawal.
The public CLI creates fresh Open identities rather than exposing wire replay;
`native_setup::run` acquires `ProfileLease` before `--revoke`/`--untrust` updates,
while the running daemon owns that lease and uses immutable command policy.
Do not rewrite its private configuration, bypass the lease, or restart it and
label that as live withdrawal. Require separately recorded hermetic evidence at
the qualified source, with each claim mapped to its actual test:
`stream_rejects_wrong_binding_author_and_current_withdrawal` models unavailable
adapter policy by setting `commands = None` and sends Hello; it does not revoke a
running runner. `authority_withdrawal_halts_queued_input_irreversibly_without_cancel`
tests the runner's irreversible input suspension while preserving its child,
then explicit cancellation. Neither establishes end-to-end live actor revocation.
`stream_requires_hello_and_cancel_invalidates_preparation_revision` covers stale
preparation cancellation; `restart_preserves_cancel_before_open_and_immutable_launch_fence`
covers persisted launch fences. None of those tests sends a second authenticated
Open. Duplicate/replayed Open never relaunching therefore remains an explicit
coverage gap until a dedicated authenticated actor test is added and its result
recorded; do not infer it from the store fence tests. Record the actual source,
test names and results, and retain the live-actor withdrawal gap explicitly.
These are hermetic coverage, never installed platform PASS cells. All public installed
cases, actual installer execution, nine-pair positive matrices, explicit cancel,
and checkpoint-proven resume remain mandatory; this boundary does not waive them.

Explicit cancellation: signal ordinary stream with INT; signal local rsync INT,
TERM and HUP, plus one actual owned process-group INT to rsync and adapter to
exercise INT+forwardedUSR1 coalescence. Test rsync's own timeout/protocol failure
forwardingUSR1; verify terminal confirmation or honest uncertainty, not exit0.
Force signal-pending plus EPIPE to use the cancel control lane; EPIPE alone and
parent SIGKILL detach and leave remote work alive/backpressured. Pre-Open signals
must cause zero remote launch and zero Cancel. Post-Open lost reply is represented by stream-sigkill-detach: it must retain
recoverable stream identity, never auto-rerun. Prove continued execution or held
bounded state via an owned fixture marker and list state, then cancel explicitly
from a second installed client command. A negative case is not complete until its
owned remote work is confirmed stopped or explicitly reported unresolved.

Each split edge cohort uses one original1140-second execution deadline and60-second
reconciliation reserve, superseding the previous aggregate per-family120/240-second
estimates. No case renews the deadline. Only edge-stream configures peer active
limit2; edge-rsync and edge-copy use the normal active limit and still create no
more than two concurrent streams.

Each copy-interruption wave uses one deterministic1.25MiB regular file per same-
platform pair (`CHECKPOINT_INTERVAL + 32 * MAX_BLOCK`, currently1MiB+256KiB) (the matrix retains64KiB). Put and get have separate1200s family budgets (1140s execution plus60s
reconciliation); neither direction borrows time from another wave. This is an honest
qualification ceiling, not an assumed transport speed; a slow relay may fail it.
Driver owns both initiating client process and receiver filesystem. It opens the
owned checkpoint read-only and validates it with marmish-stream's checksummed
checkpoint decoder, observing0<durable_prefix<file_size while the client is live. Derive the exact
.marmish-copy-v1-NAMESPACE basename from the bound launch inside the owned
receiver parent; checksum validation handles atomic checkpoint replacement.
Capture transfer ID and stream ID from the owned client stderr pipe before
sanitizing; never upload the raw stderr.
The1MiB byte checkpoint interval and2MiB durable window permit intermediate
checkpoints without relying on the5s timer. Bounded20ms polling observes this
condition; no blind fixed-delay kill or production test hook is introduced.
The1.25MiB payload retains a1MiB byte-triggered intermediate boundary while
reducing the earlier4MiB proposal. The integrated production receiver loop must
checkpoint when accepted bytes minus durable bytes reaches CHECKPOINT_INTERVAL;
a protocol constant alone does not establish that behavior. Verify that actual
helper/client loop at canonical integration, and fail any missing intermediate
checkpoint rather than relying on this baseline's pure receiver tests. A measured local relay run transferred2MiB in
384.67s (about 5.45 kB/s, 5.32 KiB/s); that observation motivates bounded qualification cost,
not a platform performance guarantee or a timeout extension. Larger-file helper
and stream tests remain separate evidence. Full completion before the live
intermediate observation fails the interruption case rather than manufacturing
an interruption proof.
Send SIGKILL through the still-owned client spawn handle to test detach (SIGINT
would request Cancel and is tested separately). Prove the server helper remains
charged/live using stream-list for OLD_ID. Put additionally requires a fresh-stream
same-transfer resume returning Busy while the server receiver retains its lock.
Get has a local receiver: SIGKILL releases its flock, so do not attempt concurrent
resume or require Busy from the sender helper. Its live/charged OLD_ID is the
observed proof before cancel; no direct sender backpressure observable is exposed.
This is not receiver-lock evidence. The driver enforces Busy-before-cancel and
charged-before-cancel ordering; rollup checks the matched closed evidence and
final server states but does not independently reconstruct an event timeline.
Explicitly cancel OLD_ID and confirm physical finish. Re-observe the old receiver
checkpoint at that stable boundary before resume, recording its durable prefix
separately from the initial live sample. Derive remaining logical payload and
resume goodput only from this stable pre-resume prefix, since writes may advance
after the initial observation. Then resume through a new stream using the same
transfer ID. Validate complete final bytes and checkpoint/commit state. Missing intermediate
checkpoint (including completion before observation), missing direction-specific
live/Busy proof, timeout or uncertain cleanup fails the case.
No automatic retry/identity remint is permitted. Expanding time or splitting more
cohorts requires another reviewed bound rather than runtime renewal.

Wrong-owner, wrong-author, forged replay and corruption states that the installed CLI cannot
produce remain required hermetic protocol/SQLCipher acceptance, listed separately
from the installed table. Do not add production test-only execution hooks or
claim installed adversarial-wire coverage from those hermetic tests. Wrong-owner remains an explicitly hermetic multi-principal case; these waves do
not create an extra granted principal solely for installed coverage. Real installed
trust withdrawal and repeat attachment observations remain required
where public operations expose them. Tests construct checkpoints only inside the
owned negative-case fixture roots, never the user's profile or private state.
Unavailable required installed cases are named failures/unimplemented acceptance,
never converted into passes.

## Cleanup, rollup and statistics

Supervisors own child handles from spawn; the existing Node ownedChild uses
child.kill, not process-group authority. Only a deliberately created/owned test
process group may receive the explicit rsync group-signal case. Artifacts
carry no signaling authority; never kill a PID/PGID read from a file or use a
process-name-wide kill. Explicitly cancel known live stream IDs through the
installed owner, then graceful daemon stop and actual child reap. The concrete local observable is an additive `streams` object on the existing
same-UID `CommandCleanupStatus` result:
`{active:u32, cancelling:u32, uncertain:u32, owned_runners:u32,
unfinished_runners:u32, observation_pending:bool, cleanup_ready:bool,
stop_complete:bool}`.
Compute counts from the actor's durable charged records and owned runner barriers;
`unfinished_runners` counts every handle whose `finished()` is false (including
terminal-but-not-reaped cleanup), `uncertain` includes Lost/UnknownLaunch/ambiguous
launch/publication holds. `cleanup_ready` requires active/cancelling/uncertain and
unfinished counts zero with no pending preparation observation; `stop_complete`
additionally requires the actor's durable and physical stop confirmation. A bounded
retained finished-handle count is permitted. No secrets or new execution operation
are exposed.
Extend the shared strict installed_cleanup Status deserializer with an optional,
bounded streams object so old finite consumers accept the additive field while
preserving their prior acceptance rules. A separate stream-aware proof validator
requires it and rejects missing objects from old candidates rather than assuming
zero. Add regressions for old candidates, new candidates and malformed objects.
After zero-state proof, close admission and stop the owner; serialize the existing
internal `LifecycleStopReport.stream_stop_complete` boolean in the final server
stop report when an Open was admitted, and require true together with
engine/MDK/runtime cleanup completion and observed daemon process exit. Client-only
owners intentionally omit this server-side observation; they still require the
physical stream status proof and factual engine/MDK/runtime joins. Evolve all affected strict in-repo stop-report
readers/tests together, preserving existing fields/schema and rejecting false
stream completion in all reports or missing completion in a qualified server
report. This small additive IPC/report change
is an explicit implementation/review deliverable, not an assumed existing field.
A zero snapshot alone cannot replace final barrier completion. Copy helper uses
that same owned-runner barrier; no separate PID-file proof is accepted. Finite
engine quiescence alone cannot prove stream shutdown. Lost/Unknown holds
require explicit administrative reconciliation with honest possible-live warning;
a test must not clear quota just to label cleanup successful. Remove only owned
private state after proven cleanup. If cleanup fails, record unresolved state and
fail the cohort; runner teardown is not a product cleanup pass.

Upload only closed sanitized records: exact commit/run/attempt, target/OS, package
and rsync versions, pins, case IDs, stream/transfer ID digests, byte counts/hashes,
exit/category, monotonic durations, bounded publication counts and cleanup status.
No keys, raw identities/configs/databases/checkpoints, raw command output, user
paths, HOME, credentials or arbitrary logs. Use digests or fixed role labels for
operational absolute paths; server-ready carries only fixed role-relative labels,
which are public exchange metadata with14-day retention, not private locators. Artifact retention14d, bounded record
size existing16MiB, no raw failure directory upload. Preserve sanitized finite
failure records even when a platform/job is cancelled or times out. The driver
must sanitize and bound stderr/stdout before inherited supervisor stdio reaches
CI logs; raw diagnostics are private owned local files, never artifact uploads.

For the three positive matrix cohorts, rollup requires all three job terminal
records and all9 directed matrix cells; edge and interruption cohorts require
their separately specified same-platform records. Require
client/server receipt agreement, installed/source bindings, byte equality and
confirmed cleanup. Any failed/missing/stale/duplicate/conflicting cell prevents
PASS. Include the six off-diagonal pairs explicitly. Report slow/missing runner
start, setup prerequisite failures, artifact-service errors and relay timeouts
separately from protocol failures, without hiding either behind a successful
partial matrix. Every dispatched run gets a sanitized repository validation report.

For each positive cell record cold setup/room-to-first-response wall latency,
first payload latency, transfer wall time, and exact raw bytes per direction.
Measure with local monotonic clocks; never subtract remote timestamps. State
whether handshake, disk sync/hash and final ACK are included. Goodput is verified
raw payload bytes divided by measured transfer seconds; label compressed logical
throughput separately from actual encoded bytes when available. No inferred
relay/network bandwidth or invented percentile from one sample. Publish per-pair
n=1 observations plus range/median across comparable cells, explicitly labeled
cross-platform/environment observations rather than a controlled benchmark.
Keep existing benchmark cohort semantics intact; add repeated warm benchmarks
only as a separately reviewed bounded workload. Record compression setting,
relay region, runner platform, rsync version and known measurement limitations.

## Implementation and exit criteria

First add candidate-only provenance/render tests and cohort schemas, then driver
modules and supervisor routes, then hermetic orchestration tests, independent
code review, and only then actual three-platform dispatch. Node tests must cover
wrong candidate workflow/source/attempt, missing stream-crate fingerprint,
malformed artifacts, expiry/queue lateness, cancellation and missing outcomes.
Rust harness tests cover fixed workload manifests, receipt reconciliation,
strict status readiness, installed-image substitution, deadlines and stream-aware
cleanup. Run actionlint and targeted package/installer/harness checks. Coordinate
shared Cargo and live test ownership with the ongoing feature work.

Acceptance requires actual install/setup/launcher plus data/command validation
on all three platforms, all27 positive directed cells across the three matrix
waves, the named per-platform edge cases, preserved failures, sanitized latency/
goodput tables, and independent Claude code review with blocking findings fixed.
Do not claim macOS/Linux ARM64 results from local Linux AMD64 tests. Publication
of reviewed source/evidence to GitHub and Radicle follows AGENTS.md; public binary
release, version cutover and installer default updates require separate authority.

## Integrated cleanup observation contract

The canonical additive stream observation is `streams` with required counters
`active`, `cancelling`, `uncertain`, `owned_runners`, `unfinished_runners` and
required booleans `observation_pending`, `cleanup_ready`, `stop_complete`.
Qualification requires zero active/cancelling/uncertain/unfinished counts, no
pending observation, and both cleanup booleans true. The owned-runner count can
include physically finished retained handles and remains bounded. Missing
observations never qualify a streaming cohort.

The stop report's optional `stream_stop_complete` is required true for a daemon
with admitted server Open state. Client-only daemons omit this server-owned
observation; their separate physical stream status and factual engine-stop, MDK
and runtime-cleanup joins remain mandatory. A false observation always fails.
This additive contract preserves old finite-driver decoding without claiming that
finite proof establishes stream cleanup. Public binding fields use stream,
session and transfer SHA256 digests; raw identifiers remain private for explicit
recovery commands. Literal argument digests cover the exact argument byte vectors;
the installed fixture image is independently pinned by its executable hash.

### Bounded negative-receipt preparation

The same-platform copy-negative family uses tiny real installed Put operations,
not a fabricated receiving checkpoint. Once the actual committed destination and
canonical checksummed receipt have been independently observed, the driver may
mutate its own source bytes to require Resume manifest refusal, select a different
owned destination to require binding refusal, and corrupt that exact committed
checkpoint to require closed-codec refusal. Restore source bytes and metadata
before the corruption case. Every refusal must preserve the already committed
destination's exact bytes, mode and timestamp, record its concrete finite error,
and reconcile the owned helper/stream. This is negative fixture preparation,
not evidence of an interrupted transport or successful resume; the separate
checkpoint-observed interruption waves remain mandatory. The detailed technique
is subject to its scoped design review before receipt mutation implementation.

### Proposed installed pre-Open signal observation

This additional process-control technique requires scoped design approval before
implementation. In the isolated same-platform edge-stream wave, no other job/client may
use the owned server. First establish its room through a completed ordinary case.
Retain the server daemon's owned Child mutex/unreaped-leader identity, temporarily
SIGSTOP that daemon, then start the actual installed stream CLI. Its exact bounded
stream-ID line is printed after room preparation and immediately before Hello;
the paused server cannot answer Hello, so Open cannot be attempted. Signal the
owned client with INT and require the exact before-Open diagnostic, no generic
may-continue warning, and its production-defined exit. The baseline stream error
mapping currently returns255 for `stream_interrupted_before_open`, whereas an
actual post-Open confirmed Cancel returns130; integration must verify the actual
qualified source rather than assume those are identical.

A synchronous RAII guard resumes the same unreaped owned server leader with
SIGCONT before releasing its Child mutex on every return, error or future drop;
no async Drop or receipt-derived PID authority is used. Failure/exited daemon
cannot qualify. After resumption, process the pending Hello, require its fresh
stream record has no Open digest and no Cancel latch, require the fixed fixture
marker absent, and compare all admitted Open/cancel fences. Public ID output alone
is not launch evidence. No fixed-delay signal, fake IPC daemon or private config
mutation substitutes for this observation.

The pre-Open observation is pinned to canonical Phase3's existing additive
`StreamEntry` fields, not a new `hello` IPC field: require the server-side fresh
record to have `state="hello"`, `open_digest=None`, `active=false`,
`cancel_requested=false`, and no terminal. Only Exchange is refused before allocation; Status, Cancel, Hello and Open can
allocate. This record is attributed to Hello because the production CLI first
and only pre-Open request is Hello and this isolated server has no other client
for the unique stream ID. If the qualified source lacks `open_digest`,
`group_sha256` or `active`, the case is blocked; state-only checking is forbidden. The client daemon's
matching locally queued Hello record is required before signalling, so seeing a
stderr line alone cannot pass. If Hello was never dispatched/received, the case
fails as unproven rather than accepting absence as success.

Take baseline entries and physical idle status before pausing. Require zero
active/cancelling/uncertain/unfinished runners and no observation pending;
retained physically finished handles may remain. Hold the Child mutex inside the
RAII guard, and query no server IPC while paused. The guard sends SIGCONT before
its mutex field drops; subsequent queries use its existing `&mut Child`, not a
second mutex acquisition. Require `try_wait()==None` immediately before STOP and
after CONT. A successful signal to an unreaped zombie does not establish liveness.
On an error, abort/reap the client, then resume server, then normal cleanup;
future cancellation still invokes the synchronous resume guard.

Clip pause duration to20 seconds and the original family/cohort deadline. Fresh
room creation (`--new-session`) is forbidden; a reused-room prepare that does
not finish inside this bound fails. Before STOP require enough remaining configured
server uptime for this pause plus normal cleanup; the edge-stream wave uses no command
count limit, no active stream idle timer and an1800-second service bound. After
CONT require active daemon Status and the present Hello-only record inside the
original cohort deadline. The still-running client daemon may finish its
already queued Hello publication even after its initiating CLI exits.

Require client `code()==255`, no termination signal and empty stdout. After the
consumed ID line, stderr must be exactly
`marmish stream: interrupted before remote Open; no remote child was launched`
and `marmish: invalid input: stream_interrupted_before_open` with their actual
qualified-source line endings/error display; verify that static Display spelling
before implementation. No requesting-cancel or may-continue diagnostic qualifies.
Compare Hello/Open/cancel observations before ordinary `cancel_all`: that cleanup
may deliberately retire the Hello-only record afterward. No pre-observation
Cancel is allowed. Final lifecycle/physical cleanup remains mandatory.

### Measured edge-wave budget refinement

The first actual Phase3 relay smoke completed12 operations in334.734399s;
64KiB transfers took30.889–33.136s, empty transfers21.263–21.559s and negative
calls12.517–35.921s. A120-second family containing10 or more sequential copy
calls cannot qualify on that evidence. Replace the aggregate `edge-stream`, `edge-rsync`, `edge-copy`
wave with three separate closed cohorts: `edge-stream`, `edge-rsync`, and
`edge-copy`. Dispatch them sequentially, each with fresh disposable identities,
owned daemons and its own normal3300-second context; no paused server or fixture
gate is shared with another wave. Every cohort retains the existing absolute
1200-second traffic bound:1140 seconds execution and60 seconds reconciliation,
plus the separate180-second final owned-process cleanup allowance. Never renew
that deadline per case, convert a timeout to an expected refusal, or shorten
required cases silently. Each cohort still runs its same-platform cases on all
three installed platforms, reconciles server/initiator evidence and publishes
sanitized latency/count/cleanup records. The27 positive cross-platform cells and
separate interrupted Put/Get waves remain unchanged.

`edge-stream` contains ordinary command admission, before-Open INT, confirmed
post-Open INT, EPIPE/SIGKILL continuation with second-client cancellation, and
two-held-helper quota refusal. `edge-rsync` contains all six maintained-rsync
signal/timeout/independent-failure cases; its aggregate deadline is1140s rather
than the old240-second estimate. `edge-copy` contains both directions' ordinary
and actual committed-receipt negative cases, currently22 actual operations
including isolated receipt seeds; its aggregate deadline is1140s rather than the
old120-second estimate. At the observed maximum35.921s per operation,22 calls
would use790.262s, leaving349.738s within the execution bound. This is a sizing
observation, not a guarantee for other relays/platforms. All failures and measured
actual elapsed times remain evidence. Per-case parser bounds are finite and may
be shorter only where justified; the common original deadline remains the cap.

Each wave is a separate manual workflow_dispatch, not an aggregate job. Its
resolve10min, matrix65min, rollup10min ceilings remain unchanged (85min maximum
per dispatch). A serial eight-dispatch campaign therefore has680min of job
ceilings, plus the separately bounded90min candidate build. These are ceilings,
not expected measured duration, and there is no shared timeout. The concurrency
group retains only one pending run: dispatch the next wave only after the prior
rollup is terminal; do not queue eight runs simultaneously.

The closed membership below supersedes all old aggregate family wording and the
previous six-wave/cohort list. Remove `transfer-edges` entirely rather than alias
it. Update Node STREAMING_COHORTS/cohortLifetime, Rust COHORTS/cases, server native
image allow-list classification, prerequisite capture and workflow choice list
together; tests must reject the old name and accept3300s for all three new names.
Only edge-rsync requires maintained rsync pin/bootstrap; edge-stream's rejected
nonsecluded bootstrap is parsed locally without executing rsync.

| Cohort | Closed case IDs | Installed relay-invoking CLI calls | Other local CLI calls |
| --- | --- | ---: | ---: |
| edge-stream | missing-executable, policy-denied, rsync-bootstrap-without-seclusion |2|1|
| edge-stream | stream-sigint, stream-epipe-detach, stream-sigkill-detach |3 initiating +2 second-client cancels|0|
| edge-stream | stream-signal-closed-output |1|0|
| edge-stream | stream-preopen-interrupt |1 Hello-only attempt, no remote command|0|
| edge-stream | stream-peer-quota |3 initiating +2 second-client cancels|0|
| edge-rsync | rsync-sigint, rsync-sigterm, rsync-sighup, rsync-group-sigint, rsync-timeout-usr1, rsync-independent-failure |6 initiating rsync invocations; in-band cancellation not a second CLI|0|
| edge-copy | put/get-empty, put/get-existing-refusal, put/get-overwrite, put/get-symlink-source, put/get-directory-source, put/get-destination-symlink, put/get-destination-directory |14|0|
| edge-copy | put/get-receipt-seed, put/get-resume-changed-source, put/get-resume-changed-binding, put/get-resume-corrupt-checkpoint |8 maximum (binding refusal may happen locally pre-Open)|0|

Counts are actual process invocations, conservatively charged as relay calls even
when an early refusal avoids transport. Edge inventory, marker, canonical receipt
and stream-list/status verification are descriptor-bound filesystem or same-UID
local IPC operations because the same-platform orchestrator owns both endpoints;
there are no extra remote inventory/receipt/check commands for these edge cases.
The positive matrices retain their separately counted remote fixture-control
commands; those do not get copied into the edge count. At the observed maximum
35.921s/call, edge-stream14 relay attempts cost502.894s, edge-rsync6 cost215.526s
plus its explicit rsync `--timeout=30` bound, and edge-copy22 cost790.262s. Remaining execution headroom
is637.106s/894.474s/349.738s respectively for startup/observations; measurements
are sizing evidence and all actual deadlines remain fail-closed.

Edge-stream rollup requires3same-platform records with9 case observations each
(the quota case includes two held+one refused), edge-rsync requires3×6=18 case
observations, and edge-copy requires3×20=60 case observations plus two explicit
receipt-seed results per platform (66 observed calls including seeds). No
nine-pair inference applies to these waves. Exact list uniqueness, no missing or
extra case IDs, both independently checked endpoint bindings, physical cleanup
and sanitized failure records are mandatory; one cohort PASS cannot substitute
for another's missing result.

Literal argv/path and stdout-purity checks remain in the27 positive platform
cells. Valid-data duplicate retry and a signal pending at the exact EPIPE branch
remain separately identified hermetic tests: the installed stream-signal-closed-
output case closes the owned reader while INT is pending and requires confirmed
cancellation, but does not claim the kernel surfaced EPIPE before cancellation.
No private wire injection is added to the public installer cohort.

The four receiver destination-symlink/directory cases preserve the original
receiver-side nonregular refusal requirement; their descriptor-bound sentinels
must remain unchanged. The scoped copy-edge design review must approve their
precise existing-file/UnsafePath diagnostic before implementation. This reviewed
split amendment is the additional bound required by the earlier statement that
further splitting needs review; old aggregate estimates are superseded.

Positive-matrix workload counts are fixed separately from the edge table: each
directed stream pair invokes1 duplex CLI (9percohort); each directed rsync pair
invokes push+pull plus2 fixture-control commands per transfer (6perpair,54total);
each directed copy pair invokes put+get plus2 fixture-control commands per transfer
(6perpair,54total). Fixture controls are pre-inventory and post-receipt operations.
Their own durations and total counts remain charged to the1200s traffic bound
and represented separately from data-operation samples in rollup. Literal argv,
awkward paths and stdout-purity assertions belong to these positive cells, not
extra edge cases. Valid-data duplicate retry preservation remains mandatory
hermetic protocol/actor coverage at the qualified source; its concrete test names
and results must be mapped before final acceptance, rather than invented as an
installed CLI case. Edge-copy's22closed IDs mean20cases plus2receipt seeds, all
required uniquely even though seeds are counted separately in summary metrics.

### Rollup publication failures

A qualification PASS requires a green rollup job from the current run and
attempt, `complete:true` in `matrix-rollup.json`, and absence of
`matrix-rollup.failure.json`. Markdown is non-authoritative. Evidence records
remain immutable. Rollup validates and renders both report formats before
publication, requires a fresh owned empty output directory, writes Markdown
first and JSON last, and attempts the immutable failure marker first on any
validation or publication error. It then attempts legacy sanitized FAIL outputs
without replacing existing files and returns the original error. A context that
cannot be read or validated produces a red job without a context-bound marker.
If storage also prevents writing the marker, the red job still prohibits PASS.
A successful link followed by a failed directory sync may leave visible JSON;
JSON alone therefore never proves qualification.

The closed failure categories are `missing_platform_records`,
`validation_or_platform_failure`, `publication_failure`, and `output_not_empty`.
The marker matches the existing `matrix-rollup.*` artifact upload glob. Regression
coverage must inject a post-link second-publication error, a first-publication
failure, and foreign preexisting output, preserving unknown bytes and inode
identity. Fault hooks are test-only scoped RAII state. The same legacy finite
matrix ordering is tracked separately by Beads `marmish-yc6`; this streaming
change does not claim that follow-up complete.
