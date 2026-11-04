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


## Follow-up owner-only chown code review

Actual Claude Code `claude-sonnet-5-5`, permission mode `auto`.
Session: 356654f9-896f-4d37-b32a-5d5b51c4b916

**Verdict: APPROVED.** I found no blocking findings. I ran only the offline Python tests. I ran no Cargo, live accounts or keychains, or network, and I edited and committed nothing.

**Root cause and fix**
- The code diff is one line, at `tools/marmish_native_keychain_check.py:764`. The chown operand changes from `str(uid) + ':-2'` to `str(uid)`.
- A chown operand with no colon sets only the owner and leaves the group unchanged. That removes the negative-group operand that the diagnostic report (`account_home_chown`, exit code 1) attributes to Apple's unsigned numeric parsing.
- The previous design approval is consistent with this. It confirmed the cause, and its non-blocking note 1 asked for the exact-argv and ordering assertion, which the new test adds.

**Regression test**
- `test_fresh_private_home_creation_precedes_owner_only_chown` runs the full mocked `mac_account` flow and records every `execute` argv through a new optional `calls` parameter.
- It checks that exactly one chown occurs and that its argv is exactly `['/usr/bin/sudo','-n','/usr/sbin/chown','55000',home]`.
- It checks that the operand has no `:`, so a group change cannot return silently.
- It checks that the exact `mkdir -m 700 home` call occurs once and comes before the chown.
- It checks that the dscl `PrimaryGroupID -2` create is still issued. The dscl record path `/Users/<name>` equals `home`, so that assertion matches real behavior.
- The old operand `55000:-2` would fail the exact-argv assertion, so the test does guard this regression.

**Isolation and cleanup**
- The diff touches no account creation, path validation (`account_home_check`), identity checks, cleanup logic or runner-keychain comparison.
- The identity checks compare uid and home, not group.
- The home keeps whatever group `sudo mkdir` gave it. At mode 0700 that group has no access, so owner-only is the right posture.

**Tests**
- `unittest discover` and direct execution each give 31/31 OK.
- The 8 `ProviderDiagnostics` tests pass, including the new one.
- Running with `python3 -I -m unittest <module>` fails with `ModuleNotFoundError`, because `-I` drops the working directory from the path. That is how I invoked it, not a defect in the code.

**Non-blocking notes**
1. The phase doc has missing spaces from line-wrapping, such as `supervisor5ccc646c…`, `product61f4…`, `mode0700`, `supervisor30+`, `Node58`, `staticworkflow3` and `version0.2.2`. Fix these in a docs-only touch-up before committing.
2. I could not verify offline the claims about hosted run 41915532600/1 (start time, `guard=assets_pass`) or the Node 58/58, static-workflow 3/3 and actionlint results. They are plausible and consistent with the diff, and I take them from the docs.
3. The docs correctly say that a fresh hosted provider run is still required. Later stages, from `stage_copy` onward, have never run, so any failure there should be recorded as a new observation.


## Finite child diagnostic code review

Actual independent Claude review recorded in
`/tmp/marmish-scj-provider-child-code.json`: **APPROVED**, no blockers. Reviewer
inspected the four-file child diagnostic increment against all nine binding
design conditions and independently ran supervisor discovery and direct
execution: 40/40 each, with `git diff --check` clean. Reviewer used no Cargo,
live account, Keychain, network, edits or commits. Node 58, static workflow 3,
and actionlint results were author evidence, not independently rerun by Claude.

Useful non-blocking observations were incorporated: capture the child tuple
before assigning the primary failure field, accept checked status only from
`Refused`, and reconcile the earlier design prose with explicit source-literal
stages, lifecycle reset requirements and precise cleanup-only status policy.
A generic exception bearing an arbitrary `exit_code` now has a regression test
proving that status is omitted. Updated supervisor discovery and direct execution
each pass 41/41; diff whitespace check passes. A narrow follow-up review of these
changes remains required before publication; no hosted provider pass is claimed.


## Child diagnostic final narrow review

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session: f7360a21-ac8f-442d-9b22-042181113953

**APPROVED.** The narrow fixes are correct, and I found no blockers or new findings. My prior full integrated approval stands.

