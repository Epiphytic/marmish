# Candidate build budget correction: local checks

Based on [the actual90-minute timeout](streaming-candidate-2ebc8b6-timeout.md)
and [approved design](../phases/streaming-candidate-build-budget.md), the
workflow's sole changed line is `jobs.package.timeout-minutes: 90`→`120`.
The same three-platform matrix, native tests, strict Clippy, isolated target,
release-tool build, product-hash checks, archive/provenance and nonpublishing
policy remain intact. Render stays5minutes; installed jobs stay65minutes;
version-lifecycle stays90minutes. No runtime/transport deadline changes.

Agent validation of the frozen two-file source change:

| Check | Actual result | Private log SHA-256 |
|---|---|---|
| Focused `node --test tools/v02-actions/streaming-policy.test.mjs` | 7passed | `47c4c6f8895296ece44478b9348e3548f4cbefd06fc2244c5d5b315eace11426` |
| `node --test tools/v02-actions/*.test.mjs` | 158passed,1.0735s | `1c629cd8796dffeb1b986f474a71af5e251787d3f6d93b536b0216301797de9d` |
| `git diff --check` | PASS | no standalone log |

The new check scopes package and render job keys and rejects missing,
duplicate, wrong and unrelated timeout values. Named-step assertions pin the
unchanged native tests/Clippy and isolated-tool tests/Clippy/release binaries,
fresh target and before/after product-hash comparison. Existing installed65
checks remain. These policy tests do not execute the workflow or compile Rust.
No local Cargo rebuild is claimed for the YAML-only budget edit.

Root inspected the exact workflow diff and whitespace. Ripwire quality-delta
exited0 with0reported regressions/gates at `2ebc8b6b2+dirty`; it is a structural
indexed-code check, not workflow execution. Log SHA-256:
`a22ca11bdec28ff794a6570784becc2bf907e5641f2dd046ba68cd2a4ae52dbc`.
Change-context exited0, SHA-256:
`e3a8a94508130460281ac4ad99e8f3f24eb98e5206a4da4a19f7ae0a73959cbb`.
It mapped five tracked files and did not include the newly untracked evidence
documents. Its zero test/caller edges for JavaScript and YAML do not establish
coverage; the actual158-test execution and manual diff review provide the
relevant checks. All untracked evidence/specification files are included in the
independent integrated review scope separately.

The passing hosted macOS diagnostic and partial failed-candidate evidence are
published alongside this correction and remain bound to their old source/run.
The full local693.03s rerun and prior actual result review were already complete;
the specification's stale pending wording is corrected to match that evidence.

Independent integrated code/evidence review approved source publication; see
[the actual review](../reviews/streaming-candidate-build-budget.md). The reviewer
independently reran7 focused and158 full Node tests, with whitespace and actual
evidence/hash checks.
A new complete three-platform candidate and all eight original installed
cohorts remain unexecuted for this source change. No qualification, release,
tag, version or installer pin change follows from local policy tests.
