# Native CI optimization reviews

Tracking: Beads `marmish-poz`. Scope: the detailed
[CI optimization design](../phases/native-ci-optimization.md), workflow split,
dependency caching and source-bound qualification inventory. No runtime or
release-publication change is authorized by this increment.

## Design review

Reviewer: actual Claude Code 2.1.301, model `claude-opus-5-5`, using
`--permission-mode auto`. Session `178d1a5d-d8d1-4e6a-8d07-b8042e2b0840`.
The reviewer inspected the repository contracts and ran 59 focused Node tests
and seven existing Python policy tests. No source edits or hosted dispatches
were performed by the reviewer.

Round 1 verdict: **BLOCK**. Findings required a unique cache namespace and pinned
existing cache actions, explicit macOS environment setup before cache restore,
required inventory API and literal job names, a bounded regular-file parser,
Python policy/mutation coverage and default target paths on separate VMs.
The caveated historical route must be described as statically preserved, since
current version/source restrictions prevent exercising it as a new candidate.

The design incorporated all six findings. It explicitly records the residual
trusted-master cache-writer risk: restored dependency rlibs are not independent
source-derived attestations; SHA pins and namespace separation do not isolate
from arbitrary trusted master job code. Existing authorized master workflows
already execute dependency build scripts and can alter source/output. No new
human acceptance was fabricated or requested for this existing trust boundary.

Round 2 verdict: **APPROVE**. The reviewer found all six findings resolved and
accepted the documented trust rationale within the existing master boundary.
Approval preceded implementation. Non-blocking reminders were incorporated:
SHA-pinned toolchain actions need explicit `toolchain: 1.97.1`, and the default
cache job-id key remains enabled. Keep exact cold/warm source stamping evidence;
cache hits never replace qualification checks.

## Integrated code review

Reviewer: actual Claude Code 2.1.301, `claude-opus-5-5`, auto mode,
session `8051cfc4-8328-4132-bca1-cb9515d6a5a5`. Verdict **APPROVE**, with no
blocking defects. The reviewer read every changed/new file and packaging
contracts; independently ran 255 Node tests, 13 Python policy tests, actionlint,
helper syntax/status checks and additional in-memory workflow mutations.

Useful findings were incorporated: whole command-line workload assertions,
rejection of job/step `continue-on-error`, identical immutable-source checks,
additional adversarial mutations, explicit documentation of the eligibility
policy gate, a cache-baseline bump reminder, readable design prose and an
invalid timing-stage diagnostic. Legacy whole-target cache competition is
recorded in the validation report. Follow-up verdict **APPROVE** in the same session: all seven fixes were
confirmed, with 255 Node tests, 13 Python tests and actionlint passing again.
Final small policy hardening also rejects first-key and downstream
`continue-on-error` and requires the exact active ancestry command. Final
narrow review also returned **APPROVE**, with all 13 policy tests and five
additional adversarial probes passing.

Ripwire quality delta exited 2: added test-class coverage increases line counts,
reflective unittest methods appear as dead code, modified contracts incur churn,
and a YAML `jobs` collision points to an unchanged workflow. Claude confirmed
these are not substantive regressions; no metrics were suppressed or gamed.
The real test assertion/duplication concerns were addressed as described above.

Hosted cold/warm measurements and final qualification remain pending. Local
code approval is not hosted qualification or authorization to release.

## Planned cache fallback implementation

Initial hosted evidence exceeded the approved 7 GB threshold. The existing
design's fallback is now implemented: product dependency targets remain cached;
installed tooling caches downloads only. A v2 prefix and removal of only this
task's retired v1 entries ensure a genuinely cold new pair without competing
obsolete storage. Claude reviewed the fallback against `c57f8a9` and returned
**APPROVE** in the same auto-mode session, with 13 policy tests, actionlint and
whitespace checks passing. Replacement hosted evidence remains pending.
