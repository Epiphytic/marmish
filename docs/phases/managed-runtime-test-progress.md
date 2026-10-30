# Managed full-suite timeout progress diagnostics

Diagnostic amendment for Beads `marmish-x6z.1.1`. Initial Claude review was
NOT APPROVED; independent follow-up by actual `claude-sonnet-5-5` explicitly
APPROVED implementation with the seven conditions incorporated below. See
[review dispositions](../reviews/managed-runtime-test-progress-design.md).
Approval covers design, not native acceptance. The harness baseline reviewed
was `fc8756a`; unrelated test-only isolation changes were separately reviewed
and committed as `3b3ef8c`, reconciling that worktree change before implementation.
Harness source was unchanged by that reconciliation.

## Evidence and boundaries

[Hosted run 41138717122](../validation/managed-runtime-native-41138717122.md)
and its [reports](../validation/managed-runtime-native-41138717122.json) show
macOS at `full_tools`, failing `child_timeout` at 900 seconds. Both artifact-bound
native guards passed: exit 1, exact 32-byte marker, no signal. Cleanup and home
checks passed. No slow test or causal connection to configured macOS SIGABRT
was established. The quiet runner gave no progress identifier.

Change only the disposable harness and focused tests after the current frozen
source is published. Preserve discovery, managed interpreter, strict environment,
cwd, 900-second child timeout, total deadline, process cleanup and acceptance.
No test selectors, arbitrary runner inputs, workflow/permission changes, new
platform exceptions or raw CI logs. This remains diagnostic work, not a runner
rewrite. The bounded failure-propagation fixes below are in the same harness.

## Capture contract grounded in the actual runner

`full_tools_acceptance()` currently calls fixed `unittest discover -q`; it
receives `(status, raw)` only on return. `_run_process()` writes combined output
to a private TemporaryFile, enforces a 2 MiB ceiling and deadline, and in finally
kills the owned process group and waits for its leader. Timeout raises before
returning bytes. Existing `run` mocks inspect the second positional argument.

Replace only `-q` with `-v`, retaining all other invocation arguments. Add
keyword-only `failure_capture=None` to `run()` and `_run_process()`. The optional
internal holder has one field, initially **None/unfilled**. It is not a callback,
CLI input, exception field or report field. On a complete successful failure
read set its field to bytes: zero bytes means empty; None means unavailable,
including cwd/Popen refusal and failed/interrupted reads. Assign once at the end
of the read, never incrementally. Successful calls keep `(status, raw)` unchanged.

Use an explicit `completed` flag, set only when normal return data is ready,
to distinguish failure capture; no `sys.exc_info()` inference. In
`_run_process` finally, after existing `killpg` and `child.wait()` and before
the observer-only `require_group_gone`, perform failure capture only when
unwinding a failure and a holder was supplied. Use `os.pread(fd, n, offset)` in
a short-read-aware loop up to 2 MiB or EOF; never seek the shared file offset.
The parent and child share that offset, and a descendant can otherwise write
at the parent's seek position. Keep capture inside a cleanup guard so read
failure or an asynchronous signal cannot skip the existing observer group-gone
check. Catch only OSError/ValueError from reading, leave None, and preserve the
original failure. Do not catch asynchronous Refused/KeyboardInterrupt as benign
capture failure. Cleanup errors retain their existing precedence; capture or
parsing must not mask them. Do not weaken group kill/reap or observer assertions.

In `full_tools_acceptance`, run Linux preflight first, then initialize
`full_tools_tests` immediately before the suite call with progress_capture
`unavailable`, last_started_test None, started_count 0, and unobserved exit,
count and skipped all null. Pre-bind `raw=None` and
use explicit try/finally around that call, passing the holder **as a keyword**.
Parse returned raw bytes on completion, otherwise the holder's bytes; None
means unavailable. Construct the diagnostic update locally and apply it in one
assignment, so interruption mid-parse leaves a valid initialized schema. Guard
only explicitly enumerated parser-local failures, never broad Exception:
Refused/KeyboardInterrupt must propagate. A parser-local fault preserves the
original child refusal and unavailable diagnostics. Drop the holder/reference
after parsing. Propagate the
original failure, never return success from finally. Raw bytes must never reach
report, exception text or `error_frames`. Keep `--failure-log` writing only on
normal nonzero suite completion, never timeout/interruption; CI never passes it.

