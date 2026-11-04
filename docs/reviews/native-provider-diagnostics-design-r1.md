# Independent Claude design review

Reviewer: Claude Code, claude-sonnet-5-5, `--permission-mode auto`.
Session: `1f265267-46c7-476f-b42a-4247591d7e8d`. Read-only review; no implementation approval is inferred beyond the explicit verdict.

**Verdict: not approved.** I found six blocking items, listed below with fixes. The overall direction is sound: a finite diagnostic extension plus a diagnostic-only workflow that reuses the passed packages. I made no edits and ran no Cargo or live processes. I read the design doc, `tools/marmish_native_keychain_check.py`, `tools/test_marmish_native_keychain_check.py`, `v02-release.yml`, `v02-installed.yml`, `tools/v02-actions/workflow.mjs`, and the existing diagnostic workflow tests.

## Blocking findings

**B1. Product-source binding is unspecified and can't work as written.**
- The doc says the upstream run "matches the exact source", but the GitHub run API doesn't expose the `source_commit` input. `run.head_sha` is the dispatch commit, and the non-caveated release path never requires it to equal `source_commit`.
- The existing `checkRun` in `workflow.mjs` checks only that `head_sha` is 40-hex. `v02-installed.yml` instead forces `SOURCE_COMMIT == GITHUB_SHA`, which can't hold here because the supervisor commit differs from the product source.
- **Fix:**
  - Name the binding mechanism. Either `head_sha` equals the pinned product commit, confirmed from the run record, or the commit comes from evidence embedded in the downloaded assets. That evidence could be the manifest's build commit (`native_manifest.rs` / `build.rs` use `BUILD_COMMIT`) or the upstream macOS provider report's `source_commit`.
  - Add a negative test for a wrong source.
  - Keep `checkRun`'s other checks: `path == .github/workflows/v02-release.yml`, `event == workflow_dispatch`, `head_branch == master`, and repository and head repository equal to `Epiphytic/marmish`. The design omits several of these.

**B2. Don't hand-roll the cross-run resolver.**
- `tools/v02-actions/workflow.mjs` already has `candidateInputs`, `checkRun`, and `validateCandidate`. It checks for exactly one artifact per exact name, `!expired`, and the run/attempt tuple, and it has tests.
- **Fix:** Parameterize that code, rather than writing bash/jq. The parameterization must allow run `conclusion=failure` and must not require `SOURCE_COMMIT == GITHUB_SHA`.
- Job verification needs these specifics:
  - Fetch attempt-scoped jobs with pagination and check the total count.
  - Matrix job names embed all matrix values, so match the exact name or an id and reject duplicates.
  - Require all three package jobs plus render to be `success`. Render consumed all three manifests, so the pinned bootstrap depends on them.
  - Require the upstream macOS provider job to be the failed one (`126848598035`, `failure`), so the diagnostic is tied to its subject.

**B3. Product/supervisor separation isn't enforced.**
- The doc records two commits but doesn't constrain how they differ. The supervisor imports `marmish_native_quickstart_check.py` from its own directory, so tooling drift would silently change what "qualifies the old assets" means.
- **Fix:** Add a provenance step in the style of `v02-macos-fixture-diagnostic.yml`:
  - `github.ref == refs/heads/master` and `HEAD == GITHUB_SHA`.
  - The product commit is an ancestor of `HEAD`, and `HEAD` is an ancestor of `origin/master`.
  - Use `fetch-depth: 0` and `persist-credentials: false`.
  - `git diff --name-status` between product and `HEAD` contains only A/M regular files from an explicit allowlist.
  - That allowlist is the supervisor, its tests, the new workflow, the new test and docs. It excludes `crates/`, `Cargo.*`, `scripts/`, the bootstrap/installer, and `marmish_native_quickstart_check.py`, unless the quickstart file is deliberately listed and unchanged.

**B4. The report envelope and artifact identity are underspecified.**
- The supervisor sets `run_id` and `run_attempt` from `GITHUB_RUN_*`, which would now be the diagnostic's run, while `source_commit` is the product's. The doc promises the old meanings are preserved but doesn't say how, and it never says where the new fields are written.
- **Fix:**
  - Leave the supervisor invocation unchanged. Have the workflow write a separate provenance envelope with `product_source_commit`, `supervisor_source_commit`, `upstream_run`, `upstream_attempt`, `diagnostic_run`, `diagnostic_attempt`, and `diagnostic: true`. Document that the supervisor's `run_id` field is the diagnostic run.
  - Use an artifact name that can't match `v02-provider-*`, such as `v02-provider-diagnostic-…`. Today `v02-release.yml` uploads exactly `v02-provider-<target>-<run>-<attempt>`, so a diagnostic pass must not be confusable with that.
  - Upload explicit file paths, not globs. Make sure a guard failure before the supervisor runs doesn't turn into an `if-no-files-found: error` upload failure that hides the real error.

