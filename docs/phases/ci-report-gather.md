# Reliable installed CI report gathering

Beads `marmish-e2s`. Direct user instruction: fix CI until it works.

Run 42885236657/1 at `c1d0c03` passed all three streaming platform jobs.
Its combined verify-context/gather step failed with an unclassified error;
the original response was not retained, so a transient cause is not yet proven.
Preserve those executions and diagnose using their immutable artifacts.

Change only CI coordination, tests and evidence. Gathering gets a single absolute
120-second deadline and the existing persisted 120-request budget. Retry only
`artifact_retry`, including a validated listed ZIP API 404 explicitly opted in by
gather. Reuse the transport's response classification, digest checks and ZIP
validation; no authentication, metadata, duplicate, digest, archive or blob-404
failure becomes retryable. Retry list failures before downloading and download
failures per record, preserving already written records and using exclusive
writes. Back off at least 30 seconds, honor larger Retry-After, and stop at the
original deadline without granting another attempt or resetting the budget.
Missing records still reach the native validator as absent and fail there.

Workflow CLI failures print a bounded closed diagnostic: allowlisted mode and
error category, optional validated HTTP status and api/blob class. Never print
exception text, stack, URLs, response bodies, environment or tokens. Tests cover
unknown errors and hostile metadata. Separate verification and gathering steps
in the workflows to make the failing stage clear.

Validate with deterministic transient-success and persistent-failure tests,
deadline/budget exhaustion and hard negative cases, plus the full Node suite.
Replay the original exact reports through gathering and native rollup, retaining
their original source/run/attempt identity. A targeted hosted rollup-only workflow
must validate original run provenance, successful platform jobs and exact context
and tooling bindings before executing verified tools. New coordinator success
does not relabel the original failed workflow or claim new platform executions.
No unrelated product qualification or full candidate rebuild is required for a
CI-only change; product inputs must be shown unchanged explicitly.

Obtain independent Claude design review before implementation and integrated
code review afterward. Publish reviewed source to both configured mirrors, then
run targeted hosted validation and repair remaining failures until it passes.

## Fixed original-stream hosted replay (design appendix)

Add one manually dispatched rollup-only workflow; no generic historical-run input.
Bind original run `42885236657/1`, source
`c1d0c0376ca0662ea10e4b6d02acc777f6303328`, cohort `stream-matrix`, and candidate
`42747078282/1`. The current coordinator commit/run remain separate provenance.
Use master-only execution, read-only contents/actions permissions, pinned actions,
a fixed 15-minute cap and distinct non-cancelling concurrency. No product matrix,
setup, build, server/client execution, relay operation or publication is allowed.

Before downloading executables, validate the exact original attempt API metadata:
repository `Epiphytic/marmish`, workflow_dispatch, master, exact source/path
`.github/workflows/streaming-installed.yml`, terminal failure; reject latest-attempt
drift. Require exactly these five completed jobs, matching ID, name and conclusion:

- `130071705822`, `resolve`, success.
- `130071963248`, `matrix (ubuntu-24.04, linux-amd64, x86_64-unknown-linux-gnu)`, success.
- `130071963269`, `matrix (ubuntu-24.04-arm, linux-arm64, aarch64-unknown-linux-gnu)`, success.
- `130071963360`, `matrix (macos-15, macos-arm64, aarch64-apple-darwin)`, success.
- `130073932027`, `rollup`, failure.

Check each job's original source/run/attempt association and reject duplicates or
partial inventories. Independently validate the original candidate exact attempt,
source, release workflow provenance, all eleven successful jobs and required
unexpired candidate artifacts. Do not require the repaired coordinator source to
match original product source; authenticate and record both explicitly instead.

Pin original Context bytes to SHA256
`60a26abf406849dfb6223beeacbd87f50dad05d068eb381ecb51396c52ac21aa`.
Verify its original repository/source/run/attempt/cohort/candidate plus original
immutable-tree harness, tooling-manifest and bootstrap pins. Retain original
Context bytes/timestamps. A separate small replay entrypoint constructs a narrowly
scoped original-binding environment for existing verifyContext/gather only after
these checks; ordinary entrypoint defaults and process.env remain unchanged.
Download original candidate LinuxAMD tools/bootstrap and verify all bytes before
any probe. Gather the original twelve role/kind artifacts using fixed collection
logic, unchanged bounded transport/digest/ZIP checks and exclusive output writes.
Run only the original verified installed-check rollup with matrix-result success,
derived from the three authenticated matrix jobs. Do not refresh expired Context.

