# CI report collection review

Beads `marmish-e2s`; base `c1d0c0376ca0662ea10e4b6d02acc777f6303328`.
Scope and acceptance: [phase design](../phases/ci-report-gather.md).

Claude Code `--permission-mode auto`, model `claude-opus-5-5`, session
`c987f827-045a-40b5-a089-fa5351a8ec03`, independently reviewed the design.
Round one approved retries/closed diagnostics and required exact replay identity,
an explicit CI-only source-delta gate, and authenticated original job results.
The revised design incorporates all three; round two returned **APPROVE** before
replay implementation. Ordinary workflow identity checks remain unchanged.
The same reviewer separately approved the CI routing appendix before code:
exclude only Node edits from legacy triggers, add locked Node test coverage and
preserve Python, fixture, mixed-change and manual coverage.

The historical failure category remains unknown. Local authenticated replay of
the original context, twelve reports and pinned native tool passes with 9/9
directed cells; reports match the previously retained bytes. This is recovered
validation evidence, not a successful original hosted workflow.

Integrated Claude auto review, session `26c27dc0-5225-4626-8bf2-ff43533e097e`,
returned **APPROVE**. The reviewer independently passed 249/249 tests both in the
full worktree and a shallow clone matching the automatic CI checkout, parsed all
five workflows, and authenticated original/candidate metadata read-only. No
blocking findings remained. The reviewer confirmed no native inputs changed.

Nonblocking notes: exhausted retries retain deadline/budget rather than the last
HTTP observation; replay guard calls remain fail-fast; pre-render refusals can
leave `guard_pass` provenance alongside a failed job; test-comparison paths beyond
the coordinator are not added to its trigger scope. These do not turn failure
into success. Quality warnings for existing hot-function churn and deliberate
hostile test getters have no behavioral defect; no suppression was added.

At code approval, hosted replay, automatic coordinator CI and a fresh
candidate/streaming run remained pending. The replay alone does not exercise the
ordinary installed workflow's
strict new-source binding; that final hosted check needs a matching candidate.
No full eight-cohort restart is required to validate this CI increment. No release
qualification or v0.3.1 publication is claimed by this code approval.

Hosted follow-up: coordinator run [43172725362](https://github.com/Epiphytic/marmish/actions/runs/43172725362) passed. Legacy installer/packaging run [43172725298](https://github.com/Epiphytic/marmish/actions/runs/43172725298) passed all six jobs across Linux AMD64/ARM64 and macOS. GitHub rejected the first replay workflow before execution because `runner.temp` is unavailable in job-level `env`. The correction exports all six owned paths from `$RUNNER_TEMP` through `$GITHUB_ENV` in the first step. A regression test covers the forbidden context and all path exports. All twelve replay tests and actionlint 1.7.12 across all five changed workflows pass. Claude auto follow-up review in session `26c27dc0-5225-4626-8bf2-ff43533e097e` returned **APPROVE** with no blocking findings. The obsolete candidate run 43172758055 was cancelled before completing builds; it supplies no qualification evidence. Final corrected hosted results are recorded below.

Replay run [43173341776](https://github.com/Epiphytic/marmish/actions/runs/43173341776) reached the complete coordinator suite but exposed an existing child-death assertion race before provenance or report collection. The test observed Linux state R immediately after SIGKILL; parallel coordinator [43173338087](https://github.com/Epiphytic/marmish/actions/runs/43173338087) passed all 249 tests. The test-only correction observes the exact fixture PID for at most one second, accepts only absence/zombie, rejects permanent survivors and unexpected errors, and preserves cleanup and primary failures. Deterministic delayed-death, live-survivor, late-proof and errno checks pass. The final focused suite passes 13/13 and full coordinator suite passes 251/251 with no skips. Native runtime and runOwned are unchanged. The replay allowlist admits only the changed test file and explicitly continues to reject local.mjs changes.

Claude auto code follow-up in session `26c27dc0-5225-4626-8bf2-ff43533e097e` returned **APPROVE**, independently passing the focused 25-test combination five times. The reviewed complete CI-only delta remains eighteen allowed source/documentation paths. Quality delta reports zero regressions; no runtime or release qualification claim is added.

Final hosted validation at `05bec8b` passed: coordinator
[43173876717](https://github.com/Epiphytic/marmish/actions/runs/43173876717)
(251 tests), original-report replay
[43173887015](https://github.com/Epiphytic/marmish/actions/runs/43173887015)
(9/9 pairs), fresh native candidate
[43174030584](https://github.com/Epiphytic/marmish/actions/runs/43174030584)
(all eleven jobs and independent fifteen-artifact inspection), and ordinary
streaming workflow
[43182899512](https://github.com/Epiphytic/marmish/actions/runs/43182899512)
(all five jobs, 9/9 fresh pairs). The normal workflow exercises the strict new-source
binding and the repaired collection path, closing the review's hosted validation
requirement. Product, Cargo and installer source diff from `c1d0c03` is empty.
The [validation record](../validation/ci-report-gather.md) distinguishes original
and fresh evidence, records ignored tests and publishes finite timing samples.
No other qualification cohorts were restarted and no release was published.


Final evidence/documentation follow-up, same Claude auto code-review session,
returned **APPROVE** with no blocking findings. The reviewer independently checked
read-only hosted metadata, original replay hashes, fresh context/rollup hashes,
all nine timing rows, package counts and exact product-source diff. It confirmed
the ordinary-workflow requirement is closed. Useful nonblocking wording fixes
were incorporated: distinguish the rejected zero-job workflow, describe task
completion without preempting Beads closure, clarify the plan amendment, and
link the strict Clippy/signing flags. No additional product tests or live
workflows were needed for this documentation-only follow-up.
