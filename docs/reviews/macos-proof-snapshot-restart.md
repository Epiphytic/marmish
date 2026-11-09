# macOS complete-proof restart: code review

Beads `marmish-4a3.1`. See the
[approved detailed design](../phases/macos-proof-snapshot-restart.md).
The implementation started at `6a4072c` and was rebased onto `87ea64e`;
all four reviewed source-file hashes remained identical after rebase.

Claude Opus 5.5 reviewed the integrated change in Claude Code auto mode,
session `8697f99c-455f-4b94-ab8c-1abeced694f2`. The initial review required
restoring unrelated Rust formatting and correcting two Darwin test expressions
which would trigger strict Clippy. Both were fixed, and useful assertions for
final-only diagnostic metadata were incorporated. Followup verdict:
**APPROVE for code review**, no blocking findings.
Final review record SHA256:
`5f576c4c5fc457839b3806cfc2a65228f5c71ec6f51ca76a708c7694a75b1999`.
Frozen evidence manifest SHA256:
`cfbf83ec311eb84982251c941aa214767fab469bb8ebe113fe1de8d3963f0ce8`.

Completed portable checks: engine142 PASS/zero failures/one existing ignored;
workflow policy9 PASS; repository formatting and diff checks PASS; strict engine
all-targets Clippy PASS in default and release profiles. Quality-delta exits2
with three churn findings and workflow verbosity112→122, disclosed and accepted
without a metric-driven safety refactor. The reviewer independently checked the
control flow, cfg boundaries, lint risks and workflow policy. Portable checks
cannot execute the seven Darwin-only tests or prove native macOS behavior.

Only a first-snapshot nonleader BSD read returning ESRCH permits one new whole
proof attempt. Both profiles use the same release-present classification.
Every successful result still requires the original complete ownership,
held-child, identity, terminal-member, two-snapshot and final-state checks.
The second attempt must succeed completely; there is no third attempt, missing
member acceptance, extra signal, reap, sleep, grace period or deadline change.
Seven deterministic Darwin tests must run in both profiles before the existing
one-hundred-iteration authority check in the manual regression workflow.
The original suites, eighty repetitions and strict Clippy steps are retained.

Run [42630636466/1](https://github.com/Epiphytic/marmish/actions/runs/42630636466)
at `42baf44` passed target iterations1–10 and failed11 at first-members BSD-read
ESRCH. Its requested member role is unknown. The original `d23fce6` group/status
failure remains separate and unexplained. The correction addresses incomplete
observation safely; neither its code review nor portable tests prove the
historical cause or release qualification. Fresh Darwin and full exact-source
qualification remain required. No new release or installer selection is made
by this increment.