**Checks I ran**
- The Python suite passes 41/41 as `unittest` and as direct execution. That is the earlier 40 plus the new generic-exception test. I ran both from `tools/`, since `-I` breaks the sibling import.
- `git diff --check` is clean.
- I did not rerun the Node, static-workflow or lint checks. I took your supervisor 41, 58, 3 and lint PASS results as given.
- I made no edits, network calls, Cargo runs, live runs or commits.

**The four changes**
1. **`child_failure()` runs before the `failure` assignment.** Both inner excepts do this (`tools/marmish_native_keychain_check.py:651-656`): `child_failure(result, error)`, then `result['failure'] = …`. The cleanup path at `:665` is also unchanged and correct. The first-failure guard `if 'child_failure' in result: return` and the single `result.update(values)` keep the stage, category and status tuple together.
2. **Only `Refused` supplies the checked exit status.** `code = error.exit_code if isinstance(error, Refused) else None` (`:87`). The values then go through `sanitize_child_diagnostics`, which keeps only `type(code) is int` in -128..255. The outer `provider_failure` still uses `getattr(error, 'exit_code', None)`, but that is a separate supervisor-side field, not the child tuple.
3. **New test `test_generic_exception_cannot_supply_checked_subprocess_status`.** It builds a `RuntimeError` with `exit_code = 7` and checks three things:
   - `child_exit_code` is absent.
   - The category is `provider_qualification_failed`.
   - The raw `SENTINEL` text does not appear in the sanitized report.
4. **Design prose reconciled.** In `docs/phases/native-provider-diagnostics.md`, lines 22-24 and 313-316 now say the status is retained only when it is the actual checked subprocess status. Generic exceptions map to `provider_qualification_failed` without a status. Lines 334-340 say generic cleanup exceptions use `provider_cleanup_incomplete` with no status, and that the first-failure capture is a single operation. I found no leftover contradiction.

**Non-blocking note:** the supervisor-side `provider_failure` still takes `exit_code` from any exception via `getattr`. It is not part of the child diagnostic boundary, and the design only requires `Refused.exit_code` there. I'm not asking for a change.


## Owned Preferences prerequisite code review

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session: 8cb3181d-089b-4049-baf1-eb8bb85db66e

**APPROVED.** There are no blocking findings. This is an offline approval only. Nothing here shows the hosted default lookup now works.

## Adherence to the approved design and the three amendments
- **Scope and ordering:** `MacProvider.prepare_preferences()` (`tools/marmish_native_keychain_check.py:328`) is the only code change that does anything. `start()` calls it before the first `security` command. `Qualification` builds the provider with `HOME` from the account database, so the real flow supplies a matching HOME.
- **Only the disposable account's HOME:**
  - HOME must be a string, absolute, and equal to `pwd.getpwuid(geteuid()).pw_dir` (amendment 2).
  - `self.root.parent == home` is an exact-parent test, not a lexical descendant test.
  - HOME and the provider root are both checked with `lstat`: directory, not a symlink, owned by the effective uid, mode exactly 0700. That is stricter than the design asked for, which is fine.
- **Directory creation:**
  - `Library` and then `Library/Preferences` are created with non-recursive `mkdir(mode=0o700)`.
  - Each is re-checked with `lstat` for directory, effective uid and exactly 0700.
  - A directory that already exists is validated and never repaired, and the exact-0700 refusal is deliberate (amendment 3).
- **Finite errors:**
  - `except (OSError, KeyError)` becomes `Refused('owned_preferences_directory') from None`, with no exit code.
  - That covers a `mkdir` race and a failed account lookup (amendment 1).
  - The stage and category were added only to `CHILD_STAGES` and `CHILD_FAILURES`, the only allowlists. The existing sanitizer therefore passes only the finite tuple.
  - No path, errno or message text can reach the report.
- **Original security flow:** the six `security` argv calls and their order are unchanged. The `default-keychain -d user` query and the equality check are still required. No sudo, environment change, fallback, retry or product change was added.
- **Cleanup:** `inner()` still removes the provider root only on success. The account cleanup still runs `rm -rf` on the home, so the new `Library` directories are removed with the account (`tools/marmish_native_keychain_check.py:931`). The failure path still reports `owned_account_removed=false`.

