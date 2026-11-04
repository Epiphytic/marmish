# Gated native GitHub release automation

Beads: `marmish-scj`. Status: design APPROVED by actual Claude rounds2–3; integrated offline code review APPROVED in round2. Hosted candidate/publication acceptance remains required.
See [review](../reviews/native-automatic-release-design-r3.md). The user authorizes a new release and appropriate
releases following versioned mainline merges. The latest constraint excludes
Radicle publication from GitHub Actions: binaries remain GitHub release assets;
source/tag mirroring and Iris synchronization remain manual agent duties. CI gets
no Radicle credentials, commands, release assets or replication gate.

## Phases and exit criteria

1. Finish and publish the diagnostic-only ancestor increment while the native
   version remains 0.2.2. Review this automation design with Claude before code.
   The 0.2.3 SOURCE has not been frozen: do not perform a separate manual-first
   release or claim an earlier candidate validates this automation.
2. Implement automation in its isolated worktree with bounded agent ownership,
   offline tests and integrated Claude code review. Integrate approved automation,
   the separately reviewed provider fix and explicit 0.2.3 version bump. Review
   the integrated result, freeze SOURCE and push it to master. That push must
   run the candidate and automatic publisher for the actual 0.2.3 release.
3. Keep master unchanged during candidate/publication, except the publisher's
   exact direct-child BOOTSTRAP. Verify all platform candidate gates, immutable
   public assets and bootstrap. Manually mirror source/tag to Radicle/Iris, then
   run the existing six-cell HTTPS installation qualification. Keep master frozen
   until that run completes. Record actual hosted outcomes and incorporate fixes
   through reviewed immutable recovery or a new source/version as appropriate.

The same-source manual recovery route is available if automation fails; it cannot
bypass qualification or mutate an existing release. Offline tests alone do not
complete the hosted publication phase.

## Version selection and eligibility

The reviewed native `crates/marmish-native/Cargo.toml` version and matching native
`Cargo.lock` entry select the release; CI never guesses an increment. Accept the
existing bounded `0.2.N` / `0.2.N-rc.N` grammar and reject mismatches. Compare
numeric patch components, then prerelease: rc numbers compare numerically and
any rc is below the final release of the same patch. A selected new version must
exceed the highest matching native version visible in repository tags and
published releases. Matching native tags are exactly the anchored pattern
`^v0\.2\.[0-9]+(-rc\.[0-9]+)?$`; ignore all nonmatching tags/releases, including
v0.1.0-experimental.1 and v0.1.0-experimental.2. Enumerate with bounded complete
pagination and compare numerically, not lexically; only inconsistent matching
native state fails closed. Read-only eligibility/inspect cannot see private
drafts. The write job is the draft authority: before its first tag write, list
all releases including drafts with bounded complete pagination and refuse any
matching native draft at or above the selected version unless it is the exact
resumable SOURCE/version/candidate/inventory match.

Add `push` for master with the native Cargo.toml path filter to the existing
candidate workflow. Use one canonical SOURCE binding everywhere:
`${{ inputs.source_commit || github.sha }}`. On push it must equal github.sha;
manual input must also equal that run's head SHA. Use it for run-name, all four
checkout refs, package/provider environment and every provenance input. Push has
caveated=false; raw optional dispatch inputs never reach source fields. Preserve
manual caveated candidates only for troubleshooting; they cannot publish.

For push compare the native version at event.before and SOURCE, not tip's first
parent. Reject zero before, nonancestor before, force pushes and incomplete
history. Inspect the first-parent mainline progression from before to SOURCE;
require before to lie on that progression (conservative fail-closed otherwise).
Every inspected mainline commit must keep its native manifest and lock entry
consistent; commit both version updates together. Split mismatched intermediate
commits fail closed, even when a later tip repairs them.
Count version transitions on that progression: unchanged is a designed skip,
exactly one advancing transition can build, multiple transitions are ineligible.
This supports a bump before the tip in fast-forward/rebase pushes and the one
resulting transition of a merge/squash push. A fast-forward feature push with a
merge-from-master may put before off the first-parent chain; it is conservatively
ineligible and uses a reviewed manual full-candidate recovery instead. Recheck final version against the
highest existing native tag/release. Docs/bootstrap pushes normally do not
trigger this path; metadata-only Cargo edits produce a designed skip.