## Bounded parser and sanitized schema

Use one `re.finditer` over the whole at-most-2 MiB captured buffer. No line-count
or whole-line-length cutoff: over 20,000 noise lines, long test output appended
to a start line, and partial stdout without newline must not hide later starts.
Use this bounded ASCII bytes prefix with `re.M`:

`(?<![A-Za-z0-9_.])(test_[A-Za-z0-9_]{1,127}) \(([A-Za-z0-9_.]{1,255})\)(?= \.\.\.|\r?$)`

All repeating character ranges in this prefix are bounded. Validate each match
in Python: owner starts `test_`, ends `.` plus the method, has no empty/`..`
components, and every dotted component is an ASCII identifier. Reject malformed
or parameterized identifiers. End-of-line acceptance supports unittest's
`str(test)` followed by a docstring line; CRLF is accepted. The negative
lookbehind permits a start following partial non-identifier stdout. Do not
capture suffixes, descriptions, docstrings, subtest values or paths.

Record only the last valid match and a count of all valid starts observed. The
2 MiB input cap inherently bounds that count; clamp to 2,097,152 defensively.
Matching test output can spoof a start, so these are progress hints, not an
authenticated trace, confirmed hang, completed count or causal attribution.
A real start can also be hidden when immediately preceding partial stdout ends
in `[A-Za-z0-9_.]`, which intentionally blocks the lookbehind; the previous
hint then remains stale. On output-limit failure only the first 2 MiB is
captured; the hint can precede
the actual last start. Do not increase output limits to make verbose tests pass.

Add under `full_tools_tests`:

- `progress_format`: fixed `unittest_verbose_prefix_v1`.
- `last_started_test`: null or `{method, class}`, matching existing failed_tests
  shape; `class` is the bounded full dotted owner including method.
- `started_count`: integer 0–2,097,152, only a last-observed-prefix count.
- `progress_capture`: `available` for nonempty captured bytes, `empty` for a
  successful zero-byte read, or `unavailable` for unfilled/failed capture.

Exit/count/skipped remain null unless observed. Select the **last anchored
summary** matching `Ran N tests? in` and the final anchored status line's `skipped=N`,
rather than first `re.search`: verbose output may contain earlier spoof lines.
Bound numeric captures (1–7 digits). Parse an anchored Ran summary even when
no OK/FAILED line exists, preserving current mocks (`Ran 7 tests in 0.1s\n`
and `Ran 1 test in 0.1s` without final newline). Only skip parsing depends on the
final status line; when that line shows no skips, zero is observed. Keep normal
summary syntax and existing success/failure semantics. Summary text is still
spoofable child output, not independent authentication. No raw skip reason,
traceback, command, token, hostname, private path or output excerpt is exported.
Keep the existing bounded failure-identifier list on normal failure. A malformed
progress input produces null/zero hints without changing suite acceptance.

## Preserve interruption and bounded failure categories

Preserve `Refused('interrupted')` and `Refused('total_timeout')` through the
native-probe, namespace-probe, observer and `expect_refusal()` wrappers:
re-raise these two before
their existing category checks/broad observer exception conversion. Other
unexpected observer errors retain `interruption_observer_failed`; neither
interrupt may become an internal-probe failure, `negative_wrong_refusal`, or
generic local failure. `hostile_ambient` may still convert an interrupt during
restoration to `ambient_restore_failed`; explicitly accept this existing
fail-closed restoration result, rather than claiming universal interrupt
identity preservation across all restore operations.
Catch RecursionError specifically from deeply nested helper-provenance JSON in
`helper_provenance()` and map it to `bwrap_provenance_invalid`, retaining its
existing size/digest/type checks. No raw nested data or exception is exported.

Retain the existing handler restoration contract, including a caller's nonzero
prior timer reduced by elapsed time. The current main restores handlers before
home checks and exclusive report writing: a restored caller timer, repeated
signal or forced termination can interrupt that emission window. Report emission
is therefore best effort, not guaranteed. Do not add signal masking, global
handler redesign or a forced-teardown cleanup promise. Test that window and
normal handler restoration honestly; missing/truncated evidence is not a pass.

