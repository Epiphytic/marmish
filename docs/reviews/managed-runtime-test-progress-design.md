# Managed test-progress diagnostics: initial design review

Reviewer: independent Claude, actual model **`claude-sonnet-5-5`**, as recorded
in the review wrapper's `modelUsage`. Scope: proposed progress design;
`run`, `_run_process`, `full_tools_acceptance`, main/interruption handlers;
existing tests patching run; and hosted run 41138717122 sanitized evidence.
The reviewer ran no tests, edited nothing and did not review native acceptance.

Initial verdict: **NOT APPROVED**. The reviewer requested a quick re-review
after B1–B3 and required corrections. The
[amended design](../phases/managed-runtime-test-progress.md) incorporates those
items below. Follow-up review by actual **`claude-sonnet-5-5`** explicitly
returned **APPROVED for implementation**, confirming all blockers and required
corrections resolved. The seven implementation conditions below are now
incorporated. This is design approval only, not code or native acceptance.

| Finding | Revised disposition |
| --- | --- |
| B1: line/length cutoffs could hide the actual last verbose start; unbounded regex risk | One bounded-prefix finditer across the entire at-most-2 MiB buffer; no line truncation; exact bounded regex with negative lookbehind and CRLF/end-of-line docstring support; owner validation; 2 MiB adversarial timing test. |
| B2: initially empty holder conflated unavailable and empty | Holder starts None/unfilled; only a complete successful read assigns bytes, with b'' distinct from unavailable. Assignment occurs once at the end, so interrupted reads do not expose partial data. |
| B3: capture could disturb shared child file offset or cleanup/exception behavior | Short-read-aware pread after killpg/wait and before observer-only group assertion; cleanup guard protects that assertion; catch only read OSError/ValueError, preserve asynchronous refusals; raw pre-bound None and try/finally; report object initialized after preflight; drop private holder after parse. |

Required corrections also incorporated: last anchored final-summary match
instead of first match; existing `{method, class}` shape; bounded started_count;
output-limit prefix limitation; local failure log only on normal completed
nonzero suite; real short-timeout run tests separately from patched fixed
900-second invocation and delegated integration wiring; keyword holder argument
preserving existing mocks; noisy/partial/docstring/CRLF/long-suffix fixtures;
short/failed pread; surviving grandchild; real isolated SIGTERM/SIGALRM handler
tests, bounded private sentinel checks and preserved cleanup/failure categories.

Related integrated-code-review nonblockers delegated into this bounded amendment
are explicit: interrupted/total_timeout survive native/namespace/observer
wrappers; a caller's existing nonzero timer restoration is tested; deeply nested
helper-provenance JSON maps RecursionError to its fixed invalid category; the
restored-handler/report-writing signal window is documented and tested as best
effort without promising delivery under forced termination. These are proposed
changes and tests, not claims of completed implementation.

The amendment does not change runner permissions, test discovery, strict
environments, child/total deadlines, acceptance requirements or the independently
unresolved configured macOS confinement SIGABRT. All three native targets and
required Linux namespace tests remain mandatory. The frozen integrated source
was not modified for this documentation task.

## Explicit follow-up approval and conditions

The reviewer inspected the corrected design against harness baseline `fc8756a`,
ran no tests/network/CI and made no edits. It approved implementation with:

1. `expect_refusal` also re-raises interrupted/total_timeout; its wrapper tests
   cover both. Existing hostile_ambient restoration can still fail closed as
   ambient_restore_failed, explicitly documented rather than misreported.
2. An explicit completed flag controls failure-only capture; no sys.exc_info
   inference.
3. No broad Exception parser guard: only named parser-local faults are guarded,
   with original timeout preserved and async refusals/KeyboardInterrupt propagated.
4. Initialized unavailable/None/0 progress schema, null unobserved skipped, and
   one-assignment diagnostic update avoid a partially parsed report.
5. Last anchored Ran summary remains compatible with existing mock summaries
   lacking OK/FAILED or final newline; skips derive only from final status.
6. The lookbehind can leave stale hints when partial stdout ends in an identifier
   character; this is a documented diagnostic limitation.
7. Adversarial timing coverage includes separated maximum-length test prefixes
   missing their delimiter, exercising bounded backtracking.

The review noticed unrelated test-only host-validator isolation edits during
its run. They were independently reviewed and committed as `3b3ef8c`; the
harness was unchanged. That reconciles the worktree note before this amendment's
implementation. No design condition remains undocumented. Implementation,
focused tests, integrated code review and all three native acceptance passes
remain outstanding; no deadline, gate, privilege or raw-output rule is relaxed.
