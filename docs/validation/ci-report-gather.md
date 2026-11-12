# Installed CI report collection validation

CI increment completed on November 12, 2026. Tested coordinator source:
`05bec8b2b4c4ea0a161a1adbc494b0d180a92ba8`. Rust product, Cargo and installer
source are unchanged from `c1d0c0376ca0662ea10e4b6d02acc777f6303328`.
All hosted runs below are attempt 1.

| Validation | Run | Result |
| --- | --- | --- |
| Fast coordinator | [43173876717](https://github.com/Epiphytic/marmish/actions/runs/43173876717) | 251 tests passed; zero failures/skips |
| Original-report replay | [43173887015](https://github.com/Epiphytic/marmish/actions/runs/43173887015) | Provenance PASS; native rollup 9/9 |
| Fresh native candidate | [43174030584](https://github.com/Epiphytic/marmish/actions/runs/43174030584) | All 11 jobs passed |
| Ordinary streaming matrix | [43182899512](https://github.com/Epiphytic/marmish/actions/runs/43182899512) | All 5 jobs passed; 9/9 fresh client/server pairings |
| Legacy installer/reference | [43172725298](https://github.com/Epiphytic/marmish/actions/runs/43172725298) | All 6 jobs passed across Linux AMD64/ARM64 and macOS; source `e50b1cf` |

The normal workflow passed resolve, three platform jobs and native rollup through
the repaired ordinary verify-context/gather entrypoints. Its downloaded rollup
has `complete=true`, nine expected/passed cells, the exact source/candidate above,
and artifact prefix `v02-installed-43182899512-1-05bec8b2b4c4-stream-matrix`.
Rollup SHA256: `e4a79c8418ecb4e179376a3b91adcb6293fb8d5384d5b3ab9efd1a91bd3accd7`.
Context SHA256: `f5d2f0894a3f5e8b766ff7d3c1953e7dbf4983774b868a48ff54412564aa3c74`.

## Fresh package and installer evidence

A separate read-only inspector authenticated exact candidate provenance, all eleven
successful jobs, fifteen closed artifact inventories/digests, manifest/tooling
bindings and ten qualification reports. All three real bootstrap/interactive
quickstart jobs and all three isolated keychain-provider jobs passed.
Package logs were independently audited; top-level Cargo totals exclude nested
helper subprocess summaries.

| Platform | Default tests passed / ignored | Installed-feature tests passed / ignored | Failures |
| --- | --- | --- | --- |
| Linux AMD64 | 832 / 5 | 916 / 3 | 0 |
| Linux ARM64 | 832 / 5 | 916 / 3 | 0 |
| macOS ARM64 | 859 / 5 | 915 / 3 | 0 |

Both default and installed-feature strict Clippy checks (`-D warnings`) passed
on each platform, as did release builds, dependency/ABI inspection, license collection, packaging
and product-byte preservation around the separate test-tool build. macOS ad hoc
signing and strict verification (`codesign --verify --strict`) passed. The
[package workflow](../../.github/workflows/v02-release.yml) records these flags.
Default ignored tests are the engine fd probe, native helper child, large four-way copy and large duplex fixtures, and
platform keychain round trip. Installed-feature suites repeat the three native
ignores. The provider jobs separately exercise the isolated keychain providers;
ignored stress fixtures are not counted as passing tests.

## Fresh finite streaming measurements

Source/candidate/run are those above. All nine directed pairings passed through
`wss://relay.us.whitenoise.chat/`, using the installed candidate and a validated
204,800-byte application payload per duplex case. One sample per pairing; these
are qualification observations, not percentile or saturation benchmarks.

| Client | Server | Wall seconds | Payload KiB/s |
| --- | --- | --- | --- |
| linux-amd64 | linux-amd64 | 48.873 | 4.092 |
| linux-arm64 | linux-amd64 | 47.234 | 4.234 |
| macos-arm64 | linux-amd64 | 48.726 | 4.105 |
| linux-amd64 | linux-arm64 | 50.676 | 3.947 |
| linux-arm64 | linux-arm64 | 50.445 | 3.965 |
| macos-arm64 | linux-arm64 | 50.671 | 3.947 |
| linux-amd64 | macos-arm64 | 51.073 | 3.916 |
| linux-arm64 | macos-arm64 | 49.539 | 4.037 |
| macos-arm64 | macos-arm64 | 50.241 | 3.981 |

Wall time is the monotonic installed CLI lifetime plus subsequent journal and
terminal observations, including transport, command and output. It excludes later
independent receipt/cleanup validation and is **not network RTT**. Payload speed
is validated application payload bytes divided by this wall time, not wire speed.
The remaining transfer cohorts and finite benchmark qualification are separate.

## Preserved original evidence

Original source `c1d0c0376ca0662ea10e4b6d02acc777f6303328` candidate
[42747078282](https://github.com/Epiphytic/marmish/actions/runs/42747078282)
passed all eleven jobs; separate macOS regression
[42747086736](https://github.com/Epiphytic/marmish/actions/runs/42747086736)
passed. Original streaming
[42885236657](https://github.com/Epiphytic/marmish/actions/runs/42885236657)
passed resolve and all three platform jobs but failed before native rollup. Its
old CLI discarded the exception; no HTTP status or transient cause can be proven.

Authenticated local reconstruction verified the exact original context/tooling
and twelve byte-identical reports. Original pinned native validation passed 9/9.
The hosted replay then independently passed with separate coordinator provenance,
retaining the original source/run/attempt. Reconstructed and hosted replay rollup
SHA256: `28630d6b9f2f0afb61baa94a736902e492eaa59d6f2c6893df72eec08ed7db2f`.
Replay provenance SHA256:
`22224c22249fbd6167f44322232171266c2421c9297591d5525e1ac468366ebe`.
This does not relabel the original failed workflow.

## Regression and review evidence

Node suite: baseline 216 passed; bounded retry/diagnostic increment 234 passed;
replay/routing increment 249 passed; final asynchronous-death test correction
251 passed, all without failures or skips. Tests cover listed ZIP404 recovery,
persistent transient deadlines/shared request budget, retained exclusive writes,
and hard authentication, list404, blob404, metadata, duplicate, digest and archive
failures. Diagnostics reject arbitrary strings, including hostile/changing getters.
Routing covers thirteen path combinations for each of push and PR. All five
changed workflows pass actionlint 1.7.12. Quality delta reports zero regressions.

Hosted validation exposed two additional CI defects, both corrected before the
successful final runs: job-level `runner.temp` context was replaced by first-step
`RUNNER_TEMP` exports, and an immediate post-SIGKILL death assertion was replaced
by a strict, test-only one-second observation bound. Runtime `runOwned` is
unchanged; permanent survivors, late proofs and unexpected errors still fail.
The obsolete candidate 43172758055 was intentionally cancelled before build
completion and provides no qualification evidence. Rejected replay workflow
43172722693 (zero jobs) and failed replay 43173341776 remain failed, rather than
being relabeled.

Independent Claude auto design/code reviews approved the integrated changes;
see [review evidence](../reviews/ci-report-gather.md). The reviewer independently
passed the full 249-test suite in full/shallow checkouts, then the final focused
25-test combination five times. No blocking findings remained.

This completes the CI increment tracked by Beads `marmish-e2s`. It does not
publish v0.3.1 or establish the other release qualification cohorts. The public installer
remains at v0.3.0 and the release publisher remains deliberately disabled.
