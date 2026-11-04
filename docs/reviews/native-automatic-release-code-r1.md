# Native automatic release code review, round 1

Reviewer: actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session: 1a8142c9-a2cb-4ec8-8092-9a0291f14320

**NOT APPROVED.** I found two blockers and a few lesser issues. I read every file you listed and the full diff against HEAD, and re-ran the offline suites. Node passes 100/100, and Python passes 4 workflow tests plus 24 HTTPS-check tests. `actionlint` isn't installed in this environment, so I couldn't re-run it. None of the findings below were exercised against live GitHub.

## Blockers

**1. `publish.mjs` verifies public assets from a stale pre-publish listing** (`tools/v02-actions/publish.mjs`, the `finalAssets` call in the draft block and `verifyPublic` after it).
- `assets` is assigned from `finalAssets(...)` while the release is still a draft. After the `PATCH` to published, `verifyPublic(assets, …)` still uses that listing. It requires `asset.browser_download_url === https://github.com/Epiphytic/marmish/releases/download/v<ver>/<name>`.
- As I remember GitHub's behaviour, draft-release assets carry `…/releases/download/untagged-<hex>/<name>` URLs, and the URLs only change on publish. I couldn't check this offline. If I'm right, the first automatic run fails with `invalid_public_url` after the release is already public and before the bootstrap cutover.
- `publish.test.mjs:38` hard-codes the final `v0.2.3` URL in its fake, so the fixture cannot catch this.
- Recovery by dispatch would work, but that is not the automatic path the design requires.
- Fix: re-list the assets after the `PATCH`, or build the expected URL deterministically and ignore the API field. Add a fixture that returns `untagged-…` URLs while the release is a draft.

**2. A candidate re-run (attempt ≥ 2) can never be published** (`release-candidate.mjs`, the `artifacts.length === names.length` check and the `artifacts.map(...)` inventory).
- `transport.list` calls `/actions/runs/{id}/artifacts`, which returns artifacts from every attempt. The workflow itself says "Re-run all jobs together: artifact bindings include the run attempt", which is why artifact names carry the attempt suffix.
- After a full re-run the list holds attempt 1's artifacts as well as attempt 2's. That is 30 artifacts, not 15, so inspect fails with `candidate_artifact_inventory`.
- Even if that check were loosened, `inventory.artifacts` would then include foreign-attempt artifacts. `canonicalInventory` caps it at 20, and `verifyOriginalArtifacts` would download all of them.
- The design's recovery dispatch is explicitly "naming run/attempt". `release-candidate.test.mjs` uses attempt 2 but only with attempt-2 artifacts, so this is untested.
- Fix: filter artifacts to the exact `-RUN-ATTEMPT` suffix before the count, the duplicate check and the inventory. Add a fixture with extra attempt-1 artifacts.

## Medium and low issues

3. **Caveated troubleshooting dispatch is dead** (`eligibility.mjs`, `evaluate`). The design says to preserve manual caveated candidates for troubleshooting, but they are now `ineligible`/`caveated_candidate` and fail the job. `eligibility.test.mjs:122-131` asserts that behaviour, and the workflow's caveated steps are now unreachable. A `build` decision with `caveated=true` would have kept them, since inspect already rejects `caveated !== false`. Either change this or amend the design.
4. **No retry on the raw `master` bootstrap check** (`publish.mjs`, `public_master_bootstrap_mismatch`). `raw.githubusercontent.com/.../master/scripts/install.sh` is CDN-cached for several minutes and is likely warm because users fetch it. The mismatch is fatal and un-retried, and it fires after the cutover push. The state is recoverable (retry sees `priorBootstrap`), but it will likely flag a healthy release as failed. Add a bounded retry or backoff for this check only.
5. **Intermediate commits with a manifest/lock mismatch make a multi-commit push ineligible** (`eligibility.mjs`, `progression` → `versionAt`). If a Cargo.toml bump and its Cargo.lock bump land in different commits of one push, `invalid_version` fails the whole candidate. Your integrated SOURCE bumps both together, so it isn't affected. Document it, or tolerate intermediate mismatches.
6. **A repeat run of an already-published version is `ineligible`, not a skip** (`eligibility.mjs`, `selectedState`). `already_published` is unreachable because `main()` supplies no `verifyPublished` adapter, so any existing matching tag gives `existing_version_conflict`. That fails closed, but the design says verified skip.
7. **60-second deadline on large transfers** (`publish.mjs`, `requestOptions` uses `AbortSignal.timeout(60_000)`). It applies to artifact zips up to 1 GiB and to asset uploads, and POST uploads are never retried. This is probably fine for these asset sizes but fragile.
8. **Tag creation sends no `tagger`.** I believe the API fills it in for the Actions token, but I'm not certain.

## Checked and found sound
- **Event validators:** the JS and Python candidate validators accept exactly push and dispatch, with the negative event tests present.
- **SOURCE binding:** `inputs.source_commit || github.sha` is used at all four checkouts, the build env and the provider env. Eligibility enforces SOURCE equal to `GITHUB_SHA`.
- **Version trigger:** the push path filter plus `needs`/`if` gating work as designed. `versionAt` parses the real Cargo.toml and Cargo.lock as 0.2.3 (I ran it), and bootstrap `install.sh` stays 0.2.2.
- **Privileged import graph:** `publish`, `release-schema`, `release-handoff`, `release-render` and `eligibility` import only built-ins. The publish job runs no npm.
- **Renderer isolation:** the renderer job has `permissions: {}`, uses an unauthenticated fetch, and runs the child with a scrubbed environment.
- **Publish ordering:** all mutations are preceded by validation. Master is rechecked before the tag, the draft, the publish and the bootstrap. The inventory digest, marker and ZIP parsing (CRC, bounds, symlink and duplicate handling) look sound. The bootstrap is a non-force fast-forward of a single-path direct child.
- **Skip path:** it reaches a successful no-op, because the skipped `reproduce-bootstrap` skips `verify` and `publish`.

I made no edits and ran no Cargo, account or relay commands.


## Disposition before round 2

B1 fixed: re-list and revalidate assets after publication, with realistic draft
`untagged-*` URL coverage and changed post-publication digest refusal.
B2 fixed: select exact run/attempt suffix before inventory validation; prior
attempt artifacts cannot enter the inventory or invalidate a good rerun.

3 fixed: caveated manual candidates can build for diagnosis; publisher refuses them.
4 fixed: bounded raw-master CDN retries, with master rechecks and explicit
exhaustion state. 5 documented: each inspected intermediate commit must keep
manifest/lock consistent. 6 narrowed explicitly in the phase design: production
already-published candidate eligibility fails closed, and immutable publication
retry uses the original candidate through the publisher. No incomplete verified
skip is claimed. 7 increased large-transfer deadlines, retaining bounded metadata
requests. 8 official GitHub tag-object API documents tagger as optional; unchanged.

Diagnostic ancestor d39a912 is now integrated. Conflict resolution preserves
fixed failed-run/dispatch-only diagnostic policy and permits production push
candidates. Skipped matrix jobs require exact category coverage, each category
collapsed or all three expected cells.

Integrated offline checks: 113 Node tests, 24 HTTPS Python tests, 4 automation
workflow tests, 31 provider supervisor tests and 3 diagnostic workflow tests
PASS. Actionlint 1.7.7 passes all three affected workflows. No live publication
or candidate qualification is inferred from these results.
