# Initial Darwin no-status owned waiting

Beads: marmish-d97.3. This changes only live TERM orchestration; the kernel cause
of the recorded no-status observation remains unknown. Hosted macOS execution
and release qualification are pending. No test failure or safety predicate is
waived.

Claude Code auto, claude-opus-5-5, independently approved the design before
implementation: session bdbe37b5-7d7a-4896-91ce-e645f67dca9e, final review SHA
b0a10c06961a55acdedb258a06b75970af9c9961fdb63e91cb2dfd36526f2ffe.
Resolved findings covered attempt-two status loss, the shared post-exit TERM
call site, the production classifier seam and preservation of all three held
status fields. The approved scope admits waiting only through the new live TERM
API; KILL and strict finite/Drop paths still reject this observation.

Completed-code review session cb9f188b-8045-4488-a824-1e86233bbd04 approved the
eight integrated code/workflow/policy files and phase. It audited owner
provenance, both profiles' cfgs by reading, original errors/debug detail,
terminal-loss cases, TERM and reap state, unchanged KILL semantics and exact
workflow guards/counts. It ran the focused policy and formatting checks; no
Darwin compile or FFI execution was claimed. Docs-only master a375f61 was then
integrated with all eight reviewed file hashes unchanged.

Actual local Linux AMD64 checks:

- Engine library: 147 passed, zero failed, one ignored; new portable state/reap
  tests: two passed.
- Affected stream-runner family: 26 passed, zero failed, one ignored, including
  real exit, cancellation, full-credit last-handle drop and authority cases.
- Release new portable engine tests: two passed; native budget tests: two passed.
- Strict engine/native all-target Clippy: debug and release passed.
- Workspace formatting, whitespace and production jq predicates against actual
  pinned-toolchain offline Cargo metadata passed; remove-bin negative cases
  were rejected.
- Focused workflow policy: 10 passed. Full Node policy suite after offline
  dependency installation: 205 passed, zero failed/skipped. The earlier missing
  dependency invocation and reviewer's temporary-copy baseline environment
  failures are setup failures, not substituted passing results.
- A temporary production TERM state mutation rejecting Pending compiled and
  made the exact portable state regression fail (zero passed, one failed).
  Restoring the byte-identical reviewed source made it pass (one/one). This
  proves the orchestration seam only, not the Darwin branch.

Quality-delta findings were reviewed: hot-symbol churn reflects consecutive
reviewed fixes; separate tiny TERM/KILL resolvers preserve different security
contracts; overlap between new TERM and retained KILL zero/failure tests
preserves the mandatory old test family. Test/cross-crate dead-code reports are
index limitations. No suppression or generic retry framework was added.

The diagnostic workflow retains its 45-minute nonroot ARM64/source guards.
It requires the five new engine tests in both profiles, then the fresh full
installed-feature native suite (expected 909/0/3 from two new tests), then the
two exact native debug tests on that cached target, then 100 feature-matched
race repetitions, stopping at the first failure. Native release checks above
are local portable checks; there is no new hosted native-release claim.
A green repetition run shows no observed failure, not proof that Pending was
exercised or that the kernel cause is established. Fresh exact-source ordinary
candidate, unchanged macOS stress and all qualification cohorts remain gates.

Final integrated followup in the same Claude auto session approved local commit
and handoff after rechecking all eight code hashes, docs-only integration and
all 205 Node tests. Review JSON SHA:
3009fcdb9c1b89f1a005ecf7ea3a0545c4c1b757063ac682733201078fd07c59.

## First Darwin compile failure and bounded correction

Diagnostic run [42674014384/1](https://github.com/Epiphytic/marmish/actions/runs/42674014384)
at source 9b12b14 failed before any tests. Rust E0004 identified the existing
debug-only `zombie_proof_restart_context_and_detail_preservation` match, which
omitted `ProofFailure::AwaitingExit`. Raw terminal log SHA:
ce0ad6a1ac1f20fa2ef7f38bef595254526078fcc21637b6ca4b17745a0183f7.
The earlier static audit and independent review missed this helper; they did
not establish Darwin compilation. No candidate or macOS stress was dispatched.

The correction adds only an explicit rejecting panic arm for the impossible
held-status variant in this BSD-read test. Every `ProofFailure` occurrence was
audited across the engine, including constructors, exhaustive matches and
debug/release cfg boundaries. Production classification, proof ordering,
signals, deadlines, strict callers and workflow counts are unchanged. Fresh
corrected-source Darwin compilation and all hosted gates remain pending.

Claude Code auto followup in session cb9f188b-8045-4488-a824-1e86233bbd04
(claude-opus-5-5) approved the three-file correction and independently
acknowledged the earlier missed helper. It audited all `ProofFailure` uses and
the related `CleanupObservation`, `ObservationClass` and `TermCleanup` matches;
no further static exhaustiveness issue was found. Review JSON SHA:
82bd4e661ea9ed5d87672f13203caf5213db852f94071ad8d07a3c43687880bd.

Local Linux AMD64 portable `initial_no_status_` tests passed two/two in debug
and two/two in release. Workspace formatting and whitespace checks passed;
edit-check reported the helper contract unchanged. Quality-delta had zero
gating findings and two minor ambient churn findings in the changed test
area. These checks do not compile the Darwin-only helper. The reviewer noted
that rustc may reveal later issues after this first error is corrected; Darwin
debug/release tests, Clippy, live execution and qualification remain pending.
