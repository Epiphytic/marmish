# Self-hosted managed-runtime proof: design review

Reviewer: independent Claude, actual model **`claude-sonnet-5-5`**, recorded in
the review wrapper's `modelUsage`. Reviewed scope: self-hosted design, existing
native workflow, authenticated Bubblewrap bootstrap, transport helper selection,
working-tree managed harness interfaces, and hosted failure report. The reviewer
performed read-only inspection; no edits, network proofs or native tests ran.

Initial verdict: **NOT APPROVED**, with six blocking findings. The reviewer
said approval would follow once B1–B6 were included. The
[revised design](../phases/managed-runtime-self-hosted.md) now includes the
corrections below. An independent follow-up review, again actual model
**`claude-sonnet-5-5`**, explicitly returned **APPROVED for implementation** and
confirmed B1–B6 incorporated with no remaining blockers. This is design approval
only; neither review ran the harness or established that a runner works.

| Finding | Incorporated disposition |
| --- | --- |
| B1: absent helper silently selected system bwrap in the helper job | Linux-only `--require-bwrap-helper`; empty pair gives `bwrap_helper_required`; hosted default remains available; Darwin rejects helper configuration; explicit Linux `bwrap_source`. |
| B2: selection errors became generic failures or namespace-unavailable | Harness `bwrap_executable()` translates only fixed SandboxError categories to `bwrap_selection:*`; initial selection validation uses the same translation; offline does not swallow it and preflight records refusal while preserving the report. Hosted system-path validation/category tightening is explicit. |
| B3: CI facts lacked an input contract and provenance did not bind selected bytes | Strict four-field `--ci-context`, no GITHUB environment reads; fixed workflow/runner-class facts; bounded regular provenance input, required executable digest equality, fixed keys/types/regexes and fixed invalid-provenance refusal. |
| B4: 30-minute budget and interruption cleanup were unrealistic | Follow-up conditions incorporated: 60-minute job; Python/Bubblewrap/probe/Rust/proof limits of 5/6/1/10/25 minutes; checkout/cache/upload/cleanup limits 2/3/2/3; harness total deadline approximately 22 minutes; SIGTERM refusal and KeyboardInterrupt interrupted report; tests kill/reap live groups and emit report; no CI failure-log upload. Forced teardown remains absent evidence. |
| B5: mutable workflow guards were described too strongly | Literal canonical-repository and ref guards; native/live explicitly push-only; runner-group approved workflow/ref restriction is an unverified operator precondition, not inferred from inaccessible policy API or labels. |
| B6: marker absence claim contradicted existing suite transport | Omit by default; any demonstrated ownership-tracking exception must be validated and recorded; absence assertions cover harness-owned uv/installed/offline/probe environments, preserving existing transport payload forwarding and tests. |

The minimal sequence is now explicit: checkout, bounded Python bootstrap,
bounded Linux helper bootstrap/probe, pinned cache, bounded Rust bootstrap,
proof; then always-run single-report upload with run/attempt/platform name,
missing-file warning and no continue-on-error; last, ownership-checked helper
cleanup using the proof outcome and its own state. Report directories are fresh
because the report is exclusive-created. Workflow tests belong beside the
existing native workflow contract tests. No new standalone workflow is proposed.

Useful nonblocking points were incorporated: explicit missing-tool blockers;
possible private-helper AppArmor denial without policy bypass; actual direct
Rust discovery testing; operator responsibility for retained random helper
paths without timeout reclamation; safe failure on concurrent uv home changes;
pending dispatch replacement/queued jobs as absent evidence; docs-only dispatch;
benign cache-save races with provenance verification; integration after or with
the completion changes to the same three harness functions.

Unverified conditions remain runner availability, runner-group policy, helper
execution capability, actual timings and the complete three-platform managed
acceptance results. The separately unresolved configured macOS confinement
SIGABRT is not fixed or waived by this packaging design. Implementation, native
runs and integrated Claude auto-mode code review remain outstanding.

## Follow-up approval conditions

The approved follow-up required three implementation details, all now recorded
in the design: explicit 2/3/2/3-minute checkout/cache/upload/cleanup limits with
a 60-minute job and approximately 22-minute harness deadline below the
25-minute proof step; same-host native/proof overlap through multiple runner
instances may cause fail-closed `real_uv_home_changed` contamination rather than
a proven runtime defect; a strict-environment test must prove the helper pair
reaches only `test_supervisor`, never uv, installed-launcher, offline or probe
children. No additional design round was requested for those conditions.

The follow-up reviewed the revised documents and relied-on helper/environment
contracts, not the full frozen completion implementation. It executed no tests
and left runner availability/policy, helper execution, timings, AppArmor
behavior, all three native passes and integrated code review outstanding.
