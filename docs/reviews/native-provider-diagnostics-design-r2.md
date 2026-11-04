# Independent Claude provider diagnostics design review, round2

Reviewer: Claude Code claude-sonnet-5-5, `--permission-mode auto`.
Session: `d2b23b5e-0a22-4816-977c-7626cdbf1810`.

**APPROVED.** The revised design covers B1–B6, and I found no remaining blocking defect. I made no edits and ran no Cargo or live tests. I did make read-only `gh api` calls and git checks against the real run to confirm the contracts.

**Checked against the real contracts**
- **B1 (product-source binding):** Run `41898839617` attempt 1 has `head_sha == 61f47606…`, conclusion `failure`, path `v02-release.yml`, event `workflow_dispatch` and branch `master`. Requiring exact `head_sha` equality is therefore achievable and not merely 40-hex. The release workflow checks out `inputs.source_commit` explicitly, so `head_sha` equality is a real extra binding here and a mismatch would refuse. The manifest build commit (`MARMISH_BUILD_COMMIT`, `native_manifest.rs:57`) gives a second binding.
- **B2 (jobs and artifacts):** The five listed job IDs, full names and conclusions match the API output exactly, and there are no duplicates. The two named artifacts exist and are unexpired until 2026-11-18. The doc requires reusing and parameterizing `candidateInputs`, `checkRun` and `validateCandidate`, and keeping the production defaults. Those defaults currently require `conclusion === 'success'` and `SOURCE_COMMIT === GITHUB_SHA`, which only the diagnostic policy may relax.
- **B3 (source separation):** The product commit is an ancestor of `origin/master`. The audit is specified as an `A`/`M`, `100644 blob`, literal 12-path allowlist. It excludes `crates/` and `marmish_native_quickstart_check.py`.
- **B4 (report envelope):** The supervisor invocation stays unchanged. A separate envelope carries the diagnostic fields and is initialized before the guards. The `v02-provider-diagnostic-` artifact name cannot collide with the production `v02-provider-<target>-<run>-<attempt>` pattern. Uploads use explicit paths, and a missing report cannot mask a guard failure.
- **B5 (sanitization):**
  - **Stages and exit codes:** `STAGES` is a closed set, and `exit_code` requires `type is int` within -128..255. The `administrative()` calls and the non-`execute` steps (UID parse at `:677`, identity check at `:701`, report parse at `:730`, interpreter checks at `:711`/`:740`) each have a stage.
  - **Failure handling:** Both except branches attach the stage. Cleanup fields use `setdefault`. A failing `stop()` leaves the stage with no exit code.
- **B6 (test harness):** The stage-per-failure harness, the real `/bin/sh -c 'exit 7'` case, the sentinel checks and the red-stays-red cases are all specified, and the harness patches the right seams.

**Non-blocking notes for implementation**
1. The main block re-reads and rewrites the report at `:825-846`, and its `except Exception` fallback at `:820` writes a bare report. Run the sanitizer in `mac_account` right before its write at `:778`, as specified. Also make sure the main rewrite cannot reintroduce unsanitized fields. It copies `value` from the sanitized file, so it should be fine, but add a test for it.
2. `ProviderSignals.__enter__` and `lifecycle_arguments`, at `:657` and `:672`, run before any stage is set. Failures there should report an absent stage, not `diagnostic_invalid`.
3. Add `workflow.mjs` and its tests to the changed-path allowlist test, since `workflow.mjs` and the supervisor are inputs to `sourceDigest`, so this increment changes the harness hash. The doc already says new release assets need their own qualification.

### Installer utility discovery amendment — actual independent review

Claude Code 2.1.301, model `claude-sonnet-5-5`, session
`1f675bdf-369a-4d88-ae4a-e3e0d7b4bd07`, APPROVED the amendment before code.
Actual JSON: `/tmp/marmish-scj-provider-discovery-design.json`. Reviewer inspected
the finite source mismatch, saved Apple build definition and actual hosted report;
no blockers. Useful nits adopted: explicit module constant, recording fake lookup,
inherited decoy PATH, exact symlink subset and missing utility/Python checks.
Sibling quickstart/HTTPS lookup loops remain outside this bounded fix; they run
under different environments. Offline evidence establishes discovery hygiene;
complete hosted qualification remains required.

### Quickstart PTY diagnostics — actual design approval

Claude Code actual session `5ad4c7aa-ccf8-49d4-958d-12fb566e18c1` APPROVED with
binding conditions in `/tmp/marmish-scj-provider-pty-design.json`. All five pins
are recorded in the phase amendment before code: internal-key stripping,
quickstart-only first-failure export, nonthrowing bounded byte parser, explicit
source literals and true observed wait-status/reason rules. Only shared CI helper
is added to the literal guard (13 paths); tests stay in existing supervisor file.
No product fix or hosted success is inferred.
