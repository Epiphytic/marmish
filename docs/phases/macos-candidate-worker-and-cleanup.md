# macOS candidate worker isolation and cleanup evidence

Status: independently approved corrective design (actual Claude AUTO session
`5dbdcf45-15d6-4b29-8b48-0af077021675`, corrected round2 APPROVE,
36.293s/3turns). Beads `marmish-8ka.5.8` owns execution status.

## Observed failure and limits

Candidate `42400376357/1` at
`53f94f99d06942a58f52fd4a76048a26ced1a99d` failed macOS job
`128486633561` during native verification. Engine library tests passed
148/148 with one existing ignored probe; integration targets failed
encrypted_storage (1/3), engine (12/14), native_storage (4/5), mostly with
NotStarted/InputClosed. Native library tests passed626, failed1, ignored3;
the failure was `final_handle_drop_releases_full_unacked_output_after_exit`
asserting `!uncertain`. Other suites and startup smoke passed. Nested isolated
test counts must not be added to outer totals. Linux jobs were cancelled after
this decisive failure; no package, installed cohort or release qualified.

The completed Mac log explicitly warns that engine and native both emit
`target/debug/marmish-worker` (and its dSYM). Engine integration fixtures stage
that executable alone. Native's mandatory sibling `marmish` layout check would
refuse that fixture before claiming work, consistent with NotStarted rather
than a persisted command LaunchFailed. The winning executable bytes were not
retained, so this is a strongly supported collision diagnosis, not a captured
wrong-binary identity or proof against every other defect.
The collision and native layout contract predate this increment (including
v0.2.0); they do not by themselves explain why this particular run failed.
The rename is a bounded hypothesis test and eliminates an actual ambiguous
build output regardless of the winning bytes.

The cleanup test observes a full stdout window but does not establish its
advertised natural-leader-exit premise. The log collapses multiple genuine
uncertain paths; it does not identify a syscall or justify weakening cleanup.
An adjacent observed-exit test and existing live last-handle cancellation test
passed. Neither fact waives the failed race.

## A: unique development worker

Rename only the engine crate's standalone binary target to
`marmish-engine-worker`, retaining its source path and startup behavior. Cargo
metadata must show exactly that engine target and no implicit engine
`marmish-worker`. Update every engine integration CARGO_BIN_EXE reference,
prototype bridge lookup/build hint, and both historical Python bundle build
consumers (`tools/marmish_ci.py` and `tools/marmish_release.py`). Update active
manual build instructions, including phase-1/fault-injection commands and
current validation instructions; retain historical run evidence as history.
Fixtures and legacy bundles may continue installing the unique source bytes
under their existing private/bundle destination name `marmish-worker`.
Native shipped binaries, manifests, layout checks, installation names and
security checks remain unchanged. Do not work around collision by splitting the
original combined candidate test command or accepting a warning.

Verify target identity structurally with Cargo metadata, compile both packages
together, and execute the engine integration fixtures (including exact binary
stdin/stdout/stderr and real exit status). Run the original combined candidate
command with all four packages. Preserve prototype compatibility by testing the
updated bridge and validating both historical bundle source/destination names.
The engine fixture staging helper must verify the source target basename when
staging the normal engine binary through a narrow `stage_cargo_worker` wrapper
(do not misclassify deliberate negative worker
fixtures). After a combined four-package --no-run build, record the metadata
target names and SHA-256 of both distinct debug worker outputs before tests;
missing outputs fail this check.

## B: preserve cleanup assertions and narrow the failing operation

Add only private `cfg(test)` failure-site recording to State and
`cfg_attr(test, track_caller)` to Shared::failed. Capture a fixed source location
when failure is recorded and include it in relevant test assertion diagnostics.
Record the finite EngineError variant where one exists, lifecycle booleans
exit_seen/cancel_started/cancelled_while_running/killed, stdout credit and EOF.
Keep this private test state and fixed diagnostics; do not format arbitrary I/O
errors. Callsite plus a finite engine error identifies the failed operation,
not necessarily its internal Darwin reason: signal, exit or reap failures may
still require a separately reviewed engine diagnostic iteration. No claim of
capturing the underlying syscall or zombie-group reason is authorized here.
No wire, persistence, provider, external callback or production behavior change.
If terminal outcome mapping creates uncertainty outside Shared::failed, expose
that distinct test diagnostic as well. Do not log command input, environment,
keys, output or arbitrary native error strings.

Make the after-exit case deterministic with a test-only latch set only after
successful `try_exit()` observes WNOWAIT-held leader exit. Wait for both that
latch and a full window before dropping the last handle. Require actual exit7,
finished and `!uncertain`. No longer grace or polling timeout.

