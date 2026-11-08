# Runner cleanup: fresh signal times and final owned exit observation

Status: independent actual Claude design re-review APPROVE ebdaef60-2eb4-4810-9c0d-f2e6da3852a9; implemented in 8d0dbf23 and independently approved in Claude code review 88378359-21f3-4260-8e07-eea41fffcea3. See [the integrated review](../reviews/streaming-runner-exit-observation.md). The user directly authorized continued testing and fixes. Diagnostic increment is separate preceding commit 651cb98. No source changes, builds, reruns or live tests were performed to produce the design.

## Evidence and limit of the claim

The macOS candidate's installed-feature suite failed `authority_withdrawal_halts_queued_input_irreversibly_without_cancel`: expected `Cancelled`, observed `Failed(Uncertain)`. Its earlier default-suite invocation passed. The failing log contains no failure-site, finite engine error or lifecycle snapshot. Therefore the historical cause remains unknown. In particular, this proposal does not establish that the observed failure was a timeout rather than a Darwin ownership/signal proof or reap failure.

The source nevertheless contains a concrete stale-observation hazard in `crates/marmish-native/src/stream_runner.rs`:

* `Lifecycle::step` captures `now` at line 423 before `try_exit` and before any signal. After successful TERM it stores that old value in `cancel_started` (460); after successful KILL it stores the same old value in `killed` (473).
* `Lifecycle::finish` captures a new clock value at 486. If `exit_seen` remains absent and the recorded KILL age reaches `drain_grace`, lines 487–495 declare uncertainty without another owned exit observation.
* Descheduling or slow identity/signal checks can make the stored signal time precede the completed signal substantially. Separately, the child can exit between `step`'s probe and `finish`'s deadline decision. Thus the branch can report an expired post-KILL observation budget based on an old signal time and an obsolete negative exit observation.

The test limits are TERM grace 40ms, post-KILL drain grace 80ms, poll 2ms. Production defaults are 1s, 2s and 10ms respectively. Neither set changes here. Existing uncertainty is fail-closed; this design does not replace it with unproved cancellation success.

## Exact implementation boundary

Only `stream_runner.rs`, with private lifecycle code and adjacent tests. No engine/platform/launcher API changes, generalized fake process abstraction, wire changes, additional signals, timeout increases or altered ownership rules.

1. After each successful `signal_term()`/`signal_kill()`, record `Instant::now()` rather than the pre-probe `now`. This includes the final group KILL in `finish`. The original pre-probe clock can still drive eligibility checks for already-recorded deadlines. A fresh successful signal completion starts the corresponding existing budget. This moves its start past the elapsed identity-check/signal-call work rather than changing the duration constant; signal-call duration is no longer charged before that signal completes. There is no signal-duration RED test: this part is justified by source inspection, not by the stale-exit regression. The final KILL timestamp in `finish` is retained for consistency, although the successful path reaps and returns immediately.
2. At the existing `finish` failure boundary only—`exit_seen.is_none()` and the post-KILL drain budget expired—call `child.try_exit()` once more before returning uncertainty. Do not add probes to ordinary output processing or add a repeated grace period.
3. If that probe returns false, preserve the current diagnostic and `Failed(Uncertain)` return. If it errors, record the actual finite `EngineError` with lifecycle/window context and return `Failed(Uncertain)`. Do not signal or reap in either failure branch; normal `ChildOwner` destruction retains its current conservative cleanup behavior.
4. If it returns true, record the leader exit once, close the shared stdin gate, and set the existing cfg(test) observed-exit latch; continue through the existing output abandonment, final group-signal and reap path in the same call. The already-expired KILL time is unchanged, so explicit cancellation can abandon unread output immediately under its existing rule. No second TERM/KILL is introduced when `killed` is already set.
5. Share only the narrow successful-observation bookkeeping between `step` and this boundary, if that avoids duplicate stdin/latch code. A private `record_exit_observed(&mut self, shared: &Shared)` is sufficient; it records a fresh observation `Instant` only if not previously observed. This also changes `step` to record exit time after the successful probe rather than from its older pre-probe clock, conservatively starting the same drain budget at actual observation. It does not query, signal, reap or modify cancellation meaning.

## Cancellation and natural exit semantics

At the new boundary, the reachable invariant is `exit_seen=None`, `killed=Some`, `cancel_started=Some`, `cancelled_while_running=true`, and shared cancellation true. Without an observed exit, drain expiration cannot initiate TERM; only a cancellation observed while running can reach this state. The fresh exit observation therefore continues to `Cancelled`, and explicit-cancellation output abandonment applies. It must not reset any cancellation flag.

`cancelled_while_running` remains assigned exclusively by `step` when its owned probe says the child is running and the cancellation latch is set. An exit between that negative probe and the subsequent cancellation load already maps to `Cancelled`; this amendment does not change that policy. The reachable natural-exit case remains covered by `cancellation_after_observed_leader_exit_preserves_natural_status`, which must still preserve the actual exit code. No new synthetic natural-status counterpart is needed.

An already-reaped child remains an error. No signal or second reap follows a failed fresh probe.

