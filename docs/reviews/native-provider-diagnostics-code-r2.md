# Native provider diagnostic code review, round 2

Reviewer: actual Claude Code `claude-sonnet-5-5`, permission mode `auto`.
Session: 240dcf6f-a887-45f4-9199-f3baf634ffce
Verdict: APPROVED.

**APPROVED.** B1 is resolved, N1–N3 are fixed, and I found no new blocker. I made no edits or commits and ran no Cargo, live accounts, keychains or network checks.

**Scope reviewed (six files):**
- `tools/marmish_native_keychain_check.py`
- `tools/test_marmish_native_keychain_check.py`
- `tools/v02-actions/workflow.mjs`
- `tools/v02-actions/workflow.test.mjs`
- `.github/workflows/v02-provider-diagnostic.yml` (new)
- `tools/test_marmish_provider_diagnostic_workflow.py` (new)

I read these against `docs/phases/native-provider-diagnostics.md` and `docs/reviews/native-provider-diagnostics-code-r1.md`.

**Round 1 findings:**
- **B1, resolved.** `provider_failure` now assigns `result['failure'] = category` for the primary failure. Cleanup uses `setdefault` on `cleanup_failure`, and the stage and exit code keep first-wins semantics. The new test `test_inner_failure_cannot_replace_supervisor_category_or_leak_sentinel` feeds an inner report with `ok:false, failure:'SENTINEL inner text'`. It asserts the outer `failure` is `owned_account_qualification_failed`, the stage is `qualification_validate`, and the sentinel is absent from the serialized report.
- **N1, resolved.** `ProviderDiagnostics` now sits above the `if __name__ == '__main__'` block. Running the file directly executes it: 30 tests ran and passed.
- **N2, resolved.** `test_stop_error_replaces_exit_without_fabricating_status` patches `stop` to fail. It asserts `exit_code is None`, the stage is attached, and the sentinel does not leak.
- **N3, resolved.** When `signals.cancelled` forces `lifecycle_provider_signal`, the code now pops the stale `failure_stage` and `exit_code`. `test_signal_override_drops_stale_primary_stage_and_status` covers it.
- **N4 (optional sentinel coverage), improved.** The one-failure-per-stage test, which runs the full `mac_account` harness, now asserts no `SENTINEL` in each report.

**Checks run:**
- **Supervisor tests:** `python3 tools/test_marmish_native_keychain_check.py` passed 30 of 30 tests.
- **Workflow contract test:** `test_marmish_provider_diagnostic_workflow.py` passed 3 of 3 tests.
- **Node suite:** `node --test` in `tools/v02-actions` passed 58 of 58. It covers the production-policy rejections, the lookalike policy object, the job and artifact guards, the 12-path audit and envelope preservation on refusal.
- **Workflow lint:** `actionlint@v1.7.7 -shellcheck=` on `v02-provider-diagnostic.yml` reported no findings.
- **Design conformance:**
  - The workflow is dispatch-only with `contents: read` and `actions: read`.
  - It runs one `macos-15` job with a 15-minute timeout and non-cancelling concurrency.
  - Checkout uses `persist-credentials: false` and `fetch-depth: 0`.
  - Action pins match the candidate workflow.
  - Step order is guard, download, asset guard, unit tests, qualification.
  - The upload step has `if: always()`, two explicit paths, `if-no-files-found: warn` and seven-day retention.
  - The artifact name prefix is `v02-provider-diagnostic-`.
  - `DIAGNOSTIC_PATHS` is exactly the 12 literal design paths.

**Unrelated test failures, not caused by this change:** a broader `unittest discover -p 'test_marmish_*.py'` over `tools/` reports 2 failures and 11 errors. They are in `test_marmish_publish`, `test_marmish_release`, `test_marmish_interop`, `test_marmish_managed_workflow` and `test_marmish_installed_release`. None of those files references the six in-scope files, and I did not run them against `HEAD`. The workflow's own step, which runs only `test_marmish_native_keychain_check.py`, is unaffected.

**Commit-time cautions (not blockers):**
- Stage only the six files plus the diagnostic docs. The workflow audits `git diff PRODUCT HEAD`, so the untracked `native-automatic-release` and `v0.2.3-publication` docs must stay out of the commit. Otherwise `diagnostic_source_delta` refuses the run.
- The allowlist includes `docs/reviews/native-provider-diagnostics-code-r2.md` and `docs/validation/native-provider-diagnostics.md`. Whichever of those you add later stays within the allowlist, and any file outside it will cause a refusal.
