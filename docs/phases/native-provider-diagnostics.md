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


## Observed macOS setup correction (41915532600/1)

The approved diagnostic ran at supervisor 5ccc646c3b5a85df73aef79dd14286c6b6d78f0c
against unchanged product 61f47606c5e7ba9048f196165a8025e7c2662839. All provenance
and offline gates passed. Actual provider report: failure_stage=account_home_chown,
failure=operation_failed, exit_code=1, elapsed_ms=530; runner_keychain_unchanged=true.
No product launch occurred. The chown operand is currently uid:-2. Apple chown
parses numeric IDs as unsigned and rejects this negative group representation.

The proposed correction changes only that operand to the allocated numeric UID.
The newly created home remains mode 0700 and its inherited group is retained;
only its owned UID needs access. Account allocation, primary group, disabled
login, path validation, isolation, cleanup scope, provider commands and product
bytes remain unchanged. Existing account/home identity checks compare UID and
home; no group ownership requirement is being weakened. No fallback, sudo
relaxation or runner Keychain change is introduced.

Acceptance: add an offline full mocked-harness argv assertion that chown receives
only the allocated UID and owned home, preserving mode 0700 creation and all
failure/cleanup tests. Re-run supervisor 30+, Node 58, static workflow 3 and lint.
Obtain independent Claude design and code reviews before committing; use only
the existing twelve allowlisted paths, keeping version 0.2.2. Reuse the same
product/bootstrap artifacts in a new diagnostic dispatch, bound to its new
supervisor source. Require actual qualification, not merely successful chown.
Any later stage failure is a new observation, never hidden by weakening checks.


The owner-only correction received actual independent Claude design approval
(`/tmp/marmish-scj-provider-chown-design.json`): no blocking findings. The
implemented operand is `str(uid)`, without a group separator. The regression
asserts the complete chown argv, exact `mkdir -m 700` before chown, and unchanged
account `PrimaryGroupID=-2`. Offline supervisor discovery and direct execution
both pass 31 tests; Node tooling passes 58, static workflow contracts pass 3,
and actionlint 1.7.7 and whitespace checks pass. This is harness validation;
actual hosted provider qualification remains required after code review.

## Finite child diagnostics after 41916591226/1

The second hosted diagnostic passed the owner-only chown stage and reached the
unprivileged child. Its outer report has `owned_account_qualification_failed`
at `qualification_validate`, elapsed 4123 ms, unchanged supervisor interpreter,
`owned_children_reaped=true`, `owned_state_removed=false`, and unchanged runner
Keychain. This establishes a child failure, without identifying whether provider
setup, installation, or product setup failed. Do not infer a product defect from
this report or weaken qualification to obtain a pass.

This increment adds failure-only finite child diagnostics in the same existing
supervisor and tests. It uses the existing pinned diagnostic workflow, product
bytes, 12-path source audit and provenance envelope unchanged. No product,
account, Keychain configuration, cleanup authority or timeout changes are in
scope. Obtain actual independent Claude design approval before implementation,
then code review before publication and another bounded hosted diagnostic.

### Child interfaces and trust boundary

Use the private `_stage` key on the child result as the live tracker, set only
from source literals. The inner supervisor supplies its existing result dict to
the provider through an optional private `_diagnostics` attribute before `run`;
this does not change constructors or the existing lifecycle test fixtures.
`Qualification.run` sets a source literal immediately before each coarse
boundary. `MacProvider.security` updates that same tracker using an explicit
source-literal stage keyword passed by `start`, `lock`, `unlock` or `close`; it
never derives a stage from an argument, path, response or exception. Linux providers keep coarse boundaries.
The tracker is removed immediately before writing a report, including successful
reports. Never export provider arguments, raw stdout/stderr, exception text,
passwords, key references, identity material or filesystem paths.

Closed `CHILD_STAGES` values are `diagnostic_invalid`, `provider_start`,
`mac_create_keychain`, `mac_default_select`, `mac_search_select`,
`mac_unlock_keychain`, `mac_keychain_settings`, `mac_default_verify`,
`mac_lock_keychain`, `mac_delete_keychain`, `install`, `initial_probe`,
`quickstart`, `quickstart_validate`, `initial_identity_validate`,
`named_identity_setup`, `named_identity_validate`, `identity_reload`,
`locked_validation`, `unlocked_validation`, `absent_provider_validation`,
`qualification_remaining`, and `provider_cleanup`. Distinguish default-keychain
selection from verification with separate explicit source-literal stages at
the call sites in `start`; neither inspects argv or path contents. Mark provider
start before calling it and installation immediately after it, so the present
4-second failure will identify a specific security operation or the first
post-provider boundary. The lifecycle override must set `qualification_remaining`
immediately after provider start, lock and unlock. Linux keeps the regular
qualification runner's coarse boundaries rather than broad new instrumentation;
existing finite primary failure categories remain available. This is attribution, not a redesign
of the qualification runner.