**B5. Finite sanitization is only described, not enforced.**
- **Closed vocabulary at serialization:** Define a closed `STAGES` frozenset. `exit_code` must be `type(x) is int` (bool excluded) within roughly -128..255. The report writer drops or replaces anything outside those bounds. Negative values mean death by signal, so they must stay valid.
- **Generic failures need a stage too:** Today `except Exception` produces `owned_account_prerequisite_failed` with no stage. That covers a `dscl` UID parse error, a `pwd` error, a UTF-8 decode error, or a JSON parse of the inner report.
  - **Fix:** Track a "current stage" variable set before each step, including non-`execute` steps, and attach it in both except branches. Wrapping only the `execute` calls leaves these unattributed.
- **Cleanup fields and stage order:** Name the cleanup fields exactly (for example `cleanup_failure`, `cleanup_failure_stage`, `cleanup_exit_code`) and keep the existing `setdefault` semantics.
  - `execute`'s `finally` calls `stop()`, which can raise a different `Refused` and replace `operation_failed`. In that case the exit status is lost.
  - **Fix:** Specify that behavior as "stage attached, exit_code absent". Don't chain exception text into the report.
- **Fresh exception per failure:** Allocate a new `Refused` per failure, because `exit_code` is a mutable attribute.

**B6. The test plan needs a harness that doesn't exist yet.**
- The existing suite has no `mac_account` test at all.
- **Fix:** Specify a harness that patches `check.execute`, `pwd.getpwnam`, `staged_provider_inputs`, `digest`, the `home` path checks, and `signal`.
- **Required cases:**
  - One parametrized failure per stage, asserting the exact `failure`, `failure_stage`, `exit_code`, `ok=false`, and `owned_account_removed=false`.
  - A cleanup failure after an earlier operation failure leaves the earlier diagnostics intact.
  - A changed runner Keychain stays red.
  - A missing account cleanup stays red.
  - Sentinel argv, stdout, stderr and exception text never appear in the serialized report.
  - A real `/bin/sh -c 'exit 7'` run gives `exit_code == 7` with unchanged `str(Refused)`, and a timeout gives no `exit_code`.
- **Workflow tests:** Add static contracts in the existing denial-workflow style:
  - `workflow_dispatch` only.
  - Exact permissions `contents: read` and `actions: read`.
  - All actions pinned by 40-hex SHA.
  - No `${{ inputs.* }}` inside `run:` blocks. Pass them through `env:` and compare them to literals, as the existing workflows do.
  - No Cargo, no release mutation, and no `workflow_run` or other triggers.
  - The exact artifact tuple.
  - The resolver's negative tests for wrong source/attempt/workflow, a missing or failed package/render job, and an expired or duplicate artifact.

## Non-blocking

- **N1. Stage plus exit code is not a cause.**
  - `sudo -n` fronts `dscl`, so exit 1 can mean either "password required" or a `dscl` failure.
  - The doc must say the report gives stage and status only, with no cause claimed.
  - Optionally add a fixed, read-only `sudo -n /usr/bin/true` stage first. Alternatively, use the existing exact-match `error_map` for known stderr lines to give a finite discriminator.
- **N2. Cleanup noise after a failed first create.** `created = True` is set before the first `dscl -create`. If that call fails, cleanup hits `KeyError` and reports `owned_account_cleanup_incomplete`. Document this as expected. It's harmless on an ephemeral hosted runner, and the failed-run non-claim of account removal stays true.
- **N3. Literal-bound inputs.** If the values are bound to literals, drop the inputs. Otherwise compare them to the literals before any download.
- **N4. Hardening and audit.**
  - Record the sha256 of the downloaded artifacts in the envelope.
  - Set `concurrency` and a short `retention-days`.
  - The pinned `download-artifact` SHA already supports `run-id` and `github-token`, as `v02-installed.yml` uses it.
- **N5. Harness hash will change.** The supervisor path is in `harness_sha256` and `SOURCE_PATHS`. The new release will carry a different harness hash than run 41898839617. That is expected, and the doc should say the diagnostic pass doesn't carry over to it.

## What's solid
- Keeping `str(Refused)` as the category and making new fields additive.
- The "first failure stays primary" rule.
- Preserving the cleanup bounds, the UID-only signal scope, the deletion conditions, and the Keychain comparison.
- Not requiring upstream overall success while insisting on package and render success.
- Reusing existing action pins, without Cargo and without mutating a release.

Re-review after the doc covers B1–B6. I expect approval then, since the rest of the design holds up.
