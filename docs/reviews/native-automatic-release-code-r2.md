# Native automatic release code review, round 2

Reviewer: actual Claude Code `claude-sonnet-5-5`, permission mode `auto`.
Session: 93b9a5ca-7cf1-4b33-8701-ecfb90ad771d

**APPROVED.** I found no blockers. The approval covers the integrated tree and is limited to what I could check offline. I made no edits, commits, network calls, Cargo runs or publication. I read the full diff against HEAD and every untracked implementation file, test and doc you listed.

## What I ran
- **Node:** 113/113 pass.
- **Python:** the three automation suites pass: release-automation workflow, HTTPS check and provider-diagnostic workflow. The full `tools` discovery ran 850 tests.
- **Failures:** the full run shows 13 failures and errors, but the same 13 fail on a pristine `git archive HEAD` copy. They are the publish, installed-release, release and interop-workflow suites, plus the legacy managed-workflow fixture. The integration adds 4 tests and no new failures.
- **Not re-run:** `actionlint` isn't installed here, so I relied on your PASS for it.

## Prior blockers
- **B1, draft `untagged-*` URLs:** fixed. `publish.mjs:364-369` re-lists and revalidates the assets after the publish PATCH, then `verifyPublic` checks the final `download/v0.2.3/` URLs. Fixtures cover the draft and public URLs and a changed digest after publication (`publish.test.mjs:262-284`).
- **B2, rerun artifacts:** fixed. `release-candidate.mjs:117-120` filters to the exact `-RUN-ATTEMPT` suffix before the duplicate check, the count of 15 and the inventory. The fixture adds expired attempt-1 artifacts with bad digests and a duplicate. It confirms none of them are downloaded, and a duplicate within the selected attempt is still rejected.
- **3, caveated runs:** `evaluate` builds caveated manual candidates, and `inspectCandidate` refuses them with `eligibility_binding`.
- **4, raw master check:** `verifyMasterBootstrap` retries 8 times with a 350 s cap. It rechecks master before each read, and non-transient errors still throw.
- **7, transfer deadlines:** metadata requests keep 60 s. Raw or over-16 MiB transfers get 600 s.
- **Skipped-job coverage:** `checkSkippedJobs` requires each category to be collapsed or fully expanded, with no duplicates or strays. The tests cover malformed cases.
- **Scope narrowing (6):** production eligibility fails closed on an existing selected tag, because `verifyPublished` is never supplied. Recovery goes through the original candidate with `allowSelectedTag`. The code claims no unimplemented skip.

## Other checks
- **Diagnostic merge:** `checkRun` is dispatch-only when a diagnostic policy applies and requires the failed run. Without a policy it accepts push or dispatch. The diagnostic test rejects `push`.
- **Privileged import graph:** `publish`, `release-schema`, `release-handoff`, `release-render` and `eligibility` import only built-ins, and the publish job runs no npm.
- **Artifact bindings and assets:** `release-handoff.mjs` binds each artifact to the run, attempt, name, SHA and digest. The release has exactly 15 assets, and original artifacts are re-downloaded and compared byte for byte.
- **Publisher SHA:** it must equal SOURCE on the first run and the bootstrap commit on retry. Anything else fails with `publisher_not_source`.
- **Renderer and bootstrap:** the renderer job has `permissions: {}`, no checkout token and a scrubbed child environment. The bootstrap is a non-force, single-path, direct-child fast-forward, with a retry path through `completedBootstrap`.
- **Version and installer:** Cargo.toml and Cargo.lock are both 0.2.3, and `scripts/install.sh` is untouched at 0.2.2.

## Non-blocking notes
1. **Concurrency can drop a pending run.** Both workflows use `cancel-in-progress: false`, but GitHub replaces any previously pending run in a group. A queued bump push could be replaced by a later `Cargo.toml` push whose `before` is the bump commit, which gives `unchanged_version`. A manual dispatch at master head recovers it. Consider documenting this.
2. **Phase-doc wording is stale in three places.** `native-automatic-release.md` line 99 says "verified skip" where it means designed skip. Line 334 lists "already-published skips" as a test case, though that path is now unreachable. Line 80 says recovery avoids rerunning eligibility, but `inspect` recomputes it in recovery mode.
3. **Reruns are capped at about six attempts.** `transport.list` allows `total_count ≤ 100` across all attempts. It fails closed.
4. **Successful caveated dispatch runs will show a red publish run.** They fail `inspect` by design.
5. **Four live-only assumptions I couldn't check:** the `untagged-` URL form, `workflow_run` runs reporting `head_sha` equal to `GITHUB_SHA` and `head_branch` equal to `master`, the optional tag `tagger`, and `permissions: {}` renderer artifact download. Hosted qualification must exercise them, as the README and spec already say.


