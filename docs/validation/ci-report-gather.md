# Installed CI report collection validation

Original source `c1d0c0376ca0662ea10e4b6d02acc777f6303328`;
candidate [42747078282/1](https://github.com/Epiphytic/marmish/actions/runs/42747078282).
All eleven candidate jobs passed, including Linux AMD64, Linux ARM64 and macOS
ARM64 packages. Separate macOS regression run
[42747086736/1](https://github.com/Epiphytic/marmish/actions/runs/42747086736)
passed. These are original-source results, not newly executed product tests.

Streaming [42885236657/1](https://github.com/Epiphytic/marmish/actions/runs/42885236657)
passed resolve and all three platform jobs. Its combined verification/collection
step failed before native rollup; the old CLI discarded the actual exception.
No HTTP status or transient cause can be recovered from that log.

On November 12, authenticated local reconstruction verified the exact original
context and candidate tooling, collected twelve original role/kind reports and
confirmed their bytes match the previously retained reports. The original pinned
native validator then passed 9/9 directed cells. Reconstructed rollup SHA256:
`28630d6b9f2f0afb61baa94a736902e492eaa59d6f2c6893df72eec08ed7db2f`.
This recovers native validation without relabeling the original workflow.

Local baseline Node suite: 216 passed, zero failures/skips. After bounded retry
and closed diagnostic changes: 234 passed, zero failures/skips. Integrated replay
and workflow routing suite: 249 passed, zero failures/skips. Tests simulate
listed ZIP404 recovery, persistent transient deadline exhaustion, shared API
budget exhaustion, retained exclusive writes and hard authentication, list404,
blob404, metadata, duplicate, digest and archive failures. Diagnostic tests include
hostile/changing getters and actual CLI nonzero exit without arbitrary strings.
These fixture tests are separate from the original live platform executions.

The legacy workflow keeps its Python/fixture and manual coverage. Node-only
changes now route to the dedicated coordinator workflow; mixed Node/Python
changes still run both. Routing tests cover thirteen path combinations for each
of push and PR. All five changed workflows parse successfully; scoped legacy and
coordinator workflows also pass actionlint.

Independent integrated Claude review approved the result and independently
passed all 249 tests in full and shallow checkouts. Hosted replay, automatic
coordinator CI and normal-path candidate/streaming validation are pending. This increment neither
publishes v0.3.1 nor establishes the remaining transfer/benchmark cohorts.

Hosted follow-up: coordinator run [43172725362](https://github.com/Epiphytic/marmish/actions/runs/43172725362) passed. Legacy installer/packaging run [43172725298](https://github.com/Epiphytic/marmish/actions/runs/43172725298) passed all six jobs across Linux AMD64/ARM64 and macOS. GitHub rejected the first replay workflow before execution because `runner.temp` is unavailable in job-level `env`. The correction exports all six owned paths from `$RUNNER_TEMP` through `$GITHUB_ENV` in the first step. A regression test covers the forbidden context and all path exports. All twelve replay tests and actionlint 1.7.12 across all five changed workflows pass. Claude auto follow-up review in session `26c27dc0-5225-4626-8bf2-ff43533e097e` returned **APPROVE** with no blocking findings. The obsolete candidate run 43172758055 was cancelled before completing builds; it supplies no qualification evidence. Corrected hosted replay and a replacement matching candidate/streaming run remain pending.

Replay run [43173341776](https://github.com/Epiphytic/marmish/actions/runs/43173341776) reached the complete coordinator suite but exposed an existing child-death assertion race before provenance or report collection. The test observed Linux state R immediately after SIGKILL; parallel coordinator [43173338087](https://github.com/Epiphytic/marmish/actions/runs/43173338087) passed all 249 tests. The test-only correction observes the exact fixture PID for at most one second, accepts only absence/zombie, rejects permanent survivors and unexpected errors, and preserves cleanup and primary failures. Deterministic delayed-death, live-survivor, late-proof and errno checks pass. The final focused suite passes 13/13 and full coordinator suite passes 251/251 with no skips. Native runtime and runOwned are unchanged. The replay allowlist admits only the changed test file and explicitly continues to reject local.mjs changes.

Claude auto code follow-up in session `26c27dc0-5225-4626-8bf2-ff43533e097e` returned **APPROVE**, independently passing the focused 25-test combination five times. The reviewed complete CI-only delta remains eighteen allowed source/documentation paths. Quality delta reports zero regressions; no runtime or release qualification claim is added.
