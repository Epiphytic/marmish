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