## Diagnostic ancestor integration review

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session: 7552e6c0-62aa-4b7f-a8cb-dfa22286fb50

**APPROVED** (offline integration approval only). I found no blocking findings.

The ancestor integration doesn't invalidate any candidate, schema, publisher or HTTPS-gating contract. It adds no product change and no qualification bypass.

## What changed since the r2 approval
- The r2 approval was recorded at 14:35. It already covered 5ccc646, which added the diagnostic workflow and the `workflow.mjs` and `workflow.test.mjs` changes.
- Only d7e31c1 and 2d9ecd2 came after it. Both touch only `tools/marmish_native_keychain_check.py`, its tests and diagnostics docs.
- Nothing under `crates/`, `scripts/`, `Cargo.*`, `.github/` or `tools/v02-actions/` changed after 5ccc646.

## Contract checks
- **Product:** there are no changes to the Rust crate, the installer or `Cargo.*`. `Cargo.toml` and `Cargo.lock` are still the dirty 0.2.3 values from the automation work, not from the ancestor.
- **Qualification bypass:** none.
  - Every gate in the supervisor is intact, and the new diagnostic code only labels stages and failures.
  - `prepare_preferences` creates `Library/Preferences` only after checking that the home is the allocated user's home, the provider root's parent, absolute, owned, `0700` and a real directory. Anything else raises `owned_preferences_directory`, a failure rather than a skip.
  - The child-failure fields come from closed allow-lists, otherwise they become `diagnostic_invalid`, and they are removed when `ok` is true.
- **Candidate schema:** `release-candidate.mjs` `checkReport` still requires an exact key set and `ok === true`.
  - Success reports carry no `child_*` keys, because they are removed on success. So the schema is unchanged and still strict.
  - A failure report carrying `child_*` keys is rejected twice, by the exact-key check and by `ok`.
- **Harness binding:** `sourceDigest` and `harness_sha256` are computed dynamically from `git ls-tree` over `SOURCE_PATHS`, which includes `marmish_native_keychain_check.py`. The ancestor's edits therefore change the bound digest at candidate time, as designed. There are no hard-coded harness digests to go stale.
- **Diagnostic policy:** the `61f4760` to `2d9ecd2` delta stays entirely inside `DIAGNOSTIC_PATHS`, so `checkDiagnosticChanges` is not tripped.
- **HTTPS gating:** `marmish_native_https_check.py` loads the supervisor only for `execute`, `Refused`, `require`, `stop` and `MAX_CAPTURE`. The ancestor changes none of those, and the only HTTPS-check change is the dirty `event in ('workflow_dispatch','push')` edit from the approved automation work.
- **Publisher:** `publish.mjs`, `release-*.mjs`, `eligibility.mjs` and `v02-publish.yml` don't reference the keychain check.

## Tests I ran (offline, no edits, network, Cargo or commits)
- **Node:** `node --test` in `tools/v02-actions` passed 113 of 113.
- **Python:** the four suites pass when run by `discover -s tools -p <file>`.
  - `test_marmish_release_automation_workflow`: 4 tests OK.
  - `test_marmish_native_https_check`: 24 tests OK.
  - `test_marmish_provider_diagnostic_workflow`: 3 tests OK.
  - `test_marmish_native_keychain_check`: 44 tests OK, matching the 44-test supervisor fixtures.

## Limitations
- The macOS supervisor is not exercised here. Real `/usr/bin/security`, the allocated uid, `Library/Preferences` behavior and keychain cleanup run only on a hosted macOS runner, so a hosted successful candidate is still mandatory.
- I didn't re-run `actionlint` (not installed) or the 13 pre-existing full-discovery failures that r2 found on a pristine tree.
- I couldn't confirm by file timestamps that the dirty automation files are unchanged since r2, because the worktree files all show 14:53:14. I compared by content instead. That showed no automation file in the ancestor range, but I didn't re-diff them against r2.
- Once the automation files are committed, the source delta from `61f4760` leaves `DIAGNOSTIC_PATHS`, so a diagnostic-policy candidate fails closed. That is expected, and a normal hosted candidate is required anyway.

## Installed-provider finite-label amendment: design review 1