Unreaped leader ownership still reserves the process-group identifier until the final required group signal. Darwin identity checks, EPERM all-zombie proof, signal errors, actual reap errors and escaped-descendant behavior remain unchanged.

## Deterministic real-child regression and RED/GREEN proof

Reuse the existing `prepare`, real `PreparedLiveExec::spawn`, `LiveChild`, nonblocking pipes, `Shared`, `Lifecycle`, and bounded direct-child fixture patterns. Use no wall-clock sleeps to induce a race. Tests may deliberately construct old lifecycle timestamps to represent a supervisor descheduled after a previous negative probe. They must label this as a controlled stale-state seam, not actual scheduler reproduction or a proof of the historical Mac cause.

### Stale negative observation with a real exited child

Declare the temporary directory before the real child so it drops last. Spawn the bounded `/bin/sh -c 'read x; exit 7'` through the existing prepared execution path, take its real nonblocking pipes, and prove a real running observation while the parent retains the open stdin write pipe. Release it by closing that pipe. Wait boundedly with real `try_exit` until exited without recording that fact into the lifecycle. No file polling, artificial sleep, or grandchild is needed.

Use `Instant::now().checked_sub(2 * limits.drain_grace).expect("test clock supports expired budget")` for a small explicitly expired timestamp. Construct the reachable lifecycle boundary: `exit_seen=None`, both signal timestamps set to that old value, `cancelled_while_running=true`, and shared cancellation true. The constructed timestamps represent a stale supervisor epoch; the test does not claim that SIGKILL caused this child's natural exit.

Call `finish` once. OLD behavior must yield `Failed(Uncertain)` at the cached-exit deadline branch. NEW behavior must yield `Cancelled` with `uncertain=false`, then assert the exit-observed latch, stdin closed, both output EOFs, and finally exact `RecoveryUncertain` errors from subsequent `try_exit` and `reap_last` calls on the already-reaped child. This ordering proves the terminal and final owned cleanup, not only a field assignment. The same assertions are used for RED and GREEN; the positive test covers the new exit re-probe, not fresh signal timestamp correctness.

### Negative controls at the same boundary

* Still running: use the same real child held on its open stdin pipe. Construct the expired lifecycle boundary deliberately, call `finish`, and require `Failed(Uncertain)`, no exit-observed latch, and no false successful reap. This tests a real false probe; the constructed expired-KILL state is not evidence that a real process survived SIGKILL. After asserting continued ownership, explicitly signal KILL, poll real `try_exit` boundedly, and assert `reap_last` inline. Do not close the stdin pipe first and accidentally turn this control into a natural exit.
* Probe error: let a real child exit and intentionally reap it through its public owned API before passing it to the boundary. Require `Failed(Uncertain)` on both revisions; additionally require the finite `RecoveryUncertain` diagnostic on NEW code, with no successful-exit latch and no attempt to signal/reap again. This is an intentional already-reaped API-error negative, not a forged PID or fake engine. No group signal occurs after the explicit final reap.

No new cleanup guard is introduced. The existing synchronous `ChildOwner::Drop` attempts owned KILL, waits at most two seconds and reaps; declare `TempDir` before `LiveChild` so state outlives that cleanup on assertion panic. The still-running negative additionally asserts explicit owned KILL/exit/reap inline on success. The positive has already reaped its real child; the probe-error control has intentionally done so. No numerical PID/group kill authority, fake finished flag, new cleanup type, or second panic is introduced.

Run both negative controls on OLD and NEW code. Both remain GREEN with their existing terminal assertions. Only NEW code adds the precise `RecoveryUncertain` engine-error attribution in the already-reaped probe-error branch; the OLD diagnostic has no engine error there because it never makes the fresh probe. Label that difference accurately rather than claiming a negative-control RED.

## Validation and review

Before implementation obtain an independent actual Claude design approval for this amendment. Capture the stale-exit test against old product code as a bounded RED result; apply the narrow change and capture GREEN with the exact same assertions. Run both real-child negative controls on OLD and NEW code with the attribution distinction above, then existing cancellation, authority-withdrawal, `cancellation_after_observed_leader_exit_preserves_natural_status`, and final-handle cleanup tests. Print existing finite failure context on relevant assertion failures so another uncertainty is attributable; diagnostics must not expose arbitrary I/O text or process state.

Run strict native all-target installed-feature Clippy, scoped formatting and diff checks. Obtain an actual scoped Claude code review. A fresh hosted macOS run is required to validate the platform result; local GREEN is neither the historical cause nor all-platform qualification. Preserve failed candidate evidence and keep the historical cause explicitly unknown unless new finite diagnostics establish it.

Timestamp correctness is supported directly by assigning the fresh post-success clock in the three signal-success sites and the fresh successful exit-observation clock in `step`. This portion has no RED test. Do not add a generic signal callback solely to simulate time passage. The real-child stale-exit RED/GREEN test demonstrates the independently testable cached-observation defect; it does not prove a signal-duration measurement on Darwin.

Land the separately approved diagnostic-only increment first; its baseline injection must establish missing diagnostic detail against the old assertion shape. Review and land this product amendment separately afterward. Both may feed one fresh candidate, but no release or historical root-cause claim follows from a pass.
