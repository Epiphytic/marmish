# macOS initial TERM no-status observation

Evidence cutoff: 2026-11-09 17:58:50 UTC. Beads `marmish-d97.2`.
This records a failed diagnostic, not a runtime correction or release qualification.

Ordinary candidate [42653492263, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42653492263)
at source `33138ebdb2ddf19103bcb58f64c2b31edc66b2c4` failed macOS
`final_handle_drop_releases_full_unacked_output_race` in the full debug
installed-feature suite: 906 passed, one failed, three ignored. Both Linux
package jobs passed; downstream cohorts were skipped. Separate exact-source
macOS [stress 42653573013, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42653573013)
passed 100 installed-feature authority iterations and 80 default-feature cleanup
repetitions. Those fixtures/features do not cover the failed full-suite result.

The [reviewed diagnostic](../reviews/macos-held-status-diagnostics.md) adds closed
failure-only categories while preserving cleanup behavior. Its first
[run42665195683, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42665195683)
at `40ab5155a972421d26f9d18a1192bab0c1d8fb63` failed before engine or native tests:
the expected worker metadata inventory omitted the existing ordinary
`marmish-release-tool` binary. This supplied no new cleanup observation.

The corrected diagnostic [run42666220530, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42666220530)
used source `e4617547d77264dd733db5a924bffb36a46a1ebf` and workflow
`.github/workflows/macos-held-status-diagnostics.yml`. The macOS ARM64 nonroot
source/host guards and metadata guard passed. The job started at 17:36:13 UTC
and finished with failure at 17:58:50 UTC.

| Check | Observed result |
| --- | --- |
| Pure engine diagnostic tests, debug | 2 passed, 0 failed, 0 ignored |
| Pure engine diagnostic tests, release | 2 passed, 0 failed, 0 ignored |
| Debug formatter test | 1 passed, 0 failed, 0 ignored |
| Strict engine Clippy, debug and release | Both passed |
| Full debug native installed-feature suite | 907 passed, 0 failed, 3 ignored; 688.60 seconds |
| Exact race repetitions | 1–21 passed; 22 failed; 23–100 not executed |

The full command was exactly:

```sh
cargo test --locked -p marmish-native --features installed-checks
```

Its main native summary is reported separately from nested/binary summaries.
The repetitions used the same debug installed-feature target and exact test
`stream_runner::tests::final_handle_drop_releases_full_unacked_output_race`.
The first failed repetition emitted this complete closed record:

```text
marmish-engine-signal-failure origin=term stage=proof-held/darwin-held-status errno=none wait_pid=zero wait_code=zero proof_attempt=1
```

It then reported `RecoveryUncertain` before the runner set `cancel_started`.
The initial `waitid(WEXITED|WNOHANG|WNOWAIT)` succeeded but returned zero PID
and zero code: no exit status was available at that observation. This does not
prove the underlying kernel cause of the group TERM EPERM, establish a terminal
leader or group, or authorize reaping. The diagnostic does not weaken strict
failure, ownership, identity, membership or cleanup deadline requirements.
The new runtime-correction proposal under `marmish-d97.3` was in design review
at this cutoff; no runtime fix had been approved or implemented.

The sanitized terminal summary SHA256 is
`99d05120bbdaa0b08b35f8a5500dddc889623e1e928852a04108887a36cb21aa`.
The privately retained raw job log SHA256 is
`f26f2f29be301ca9986b12c773cc8666b7e9f89ecf04813024436194602260ee`;
raw log contents and private filesystem paths are not published here. Validation
required exact source/run/attempt/workflow association, ordered unique iteration
announcements and the sole summary per repetition; nested summaries were not
aggregated. A fresh diagnostic full suite does not recreate all preceding
candidate build/test history. Neither its passing full command nor 21 isolated
passes qualifies a release or proves a fix. Operation latency/throughput
benchmarks remain unmeasured in this qualification sequence.