Eligibility is a closed tri-state: `build`, `skip-by-design`, `ineligible`.
Reasons are finite enum strings. `skip-by-design` means an unchanged version. This skip grants no publication
authority and does not consult historical tag state. Production eligibility
fails closed on an existing matching tag or release (`existing_version_conflict`),
because its read-only producer cannot completely verify original publication
inventory and public bytes. Retry a completed publication through the publisher
using its original candidate, rather than producing a fresh candidate. Inspect still recomputes eligibility
in same-SOURCE recovery mode. An
optional verified-state adapter is a testable contract, not enabled production behavior. An
ineligible decision fails the eligibility job and candidate. A manual full
candidate can rebuild the same unpublished version at a newly reviewed master
head without a before/version-change requirement. An exact immutable release
retry may use its original SOURCE/candidate; it must match all retained state.

The exact successful job name is `release-eligibility`; it uploads one bounded
`v02-eligibility-RUN-ATTEMPT` artifact containing canonical `eligibility.json`:
`schema:1`, repository, workflow_path, run_id, run_attempt, event, source,
`before` (40hex for push, null for dispatch), version, caveated, decision, reason.
The producer reads before from its actual push event, never a dispatch parameter.
The publisher binds the artifact to the exact candidate run/attempt and exact
successful eligibility job through GitHub APIs, validates the closed schema and
recomputes ancestry, version transitions and current immutable state. The report
is the trusted eligibility job's event-before attestation; the run API alone
does not expose before. This job executes reviewed master SOURCE; anyone able
to push master already has release authority, while the publisher still
independently recomputes ancestry and transitions. A mismatched/missing report or job is not a clean skip.
For designed skip only, packaging may be skipped and the publisher reports a
successful no-op; every other skipped required job fails publication.

## Separate candidate and publisher runs

Use new `v02-publish.yml` on completion of the exact candidate workflow, plus a
bounded recovery dispatch naming run/attempt. The workflow_run publication jobs
have a job-level success-conclusion condition; failed candidates cannot enter
publication. A separate run establishes completed candidate evidence; a same-run
reusable publisher cannot establish that fact.

Treat selection payloads/artifacts as untrusted until validation. Requery the
exact GitHub run/attempt and require repository/head repository Epiphytic/marmish,
workflow path `.github/workflows/v02-release.yml`, branch master, event push or
workflow_dispatch, completed success, full 40-hex head=SOURCE and trusted repository
ancestry. Manual recovery does not substitute the dispatch head for SOURCE.
Require one whole attempt: no combining selected-job reruns across attempts.

For a build require exactly the successful package job for each of three targets,
render, three install-quickstart jobs, three provider-qualification jobs and
release-eligibility. Reject missing, skipped, duplicate, cancelled or failed
required jobs. Require closed-schema passing quickstart/provider reports for
all targets, with their existing source/target/artifact bindings. A green upload
step is not a passing report. Require unique exact artifact names/IDs/digests,
run/attempt suffixes, unexpired state, bounded complete listing/downloads and
regular bounded archive entries without symlinks, duplicates or traversal.

Update both event validators together: `tools/v02-actions/workflow.mjs` checkRun
and `tools/marmish_native_https_check.py`, plus their tests, accept exactly push
and workflow_dispatch. Preserve all other source/run/attempt/ancestry checks.
Do not reuse candidateInputs unchanged: its relay requirement and GITHUB_SHA
binding belong to candidate execution, not publisher execution. Packaging's
installed feature tests and quickstart/provider checks do not imply that the
independent public CLI/cohort/lifecycle suites ran; release notes say what ran.