On the first child qualification exception, record failure-only
`child_failure_stage`, optional `child_exit_code`, and `child_failure` before
cleanup changes `_stage`. `Refused.exit_code` is retained only when it is the
actual checked subprocess status; timeouts and stop-error replacements have no
invented status. Generic exceptions map to `provider_qualification_failed`.
The closed child category enum consists of `diagnostic_invalid`,
`operation_failed`, `operation_timeout`, `operation_input_limit`,
`operation_output_limit`, `child_cleanup_incomplete`, `owned_descendant_remained`,
`qualification_deadline`, `provider_qualification_failed`,
`provider_cleanup_incomplete`, `owned_default_keychain`,
`keychain_cleanup_incomplete`, `provider_state_unavailable`, `helper_contract`,
`provider_metadata_limit`, `quickstart_report`, `quickstart_auto_keychain`,
`fresh_identity_reload`, `profile_metadata`, `profile_custody`,
`plaintext_fallback`, `separate_key_references`, `provider_list`,
`provider_item_count`, `provider_item_uniqueness`, `named_identity_separation`,
`named_reference_separation`, `locked_existing_accepted`,
`locked_fresh_accepted`, `locked_profile_mutated`, `locked_files_fallback`, and
`locked_native_items_changed`. Any other category becomes `diagnostic_invalid`;
additional known categories require an explicit reviewed enum addition, never a
prefix match. This bounded list is sufficient to separate the present early
failure without instrumenting every lifecycle assertion.

For a cleanup-only failure, capture the same child fields at the precise
current stage, including `mac_delete_keychain`. Retain a real `Refused` category
and checked status subject to closed validation; generic cleanup exceptions use
`provider_cleanup_incomplete` without a status. If a primary child failure already exists,
its three child fields are immutable through cleanup: use a single first-failure
capture operation, rather than independently filling a missing exit code from a
later failure. Existing `failure`, cleanup booleans, return status, account/home
retention and group reaping behavior are unchanged. Success reports gain none
of these new fields. Lifecycle signal/timeout behavior must not overwrite the
captured primary child cause with a later cleanup cause.

The outer process treats every received child field as untrusted. It copies
only the three named child diagnostic fields, on a failed qualification, through
one sanitizer. Require exact closed membership for the category and stage;
non-string, missing required or unknown values become `diagnostic_invalid`.
Optional statuses require `type is int` in -128..255; bool and invalid values are
omitted. Discard all child diagnostic fields on success. A legacy failed child
report without these fields gains no fabricated diagnostics. If a failed child
provides either required field, normalize both required fields, so a malformed
partial diagnostic cannot pass as valid. Keep the outer primary
`failure=owned_account_qualification_failed` and its `qualification_validate`
stage; never restore copying an unchecked inner `failure` into the outer public
report. Reapply this sanitizer at both existing report writes, including the
main report rewrite, and strip `_stage` in every case.

### Acceptance and review

Add offline full mocked-child/outer tests for a specific Mac security failure
with a real finite exit status, install failure, generic exception with sentinel
text, and cleanup failure after a primary failure with and without a primary
status. Assert child stage/category/status survive unchanged when outer failure
is assigned, and cleanup cannot add its own status to the primary diagnostic.
Exercise real `/bin/sh` exit 7 and the stop-error replacement contract without
raw capture output. Test unknown categories, unknown/non-string stages,
non-integer/bool/out-of-range statuses, partial fields, legacy missing fields,
and successful reports. Sentinel text in argv, output and exception must never
appear in serialized child or outer reports. Confirm private tracker removal,
first-failure preservation, existing cleanup red-stays-red assertions and
unchanged successful report shape. Repeat the supervisor discovery/direct
suite, static workflow tests, full Node tests and actionlint. After independent
code approval, dispatch the existing bounded diagnostic and report its exact
product/supervisor/run binding and finite child failure or actual provider pass.


The child addendum received actual Claude design approval recorded in
`/tmp/marmish-scj-provider-child-design.json`, with nine binding conditions.
Implementation uses separate `CHILD_STAGES` and `CHILD_FAILURES` enums and a
distinct `child_stage` helper. Mac security calls pass explicit source literals;
there is no argv inference. A missing/None provider tracker is a no-op.
Non-security transitions reset the tracker immediately, including
`quickstart_validate` before parsing and `qualification_remaining` in the
lifecycle override immediately after provider start, lock and unlock. Lifecycle,
Linux and absent-provider categories outside the reviewed category enum map to
`diagnostic_invalid` in the supplemental diagnostics; existing primary categories
are unchanged.

The first child cause is assembled into locals and captured by one guarded
`dict.update`, preserving the complete tuple through cleanup. Cleanup-only
failure uses the precise current stage, including `mac_delete_keychain`; real
`Refused` category/status is retained subject to closed validation, while a
generic cleanup exception maps to `provider_cleanup_incomplete` without status.
This clarifies the earlier coarse cleanup wording. Child fields are sanitized
again at the outer report boundary and removed on success. Review records stay
in existing allowlisted documentation and `/tmp`, without new review paths.

Offline supervisor discovery and direct execution each pass 40 tests, including
specific Mac create/delete failure, generic sentinel and cleanup preservation,
outer revalidation, malformed fields, successful shape, lifecycle and install
stale-stage checks, quickstart parser placement, and real exit/stop replacement.
Node tooling passes 58 tests; static workflow contracts pass 3; actionlint 1.7.7
and whitespace checks pass. These tests establish diagnostic contracts only;
actual code approval and a new pinned hosted provider run remain required.
