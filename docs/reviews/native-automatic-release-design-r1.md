# Independent Claude design review

Reviewer: Claude Code, claude-sonnet-5-5, `--permission-mode auto`.
Session: `8e54b5cb-e87f-4640-821d-9ea34c66bac3`. Read-only review; no implementation approval is inferred beyond the explicit verdict.

**Verdict: not approved.** There are seven blocking findings, listed below. I read `native-automatic-release.md`, `v02-release.yml`, `tools/v02-actions/workflow.mjs`, `marmish_native_https_check.py`, `v02-installer-https.yml` and `v0.2.3-publication.md`. I made no edits and ran no live processes. The overall shape is sound and I'd keep it: static publisher, a separate `workflow_run` workflow, fail-closed recovery, and bootstrap as SOURCE's direct child. The gaps are in trust boundaries and liveness.

## Blocking findings

1. **The publisher's trusted code ref is unspecified.**
   - A `workflow_run` workflow runs the default-branch definition, so `GITHUB_SHA` is master HEAD at event time. That is not the candidate SOURCE.
   - The doc says "read versions from SOURCE" and "reuse tools/v02-actions". It never says which ref the write job checks out or executes.
   - If the write job runs Node from SOURCE, SOURCE-controlled code runs with `contents:write`. For manual candidates it could also be an older publisher.
   - **Change:** the write job runs only code from the publisher run's own `GITHUB_SHA`. That SHA is already master==SOURCE at mutation time, and the job must assert so before the first write. The job reads SOURCE only as data through git/API and never executes anything from SOURCE.
   - Install no dependencies in the write job. If `tools/v02-actions` needs npm packages, use `npm ci --ignore-scripts` in a token-less step, or require zero dependencies.
   - Also note that `candidateInputs` (`workflow.mjs:29`) requires `TEST_RELAY` (wss) and `SOURCE_COMMIT === GITHUB_SHA`. Reuse `checkRun`, transport and archive code, but not `candidateInputs` as-is.

2. **The read-only gate both executes the candidate renderer and decides trust.**
   - A read token is still a token. The executed tool can read the process environment, `GITHUB_OUTPUT`/`GITHUB_ENV` file paths, `ACTIONS_RUNTIME_TOKEN` and the workspace. That lets it forge the gate's verdict or inventory.
   - **Change:** run `marmish-release-tool render` in its own job. That job has `permissions: {}` and no `GITHUB_TOKEN`. It downloads only already-hashed inputs and emits only the rendered script's SHA256 and bytes. The trust-deciding job compares those to the candidate bootstrap and never executes artifacts.
   - Use a scrubbed child environment inside that job as well.
   - State why the candidate-built tool is trusted at all. Re-rendering with the same binary only proves reproducibility. The stronger option is to build the renderer from SOURCE in that token-less job. Choose one and record it.

3. **Eligibility by first parent breaks on multi-commit pushes.**
   - Rebase-merge or fast-forward pushes can put the version bump in a non-tip commit. Tip equals its first parent, so the release is silently skipped.
   - **Change:** compare the version at `github.sha` against `github.event.before`. Verify `before` is an ancestor of the head, and handle the all-zero `before` of a new branch or force push. Reject two bumps in one push.
   - "Advance semantically" is not defined. Compare against the highest existing release or tag, not only the previous commit. Otherwise a stale 0.2.3 bump merged after 0.2.5 is "advancing" yet behind. Specify the ordering function, including `-rc.N < final`, and test it.

4. **The skip path can't be distinguished from failure.**
   - If package jobs are `needs: eligibility` and skipped, the candidate run concludes `success`. `workflow_run` then fires the publisher on every skipped Cargo.toml-path push.
   - The doc's "reject skipped required jobs" rule would make each of those red.
   - **Change:** make the eligibility job's output a closed tri-state (`build`, `skip-by-design`, `ineligible`). The publisher no-ops cleanly only on a verified `skip-by-design`, read through the API from the exact job. Skipped jobs from any other cause still fail closed.
   - Also add a job-level `if` on the publisher for `conclusion == 'success'`.