## Trusted code and isolated renderer

Every privileged publisher helper is static zero-dependency Node code from the
publisher run's own GITHUB_SHA, checked out without persisted credentials.
SOURCE is read as data by API/git, never checked out for execution in the write
job. No npm install, candidate executable/script, artifact-supplied command or
untrusted shell interpolation runs with write credentials. Normal first mutation
requires master==SOURCE==publisher GITHUB_SHA. Recovery at a completed bootstrap
keeps inventory.publisher_sha bound to original SOURCE while executing trusted code at the exact verified BOOTSTRAP; it only after proving the exact
already-completed SOURCE/direct-child boundary; no alternative source execution.

The trust-deciding jobs never execute artifacts. Split the publisher into:

- `inspect`: read-only GitHub/API and archive validation; stage hash-bound public
  renderer and candidate payload as current-workflow artifacts for rendering.
- `reproduce-bootstrap`: `permissions: {}`, no GITHUB_TOKEN. Download only those
  already-hashed inputs using same-workflow runtime artifact transfer. Fetch
  trusted orchestration from the public repository at publisher GITHUB_SHA
  without credentials. Run only the digest-bound candidate Rust release renderer
  to a new file. Run its child with an explicit scrubbed environment: no GitHub
  tokens,
  runtime token, GITHUB_OUTPUT/GITHUB_ENV paths or inherited credential settings.
  Preserve only required bounded toolchain/PATH/home/temp inputs. This is reviewed
  source execution, not a sandbox for arbitrary hostile code. The rendered script
  and its SHA256 are the only output; no output from this job grants authority.
- `verify`: trusted read-only code rechecks inspection provenance/artifacts,
  source/manifests and reproduction bytes against the candidate bootstrap; run
  `sh -n` as a syntax check, never execute the script. Produce the canonical
  inventory and trusted digest output only after every check passes.
- `publish`: trusted zero-dependency static code rechecks the inventory and all
  downloaded bytes, then performs bounded immutable API/git mutations.

The Rust `marmish-release-tool render` remains authoritative; no parallel
bootstrap renderer or fourth build is introduced. SOURCE and the renderer's
implementation are independently reviewed before master/source freeze. The
renderer executable comes from the same completed, source-bound candidate with
bounded artifact and file hashes checked by inspect. Its tokenless rerun proves
consistent canonical output, not independent binary authorship; the trusted
static gate separately validates source, manifests and all product bytes. The
reviewed-source trust premise is explicit; isolation still prevents inherited
tokens or verdict files from being available to its child. Authenticate all
payload independently in verify. Only trusted verify writes the authoritative
inventory digest.

Use exact current-publisher-run artifact handoffs, never name-only newest lookup:
inspect uploads `v02-publish-inputs-PUBLISH_RUN-ATTEMPT`, returning numeric
`inputs_artifact_id` and 64-hex `inspection_sha256`; it contains bounded canonical
inspection.json, all 15 assets, candidate bootstrap and the hash-bound candidate
Linux renderer. Reproduce downloads that exact ID, validates the inspection hash
and renderer bytes before running it, and uploads only `install.sh` plus bounded
render.json as
`v02-publish-render-PUBLISH_RUN-ATTEMPT`, outputting `render_artifact_id`.
Verify consumes these exact IDs and trusted inspect hash, independently validates
the candidate bindings and bytes, and uploads `inventory.json` as
`v02-publish-inventory-PUBLISH_RUN-ATTEMPT`, outputting `inventory_artifact_id`
and `inventory_sha256`. Publish consumes all three exact IDs and that digest;
any differing name/run/attempt, missing output or duplicate artifact fails.
All numeric IDs, 64-hex outputs and source fields are validated before use. The
runtime artifact service supports same-workflow transfer without a repository
GITHUB_TOKEN; no cross-run privileged token is passed to renderer children.

## Exact payload and integrity channel

