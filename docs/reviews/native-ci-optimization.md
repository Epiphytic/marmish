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
whitespace checks passing. The replacement cold and initial warm candidates
subsequently passed all 14 jobs and complete 15-artifact/10-report inspection.

## Hosted image rollover evidence review

The same actual Claude auto-mode session reviewed the validation-only amendment
after the initial warm run correctly missed one ARM tools cache. Verdict
**APPROVE**, requiring one wording correction before dispatch: the cold run
itself used mixed ARM images, so there was never one common ARM image to pin.
That correction was incorporated before repeating all jobs on unchanged warm
source. The reviewer independently checked private image lines, cache keys,
retained IDs/sizes and timing observations, without executing artifacts or
accessing credentials.

Useful recommendations were incorporated: at most two further all-job repeats
before independent reassessment, stop/report if variants exceed 7 GB, preserve
entries rather than manufacturing a fit through deletion, bind each restored
key to its actual cold saving run/attempt/job/cache ID, and avoid describing
the small 40-second initial warm improvement as a controlled timing result.
Those original simultaneous-hit criteria are superseded by the reassessment
below; fresh corrected-source qualification remains pending.


## Bounded experiment reassessment and correction design

Actual Claude Code in the same auto-mode Opus session independently reassessed
the two allowed repeats and final nine-cache inventory. Verdict: **APPROVE
conditional on removing tooling caches**; keeping the current code and raising
the allowance was blocked. Download-only tooling showed no measurable whole-job
speed benefit. Product cache keys remain strict; actual independent product-lane
hits can establish reuse without repeatedly chasing simultaneous hits during
uncontrollable image rollover. New product image variants were saved but had
not yet been restored. Failed 7 GB peaks must remain visible.

The concrete design amendment then received **APPROVE before coding**. It adds
explicit tools-only `CARGO_INCREMENTAL: '0'`, removes both cache and hit-reporting
steps, updates misleading size-summary wording and adds missing/nonzero/env and
cache-reintroduction policy mutations. The reviewer confirmed the standing
direct-user implementation authorization applies; its earlier generic request
for fresh human approval was not a gate. Required follow-through is integrated
code review, local checks, one fresh candidate with 14 jobs/15 artifacts/10
reports, product-key provenance and installed nine-pair proof on that new source.

Storage limitations remain explicit: five observed product variants use 6.35 GB;
modeled two-image product retention is 7.54 GB, and overlap with four orphaned
tooling entries is about 8.76 GB. Neither 7 GB nor immunity from eviction is
guaranteed. No caches are deleted and no allowance is raised under this amendment.


## Integrated product-only correction review

Actual Claude Code 2.1.301, claude-opus-5-5, in the same auto-mode session
reviewed the integrated workflow, policy mutations and evidence. Verdict:
**APPROVE**. Local checks passed: 14 workflow policy tests, 255 Node tests
and actionlint for all three affected workflows. The reviewer assessed the
Ripwire churn/class-size findings and reflective-test reachability flag as
nonblocking for this focused policy change; no suppressions were added.

Useful low-severity findings were incorporated: explicitly mark historical
six-hit criteria superseded, reject incremental overrides in global/product
environments, and document the caveated-mode setting separately. The ordinary
tools lane preserves its prior incremental=0 setting; caveated mode now also
sets it explicitly, with static policy coverage only. Fresh hosted qualification
will exercise the ordinary path, not establish caveated-path runtime coverage.

The narrow follow-up independently received **APPROVE, no blockers** in the
same actual Claude auto-mode session. The reviewer reran all 14 policy tests
and actionlint and verified both environment mutations target the intended
YAML blocks. The next gates are the one fresh candidate and exact-source
installed proof; no hosted success is claimed for this correction yet.

A separate agent's read-only audit of committed source
`9131926f85ce2cacc04805027871bdfe92dafc61` independently compared whole YAML
sections with its predecessor. Product caching, eligibility, rendering, all
installer/provider jobs and the candidate inspector are byte-identical.
Installed-tools workloads, source/harness/artifact checks, matrix limits and
14-job/15-artifact/10-report contracts are unchanged. Only caching, its explicit
compiler setting and diagnostic wording changed. The publisher was separately
confirmed `disabled_manually`; no runtime, Cargo, version or public installer
file changed. No live tests were duplicated by this audit.

The final documentation/evidence quality scan flags large JSON arrays as
verbosity and associates the new `cells`/`samples` keys with two unchanged
historical JSON paths. The final diff changes only Markdown and adds aggregate
JSON; neither historical file was edited. The new arrays preserve measurements
and provenance, rather than executable functions. This data-only finding is
recorded for independent assessment; no quality baseline or suppression changed.


## Final evidence review

Actual Claude Code 2.1.301, claude-opus-5-5, using `--permission-mode auto`
in the same independent review session returned **APPROVE** for the final
documentation and aggregate JSON. It independently checked all six samples,
unchanged first-five observations, exact cache/save/restore bindings, inventory,
three disabled tooling lanes, installed source/candidate/digest bindings and
the limitations on timing, throughput, storage and release scope.

The reviewer independently reproduced the private parser correction on raw
completed logs and synthetic positive/negative cases: 32 AMD product tuples
match ARM and all 44 Linux tuples match baseline exactly. Unknown fragment
status/duration stays null; arbitrary suffixes, malformed counts and suffixes
without the observed interleaving prefix are rejected. No artifact was executed,
credential accessed or live test duplicated. JSON-array quality warnings were
assessed as measurement data, not substantive code regressions.

Useful low findings were incorporated: make phase wording explicitly say
GitHub/Radicle refs verified and Iris readback unverified, and retain a runnable
private eight-case parser regression script beside the measurement helper.
CI qualification passed; Iris readback is externally blocked and remains
tracked separately as `marmish-j3r`. The parent task stays open until this
publication check is resolved. No further native rerun is required for the
final documentation/aggregate-JSON commit.
