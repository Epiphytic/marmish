# Native provider qualification diagnostics

## Scope and observed failure

Candidate run `41898839617`, attempt `1`, built product source
`61f47606c5e7ba9048f196165a8025e7c2662839`. All three package jobs, three actual
installer quickstart jobs, and both Linux provider jobs passed. The macOS
provider job `126848598035` failed in its outer account supervisor after 748 ms,
reporting only `operation_failed`. Its report recorded
`runner_keychain_unchanged: true` and `owned_account_removed: false`. This is
not evidence identifying a failed Keychain API or a Marmish runtime defect.

The supervisor's `execute` currently drops subprocess exit status when raising
its finite error category. `mac_account` uses that same category for multiple
administrative operations. Add diagnostics to identify the failed operation,
then rerun this qualification against the existing product assets without a
full native rebuild. This increment changes CI tooling only: no Rust runtime,
provider custody, release asset, or installer behavior changes.

## Diagnostics interface and trust boundary

Keep `str(Refused)` as the existing finite category. Add an optional integer
`exit_code` attribute; checked nonzero subprocess exits set it from the actual
`returncode`. Timeout, input/output bounds and spawn failures must not invent
an exit status. Allocate a fresh exception per failure. Never include subprocess argv, stderr, stdout, passwords,
account names, private paths, key references or exception text in reports.

Define `STAGES` as a closed frozenset of literal stage values. Within
`mac_account`, track the current stage before every operation, including
non-subprocess parsing and checks, and attach it in both `except Refused` and
generic exception branches. A generic exception retains the finite
`owned_account_prerequisite_failed` category. Stage values are:

- `runner_default_snapshot`, `runner_search_snapshot`, `account_uid_list`,
  `account_uid_parse`, `account_identity_lookup`, `account_home_check`,
  `account_create`;
- `account_attribute_uid`, `account_attribute_gid`, `account_attribute_home`,
  `account_attribute_shell`, `account_attribute_realname`,
  `account_attribute_password`, `account_attribute_auth`;
- `account_identity_check`, `account_home_create`, `account_home_chown`,
  `input_stage`, `stage_copy`, `stage_permissions`, `interpreter_lookup`,
  `interpreter_validate`, `interpreter_hash`, `inner_budget`, `inner_launch`,
  `inner_report_read`, `inner_report_parse`, `inner_report_validate`,
  `interpreter_recheck`, `qualification_validate`;
- `cleanup_identity`, `cleanup_bootout`, `cleanup_processes`,
  `cleanup_delete_account`, `cleanup_delete_home`, `cleanup_verify_home`,
  `cleanup_runner_default`, `cleanup_runner_search`, `cleanup_runner_compare`.

Stages identify operations, not argument values. Account attribute stages come
from a fixed literal mapping, not user text. At serialization, retain a stage
only if it is a member of `STAGES`; otherwise replace it with a fixed
`diagnostic_invalid` value also in `STAGES`. Keep an exit code only when
`type(value) is int` and `-128 <= value <= 255`; bool is invalid, and negative
values represent signal termination. Drop invalid or absent status values.
Validate the primary and cleanup fields with this same sanitizer immediately
before report writing, including paths that use generic exceptions.

The first failure remains primary. Cleanup must neither replace its category,
stage or exit code nor imply success. Cleanup diagnostics use exactly
`cleanup_failure`, `cleanup_failure_stage`, and `cleanup_exit_code`, preserving
the first cleanup failure with `setdefault`. If cleanup is the first failure,
retain the existing finite overall cleanup category as well. If `execute`'s
`finally: stop()` itself fails and replaces a subprocess failure, attach the
current stage and omit the unavailable exit code; do not chain exception text
or claim that the original status survived. Existing report consumers
receive the same `failure` categories and successful report semantics; new
diagnostic fields are additive. Do not loosen any success criterion.

## Cleanup and bounded execution

Preserve `execute`'s current owned-process-group cleanup and pipe closure.
Preserve `mac_account`'s non-lifecycle 600-second budget, 60-second account
setup bound, and cleanup command bounds of at most five seconds. Preserve
the separately bounded lifecycle mode and its signal handling.