Public assets are exactly15 regular files: for each supported Linux AMD64,
Linux ARM64 and macOS ARM64 target, three versioned executables, versioned
THIRD-PARTY-LICENSES and versioned manifest. Never glob an unbounded directory or
publish renderer/test tools, reports, runtime state or private data. Validate
manifest SOURCE/version/target/state-format tuple, names, sizes and SHA256 bytes.

Verify emits canonical bounded JSON `inventory.json` (schema 1, fixed key order,
UTF-8 with one final newline): repository, publisher_sha (the stable original SOURCE publisher code), SOURCE, version,
candidate run/attempt, all candidate artifact IDs/names/API digest fields, exactly
15 lexically sorted public `{name,size,sha256}` entries, bootstrap SHA256 and
relevant qualification report digests. Reject unknown fields, duplicate names,
invalid numeric bounds and absent artifact API digests. Hash the exact bytes.
Pass only its 64-lowerhex SHA256 as trusted verify job output; stage the document
and payload via artifacts as data, not as the authority. Publish first matches
the downloaded inventory to that output, then redownloads original candidate
artifacts and checks IDs/API digests and every file against it. Requery source
and run/attempt bindings. Changed artifacts/inventory fail before writes.

## Permissions, publication and retries

Candidate jobs keep contents:read. Inspect/verify have contents:read and
actions:read; renderer has permissions:{}; publish alone gets contents:write and
necessary actions:read. Tokens are passed only to bounded trusted API/git
operations. No actions:write, package credentials, PAT, PR-trigger checkout or
Radicle secrets. SHA-pin actions. A release Environment is optional only if it
already permits unattended master publication; no new approval requirement or
host settings change is part of this phase.

Root verified live master protected:false, protection.enabled:false, no required
checks, and repository rulesets=[]; explicit job contents:write is permitted.
Preflight requery protection/rulesets before mutation and fail if these changed
to block the direct fast-forward bootstrap. No bypass, extra token or automatic
release PR is introduced. Do not infer push permission from workflow defaults.

Use repository-wide publisher concurrency, cancel-in-progress:false. Concurrency
is not FIFO: GitHub retains only one pending run and can cancel older pending
runs, including candidates. Report any uncompleted eligible version and its
manual recovery route; never claim queued versions necessarily ran. Root holds
a documented release-in-flight master freeze through bootstrap/HTTPS completion.
Use bounded pagination, downloads, request deadlines and finite retries for
transient 429/5xx/network/digest availability; auth/validation errors fail at once.
A timeout records exact partial state, not success or a new independent draft.

Immediately before first tag write, release publication and cutover push, requery
master and require SOURCE (or the exact completed-bootstrap no-op case). Create
one annotated vVERSION tag at SOURCE, never move/force it. Use REST consistently:
requery and verify the tag target before creating a draft; there is no REST
verify-tag field. Require the release target's workflow tree to equal current
master's workflow tree (SOURCE equality or verified script-only BOOTSTRAP); this
avoids any workflow-write credential requirement for release creation. Create
draft with prerelease:true and make_latest:false, notes
binding SOURCE/run/attempt and actual validation/platform/Tor caveats. Upload
only exact 15 names without clobber. Verify REST asset inventory exactly: state
uploaded, names, sizes and required server SHA256 digests. Publish only after
matching; anonymously download/hash all 15 assets afterward.

Retries retain original candidate attempt and inventory. Find drafts via bounded
release-list/numeric-ID lookup, not only by-tag404. Matching tag/draft can resume
with byte-identical existing assets and upload missing exact names. Extra,
duplicate, partial/nonuploaded or mismatched assets fail; never automatically
delete/replace them, including on drafts. Published assets are immutable. A tag
created before an unrecoverable failure burns the version: preserve evidence
and choose a new reviewed version rather than pretending it is free. Conflicting
state requires operator disposition; a recovery flag cannot relax invariants.
Candidate artifacts expire after 14 days: after expiry a missing immutable retry
input requires a fresh reviewed source/version/candidate, never reconstructed
provenance under the old release.