## Tests I ran
- `python3 -I -m unittest discover -s tools -p test_marmish_native_keychain_check.py` ran 44 tests, all OK.
- `python3 tools/test_marmish_native_keychain_check.py` (direct run) ran 44 tests, all OK.
- `git diff --check` is clean.
- I did not rerun the Node tooling (58), static workflow (3) or actionlint results the docs cite, so I can't confirm them.
- I touched no live accounts, Keychains or runner HOME. The tests use temporary directories with mocked `pwd` and `execute`.

The new tests cover these cases:
- **Fresh and reused directories:** the argv and order stay unchanged across a fresh run and a second `start()`.
- **Refused paths:** symlink, file, wrong mode, foreign uid, HOME mismatch, symlinked HOME, symlinked root and unrelated root. None runs `security` or sets an exit code, and the stage is `mac_preferences_prepare`.
- **`mkdir` failures:** a permission error and a target-appears race both produce the finite report with no `SENTINEL` text.

## Nonblocking findings
1. The `mode` fixture in `test_invalid_existing_paths_refuse_before_any_security` calls `library.mkdir(mode=0o755)`. That mode is subject to the umask, so under a restrictive umask such as 077 it would create 0700 and the test would fail for the wrong reason. A `chmod(0o755)` afterwards would make it deterministic.
2. The foreign-uid, wrong-mode and symlink cases only exercise `Library`, and the foreign-uid case only mocks HOME. The `Preferences` component and a `KeyError` from the account lookup share the same code path but have no dedicated case.
3. A restrictive umask makes the created directories fail the exact-0700 check. That fails finitely, which is acceptable.
4. In `docs/phases/native-provider-diagnostics.md` the last section has a stray double blank line. The doc also still has a "descendant/generic error wording" sentence that the amendments supersede, and it says so.

## Hosted-evidence limits
- The offline tests prove the preparation logic and the diagnostics contract only. They do not prove macOS `security` behaves as hypothesized.
- The missing-`Library/Preferences` theory rests on Apple source reading. The run's stderr was suppressed, so nothing observed confirms it.
- A hosted pass past `mac_default_verify` would show the directories are sufficient. It would not prove they were the original cause.
- A hosted run is still needed to show a successful default query and then full qualification, including cleanup (`owned_account_removed`, `runner_keychain_unchanged`).
- If that run fails, the failing stage is new evidence to report. If it fails at `mac_preferences_prepare`, the first thing to check is whether the real account's HOME is exactly 0700 and equals `pw_dir`.

Disposition: fixed the wrong-mode fixture with an explicit chmod so it is independent of umask. No product or provider behavior changed after approval. Additional component-specific fixtures are nonblocking because both components use the same validation loop. Hosted proof remains required.

### Installer utility discovery — actual code and integration review

Claude Code 2.1.301 (`claude-sonnet-5-5`), session
`c2dc2382-9d7e-4307-9d58-e5e93d3ab186`, APPROVED the five-file diff without
blocking findings. Actual JSON evidence:
`/tmp/marmish-scj-provider-discovery-code.json` and the same-session final
`/tmp/marmish-scj-provider-discovery-integration.json`. Useful nits incorporated
before final review: discovery-only constant comment and a recording fake that
finds sysctl only when `/usr/sbin` is included. Reviewer independently passed
46 supervisor tests (discovery/direct initial review; discovery final review),
58 Node tests and whitespace checks. It confirmed unchanged success schema and
failure-only child diagnostics, dynamic sourceDigest inclusion of the supervisor,
the literal diagnostic path audit, and no product/provider gate bypass. The
separate automation approval remains applicable to this finite source class;
reviewer inspected its exact-key report validation but did not rerun its 113-test
suite. No hosted success is asserted. Parent will run integrated validation.

Author checks: supervisor discovery/direct 46/46, static workflow 3/3, Node
58/58, actionlint 1.7.7 and diff whitespace PASS. Ripwire quality-delta flags
historical short-horizon churn on the existing Qualification constructor and
unittest-discovery symbols as dead code; no structural expansion is needed for
the one-call lookup correction. No local accounts, Keychains, product builds or
live commands were used. Hosted qualification is still required.