Only the newly allocated UID is a cleanup signal scope. Keep identity checks,
`launchctl bootout`, owned-process absence checks, and the current account/home
deletion conditions. Do not expand deletion authority or remove unrelated
accounts. A failed qualification does not claim account removal. Always retain
the comparison of runner default/search Keychain snapshots, and keep missing
cleanup or changed runner Keychain evidence a qualification failure. This
diagnostic increment does not redesign recovery for partially created accounts.
In particular, `created` is currently set before the first `dscl -create`;
failure of that first call may make cleanup's lookup fail too. Record its
separate cleanup category honestly without replacing the primary diagnostic.
Stage plus exit status identifies an operation, not a root cause: for example,
`sudo -n` and the administrative utility can both yield the same status.

## Targeted workflow

Add `.github/workflows/v02-provider-diagnostic.yml` with `workflow_dispatch`
only, `contents: read` and `actions: read`, a single `macos-15` job, and a
15-minute job timeout. Use immutable action revisions already employed by the
candidate workflow and checkout without persisted credentials, with full
history (`fetch-depth: 0`). Set concurrency to one diagnostic run with no
cancellation of an active provider process, and retain artifacts seven days.
Do not compile
or publish binaries, run Cargo, mutate a release, or invoke another workflow.

Expose no dispatch inputs. Bind source-defined constants to run `41898839617`,
attempt `1`, source `61f47606c5e7ba9048f196165a8025e7c2662839`; target is
`aarch64-apple-darwin`. No arbitrary source, script, URL or artifact-name input
is accepted. The reviewed supervisor comes from the dispatched repository ref.

Reuse and parameterize `candidateInputs`, `checkRun`, and `validateCandidate`
from `tools/v02-actions/workflow.mjs`; do not implement a parallel resolver in
shell. Add an explicit diagnostic policy bound to the above literals. Their
default production policy must remain unchanged, including successful-run and
source-equals-`GITHUB_SHA` requirements. Only that diagnostic policy permits
this failed upstream run and separate supervisor source, and requires exact
`head_sha == 61f47606c5e7ba9048f196165a8025e7c2662839` as its product-source
binding. A mere 40-hex `head_sha` is insufficient. The downloaded native
manifest's build source must also match that same product source.

Preserve repository and head-repository equality to `Epiphytic/marmish`,
`path == .github/workflows/v02-release.yml`, `event == workflow_dispatch`,
`head_branch == master`, completed status, exact run and attempt checks.
Require upstream conclusion `failure` for this diagnostic. Fetch
attempt-scoped jobs with pagination, bounded response sizes and API budget,
and verify fetched count matches `total_count`. Require these exact job IDs
and full names once each, with no duplicates:

- `126816261687`: `package (ubuntu-24.04-arm, aarch64-unknown-linux-gnu, none, glibc=2.39)`, success;
- `126816262401`: `package (macos-15, aarch64-apple-darwin, adhoc, macos=15.0)`, success;
- `126816262683`: `package (ubuntu-24.04, x86_64-unknown-linux-gnu, none, glibc=2.39)`, success;
- `126848469757`: `render`, success;
- `126848598035`: `provider-qualification (macos-15, aarch64-apple-darwin)`, failure.

The bootstrap render depends on all three packages, so none may be omitted.
Reuse existing exact artifact selection, uniqueness, unexpired and tuple
checks. Download only these exact named artifacts from that run using the
authenticated artifact action:

- `v02-candidate-aarch64-apple-darwin-41898839617-1`
- `v02-pinned-bootstrap-41898839617-1`

Require unexpired artifact metadata and check downloaded content using the
existing pinned manifest/bootstrap and supervisor input verification contracts.
The qualification must receive the downloaded pinned bootstrap and candidate
asset directory, not an installer fetched from a mutable branch.

Require `github.ref == refs/heads/master`, checked-out `HEAD == GITHUB_SHA`,
product source ancestor of HEAD, and HEAD ancestor of `origin/master`. Audit
`git diff --no-renames --name-status -z PRODUCT HEAD`: only A/M entries whose
resulting tree mode is `100644 blob` are allowed. The exact allowlist is:

