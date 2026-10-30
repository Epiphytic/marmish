# Managed runtime diagnostic increment

Baseline: `dd9abc7`. This increment corrects the macOS packaging expectation
and adds Linux namespace diagnostics; it does not change the production
installer or resolve the native confinement abort.

The unchanged historical native reports are recorded in
[run 41136583828](managed-runtime-native-41136583828.json). All three original
installer/PTY jobs passed; all three managed packaging jobs failed. The old macOS
report omitted the native exit and cannot establish the cause. Both Linux reports
name the same two required namespace tests. No cause or policy workaround is
inferred from those failures.

Current local checks: 35 focused tests pass normally and under an owned symlinked
TMPDIR; `git diff --check` passes. No new full packaging proof was run before this
diagnostic publication. A separate real local execution of the new Linux probe
ladder passed all three commands (exit 0, no signals/denial hints), with ip/sysctl
present. Read-only policy values were AppArmor userns restriction 0,
unprivileged_userns_clone 1 and max_user_namespaces 510960; no values were written.
It used a fresh owned environment and the full suite PATH. This is diagnostic
evidence on this Linux AMD64 host, not native-matrix or full-suite acceptance.
The next native run must establish the actual exit/marker
and namespace-stage results. Historical Trial12 is not evidence for this snapshot.

Quality analysis detected a real complexity increase in installed acceptance;
extracting coherent native-signature, manifest-contract and verdict helpers
removed it. Remaining diagnostics concern unittest class length, discovery-based
false dead-code reports, similar private-output assertions in distinct test
modules, and JSON evidence length. These were inspected; no unrelated cross-module
assertion abstraction or evidence deletion was introduced merely for a score.
The quality tool therefore remains nonzero; this is not recorded as a pass.

The three executable extraction/substitution/hostile-state cases remain explicitly
unmet. Phase 0 and production managed installation remain incomplete. Independent
Claude code review and new native results are recorded separately.

The existing self-hosted workflow run 41136584076 was pending behind obsolete
queued run 40999071717. Cancelling the obsolete run released its concurrency
slot at 22:23:28 UTC on 2026-10-30. All three current jobs remained queued without
a runner assignment for more than five minutes afterward. Required labels match
the referenced configuration; organization runner/group API access returns 403,
so offline availability, supervisor admission and repository eligibility cannot
be distinguished with the current credentials. The user was notified. No runner
administration or host policy was changed.