### Complete quickstart PTY failure diagnostics — actual code/integration approval

Claude Code 2.1.301, `claude-sonnet-5-5`, session
`9614455e-0370-4058-95cf-520ae9f72d13`, APPROVED code and integration without
blockers. Actual evidence: `/tmp/marmish-scj-provider-pty-code.json`,
`/tmp/marmish-scj-provider-pty-integration.json`, and final test-only approval
`/tmp/marmish-scj-provider-pty-test-bound.json`. All five binding design conditions
were checked, including immutable pre-cleanup snapshot, strict nested rebuilding,
quickstart-only export, success stripping/internal-key removal, true observed
status and bounded parser. Useful nits adopted: exact string enum types, no
`absent` reason, removal of overwritten test assignment. Final test-only bound
uses two seconds for exit/signal cases; only the deliberate timeout uses 80 ms.
Reviewer independently passed 53 supervisor tests, static workflow 3/3 and Node
58/58, and probed ANSI/malformed/partial-line inputs without leakage. It explicitly
verified dynamic git-tree sourceDigest inclusion of helper+supervisor, exact
13-path regular-file guard and product-change refusal. Production candidate and
provider gates remain unchanged. Root's separate automation suite still needs
integrated validation; no hosted path was exercised by reviewer.

Author scoped checks pass supervisor discovery/direct 53/53 each, static workflow
3/3, Node 58/58, actionlint 1.7.7 and whitespace. Ripwire flags historical churn on
edited existing symbols, minor added lines and discovery-based test false dead
code; this bounded diagnostics change adds no runtime product behavior. Local
PTY tests execute only tiny disposable scripts, never Marmish, real accounts or
Keychains. This increment identifies future failure evidence; it does not fix or
claim success for the current hosted quickstart failure.

### Identity vocabulary and owned-root socket budget — bounded approval

Claude Code 2.1.301, `claude-sonnet-5-5`, session
`54caee6f-012b-4b13-b864-a89f699851db`, APPROVED this bounded increment with an
explicit open integration blocker; this is not full-phase acceptance. Actual
reviews: `/tmp/marmish-scj-provider-identity-code.json` and
`/tmp/marmish-scj-provider-identity-final.json`. Reviewer verified the exact
65-production-label vocabulary plus the single `corrupt` compatibility label,
18 static provider mappings, LF/CRLF/hostile/partial parser tests, disposable PTY
failure propagation, all100 reserved UIDs, actual private root creation and
unchanged product socket bounds. Useful nits adopted: consistent cfg(test) mod
boundary, three partial-line cases, and clearer unknown-token prose. Initial
review independently passed59 Python and58 Node tests, final review passed59
Python tests and whitespace. Scratch mutation checks made old prefix and a
renamed label fail tests; tracked source was unchanged by those checks.

Reviewer checked unchanged success schema and failure-only child reports,
dynamic sourceDigest inclusion of both helpers, unchanged13-path diagnostic
guard, and no product or provider-gate bypass. The source-proven fixture budget
violation does not establish the earlier hosted native error category. No
product, account or Keychain was executed locally.

Author checks: supervisor discovery/direct59/59 each, static workflow3/3,
Node58/58, actionlint1.7.7 and whitespace PASS. Ripwire quality-delta flags
historical short-horizon churn, unittest discovery false dead-code and test
verbosity; no product behavior or structural expansion was added.

The actual central Rust contract test FAILED (0 passed,1 failed), log
`/tmp/marmish-scj-provider-final-rust-contract.log`. The earlier diagnostic
ancestry added `owned_preferences_directory`, missing from Rust REPORT_FAILURES;
the fallback `owned_account_prerequisite_failed` remains reachable through a
nested conditional. Reviewer confirmed the current increment leaves the92-label
HEAD/working AST sets identical. Root coordinates a separately reviewed
installed-checks-only mapping and recursive-extraction correction; this worktree
does not edit Rust or expand the diagnostic guard. The combined-tree contract
must pass centrally before source freeze/full acceptance. A new hosted run is
also required to observe the actual native category and qualify the provider.