- `tools/marmish_native_keychain_check.py`
- `tools/test_marmish_native_keychain_check.py`
- `.github/workflows/v02-provider-diagnostic.yml`
- `tools/test_marmish_provider_diagnostic_workflow.py`
- `tools/v02-actions/workflow.mjs`
- `tools/v02-actions/workflow.test.mjs`
- `docs/phases/native-provider-diagnostics.md`
- `docs/reviews/native-provider-diagnostics-design-r1.md`
- `docs/reviews/native-provider-diagnostics-design-r2.md`
- `docs/reviews/native-provider-diagnostics-code-r1.md`
- `docs/reviews/native-provider-diagnostics-code-r2.md`
- `docs/validation/native-provider-diagnostics.md`

Implement the allowlist as those literal paths, not directory globs. Exclude
all `crates/`, Cargo files, scripts, installers, and especially the imported
`tools/marmish_native_quickstart_check.py`. Later unrelated changes must make
this single-purpose diagnostic refuse to run.

Run the isolated supervisor unit contracts before actual qualification. Invoke
`marmish_native_keychain_check.py` exactly as the existing macOS candidate
provider job does, with explicit product source and target. Always upload the
finite report, including failure reports. Keep the supervisor invocation and
report source semantics unchanged: `source_commit` is the product commit,
while `run_id`/`run_attempt` now identify the diagnostic invocation. Write a
separate workflow provenance envelope containing `diagnostic: true`, full
`product_source_commit`, `supervisor_source_commit`, upstream run/attempt,
diagnostic run/attempt, artifact IDs and API artifact SHA256 digests, plus
digests of the downloaded manifest/bootstrap. Initialize this finite envelope
before metadata guards, with a finite guard outcome, so guard refusal is
preserved even when no provider report exists. Upload only explicit envelope
and report paths under an artifact name beginning
`v02-provider-diagnostic-`, which cannot be confused with the production
`v02-provider-<target>-<run>-<attempt>` pattern. Missing optional provider
report before execution must not replace a primary guard failure with an
upload failure. A diagnostic pass qualifies those exact older assets; it
does not qualify future binaries or repair the original run's verdict.
The supervisor and `SOURCE_PATHS` harness hash change in this increment, so
new release assets require their own qualification.

## Tests and acceptance

Add focused supervisor tests for a real short-lived command exiting with a
known nonzero status, unchanged finite error text, absent invented status on
timeouts, and fixed-stage attribution in outer operations. Build an offline
`mac_account` harness patching `execute`, `pwd.getpwnam`,
`staged_provider_inputs`, `digest`, home existence/lstat/symlink operations,
and lifecycle signal APIs. Simulate the full administrative sequence without
OS account creation or Keychain access. Parameterize one failure per stage,
including non-execute parsing and validation stages; assert exact category,
stage, available/absent status, `ok: false` and no account-removal claim.
Use a real `/bin/sh -c 'exit 7'` for numeric exit propagation. Inject sensitive
argv/output/exception sentinels and assert their absence from serialized
reports. Verify that a cleanup failure cannot overwrite an earlier operation's
diagnostics and that changed runner Keychain or missing account cleanup stays
red. Test rejected unknown stages, boolean/out-of-range exit codes, accepted
negative signal statuses, cleanup-stage serialization, and the stop-error
replacement case without a fabricated status. Local tests must not create OS
accounts or touch a developer's Keychain.

Validate workflow syntax and literal/metadata guards, including wrong source,
attempt, workflow, repository/head-repository, branch, event, incomplete job
pagination, duplicate/missing/failed package or render jobs, wrong failed
provider job, and expired or ambiguous artifacts. Add parameterization tests
showing default `workflow.mjs` production policy still rejects failed runs or
source/supervisor mismatch. Add static contracts in the existing denial-
workflow style: dispatch only, exact read permissions, all actions pinned by
40-hex SHA, no inputs interpolated into run blocks, no Cargo/release mutation,
exact artifact tuple, regular-file allowlist and distinct diagnostic evidence.
Review the exact separation of product/supervisor source and tests refusing
an unexpected changed path, rename, symlink or mode. Do not introduce a broad
new workflow framework.

Obtain actual independent Claude design approval before implementing. After
implementation, run the offline Python contracts and workflow checks, obtain
Claude code review, publish the reviewed diagnostic source as authorized, then
dispatch the bounded macOS qualification. Record its actual report and result.
If diagnostics expose another prerequisite failure, report the exact finite
stage/status and design its remedy rather than claiming the provider passed.