Actual Claude Code, permission mode `auto`, session
`2e462323-7711-49da-82b3-2848aeebaf7e`: **NOT APPROVED** before Rust edits.
The reviewer found that Python's nested conditional fallback still emits
`owned_account_prerequisite_failed`; the AST extractor only read a direct
constant `orelse`, so the apparent removal was an extractor blind spot.
`owned_preferences_directory` correctly belongs to `PrerequisiteFailed`.
Unknown-label rejection and independent cleanup facts remain appropriate.
No Rust was changed and no Cargo/live tests ran for this rejected proposal.
The amendment must retain the reachable fallback and strengthen nested-branch
extraction, or independently change the Python source contract, before approval.


## Installed-provider finite-label amendment: corrected design review

Actual Claude Code, permission mode `auto`, session
`908a53a3-19d1-41b3-bc68-86c9342c2dbf`: **APPROVED** before Rust edits.
The corrected scope retains the reachable prerequisite fallback, adds the owned
preferences prerequisite, and strengthens only the embedded AST helper to
extract both nested conditional branches. The reviewer verified actual Python
emissions and Rust mapping/parser behavior; exact parity, uniqueness, unknown
refusal and per-label cleanup facts remain mandatory. No Python/runtime/gate
changes were authorized or made.


## Installed-provider finite-label code review

Actual Claude Code, permission mode `auto`, session
`25d7702f-07c0-41fb-8407-bcd62ab638e6`.

**APPROVED**

**Scope reviewed**
- `git diff` of `crates/marmish-native/src/installed_lifecycle_provider.rs` only, which has three hunks.
- The final amendment in `docs/phases/native-automatic-release.md:374`.
- The Python label emissions in `tools/marmish_native_keychain_check.py`, at lines 61, 92, 111-112, 344-362 and 952-963.
- The existing parser and tests in `installed_lifecycle_provider.rs`.

**Findings:** none.

**What I checked**
- **New mapping:**
  - `owned_preferences_directory` maps to `PrerequisiteFailed`.
  - Python emits it via `require(..., 'owned_preferences_directory')` at lines 344-360 and `Refused('owned_preferences_directory')` at line 362. It is also in `CHILD_FAILURES` at line 61.
  - It appears once in `REPORT_FAILURES`, and no existing mapping was changed.
- **Retained fallback:** `owned_account_prerequisite_failed` is still in `REPORT_FAILURES` at line 287. It is still reachable in Python at line 112, as the non-cleanup fallback of the nested conditional. The earlier removal proposal was correctly dropped.
- **AST helper:**
  - `add()` now recurses only through `IfExp.body` and `IfExp.orelse`.
  - The `category` assignment now passes the whole `IfExp` instead of only `orelse`. That is what extracts both fallbacks.
  - The change is purely additive to the extracted set, and the test still only parses syntax with `python3 -I`.
- **New assertion:** the test now requires that the extracted set contains both `owned_account_prerequisite_failed` and `owned_account_cleanup_incomplete`. This runs before the exact `python == rust` comparison, so it cannot hide a mismatch.
- **Unchanged checks:**
  - Exact set parity is still asserted.
  - Uniqueness is still asserted, because the set length must equal the `REPORT_FAILURES` length.
  - Every label is still round-tripped through `outcome()`, and each is asserted to give `!passed`, the mapped failure, and `cleanup_complete == true`.
  - `report_failure("unrecognized_provider_error")` is still an error.
- **Other tests:** the neighbouring test `failed_report_preserves_finite_category_and_independent_cleanup_facts` is untouched. It still covers independent cleanup facts and arbitrary-failure rejection. Neither the parser nor the runtime logic changed. The only non-test change is one added table row.

**Independent rerun**
I ran `CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target cargo test -p marmish-native --all-features --lib --locked lifecycle_provider::tests`. All 9 tests passed, with 0 failed and 474 filtered out. That includes `supervisor_failure_labels_match_python_ast_and_retain_cleanup`. I ran nothing broader, made no edits or commits, and used no live account, process or network tests.

**Limitation:** these are Linux unit tests of label parity and parsing only. They say nothing about live macOS success.


## Final diagnostic 699f53c and automation integration review

Actual Claude Code in `--permission-mode auto`, session
`8c573880-8c4e-42be-90c1-dc295e6bb58b`. The following actual bounded approval
is conditional on the root full native test run passing; that run was still
in progress during this review. Hosted diagnostic evidence is recorded
separately and was not independently inspected by this reviewer.

**APPROVED**, with the scope and limits below.

