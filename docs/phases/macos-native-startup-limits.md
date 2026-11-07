# macOS native startup descriptor limits

Status: independently approved bounded product design; see the
[actual review record](../reviews/macos-native-startup-limits.md). Beads and the
coordinator own execution status.
This changes process startup compatibility, not streaming traffic or acceptance.

## Evidence and diagnosis boundary

Actual cheap diagnostic run `42397433443/1`, source
`fa7130238958ced782677ccdfea5b64cae7b3353`, passed its closed summary on
macOS 15.8.2, image `20261104.0514.1`. The shell inherited soft NOFILE 10240
and hard infinity. The /bin/sh child under the actual bundled Node v24.22.1 action reported soft
1048575 and hard infinity. The original qualification launches its Rust driver
through Node24; the successful separate diagnostic launched it through bash.

`crates/marmish-engine/src/platform/macos.rs::prepare_spawn_below` rejects
soft NOFILE greater than its fixed 65536 descriptor ceiling, even when no
actual descriptor reaches that ceiling. `launcher::worker_preconditions` calls
this during engine bootstrap. `CommandEngine::start` currently collapses an
engine open error to `Event::OpenFailed`; subsequent live preparations are
unavailable while admitted stream records can remain Preparing. The measured
Node limit is incompatible with this concrete gate. The failed original engine
error itself has not been observed; do not claim a captured root-cause error or
that this proves every Mac failure has this cause.

## Smallest supported compatibility policy

Add one public engine entry point, `initialize_process_limits() -> Result<()>`,
implemented through the existing platform module. Its documented caller
contract is executable startup before creating threads or handing out engine
work. It is not automatically called by `Engine::open`, `prepare_spawn`, live
preparation, an actor, or `pre_exec`. No process-global limit changes happen in
an already running daemon. The API is ordinary safe Rust; its startup timing is
an explicit operational contract, not an invented memory-safety precondition.

On macOS it reads RLIMIT_NOFILE and applies these ordered rules:

1. Read failure returns the existing fixed `EngineError::WorkerUnavailable`.
2. If soft is at most 65536, leave both limits unchanged.
3. Otherwise lower only soft to 65536 using the original hard value. Never raise
   soft, alter hard, or special-case infinity by converting it to a finite hard
   limit. Failure returns `WorkerUnavailable`.
4. Verify the resulting soft and hard values; unexpected values fail closed.
5. Open no descriptors, enumerate no descriptors and change no descriptor flags.
   This startup API normalizes and verifies limits only. Actual high-FD refusal
   remains at existing engine bootstrap and spawn boundaries, not help, client,
   vault or copy-helper entry. Lowering soft does not close inherited high FDs.

Keep this operation in the existing platform module without copying Darwin
descriptor enumeration into startup. The
existing `prepare_spawn` check remains in place at engine bootstrap and every
spawn. It still rejects library callers with inflated limits and any later
unsafe limit change. Repeated startup calls are harmless but do not authorize
threaded use. Linux implements a no-op success without reading or changing
limits or descriptor flags.

This supports inherited excessive *allocation capacity*, not arbitrary inherited
high descriptors for engine execution. Engine refusal of an actual high FD is
deliberate; help/client commands acquire no new high-FD refusal. Do not close
unknown parent descriptors or silently proceed after engine enumeration failure.

## Earliest executable integration and errors

Call the API first in both native binary wrappers,
`marmish-native/src/bin/marmish.rs` and `marmishd.rs`, before calling
`marmish_native::cli::main`. This is earlier than argument allocation, copy helper
and provider helper dispatch, daemonization, keychain operations and Tokio
construction (`cli::main` currently builds Tokio near line1307). Both wrappers
have one native process entry; no mutation is hidden in a reusable async API.
On failure emit only `marmish: process_limits` and return exit1 before resources
or threads are started. Exit1 is intentional before argument parsing, including
commands whose later operational failures normally use255. No path, FD
inventory, account or key appears in errors.

Apply the same startup call to both dedicated worker wrappers,
`marmish-native/src/bin/marmish-worker.rs` and
`marmish-engine/src/bin/marmish-worker.rs`, before their current initialization.
The native worker is the third shipped binary; the engine-crate worker is an
optional library development binary, not a fourth shipped artifact. Install each
worker panic hook first, then normalize, then preserve the existing core-limit
setup. Use only `marmish worker: process_limits`, exit1 on failure. Normal engine-launched
workers inherit the normalized limit; direct supported worker startup also gets
the contract. Preserve current panic/core-limit/layout and handoff checks in
order after this new preflight (except the panic hook, which remains first). Do not change feature-only installed-check or
fixture entry points: the actual installed product children normalize their
own inherited limit. Do not normalize the Node test supervisor or lower its
traffic to hide the product incompatibility.

