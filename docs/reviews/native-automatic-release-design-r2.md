# Independent Claude automation and publication design review, round2

Reviewer: Claude Code claude-sonnet-5-5, `--permission-mode auto`.
Session: `ffc3f5cf-1c6a-4477-bde6-140e9be751e2`.

## Round 2 verdicts

I made no edits and ran no Cargo or live tests. I read the three revised designs and the r1 reviews. I checked them against `v02-release.yml`, `tools/v02-actions/workflow.mjs`, `marmish_native_https_check.py`, the local tag list and the supervisor at HEAD. Everything below is design-level. The phased future evidence (hosted proof, HTTPS, installed 0.2.3) is correctly not claimed as passed anywhere.

### 1. `native-provider-diagnostics.md`: APPROVED

B1–B6 from r1 are resolved. The contracts match what I checked:
- **Product-source binding:** `checkRun` now requires exact `head_sha` equality under a literal diagnostic policy. The production defaults (`success`, `SOURCE_COMMIT == GITHUB_SHA`) stay unchanged.
- **Jobs:** the five job IDs and names are pinned, and the closed `STAGES` set plus the `int` and -128..255 bounds are specified.
- **Supervisor:** it has no HEAD-versus-`--source-commit` check, so running the new supervisor against the older product assets works.

None of these notes blocks approval:
1. **Allowlist trap.** The 12-path allowlist does not include `native-automatic-release.md`, `v0.2.3-publication.md`, or their reviews. Those are untracked in this worktree. If any of them lands on master before the diagnostic dispatch, the audit will refuse to run. The diagnostic commit and any commit before dispatch must contain allowlisted paths only.
2. **Artifact expiry.** The two source artifacts expire on 2026-11-18, and the design has no fallback after that. Review, publish and dispatch must finish before then.
3. **Token scope.** The `actions: read` token should reach only the resolver and download steps. It must not be in the environment of the step that runs `sudo` and the supervisor. Add that to the static workflow contract.
4. **Existing diff.** The worktree already has uncommitted edits to `workflow.mjs` and the supervisor, though the brief says no implementation exists. I did not review them. They match the design's literals, including `PROVIDER_DIAGNOSTIC` and `DIAGNOSTIC_PATHS`.
5. I agree with the three round-2 notes in `native-provider-diagnostics-design-r2.md`: sanitize at the rewrite, leave the stage absent for failures before any stage is set, and add `workflow.mjs` to the allowlist test.

### 2. `native-automatic-release.md`: NOT APPROVED, two small blocking items

r1 findings 1–7 are resolved:
- **Trusted code ref:** the write job runs only code from its own `GITHUB_SHA`.
- **Renderer isolation:** the renderer runs in a tokenless job with a scrubbed environment, and the reviewed-source trust premise is stated honestly.
- **Eligibility:** it compares against `event.before` and uses the tri-state result.
- **Source binding:** one `inputs.source_commit || github.sha` binding is used everywhere, and both validators are named.
- **Handoff:** artifacts are passed by exact ID, and a digest job output carries the trust.
- **Bootstrap push and master races:** live branch protection and rulesets are queried and recorded.

The candidate workflow already uploads `v02-render-tool-RUN-ATTEMPT`, so the renderer handoff needs no change to the candidate workflow.

**Blocking**

- **B1. Draft releases are invisible to the read-only jobs.**
  - The design says eligibility and inspect enumerate "all repository tags and releases, including drafts" with bounded complete pagination.
  - Those jobs run with `contents: read`. GitHub's list-releases endpoint returns drafts only to callers with push access, so the listing is silently incomplete.
  - A draft `v0.2.4`, or a draft for the selected version, would not be seen.
  - Remedy:
    - Say eligibility and inspect see tags and published releases only.
    - Make the publish job (`contents: write`) the authority on drafts. It lists them by bounded pagination, and refuses on any draft at or above the selected version that is not the exact resumable match. Do this before the first tag write.
    - Add a test where the read-token fixture hides a draft and publish still refuses.
- **B2. "Matching native" version is undefined, and the repo has non-native tags.**
  - Existing tags include `v0.1.0-experimental.1` and `v0.1.0-experimental.2`.
  - The design fails closed on "unsupported or ambiguous matching native release state". Read literally, those tags would block every release forever.
  - Remedy: define matching as the anchored pattern `^v0\.2\.[0-9]+(-rc\.[0-9]+)?$`. Ignore non-matching tags explicitly. Fail closed only on matching tags whose release or tag state is inconsistent. Add a test with the two experimental tags present.

Once those two sentences and tests are in the design, I would approve without another full round.

**Non-blocking**
- `before` is attested by the eligibility job, and that job runs SOURCE code. This is acceptable because the publisher recomputes ancestry and transitions, and anyone who can push master can already release. Say so in one line.
- Build the bootstrap commit with git plumbing (`commit-tree` over SOURCE's tree plus the rendered blob). Avoid a SOURCE worktree checkout in the write job, so no checkout-time filters can run.
- Candidate artifacts live 14 days, so retry after that means a new release. State it.
- A fast-forward push containing a "merge master into feature" commit puts `before` off the first-parent chain and is conservatively ineligible. The manual full-candidate route covers it. Document it.

### 3. `v0.2.3-publication.md`: APPROVED, conditional on automation approval

Both r1 blockers are resolved:
- **B1:** the automation design now exists at the linked path.
- **B2:** no master commit lands until the HTTPS run completes or a guard-blocked result is recorded.

The six r1 edits are done: release-notes timing, the Radicle and Iris completion rule, the inherited-step list, the SOURCE descends-from-master and no-tag preflight, the version-pin audit, and the review record. Phase 3 execution cannot start until the automation design is approved and code-reviewed. Phases 1–2 have no such dependency.

Required wording, none of it blocking:
1. **Operator abstention.** The "Release contract" section still describes the manual tag, draft, upload and bootstrap steps in the imperative. Add: while the automatic publisher is running, the operator performs no release mutation. Manual recovery starts only after that publisher run is complete or failed and its state is recorded. This prevents racing the publisher on the tag.
2. **Diagnostic commit scope.** State that the diagnostic ancestor contains only `DIAGNOSTIC_PATHS`, and that the automation and publication docs enter master only after the diagnostic dispatch completes (see item 1 of the diagnostics notes).
3. **macOS stop condition.** The design has no automation-eligible caveat path. If the diagnostic shows a macOS provider failure that tooling cannot fix, stop and escalate to the user rather than designing around it. Say that.
4. **Stale status lines.** The `Status: … approval required` lines carry into the immutable SOURCE. Update them to the recorded approvals before freeze.

I saved nothing to memory and left the review files for you to write.