## Bootstrap, races and manual mirrors

After public assets verify, construct BOOTSTRAP using git plumbing or GitHub
Git Data REST: SOURCE tree plus one rendered blob, mode 100755, one parent SOURCE.
Never check out SOURCE in the privileged job, so checkout-time filters cannot
execute. It is SOURCE's direct child and changes only `scripts/install.sh` to
the exact verified rendered bytes with executable mode. Normal fast-forward push only if
master remains SOURCE. Never rebase, merge intervening changes or force. Verify
remote ref and anonymous script bytes. GITHUB_TOKEN pushes do not trigger another
push workflow; the Cargo path/version eligibility also guards bootstrap loops.

If master races before any tag exists, record `no_tag_created_version_free`.
After review a manual candidate at the new master head may reuse that unpublished
version, obtaining new SOURCE/provenance; it must exceed prior published versions.
Once a tag exists, only the original immutable source/candidate retry is allowed;
a different SOURCE needs a new version. If release is public but cutover races,
leave assets/old bootstrap intact and record partial publication. Recovery accepts
master==SOURCE or an already-existing exact single-path bootstrap direct child;
ancestor status alone is insufficient. Never reset raced master. A lost direct-
child boundary after a tag requires a new reviewed release plan/version.

Emit a bounded summary/state artifact: SOURCE/version/tag, candidate run/attempt,
15 digests, bootstrap digest/commit, finite publication state and
manual_source_mirror_pending. Manually mirror branches/tags to Radicle, synchronize
Iris and verify external bytes; only then manually dispatch existing six-cell
HTTPS qualification. GitHub success is not a Radicle or HTTPS success claim.

## Interfaces and implementation ownership

Root owns workflow integration, CLI glue, documentation and Beads evidence.
Delegate `tools/v02-actions/publish.mjs` and `publish.test.mjs` as the isolated
zero-dependency write coordinator. Another component owns eligibility/report
validation and event-validator tests. Reuse pure hash/schema/API building blocks
where dependency-free; keep archive/npm dependencies out of the privileged
module import graph. Existing ArtifactTransport is optimized for small JSON
archives (including its 16MiB limit) and imports archive dependencies; do not
import it into publish or apply that limit to product archives. Reuse its bounded
archive-validation patterns in a narrowly scoped read-job extractor for large
assets, with explicit per-file/aggregate limits, regular-file checks and no
filesystem extraction outside owned staging. No new Python coordinator or
product/renderer change.

Narrow commands consume bounded JSON files/env and produce closed JSON, never
shell snippets:

- `eligibility`: event/repo/run/attempt/SOURCE/before plus git/API adapters -> the
  exact eligibility report and tri-state job output. All event data passes env or
  files, never direct expressions embedded in shell source.
- `inspect-candidate`: candidate run/attempt -> qualified source/payload staging,
  report and artifact bindings; no mutation or execution.
- `verify-inventory`: inspection data plus reproduced bytes -> inventory.json
  and inventory_sha256 trusted job output. Skip requires verified eligibility
  and returns a separate finite no-op result, not an empty build inventory.
- `publish`: inventory path, expected inventory SHA256, candidate run/attempt and
  publisher SHA -> bounded publication-state.json; zero-dependency REST/git
  adapters, exact source checks, immutable draft/tag/release and bootstrap logic.

Adapters are injectable for offline tests; production uses finite REST fetches
and argv-based git without shell commands. Interfaces share an agreed schema
module that itself has no external dependencies. Cross-job inventory artifacts
are data; only trusted job digest outputs authorize their contents.

## Required validation

