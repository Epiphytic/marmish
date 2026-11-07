# Worker isolation and cleanup evidence reviews

Scope: corrective design and integrated increment following failed candidate
`42400376357/1`, source `53f94f99d06942a58f52fd4a76048a26ced1a99d`.
The [design](../phases/macos-candidate-worker-and-cleanup.md) and
[actual failure evidence](../validation/streaming-candidate-53f94f9-failure.md)
define the bounds. This record does not qualify a release.

## Actual design review

Claude Code, `--permission-mode auto --model claude-sonnet-5-5`, actual session
`5dbdcf45-15d6-4b29-8b48-0af077021675`:

| Round | Verdict | Duration | Turns | Private JSON SHA-256 |
|---|---|---:|---:|---|
| 1 | REQUEST_CHANGES | 270.381s | 33 | `d93b6e197d0ef9fd2371ff09301602f727560f1e28385a6b475a945c69884253` |
| 2 | APPROVE | 36.293s | 3 | `ce83a5bcccc6abded722adb3c3d6deb245ab627ea2b98499aef4739bf21d2206` |

The first review identified insufficient failure context, detached test cleanup
ownership, fail-fast loss of race evidence and missing second legacy bundle
consumer. The revised approved design records operation-site/finite-error and
lifecycle/window context, requires a bounded RAII cleanup guard with retained
state on cleanup failure, runs focused tests despite an unrelated suite
failure, captures worker hashes, and updates both legacy consumers.

Useful final nits incorporated: exact workflow/source GITHUB_SHA equality;
normal-Cargo staging assertion separate from deliberate negative fixtures;
missing worker hashes fail; exact full-path test filters reject zero tests;
guard moves TempDir and never retains Control ownership during final-drop.

Reviewer inspected actual source and failure log, checked log size/hash and
counts, and tested Cargo auto-discovery on a synthetic offline crate with
Cargo1.93.1. This was not pinned Cargo1.97.1 or macOS execution. Round2 ran no
builds/tests. The supplied log lacked run/job IDs; root separately retained
actual GitHub run/job API records binding those IDs and source in the failure
JSON. Winning historical worker bytes and cleanup root cause remain unknown.

Design approval was recorded in Beads before bounded agent implementation.
Integrated code approval and hosted validation remain separate requirements;
their actual results will be recorded when available.

## Initial integrated code review

Claude Code AUTO, `claude-sonnet-5-5`, actual session
`17daba31-37b8-4b31-aa3a-4f679cc87085`, round1 REQUEST_CHANGES,
238.661s/29 turns. Private JSON SHA-256:
`7307d8372f7e74d218d962f704ab73d5582567473e4a1bd5db4535cf411437cd`.

The reviewer inspected the complete current diff and new files, checked actual
Mac log/API evidence hashes, ran the two new Python regression tests and all six
Node workflow policy tests, parsed YAML and checked whitespace. Cargo was
explicitly excluded while an agent owned the single build lane. This is not
final approval and does not approve subsequent test changes.

Blocking finding: the new engine staging helper is unused in the private module
included by fault-injection fixtures, potentially breaking strict feature-enabled
Clippy. A narrow `allow(dead_code)` with the fixture reason was added; actual
feature-enabled Clippy remains required. Optional observations concerned metadata
failure suppressing focused tests, additional spawn labels, guard timeout during
unwinding and unmeasured hosted job headroom. No production cleanup behavior
defect was found. Test-only setup/caller simplifications still require final
review.

The local Linux combined suite subsequently failed a different test:
closed-stdin delivery expected0, observed13. That result blocks qualification
and requires independent diagnosis before a fix; this initial review did not
inspect it and cannot approve a later correction.

Optional-finding dispositions: metadata/hash checks remain part of the compile
gate because focused results from ambiguous or unidentified workers would be
unusable evidence. Their failure must stop that execution rather than silently
relax worker identity checks. The fixed spawn/panic callsites meet the approved
diagnostic contract; no additional product logging was added. The guard can
wait again during unwinding after an assertion deadline, so its worst-case
test cleanup is approximately16s; this does not extend a product deadline.
Hosted45-minute job headroom remains unmeasured and will be reported from the
actual run. None of these observations substitutes for resolving a failed test.

## Closed-stdin corrective design review

Actual Claude Code AUTO resumed the same session
`17daba31-37b8-4b31-aa3a-4f679cc87085` before implementation of SectionD:

| Round | Verdict | Duration | Turns | Private JSON SHA-256 |
|---|---|---:|---:|---|
| 1 | REQUEST_CHANGES | 97.115s | 7 | `09f3cd94b408f2627c2659ec328cc6cc6305726b950e9aad9c623042846c35e1` |
| 2 | APPROVE (design only) | 13.452s | 2 | `d7b9bcc26809bc19d578df3ae4ea6e2cf2a9b20e2af0a9da2f2520218d25c906` |

Review found that the initial30s supervisor could interrupt the child's own
bounded cleanup, and that the original inherited thread setting was unknown.
The approved correction derives60s from32s of child waits plus margin, bounds
post-kill reaping to2s, adds parent RAII retention/uncertain handling, caps
captured diagnostics, requires exact marker and named successful-test summary,
and records source/thread/binary facts without claiming an observed concurrent
fork caused the historical failure. An isolated recurrence remains a product
investigation blocker. The same-process fork-storm negative control was optional
and was not performed or claimed. Reviewer verified evidence hashes and original
assertions; no Cargo or source edits were run. The strict feature Clippy results
were supplied by agents, not independently reproduced by this design review.

Approval was recorded in Beads before delegating implementation. Final source
review and the unchanged combined-suite rerun remain mandatory.

## Final-source integrated code review

Same actual Claude Code AUTO session, integrated round2 APPROVE for code
correctness only,142.306s/7turns. Private JSON SHA-256:
`5a23e2830bef175d8f2c9530d45faa460b9c499c99e90e759b1057a3feda378e`.
Reviewed runner SHA-256:
`f8edaf4315811ec3907c7192fe9823a2a0acf513e20826e5f61a7c7c9dd1b5ea`.

Reviewer inspected all integrated changes, original unchanged assertions,
supervision/retention and exact-test proof, private cfg(test) diagnostics,
unique worker/legacy consumers, workflow and quality dispositions. It read the
before-run source/binary attestation and corroborated focused/Clippy results
from log tails. It ran no new builds, tests or edits during this turn because
the agent retained the build lane. Earlier independent Python/Node/YAML checks
remain distinct from this source review.

There are no blocking code findings. One low-priority limitation is retained:
the pathological60s selfexec timeout prints a fixed failure and cleanup-uncertain
labels but does not print the bounded capture tail used for ordinary child-result
failure. Private state is retained without printing its path. The reviewer
explicitly accepted recording this known limitation and batching the diagnostic
improvement with later test-source changes rather than invalidating the current
frozen run. This does not suppress a failed test or prove physical cleanup.

The review does not claim the pending combined suite, macOS, hosted headroom or
release qualification passed. Updated after-run identity/result evidence and an
actual final result review are required before source publication.

## Tracking availability

After design approval and delegation had been successfully recorded, Beads1.3.4
began refusing additional updates: the shared remote-backed database reports
schema53 with13 migrations to66 pending. No migration, reinitialization, manual
SQL or schema-check bypass was attempted. The last successful Dolt tracking
push completed. Subsequent validation evidence remains here pending compatible
tracking access; the task is not closed or represented as fully synchronized.

## Final test-result review

Actual Claude Code AUTO resumed session
`17daba31-37b8-4b31-aa3a-4f679cc87085`, round3 APPROVE,
27.199s/5turns. Private result JSON SHA-256:
`b48f867379018b24c8da19a9991adc18608399243efa9fb050b7735198f057b2`.

The reviewer verified the actual combined log/hash, top-level counts and
693.03s duration; source maps and native-image identity match in both actual
attestations. It checked the unchanged approved runner hash and absence of
unstaged code changes. Its14-file diff count covers code/tools/workflow scope;
the complete increment also includes eight documentation/evidence files.
It independently reran six Node policy tests and the two new Python worker
mapping tests, and checked the full whitespace diff. No Cargo rebuild or source
edit was necessary. There are no blocking findings.

Approval covers publishing this corrective source increment and proceeding to
the exact-source macOS diagnostic workflow. It does not qualify a release or
approve a version, tag or installer pin change. All known limits above remain:
Linux-only execution, unproven historical causal interleavings, diagnostic
cleanup instrumentation, accepted low timeout-capture limitation, non-clean
quality tool gates, preexisting broader Python marker failure, unavailable
Beads tracking and unmeasured hosted headroom. The Mac focused loop repeats the
four cleanup cases; its combined suite separately includes closed-stdin.