## Descriptor and execution invariants

Keep Darwin's existing syscall-only `pre_exec` sweep over descriptors3..65536,
which marks them CLOEXEC, and its later explicit exemption for the authorized
copy context FD greater than3. Keep stdin/stdout/stderr, the retained copy
context owner and the `Command::spawn` exec-error pipe semantics unchanged.

Do not substitute `closefrom(3)`: that would also close the authorized context
and Rust's spawn-error channel. Do not enumerate via libproc after fork. Do not
compute a parent snapshot then permit unbounded high allocations before fork.
Do not merely drop the `soft > ceiling` check or sweep an unbounded inherited
limit. No dependency, protocol, journal, command policy or runtime quota change
is required. Child processes will inherit soft at most65536 on macOS; this is
an intentional documented compatibility limit, not an unchanged-environment
claim.

## Validation and ownership

Use isolated subprocesses for every limit mutation; never alter the parallel
test process's limits or global environment. Divide implementation between an
engine/platform owner and a native entry-point/integration-test owner with
explicit disjoint files. The coordinator integrates and obtains actual Claude
AUTO code review. Tests must cover:

- Actual macOS high-soft success: record original hard, set soft to a
  kernel-accepted value greater than65536 (65537 suffices), read it back, invoke
  the startup API and assert exact65536 with hard unchanged. No skip counts as
  decisive normalization coverage. A required hosted Mac runner unable to set
  any high test value fails this qualification with a fixed unsupported-fixture
  reason; do not interpret setrlimit failure as product success.
- Lower valid soft unchanged and hard unchanged; repeated startup unchanged.
  Existing isolated Darwin inherited-descriptor test remains valid: no startup
  call implicitly fixes library `prepare_spawn`'s inflated-limit refusal.
- Preserve `inherited_descriptor_above_lowered_limit_is_rejected`, including its
  real kernel-allocatable reduced-ceiling FD test. Normalize limits then exercise
  that existing engine-boundary rejection without adding startup enumeration.
  Do not assume Darwin permits allocating an actual FD65536; the same existing
  production predicate runs at the reduced ceiling and no unallocatable numeric
  descriptor is claimed as tested.
- Real shipped binary boundary: a subprocess launcher sets verified high soft,
  then execs actual built marmish and marmishd (`--help` or another existing
  finite no-network mode) and observes successful exit. A test-only binary that
  merely calls the API is not this proof. Also execute an existing offline real
  engine spawn under a normalized high-soft launcher; success requires actual
  child bytes/status, not only help text. A help success alone also succeeds
  before this fix and is wrapper smoke coverage, not decisive normalization
  proof. Do not invent a new high-FD help refusal to make this smoke test RED.
  Keep all temporary state owned and
  current process IDs unreaped until safe cleanup.
- Existing `cloexec_sweep_closes_unmarked_descriptor_at_exec`,
  `live::copy_context_tests`,
  `copy_launch::tests::context_fd_allocation_and_spawn_survive_closed_stdio_and_original_fd_three`,
  multithreaded descriptor-inheritance and typed exec-failure tests. Run their
  nonroot paths: context FD survives exactly, unrelated FDs do not, closed0..3
  remain supported and a real exec failure remains NotStarted rather than false
  success. Limit normalization does not justify weakening any existing fence.
- Linux API no-op, preferably with unchanged read-back limits and descriptor
  flags in an isolated child. Existing Linux exec/context tests remain gates.

After focused tests run all affected native/engine suites, strict Clippy, fmt,
diff/ignore audit and quality-delta disposition. Record actual commands,
platforms, nonroot conditions, errors and skipped branches honestly.

## Exit criteria and separate decisions

After reviewed source publication, build one fresh three-platform installed
candidate through the existing workflow, including actual installer and native
security/descriptor tests. Then run the unchanged original nine-pair
stream-matrix once. Require byte/status/binding/cleanup/rollup gates unchanged;
if it fails, preserve its existing progress observations and investigate rather
than repeatedly rerunning or substituting the successful diagnostic. Only a
successful rollup unlocks the other seven required installed cohorts in their
existing serial fail-stop order. All remaining cohort results are real gates;
this fix and a successful Mac startup unit test do not qualify them.

## Bounded companion: permanent bootstrap failure

This is a second explicit review decision in this phase, not implicit authority
to broaden the startup fix. Implement A (engine/platform startup plus wrappers)
and B (native permanent-bootstrap handling) with disjoint agent ownership after
both interfaces are approved. No bootstrap-stage observer or inspection-schema
extension is needed.