Offline tests cover version numeric/rc ordering/highest visible published tags;
ignore the existing 0.1 experimental tags and unrelated tags. A read-token fixture
hides a higher/equal draft, and the write-token authority must still refuse it
before any tag write (except an exact resumable match). Also cover merge,
squash, multi-commit/rebase bump-before-tip, two bumps, zero-before/force-push and
nonancestor cases; unchanged/already-published conflict refusals versus malformed/ineligible
reports; exact eligibility job/run/attempt; wrong repo/event/head/workflow/fork;
missing/skipped/caveated/duplicate required jobs and bad reports; source-binding
fallback at every checkout/env; expired/missing/duplicate artifacts, API digest
change, archive traversal/symlink and exact 15 inventory; wrong source/version/
target/formats/name/size/hash; isolated renderer scrubbed environment and
reproduction mismatch; canonical inventory digest handoff and TOCTOU mutations;
matching partial draft retry versus incomplete/mismatched uploads; conflicting
tag/draft/public release, missing server digests, bounded transient retry and
GitHub timeout. Assert zero write calls until full validation succeeds.

Git fixtures verify direct-parent/single-path/mode, master rechecks immediately
before tag/publish/push, source races before/after tag, and exact completed retry.
Static workflow tests assert no pull_request triggers, no untrusted expressions
in run scripts, SHA-pinned actions, per-job least permissions, no renderer token,
no npm/dependency execution in writejob, no Radicle access and no force/delete/
clobber. Assert release target/current master workflow-tree equality, with no
workflow-write token requested. Run Node tests, actionlint/workflow validation and affected existing
provenance/renderer checks. Obtain actual integrated Claude approval.

Actual0.2.3 hosted acceptance requires every candidate package/render/quickstart/
provider gate across all three platforms, trusted publisher results, public tag,
15 REST/anonymous-verified assets and direct-child bootstrap, followed by manual
mirrors and six-cell HTTPS results. If any gate fails, document precise failure
and use reviewed recovery; do not downgrade required qualification to ship.


## Offline approval and hosted limits

[Integrated Claude code review round 2](../reviews/native-automatic-release-code-r2.md)
approves the implementation offline after correcting both round-1 blockers.
Hosted candidate, publisher and public installer acceptance remain required.
Non-cancelling GitHub concurrency still replaces older pending runs; a newer
Cargo edit can supersede a pending version bump. Recover by dispatching a full
candidate at reviewed master head, never by bypassing qualification. Listing
retains the existing 100-artifact bound across all attempts (roughly six whole
reruns); exceeding it fails closed and requires a fresh full candidate. A
successful caveated diagnostic candidate intentionally fails publisher inspect.

## Installed-provider finite-label integration amendment

The Python diagnostic ancestor added `owned_preferences_directory` for its
owned-home preferences prerequisite. The installed-checks Rust adapter's exact
AST parity test detects a label-set mismatch. Initial design review rejected
removal of `owned_account_prerequisite_failed`: it remains reachable as the
non-Refused fallback in Python's nested conditional expression, which the test's
AST helper did not recursively extract. The earlier proposal misread this as a
source removal; no Rust changes preceded the rejection.

Retain `owned_account_prerequisite_failed` and all existing mappings. Add
`owned_preferences_directory` mapped to `ProviderFailure::PrerequisiteFailed`.
In the existing test's embedded Python AST helper `add(node)`, recurse only
through `IfExp.body` and `IfExp.orelse` and route the existing category conditional
through that helper. Assert the extracted set includes both fallback labels
`owned_account_prerequisite_failed` and `owned_account_cleanup_incomplete` before
comparing exact Python/Rust label sets. Keep exact parity, mapping uniqueness,
strict unknown-label refusal, report parsing, independent cleanup facts and all
existing cleanup assertions unchanged. This strengthens the source-derived test
rather than dropping a legitimate diagnostic or weakening the finite contract.

This is installed-checks tooling for final automation SOURCE, not a diagnostic
ancestor or default runtime edit. Do not change Python fallbacks, product behavior
or success/publication gates. Obtain actual independent Claude design approval
on this corrected scope before Rust edits. Run the targeted parity test and all
`lifecycle_provider::tests` with installed checks enabled on the coordinated
shared Cargo target, then obtain actual Claude code review in auto mode. Record
actual sessions, counts and limitations in existing review/validation evidence.
