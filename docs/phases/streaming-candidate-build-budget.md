# Candidate build job budget correction

Status: Actual Claude AUTO design and integrated source/evidence approval recorded after the observed timeout; the120-minute cap and policy tests are implemented. Fresh hosted qualification remains required. Prepared against source2ebc8b6 and completed candidate42408127994. The original conditional analysis is retained below, followed by the observed trigger and revised estimate. See [the actual review record](../reviews/streaming-candidate-build-budget.md).

## Trigger and scope

Implement only after the actual candidate package job concludes with a GitHub job-budget timeout at its existing 90-minute limit, confirmed by completed job/run API, elapsed time near5400s, and actual timeout annotation/log text (for example, exceeded maximum execution time), with no evidence of an external cancellation request. GitHub may label a budget expiry cancelled; conclusion label alone neither satisfies nor excludes the trigger. A long stage, prediction, test failure, compiler failure, externally requested cancellation, or failure of an installed cohort is not this trigger. If candidate 42408127994 passes, discard this proposal. If it fails for another cause, investigate that cause rather than applying this change automatically.

The sole behavior change would be the package job wall-clock cap in `.github/workflows/streaming-candidate.yml`, from `timeout-minutes: 90` to `timeout-minutes: 120`. Apply to the existing three-target matrix, without runner-specific exceptions. No new workflow, cache, build flags, dependency, Cargo target/profile, tests skipped, or product/runtime behavior change.

## Initial timing projection (superseded by observed assessment)

Read-only GitHub REST jobs API snapshot at 2026-11-07T20:32:39Z, fetched with `gh api repos/Epiphytic/marmish/actions/runs/RUN/jobs?per_page=100`:

| Stage | Successful run 42387811714 AMD | Current 42408127994 AMD |
|---|---:|---:|
| Native components | 2030s | 2379s completed |
| Final native executables | 592s | 148s in progress at snapshot |
| Isolated installed-test tools | 2652s | not started at snapshot |
| Complete package job | 5311s (88m31s) | in progress |

The prior successful AMD job had only 89 seconds of margin under 5400 seconds. Current native verification was 349 seconds slower. Repeating the prior remaining durations projects 5660 seconds (94m20s), which would exceed 90 minutes; this is a projection, not proof of failure. Its 19:50:20Z start places the present cap at 21:20:20Z.

Prior Mac total was 55m32s and ARM 56m31s. Current completed Mac native verification/final build took 1094/556 seconds versus prior 1115/601. ARM took 1386/443 versus prior 1242/419. These observations show compiler/test workload variation; they do not establish its cause.

120 minutes gives 30 additional bounded minutes and approximately 25m40s above the AMD projection (1.27 times that projection). It is an engineering build ceiling, not a claim of fourfold transport headroom. The copy operation fourfold measured-latency rule concerns executable workloads over Marmot and is not a compiler-build margin rule. Record the actual timeout elapsed/stage before final approval; if it disproves the timing model, revise this design rather than silently increasing again.

## Observed trigger and revised timing assessment (2026-11-07)

The conditional trigger has now occurred, but implementation still requires explicit review/authorization. AMD job128510172415 of candidate42408127994 completed cancelled at21:20:36Z after starting19:50:20Z (5416s). Its actual failure annotation states “The job has exceeded the maximum execution time of 1h30m0s”; the coordinator reports no requested cancellation. The candidate is unqualified. Passing individual test blocks do not make the interrupted tools-build/package gate pass.

The preserved current full job log is `/tmp/marmish-release-testing-2ebc8b6/candidate-linux-amd64-job.log`, SHA256 `7364288ab1a8ba3bbf33a32ff7162bcd0950906856494e804b769909368ca25c`; annotation file is adjacent `candidate-amd64-annotations.json`. The successful baseline AMD full job log was fetched read-only with `gh api repos/Epiphytic/marmish/actions/jobs/128448837949/logs` into `/tmp/marmish-candidate-baseline-42387811714-amd.log`, SHA256 `449a575697e9cbe3be38ce35ebde667c68fbcecf511ef2fb6167dc1a431664cb`.

| Observed internal block | Baseline42387811714 AMD | Timed-out42408127994 AMD |
|---|---:|---:|
| Native library tests | 620passed/3ignored,1573.70s | 630passed/3ignored,1932.94s |
| Installed-feature compile | 5m11s | 5m15s |
| Installed-feature library tests | 833passed/3ignored,1600.18s | 843passed/3ignored,1974.74s |
| Installed-feature Clippy | 1m35s, ended17:01:51.887Z | 1m33s, ended21:20:03.202Z |
| Installed-tools release build | 10m40s, ended17:12:31.993Z | Started afterClippy; still compiling at cancellation |