Reuse the existing `CommandEngine::readiness()` method, which already calls
collect and returns Opening/Ready/Failed. Retain the existing private
Event::OpenFailed transition without adding an unused typed error field or a
new readiness query. EngineError contains closed variants, not path/string
payloads. No per-tick debug logging or bootstrap-stage observer is added.
Only the actual collected OpenFailed event authorizes this path. Full,
InvocationBusy, Opening and generic Unavailable remain retryable. Panics before
OpenFailed are not converted to permanent failure; that separate limitation
remains explicit rather than catching arbitrary thread panics in this phase.

In `prepare_streams`, after its stopping check and active-server scan, retain
Preparing/cancel/invalidation guards, current command_context authorization,
check_stream_binding and copy_attempt_matches. Insert the permanent-failure
gate **immediately after copy_attempt_matches**, before construction of kind
(which may call fallible copy_helper_context) and before entry/or_insert or the
live.preparing guard. Read an existing live entry with get; preparing=true is
expected for the queued-before-failure case, not a reason to skip. Any owned
runner excludes this path.

Before finishing, re-fetch with stream_get and verify current active Preparing
state, revision and exact binding, no cancel/invalidation, current authority and
copy attempt. stream_finish is a blind update, so a stale copied record is not
sufficient. No await separates these guards and commit. Finish with state
terminal and Terminal::Failed { code: Invalid } using stream_finish. This is a
deliberate common bootstrap preparation failure for Exec and NativeCopy; the
existing per-preparation NativeCopy WorkerUnavailable→Unsupported mapping stays
unchanged. Do not claim Denied, child launch, Exited or confirmed cancellation.
An optional fixed local preparation_failed category must not change wire/schema.

Commit **before** any local reclamation. stream_finish transaction failure
aborts this tick through the existing ?; retain pending engine/live IDs so the
next tick can retry the same durable finish. After successful commit, retire
that stream's local pending preparation and LiveStream preparing/operation ID.
Never clear pending state first to avoid losing the finish retry fence.

Opening currently permits queuing requests. OpenFailed is emitted only before
Worker construction, while live_rx is consumed only after Ready. This supplies
the proof that its pending preparation was never submitted to the engine and
could not launch a child. Add a narrow failure-gated bookkeeping retirement
method on CommandEngine; generic disconnected/stopping/unavailable is not this
proof. Do not replace this with disallowing Opening requests and overlook those
already queued.

Separately run a local bookkeeping retirement sweep **outside the stopping
early return**, before new preparation work. Only when readiness==Failed and no
runner exists, find LiveStream entries whose journal record is absent or already
inactive. Retire their engine/live pending IDs without any journal mutation.
This is required for Cancel or stream_begin_stop that already durably finished
a Preparing record while its Opening request was queued; otherwise observation
pending and cleanup_ready/stop_complete remain stranded. Inactive retirement
must not revive or rewrite grants. Active revoked/invalidated records remain
conservative and charged; Launching, owned runners, Lost/UnknownLaunch and any
uncertain ownership remain untouched. Finite-command recovery is outside B.

Tests use a new cfg(test) gated-open fixture modelled on gated_stop_for_test:
queue while actually Opening, then release bootstrap with an invalid worker
executable to produce real OpenFailed. The engine crate's private hooks are not
visible in native tests. Tests must establish unqueued and queued permanent
failure→FailedInvalid with quota released, no engine job/runner, no resubmit or
double finish count; Opening and Full remain retryable; failed journal commit
preserves pending state then next tick finishes once; queued Cancel and queued
stop retire local pending state only after their durable inactivity and reach
physical cleanup proof; Launching/runner and active revoked/invalidated/wrong
binding or revision are untouched. Test fixed category if one is emitted,
without impossible path-bearing error fixtures or absence-of-log launch claims.

## Exact implementation ownership

A owns engine src/lib.rs, platform/mod.rs, platform/macos.rs (and a Linux no-op
only if routing requires it), new engine src/process_limits_tests.rs registered
from lib.rs, native bin/marmish.rs, bin/marmishd.rs, bin/marmish-worker.rs and
engine bin/marmish-worker.rs. A also owns a new native
`tests/process_limits.rs` integration target, using Cargo-provided actual
binary paths and isolated child launchers. Existing live and context test
helpers may be reused inside an adjacent engine test module without copying a
new public execution framework. Any extra source file needs coordinator consent.

B owns native src/command_engine.rs and src/stream_runtime.rs, adjacent
cfg(test) helpers, and append-only bounded tests in src/mdk_adapter_tests.rs
using its existing private authenticated MLS/SQLCipher Fixture. Do not duplicate
that substantial fixture in stream_runtime.rs; only minimal cfg(test) fixture
visibility changes are permitted if necessary. No A/B overlap; Cargo lane is
shared serially. The
coordinator owns this design, specs, review and sanitized validation evidence.

Fresh candidate and original matrix acceptance above covers A+B together. No
new release, version bump, public release publication, changed workload, or
command/traffic deadline is authorized by this phase.
