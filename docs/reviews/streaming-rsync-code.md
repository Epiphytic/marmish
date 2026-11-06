# Rsync adapter integrated code review

Scope: native `rsync_cli.rs`, CLI production parsing and routing, the owned
signal listener extension in `stream_cli.rs`, tests, and README usage against
[the approved design](../phases/streaming-rsync.md). Baseline native stream source
is `78641a42393e202e4379457159e14ea5a2209a9a`.

Independent Claude Code `--permission-mode auto` review is in progress. No
approval is inferred from passing tests. Root coordinated validation passed
48 selected native CLI tests in 1.00 seconds, including isolated subprocess
SIGUSR1/EPIPE, no-signal detach, pre-Open interruption, and rsync burst policy.

Strict all-target Clippy initially failed on an unused import and redundant unit
expression in the new parser. Removing both preserved behavior; the rerun passed.
Formatting and whitespace checks passed. The native package was separately built
successfully in 13.07 seconds; actual rsync relay qualification is in progress.

## Round 1

Claude Code `claude-sonnet-5-5`, auto, read-only: **REQUEST_CHANGES**,
338.630 seconds, 20 turns. Inspected actual integrated parsing, routing, signal
and launcher code and captured local rsync 3.2.7 bootstrap argv. No Cargo,
build, native or relay tests were run by reviewer.

Required fixes: suppress the generic "remote execution may continue" message
when no Open was attempted; exercise actual cancellation-confirmation grace,
late signals and fixed deadlines, multi-thread signal/EPIPE priority and early
CLI interruption; ensure an abandoned blocking daemon-startup wait cannot delay
runtime shutdown after interruption. Corrections and affected checks are in
progress; passing prior tests did not close these findings.

## Round 2

Claude Code `claude-sonnet-5-5`, auto, read-only: **REQUEST_CHANGES**,
179.934 seconds, 16 turns. All round-1 corrections accepted by inspection;
latest source was deliberately not compiled during the owned live run.

A remaining blocking race was identified by inspecting Tokio 1.54.1:
`yield_now` may self-wake immediately on the multithread runtime's outer
`block_on` thread, before the signal driver broadcasts SIGUSR1. Replace the
yield-only recheck with a bounded signal wait, retaining both real subprocess
runtime variants. An ordinary EPIPE still detaches without cancellation.
Also suppress the generic may-continue diagnostic for ordinary failures before
Open. These corrections require affected checks and another independent review.

## Round 3 and affected checks

Claude Code `claude-sonnet-5-5`, auto, read-only: **APPROVE**,
46.952 seconds, 13 turns, scoped to the dispatcher fix, diagnostics, runtime
shutdown and actual signal subprocess regressions. Approval was explicitly
conditional on the affected tests passing; the reviewer did not compile or
run native/live checks. The fixed wait is bounded at50ms; it is not an absolute
guarantee under arbitrary scheduler starvation. Plain attachment failures
still detach, with at most the added signal-dispatch wait.

Root's updated native CLI selection passed **51 tests in7.71 seconds**,
including both real signal/EPIPE runtime variants, all late-signal/deadline
cases, pre-Open diagnostics and interruption of a blocking startup. Strict
all-target Clippy passed in4.24 seconds; formatting passed. The focused Python
harness/capture selection passed32 tests in0.193 seconds. An initial unittest
invocation from the repository root used the wrong import path and failed two
module imports; rerunning from `tools/` used the intended test environment.

The final corrected snapshot passed the real rsync relay suite in679.772292
seconds, including confirmed cancellation and partial restart. See the
[separate report](../validation/rsync-runtime-live.md) for exact scope and hashes.
No platform or installer support is inferred.

## Structural checks

Ripwire at `78641a423+dirty` reported169 quality rows,35 gating (exit2):
this is **not a clean quality gate**. The scan includes unlinked Phase3 drafts
that are excluded from this increment. Reviewed Phase2b growth includes the
shared stream dispatcher (73 to118 lines), a real subprocess regression expanded
for preparation and Hello stages (90 to128 lines), and a closed rsync parser
reported at complexity35. These are bounded routing/grammar and regression
cases; independent code review and targeted executable tests cover them.
Reported cross-module clones such as rsync allowlist versus transport error
classifiers and target resolution versus a synchronous test resolver are
structural matches with different contracts. The production resolver delegates
to the same shared target parser/finalizer; it does not duplicate that parser.
Some reported functions/JSON keys are unrelated name matches. No suppression
or new baseline was used to hide findings. Remaining size is recorded rather
than claimed fixed by the passing compiler or review.

The bounded change-context scan reported six tracked changed files and includes
name-based overapproximation of callers; it misses untracked new files until
staged. The actual rsync module, harness and capture tests were independently
read in the Claude reviews. Affected verification used the explicit native CLI
and tooling suites rather than treating the static graph as runtime coverage.
