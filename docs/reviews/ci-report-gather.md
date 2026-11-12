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

Hosted replay, automatic coordinator CI and a fresh candidate/streaming run are
pending. The replay alone does not exercise the ordinary installed workflow's
strict new-source binding; that final hosted check needs a matching candidate.
No full eight-cohort restart is required to validate this CI increment. No release
qualification or v0.3.1 publication is claimed by this code approval.
