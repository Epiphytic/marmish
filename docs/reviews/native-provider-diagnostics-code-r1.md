# Native provider diagnostic code review, round 1

Reviewer: actual Claude Code `claude-sonnet-5-5`, permission mode `auto`.
Scope: six diagnostic implementation files against native-provider-diagnostics.md.
Verdict: NOT APPROVED; B1 sent to implementation agent for correction.

**Verdict: NOT APPROVED. There is one blocking defect (B1).** The rest of the scope matches the approved design. I did not edit anything. I ran no Cargo, live accounts, keychains or product processes. `actionlint` isn't installed here, so the workflow YAML is checked only by reading and by the Python static contract test.

## Blocking defect

**B1: the primary failure category is not preserved, so `failure` can be wrong.** In `tools/marmish_native_keychain_check.py`, `provider_failure` uses `result.setdefault('failure', category)`, and `mac_account` copies the inner report's `failure` into `result` before `provider_result_check`.
- **Failure scenario:** the inner report has `ok:false, failure:'X'`, then `provider_result_check` raises `Refused('owned_account_qualification_failed')`. I reproduced it offline: `provider_failure({'ok':False,'failure':'inner_x'}, Refused('owned_account_qualification_failed'))` returns `failure:'inner_x'`.
- **Behavior change:** the old code was `result.update(failure=str(error))`, which overwrote the inner value. The design says "existing report consumers receive the same `failure` categories", and that is no longer true.
- **Second problem:** the inner process's `failure` string now survives unvalidated into the diagnostic report on every failure path.
- **Minimal fix:** in `provider_failure`, assign the primary category instead of using `setdefault`: `if not cleanup: result['failure'] = category else: result.setdefault('cleanup_failure', category)`. The primary branch runs once, so the "first failure" rule is kept.
- **Test:** add a case where the inner report carries `ok:false, failure:'X'` and assert the outer `failure` is `owned_account_qualification_failed`, with the stage `qualification_validate`.

## Non-blocking, worth fixing in the same commit

1. **Tests placed after `unittest.main()`.** `ProviderDiagnostics` in `tools/test_marmish_native_keychain_check.py` (about line 380) is defined after the `if __name__ == '__main__'` block. Running the file directly skips those tests. The workflow's `discover` run is unaffected (27 tests pass). Move the class above the main block.
2. **Missing stop-error test.** The design requires "the stop-error replacement case without a fabricated status". I found no test where `stop()` fails and replaces a subprocess failure. Add one that patches `stop` to raise, then asserts `exit_code is None` and the stage is attached.
3. **Stale stage after a signal.** When `signals.cancelled` forces `failure='lifecycle_provider_signal'`, any earlier `failure_stage` or `exit_code` stays in the report. That is pre-existing override behavior, but the stage then describes a different failure. Consider dropping those two keys in that branch.
4. **Sentinel coverage is thin.** The tests inject a sentinel only through the early `ValueError` and the sanitizer. There is no end-to-end `mac_account` run with sentinel argv, output or exception text. This is optional hardening.

## Checked and found correct

- **Source, run and job bindings.** The default production policy is unchanged. `diagnosticPolicy` requires identity with the frozen `PROVIDER_DIAGNOSTIC` object, so a copied lookalike is rejected, and the tests cover that.
  - **Diagnostic policy:** it requires conclusion `failure`, `head_sha == product`, and run and attempt equal to the literals. The repository, path, event and branch checks are kept.
  - **Jobs:** the jobs endpoint is attempt-scoped and paginated, with a 2 MiB response cap, a 10-page cap and a `total_count` match. The five exact id and name pairs must each match exactly once. The shared transport applies its API budget to the new jobs requests.
  - **Artifacts:** only the two exact named artifacts are selected. The existing uniqueness and unexpired checks apply.
- **Manifest guard.** The `provider-diagnostic-assets` check expects 13 split lines. That matches `native_manifest.rs`: 12 lines plus the trailing empty one, with target on line 2, commit on line 3 and `end` on line 11.
- **Literal 12-path audit.** `DIAGNOSTIC_PATHS` matches the design list exactly. The audit uses `diff --no-renames --name-status -z`, allows only `A`/`M`, and requires a `100644 blob` tree entry. It rejects an empty delta, and `crates/`, Cargo files and the quickstart check are not on the list. The git ancestry checks match the design.
- **Envelope.** It is written before the guards with a finite `guard` value. Refusal preserves it, and the test confirms no error text leaks. The product and supervisor sources are separate fields, and the artifact ids, API digests and manifest and bootstrap digests are recorded.
- **Workflow YAML.**
  - **Trigger and permissions:** `workflow_dispatch` only, with no inputs, and `contents: read` plus `actions: read`.
  - **Job setup:** `macos-15`, 15 minutes, concurrency without cancellation, and checkout with no persisted credentials and full history.
  - **Pins and invocation:** action SHAs are identical to the ones in `v02-release.yml`. The supervisor invocation mirrors the release job.
  - **Step order and upload:** the order is guard, then download, then asset guard, then unit tests, then qualification. The upload step has `if: always()`, explicit paths and `if-no-files-found: warn`, so a missing report cannot mask a guard failure. The artifact name prefix is distinct and retention is seven days.
- **Stage and exit-code handling.**
  - **STAGES:** it has 40 stages plus `diagnostic_invalid`, covers every operation, and `ProviderSignals` setup correctly gets no stage.
  - **Exit codes:** they come from the real `returncode` through a fresh `Refused`. Timeouts and bound violations get none. The sanitizer enforces `type is int` and the -128..255 range, and rejects bool.
  - **Report paths:** the sanitizer runs on both report-write paths and is idempotent.
  - **Cleanup:** cleanup fields use `setdefault`. Changed runner keychains and missing account removal both keep the report red.
- **Tests.** The Python supervisor tests pass (27 tests), the diagnostic workflow contract test passes (3 tests), and the Node suite passes (58 of 58).

Once B1 is fixed and re-tested offline, I expect to approve. Fixing items 1 and 2 in the same change is cheap and keeps the acceptance list in the design complete.
