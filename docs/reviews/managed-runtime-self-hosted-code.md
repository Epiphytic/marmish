# Managed completion and self-hosted proof: integrated code review

Independent Claude Code, actual model `claude-sonnet-5-5`, in
`--permission-mode auto`, reviewed the integrated increment against `8e19cdf`.
Verdict: **APPROVED to publish this infrastructure and proof increment**, no
blockers. This is neither phase 0 acceptance nor production phase 1 approval.
Scope included both approved designs, the harness/completion cases, helper
selection/provenance, CI context, cancellation, workflow and affected tests,
specification and sanitized evidence. The preexisting Beads export is excluded.

The reviewer traced B1–B6 to implementation: required helper fails closed;
Darwin rejects helper configuration; fixed selection refusal categories are
preserved; provenance is bounded, no-follow and digest-bound; explicit CI
context matches the target; only the full-suite supervisor receives the pair;
owned groups/scratch unwind on interruption and deadline. It inspected real
extraction interruption/retry, uv's exact substitution checksum rejection,
hostile ambient restoration and scratch-bound cwd. Workflow labels, admission,
pins, timing, report-only upload, unmasked outcomes and conservative cleanup
matched the design. Mutable guards do not establish runner-group isolation.

Tests actually run by Claude: 68 focused harness/workflow cases including real
cancellation; three existing trust-boundary/workflow cases; 21 CI-helper and
15 transport-sandbox cases both with and without the helper pair; syntax and
whitespace checks. It used a throwaway HOME, performed no network proof, edited
nothing and triggered no workflows. Root additionally passed 62 focused cases,
6 workflow cases, 16 interop cases and 14 native-spike cases (1 reported skip),
and independently parsed workflow YAML with local PyYAML to check admission,
matrix and timing. The committed workflow tests use only stdlib and have no
optional dependency skips.

Useful nonblocking findings are retained for the next bounded diagnostic
increment (Beads `marmish-x6z.1.1`): pass `interrupted`/`total_timeout` through
native/namespace/observer wrappers instead of relabelling them; clarify/test
report-emission signal windows without promising survival of forced teardown;
cover restoration of an existing nonzero timer; map deeply nested provenance
JSON refusal to its fixed category. No claim is made that these were fixed in
this snapshot. The reviewer considered them nonblocking: failures remain closed
and owned-child cleanup still occurs. The diagnostic design/code review must
cover any subsequent fixes.

The reviewer read honest historical Trials13–15 and their source hashes; it did
not repeat the live proof. Final Trial16 and separate actual authenticated-helper
smoke evidence were completed during this review and require the final evidence
check below. No full helper-mode managed suite or three-target native success is
claimed. Native macOS full-suite timeout, hosted Linux namespace failures,
unassigned self-hosted jobs, unverified runner-group policy and the separate
configured macOS SIGABRT remain open. Forced teardown can prevent a report and
same-host overlap can fail the real-home snapshot guard.

Final evidence verdict: **APPROVED for publication**, independently checked by
Claude Code, actual model again `claude-sonnet-5-5`, in auto mode. It verified
all four Trial16 source hashes against unchanged source; exact byte equality of
the actual Trial16/helper reports and repository copies; 478/7/exit-0, no unmet
cases, system mode and enforced offline retention; helper smoke boundaries;
historical failure honesty; native failure limits; parsed workflow admission,
budgets and cleanup; no broken documentation links or private-state inclusion.
It performed read-only checks and did not repeat live execution. Report JSON
has no embedded source hash: the companion records run-time hashes, and this
review checks their current match, not a cryptographic run attestation.

Root's final scope audit passed: exactly 23 intended text files staged, no
Beads export, raw wrapper or unreviewed diagnostic draft; private-key/token
pattern scan and representative secret/database/log/core ignore checks passed.
Ripwire's bounded change map at `8e19cdf87+dirty` found harness coverage and a
broad name-based transitive radius; dynamic unittest discovery/text workflow
contracts are not completely mapped. The actual full 478-test suite and scoped
independent tests supply execution evidence rather than treating static graph
counts as complete. No production installer/application file is changed.