5. **Master-race liveness and bootstrap push permissions are unresolved.**
   - Candidate and publish take tens of minutes. Any unrelated merge in that window makes `master != SOURCE`. The doc then says to plan a new source and version.
   - That contradicts the doc's own manual route. A manual dispatch at the new head with the same unpublished version is allowed and should be the stated recovery. Reconcile the two.
   - Define the policy: a release-in-flight merge freeze (documented, or a ruleset), plus recording the outcome as "no tag created, version still free".
   - The doc also assumes `GITHUB_TOKEN` can push a non-PR commit to master. If master has PR-required rules, required checks or a ruleset, that push is rejected. A `GITHUB_TOKEN` push also triggers no checks. Decide how the bootstrap lands (confirmed bypass for `github-actions`, or another route) before approval. I did not query live repository settings.

6. **Wiring `push` into `v02-release.yml` is under-specified.**
   - The workflow uses `inputs.source_commit` in `MARMISH_BUILD_COMMIT`, the checkout `ref` in all four checkout jobs, `run-name`, the provider job env and the caveated conditions. On push these are empty, and the `^[0-9a-f]{40}$` check fails.
   - **Change:** list one `SOURCE=${{ inputs.source_commit || github.sha }}` binding used everywhere. On push, require `SOURCE == github.sha`. Add a test that no job reads `inputs.*` without that fallback.
   - Candidate event validation is hard-coded in at least `workflow.mjs:45` and `marmish_native_https_check.py:158`, plus their tests. The doc mentions this in general terms. Name both files and require updating them together.

7. **There is no defined integrity channel between the gate and the write job.**
   - The doc says the inventory is "carried", but not how. The write job re-downloads candidate artifacts after the gate's check, which is a TOCTOU window unless it re-hashes against a trusted inventory.
   - **Change:** the inventory (15 names, sizes, SHA256, plus the bootstrap digest and artifact IDs and digests) lives in a canonical JSON document. Pass its SHA256 as a gate job output, which is size-bounded and trusted. The write job fetches the document and the artifacts, then checks both against that digest and the inventory.
   - Don't upload a second mutable "verified" artifact as the source of truth.
   - Record each artifact's `digest` field at gate time and re-check it at write time.

## Non-blocking recommendations

- Put the write job in a GitHub Environment (`release`) restricted to master. That limits who can use the recovery dispatch and gives a durable audit record.
- GitHub keeps only one pending run per concurrency group and cancels older pending runs. This affects candidates (a middle versioned merge is silently dropped) and publishers (a lost `workflow_run` event). Say so, and make the summary list unreleased versions.
- A partial asset upload can leave an asset not in the uploaded state on a draft. The "never delete" rule then blocks forever. Allow deleting only non-uploaded or incorrect assets on a draft, never a public release, under an explicit recovery input.
- The tag is created before the draft is verified. A permanent verification failure burns the version. State that, and that the next release needs a new version.
- `verify-tag` is a `gh` CLI flag, not a REST field. Say whether the adapter uses REST (consistent with injectable Node adapters) or `gh`. Set `prerelease: true` and `make_latest: false` explicitly.
- Add these tests:
  - static workflow assertions (no `pull_request*` triggers, no `${{ }}` event values in `run:`, actions pinned by SHA, permissions per job, write token present only in the write job);
  - multi-commit, force-push and merge/squash/rebase eligibility;
  - the tri-state skip;
  - scrubbed renderer environment;
  - the digest-bound inventory handoff;
  - master==SOURCE re-check immediately before the tag, release publish and cutover push.

## What is already good

- Treating `workflow_run` payloads as untrusted and re-querying run/attempt by API.
- Exact workflow path, repository, branch and head-SHA binding.
- The exact 15-file inventory, no-clobber, no-delete, no-force rules, and idempotent draft recovery.
- Direct-child, single-path bootstrap, and the `GITHUB_TOKEN` no-retrigger plus path-filter loop guard.
- The decision to keep Radicle out of CI.
- Keeping v0.2.3 manual first, with automation after the bootstrap boundary.

I won't record any approval until the design is revised and re-reviewed. Beads `marmish-scj` should remain blocked on design approval.