Current feature test result was recorded21:18:28.145Z, with subsequent targets/doc tests completing through21:18:30.116Z; no failed-test result is present. Cargo/rustc and compiler children were terminated during release compilation at21:20:34Z. This distinguishes the cancelled build from a test failure without claiming the unfinished release build would succeed.

The earlier94m20 projection underestimated the observed installed-feature test duration:1974.74s versus1600.18s, an additional374.56s (23.4%). It also preceded the actual final-native-build completion. Using actual progress through21:20:03 plus only the baseline10m40 release duration projects completion around21:30:43Z, approximately100m23s afterjobstart, before a few seconds of copy/upload/post steps. This remains a counterfactual estimate, not an observed successful completion or causal explanation. A120-minute cap offers roughly19m37 headroom over that updated estimate; it does not guarantee completion and does not establish a fourfold margin. Preserve failures if the new cap is exhausted; do not silently increase again or remove checks. Test counts differ between source versions, so durations are evidence of the actual workload, not a controlled speed comparison.

## Unchanged validation and trust boundaries

Keep all existing package step boundaries and commands. In particular, native verification still runs the four-package locked test suite and strict Clippy. The isolated tools step still creates its separate `$RUNNER_TEMP/installed-tools-target`, runs installed-check feature tests and strict Clippy, release-builds the two tools, signs Mac tools, and checks unchanged product hashes. Product archives, tools manifests, source digest, immutable SHA, licenses, dependency inspection, installer/bootstrap checks, artifact provenance, publication prohibition, dispatch restrictions and target set remain unchanged.

No installed workflow, operation timeout, traffic budget, workload bytes, acceptance predicate, cleanup proof or eight-cohort membership changes. `streaming-installed.yml` remains at its existing 65-minute job ceiling and its current internal deadlines. No 90-minute timeout is reclassified as a PASS.

## Exact implementation and tests, if triggered and approved

1. `.github/workflows/streaming-candidate.yml`: change only `jobs.package.timeout-minutes` from 90 to 120.
2. `tools/v02-actions/streaming-policy.test.mjs`: add a job-scoped assertion identifying the `package` block before `render`, requiring exactly one intended 120-minute package cap and preserving the render job cap at5minutes. Do not change v02-version-lifecycle.yml or its independent90-minute cap. Pin the unchanged isolated-tool commands/fresh target/product-hash comparison and the existing native-test command in the corresponding named step blocks. Retain existing three-platform and nonpublishing assertions and installed 65-minute assertion; do not accept an unrelated `timeout-minutes: 120` elsewhere as proof.
3. Coordinator-owned design/review/validation/spec documentation only as needed to record the actual timeout and bounded change. No unrelated test or implementation files.

Run `node --test tools/v02-actions/streaming-policy.test.mjs`, then `node --test tools/v02-actions/*.test.mjs`, `git diff --check`, and inspect the exact workflow diff. Negative-test the job-scoped parser/assertion against local strings containing an unrelated 120-minute cap with package still 90; missing/duplicate/wrong package cap must fail. These are test-only strings, not mutation of repository workflow during tests. No Cargo rebuild is required merely to validate the YAML budget edit locally. Obtain actual independent Claude design and integrated code approval, preserving source ownership and the exact two-source-file scope.

## Fresh qualification and preserved failures

Keep failed candidate 42408127994, its attempt, logs, stage timing, conclusions and artifact records as FAIL/timeout evidence. Commit and publish a new reviewed source SHA after this small change. Build all three platforms from that new exact SHA through a fresh candidate run and require all native, packaging, tools and render gates. Do not retry only the timed-out platform or mix previous-platform assets into a new provenance tuple.

After all candidate gates pass, dispatch the original stream matrix and then the remaining seven separately bounded cohorts in the existing serial fail-stop order. All eight must consume that same fresh candidate run/attempt/source tuple, use the actual installer, and satisfy their unchanged rollups and cleanup criteria. Earlier partial cells, unrelated successful diagnostics, prior candidates and a budget-adjusted build are not installed qualification. The2ebc8b6 macOS focused diagnostic remains bound to its original SHA and is not transferred as execution evidence for the new SHA; the fresh candidate must provide actual new-source native verification. Stop on a real failure and preserve evidence. No release/version/publication authorization is added.