**Scope reviewed:** `git show 699f53c` and the working-tree diffs against it. That covers `tools/marmish_native_keychain_check.py`, `tools/marmish_native_quickstart_check.py`, `tools/test_marmish_native_keychain_check.py`, and the `installed_lifecycle_provider.rs` and `Cargo.toml` changes. I also read the consumers: `sanitize_provider_report`, `sanitize_child_diagnostics`, `report_failure()` and the strict success schema in `tools/v02-actions/release-candidate.mjs`.

**Findings**
- **AST label parity:** The Rust test's Python snippet now recurses through nested `IfExp` via `add()`, and the `category` assignment adds the whole expression. Before, it added `.orelse`, which was itself an `IfExp` and extracted nothing, so the retained fallback assertion was needed. I ran an exact copy of the snippet against the Python supervisor source and compared it with a regex parse of `REPORT_FAILURES`. Both sides have 93 labels, with no labels on one side only.
- **Fallback labels:** `owned_account_prerequisite_failed` maps to `PrerequisiteFailed` and `owned_account_cleanup_incomplete` to `CleanupIncomplete`. The new `python.contains(...)` assertion is no longer vacuous, because the nested extraction now yields both labels. The same extraction now also yields `provider_cleanup_incomplete` (`CleanupIncomplete`) and `provider_qualification_failed` (`QualificationFailed`). Both are present in the Rust table.
- **`owned_preferences_directory`:** It is raised only by `Refused` and `require` in `prepare_preferences`, and is now in the Rust table as `PrerequisiteFailed`. It is also in the Python `CHILD_FAILURES` set, which is finite and unchanged by this commit. Unknown values fall to `diagnostic_invalid`, and `report_failure()` returns `bad()` for any label not in the table.
- **Identity vocabulary:** `INTERACTIVE_IDENTITY` is the exact set of source-derived labels plus `corrupt`, and `INTERACTIVE_NATIVE` is built as `identity_` plus each label. The 59-test supervisor suite asserts 65 literals against the production Rust source. It also rejects unknown, hostile and partial lines (`SENTINEL`, ESC, NUL, truncated labels), and checks that a real PTY line survives the child and outer sanitizers.
- **Cleanup facts:** `699f53c` does not touch the cleanup logic. Setup failures go to `failure`, and cleanup failures go to `cleanup_failure` plus `cleanup_failure_stage`. `ok` still requires `owned_account_removed` and `runner_keychain_unchanged`. Cleanup success cannot mask a setup failure, and a cleanup failure cannot be hidden by setup success.
- **Socket budget:** The `.mp-` prefix keeps the longest owned runtime socket path at 91 bytes (`mr2-<uid>-<32 hex>` plus 10), below the 104-byte bound. The old `.marmish-provider-` prefix gave 105. The tests cover this for all reserved UIDs, check that the actual root is created with mode `0700` directly under HOME, and bind the Rust path checks in `identity.rs`/`ipc.rs` and the installer's `TMPDIR` contract.
- **Release candidate:** `release-candidate.mjs` is not modified by `699f53c`. Its strict provider success schema still requires an exact key set with every qualification flag set to `true`, plus matching source commit and target. Failure and diagnostic keys (`child_*`, `failure*`) are not in the allowed set. Those keys are also stripped when `ok` is true, so a diagnostic cannot satisfy or bypass qualification.

**Checks actually run (offline, no cargo)**

| Suite | Result |
|---|---|
| Keychain supervisor (`test_marmish_native_keychain_check`) | 59 OK |
| HTTPS check (`test_marmish_native_https_check`) | 24 OK |
| Release workflow (`test_marmish_release_automation_workflow`) | 4 OK |
| Node (`tools/v02-actions`) | 113 of 113 pass |
| Python AST copy vs Rust table | 93 vs 93 labels, no difference |

Running the Python tests with `-I -m unittest tools.…` errored on import paths, so I reran them from `tools/` without `-I`.

**Limits**
- I did not run any cargo, so the Rust test `supervisor_failure_labels_match_python_ast_and_retain_cleanup` and the full 483 native tests are not covered by my review. I only reproduced its snippet and table comparison in Python. The root run of the 483 tests is the authority for the Rust side.
- Nothing here is live qualification. I did not run the macOS disposable-account provider, keychain, PTY on a real macOS runner, or any network or publication step. The socket-budget and `.mp-` claims are arithmetic and source-text checks. Whether the real macOS provider run actually succeeds still has to come from the hosted run.
- The tree is uncommitted automation work on top of `699f53c`, and I made no edits.

**Condition on the approval:** the root's 483 native tests, including that label-parity test, must pass.