Upload a uniquely replay-bound report wildcard and closed sanitized provenance:
original source/run/attempt/candidate/Context hash, consumed artifact IDs/digests,
job conclusions, tooling/bootstrap hashes, actual coordinator source/run/attempt
and replay status. Preserve failure markers/exit status; original workflow remains
failed. Replay PASS demonstrates repaired report collection/rendering over original
executions, not new product tests or full qualification.

Necessary files: new replay workflow, small replay entrypoint and focused tests,
plus a workflow-policy test or narrow existing policy-test section; reuse the
parent-owned gather fix. Test wrong/missing/duplicate jobs, source/attempt drift,
Context/hash/pin substitution, foreign artifacts and pre-execution refusal, then
inert complete twelve-record/native-argv success and whole Node suite. No Rust
changes. Independent Claude design approval precedes code; completed-code review
and exact hosted operational admission precede root-only dispatch.

Replay additionally requires original source to be an ancestor of the current
master coordinator. Check the complete git name/status diff and current modes:
only A/M regular 100644 entries from this exact allowlist are admitted:
`tools/v02-actions/workflow.mjs`, `workflow.test.mjs`, `transport.mjs`,
`transport.test.mjs`, `supervisor-diagnostic.mjs`, `supervisor-diagnostic.test.mjs`,
`workflow-diagnostic.mjs`, `workflow-diagnostic.test.mjs`, `report-replay.mjs`,
`report-replay.test.mjs` (all under `tools/v02-actions/`),
`.github/workflows/streaming-installed.yml`, `.github/workflows/v02-installed.yml`,
`.github/workflows/installed-report-replay.yml`, `docs/phases/ci-report-gather.md`,
`docs/reviews/ci-report-gather.md`, `docs/validation/ci-report-gather.md`,
`docs/agent-spec.md`. Any other delta refuses replay, including product, Cargo,
installer scripts, dependencies, candidate/release workflows or removals.

Persistent transient failures terminate with `artifact_deadline` or the shared
`artifact_budget`, never a missing-record success. Replace the existing gather
hard-404 regression with retry-success and exhaustion tests; retain transport
default-hard and blob-hard coverage. The original cause remains unknown even
if the improved collector passes. A later hard-category failure must be fixed
on its own evidence rather than relabeled as a covered transient.

## Coordinator CI routing

Actual old-reference failures 42746956140 and 43167099502 were triggered only by
Node coordinator edits: legacy `tools/**` indiscriminately starts the full Python
reference suite. Preserve its Python/fixture coverage, and add ordered negative
`!tools/v02-actions/**` after `tools/**` in both push and PR paths. Keep manual
legacy dispatch available. Add `.github/workflows/installed-coordinator.yml` for
push(master)/PR changes to `tools/v02-actions/**` or that workflow itself, using
pinned checkout and the runner's default Node, `npm ci --ignore-scripts` and `npm test`.
No native builds or packages. Include static trigger regression tests in
`tools/v02-actions/coordinator-policy.test.mjs`: Node-only, Python-only, mixed,
docs-only, each workflow's own edits and push/PR equivalence. Extend the replay
CI-only delta allowlist with exactly those three files (including legacy YAML).
This routes Node edits to their actual tests; it does not mark the old failing
reference tests passed or change their assertions. Obtain Claude approval for
this bounded routing addition before implementation.

## Asynchronous child-death assertion

Hosted replay 43173341776 exposed an existing coordinator fixture race: the test inspected `/proc` immediately after SIGKILL and observed state R before asynchronous signal delivery completed. The parallel full coordinator suite passed. Keep `runOwned` and native product code unchanged. Add a test-local helper with a fixed one-second observation deadline accepting only Linux absence/zombie or macOS ESRCH. Unexpected errors and a live survivor at the deadline must fail. Cover delayed death and permanent survival deterministically; retain both must-reject fixture cases. Clean only exact owned fixture PIDs whose death has not been confirmed, and preserve the original assertion if PID-file cleanup is unavailable. Extend the replay delta allowlist with only `tools/v02-actions/local.test.mjs`. Claude design review session `c987f827-045a-40b5-a089-fa5351a8ec03` approved this amendment before implementation, with these conditions.