Retain the original dd/shell full-window final-drop race as a separate named
test, preserving its completion and `!uncertain` assertions. Also add a
guaranteed-running single-image full-window producer (`exec yes`) final-drop
case, require Cancelled and `!uncertain`, and retain existing TERM-ignoring live
drop coverage. Exact test names are
`final_handle_drop_releases_full_unacked_output_after_exit`,
`final_handle_drop_releases_full_unacked_output_race`,
`final_handle_drop_cancels_live_full_unacked_output`, and
`final_service_handle_drop_finishes_owned_cleanup`.
This separates natural-exit abandonment from live cancellation
without removing either race or backpressure coverage. All owned children and
test state must be cleaned on assertion failure. Add a test-only RAII guard
constructed by moving the TempDir into it: it clones shared state/wake only, requests cancel
and waits on finished within the existing eight-second test bound before the
directory can drop. A deadline is a failed cleanup assertion (during unwinding,
emit a fixed diagnostic and avoid a double panic; the original test still fails).
The guard owns the TempDir and retains it on cleanup deadline rather than
deleting state while a detached runner remains active. It can borrow the
handle during construction to clone only shared state and the wake Thread.
Explicitly release the last service handle for each tested action: the guard
must hold only shared state/wake capability, not prolong Control ownership.
Place the leader-exit latch in cfg(test) State, which already has Default, or
explicitly update every Shared literal if Shared storage is chosen.

This increment instruments the unclassified failure; it does not claim a cleanup
product fix. Any demonstrated product correction requires a concrete follow-up
design and independent review. Never swallow RecoveryUncertain, relax process
identity/group checks, ignore a test, increase deadlines, or reinterpret a
failed assertion as qualification.

## C: focused macOS reproduction gate

Add a manual-only `macos-native-regression.yml`, contents:read, macos-15,
nonroot, fixed Rust toolchain, 45-minute job limit and immutable source input.
Dispatch from master only after committing and pushing the approved workflow to
both remotes under the existing phased publication authorization. Require the
source input to equal GITHUB_SHA (the dispatched workflow commit on master),
and verify checked-out commit equals the exact input. No relay, credentials,
product installation, live transport, package/release publication or changed
candidate workflow. Preserve single-thread native tests and the existing
nonroot permission-test requirement. Run the original combined four-package
test command first with --locked and --no-fail-fast, preceded by the combined
--no-run build and target/hash capture described above. In a separate
`if: always()` step conditioned on successful compilation, repeat the four
focused after-exit/original-race/live-full-window/existing-live-drop tests
20 times, preserving --nocapture diagnostics and full-path --exact filters with nonzero
execution. A combined-suite failure must not prevent collection of the focused
race evidence; its failed step still makes the job fail. Focused repetitions
stop on their first failure. This is diagnostic evidence,
never release qualification or a substitute for all3 candidate+all8 cohorts.

## Integration and exit criteria

Delegate A, B and C with disjoint file ownership. One local Cargo lane. Root owns
docs, integration and actual Claude AUTO review. Local focused suites, combined
engine/native tests, strict Clippy, fmt, workflow policy checks and staged
secret/ignore review must pass. Record honest static-quality limitations.
Obtain actual integrated Claude AUTO code approval, commit/push both remotes,
then dispatch the focused Mac gate once. Preserve failures and exact source
pins. If it passes, run a fresh all-platform candidate and original installed
qualification; if it fails, classify the recorded branch before another fix.
No version bump, public installer-pin change or new release is authorized here.

## D: closed-stdin fixture isolation (new local failure)

Status: independently approved before implementation, actual Claude AUTO session
`17daba31-37b8-4b31-aa3a-4f679cc87085`, round2 APPROVE,13.452s/2turns.
Approval is recorded in Beads `marmish-8ka.5.8` and the review record.

The unchanged combined local command failed after665.33s: native passed629,
failed1, ignored3. `closed_stdin_retains_delivery_cursors_and_natural_exit`
expected stdin_delivered0 but observed13. Preserve this failure and its log;
isolated successful repetitions cannot reclassify it as a green suite.