## Verification and acceptance

Use bounded synthetic children and pure parser fixtures; no live Marmot traffic
or new credentials are required for these checks.

1. Call real `run()` with a short timeout and real holder: two starts, final
   partial start and a blocking child. Require original `child_timeout`, last
   start, owned group cleanup and bounded capture. Include a grandchild which
   outlives the leader and keeps writing; require it is killed and no cleanup
   status is changed by capture. Test short pread returns without seek.
2. Patch run in `full_tools_acceptance` to assert `-v`, `timeout=900`,
   `capture=True`, keyword failure_capture and unchanged strict suite env.
   Add an integration wrapper delegating to real run with a substituted fixed
   synthetic command/short timeout, proving sanitized report wiring without
   waiting 900 seconds. No production CLI selectors are added.
3. Parser fixtures: prefix after partial non-newline output, docstring two-line
   form, CRLF, long suffix, over 20,000 noise lines, invalid/mismatched owners,
   empty input and earlier fake `Ran 999 tests`/skip summary before the real final
   summary. Test full 2 MiB repeated `test_test_...` completes within a generous
   fixed two-second parser-only deadline, guarding quadratic patterns. Include
   space/newline-separated `test_` followed by 127 identifier characters and no
   ` (`; this exercises bounded failed matching rather than just lookbehind.
4. Patch pread to raise OSError: holder stays None, progress is unavailable,
   original child_timeout and cleanup are unchanged. Cover cwd/Popen failure,
   empty successful read, output-limit prefix limitation and capture/parser
   failure during refusal. Test an explicitly parser-local fault during timeout
   and asynchronous interruption during parsing; original refusal/cleanup must
   not be swallowed, and partial schema updates must not escape. Token/path/command sentinels must not appear in
   `json.dumps(report)`; raw capture is never persisted/uploaded by default.
5. Install and restore real SIGTERM/SIGALRM handlers in isolated test processes;
   assert interrupted/total_timeout categories survive each affected wrapper
   including expect_refusal, and hostile restoration retains its documented
   fail-closed ambient_restore_failed behavior;
   owned groups die, no private sentinel is serialized, and handler restoration
   runs. Test KeyboardInterrupt and nonzero prior timer restoration without
   arming an uncontrolled timer in the parent test runner. Exercise a signal
   during capture and the report-emission window without promising a report
   after forced teardown. Deep provenance JSON gets the fixed refusal.

After independent Claude design approval, implement with bounded agent ownership,
run affected tests and obtain integrated Claude auto-mode review. Then rerun all
three native managed targets on the reviewed source. A new timeout with progress
is useful evidence but remains failed acceptance; further diagnosis needs its
own supported change rather than a deadline increase or test skip. Required
Linux namespace tests and all three native passes remain mandatory. The
configured macOS confinement SIGABRT remains independent and unwaived.

## Initial code review refinements

The initial integrated code review by actual `claude-opus-5-5` returned
APPROVED with no blockers and six useful follow-up findings; see
[code review evidence](../reviews/managed-runtime-test-progress-code.md).
Incorporate these bounded refinements before the proof and follow-up review:

- Allow three seconds, or explicit readiness synchronization, for synthetic
  Python children to start before timeout assertions. This changes fixture
  startup tolerance only; production child/total deadlines stay unchanged.
- Combine a real timeout, real failure holder and writing grandchild in one
  regression test, checking the original child_timeout, bounded capture and
  owned-child cleanup together.
- Set `holder.data=None` before deleting the local holder, clearing private bytes
  even when traceback frames retain references to the holder object.
- Assert the entire fixed suite argv and exact expected strict supervisor
  environment, rather than only `-v` and one absent variable.
- Observe skipped only from a final status line **after the last anchored Ran
  summary**. A lone/spurious status without that summary leaves skipped null;
  this tightens diagnostic observation semantics, not suite acceptance.
- Make the unit main/report-window fixture portable by mocking only its
  root/target admission gates. It must exercise the intended reporting path on
  root or unsupported test hosts without altering real harness admission.

These are test/privacy/diagnostic refinements, with no product, privilege, gate,
runner or deadline changes. Initial code approval does not automatically approve
subsequent edits; validation and a follow-up integrated review remain pending.