Read-only inspection finds no deliberate retained reader in LiveChild. The
launcher uses Command's piped stdio with a custom pre_exec; Linux marks inherited
descriptors CLOEXEC rather than closing them before exec. Another concurrent same-process
fork could temporarily retain a pipe reader. A controlled Linux pipe/fork check
demonstrated that a target's stdout marker after fd0 closure does not establish
global absence of readers: a write of13 bytes succeeded while a separate child
held the inherited CLOEXEC reader, then returned BrokenPipe after that child
released it. The target exited9. This demonstrates the missing fixture premise;
it does not capture the actual failed combined run's interleaving or prove the
same mechanism on macOS. The stream's successful-write delivery accounting is
correct and must remain unchanged.

Run this existing exact named test in its own self-executed test process, with
only the original test body executing after exec. Preserve original command,
accepted13/delivered0, immutable retry cursor/conflict/EOF, closed stdin and
natural exit9 assertions. Do not relax values, add a retry-until-green, ignore
the test, serialize the whole library, or change runner production behavior.
Parent uses a private marker only on Command's child environment, never process
environment mutation. Child starts from the same executable and full exact test
name; supervisor must reject zero execution/ignored/skipped false successes.
Reuse an existing suitable bounded native test helper if available; avoid a
generic framework for one fixture. Keep child supervision bounded, clean owned
test processes and state on panic/failure, and preserve diagnostics. A test
supervision watchdog must not extend any product cleanup or command deadline.
Child fixture uses the existing cleanup guard so assertion failure requests
owned cleanup before private state disappears. Do not signal an unverified
persisted PID/group. Exact supervision and failure handling must be reviewed
before implementation, including the limit on physical cleanup guarantees if a
test binary itself hangs.

Concrete supervisor: parent owns a private TempDir and supplies its canonical
path as child-only TMPDIR alongside a fixed recursion marker. The unchanged
prepare() creates the child fixture below that root. Selfexec passes the full
exact test name and `--test-threads=1`; verify successful exit and the summary
of exactly1 passed,0 failed,0 ignored. Capture diagnostics without a pipe-fill
deadlock (private file or bounded continuously drained stream). Parent has a
monotonic60s watchdog. On timeout retain the parent-owned root, kill/reap only
its owned test Child, poll reaping for at most2s, and fail explicitly as cleanup uncertain. A
failed kill or unreaped child is also uncertain, with private state retained;
never block in wait() beyond the supervision deadline. Never claim
that killing the test process proves cleanup of the runner's independently
owned process group. This pathological timeout has a documented physical-cleanup
limit; success/panic cleanup remains the child's guard responsibility. Retain
private state on child failure as well, because a child guard may have retained
its own state after a cleanup timeout. Normal success removes the parent root
only after successful child reaping and exact successful-test verification.
The60s test watchdog exceeds the child's three8s waits plus possible8s guard
cleanup (32s), leaving28s for process startup/scheduling. It does not extend
product/command deadlines. Parent RAII failure cleanup must retain the root and
attempt bounded owned-child kill/reap even if an assertion panics after spawn;
avoid double-panic. Capture reads are capped (for example64KiB); on failure print
only a bounded8KiB diagnostic tail. Require the exact successful named-test line
in addition to the1-pass summary. Accept child mode only for the fixed exact
marker value. Construct CleanupGuard around the unchanged prepare() output.
Do not print private root paths. No new production API or runner poll hook.

Implemented code-review disposition: ordinary child-result failures print the
bounded capture tail; a pathological supervision timeout currently emits only
fixed timeout/cleanup-uncertain labels and retains the root. Actual integrated
Claude round2 accepted this as a non-blocking known diagnostic limit. It does
not change failure status, retention or physical-cleanup uncertainty. A future
test-source increment may share the tail reader across both failure paths.

Validate the controlled missing-premise demonstration, unchanged-body isolated
execution, focused cleanup regressions and the original combined four-package
command without changing its flags/workload. Strict all-target engine Clippy
must also enable fault-injection to check the staging-helper finding from the
initial integrated review. Repeat final integrated review after these fixes.

The failed run used `/usr/bin/time -p env
CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target cargo test --locked
--no-fail-fast -p marmish-engine -p marmish-stream -p marmish-native -p marmish-vault`
from `/home/openclaw/repos/marmish-installed`, with native image
`target/debug/deps/marmish_native-786968b731ec437d` in that shared target. No thread
setting was explicit, and the historical inherited RUST_TEST_THREADS was not
captured; it is unknown. Source was held by agent coordination, not pre/post
file-hash attestation. Do not assert that parallel tests caused this failure.
Any same-process background forker could demonstrate the mechanism, including
under a single-thread test harness. Capture thread configuration and source/binary
identity for subsequent validation. A recurrence inside the isolated child is a
retained-reader product defect to investigate, not something this helper may
retry or suppress. The original combined suite remains mandatory.
