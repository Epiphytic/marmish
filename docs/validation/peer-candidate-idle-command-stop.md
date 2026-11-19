# Registered idle Command stop validation

Linux AMD64 author checks used source `26fd90e1ec118ef6150a4ba57299444924c23317`
and the locked, offline dependency set. The stock and private-candidate unit
binaries were built separately with debug info and incremental compilation
disabled. The private candidate used `--target x86_64-unknown-linux-gnu` and
`--cfg marmish_peer_receiver_candidate`. Strict `cargo clippy --all-targets`
with `-D warnings` passed in each build.

| Immutable binary and scope | Result | Duration |
| --- | --- | ---: |
| Stock unit test build | Passed | 44.66 s |
| Stock strict Clippy | Passed | 30.18 s |
| Stock `registered_` | 7 passed, 7 ignored | 15.99 s |
| Stock `command_service::tests` | 16 passed | 5.78 s |
| Stock `healthy_owned_stop` | 7 passed | 29.86 s |
| Candidate unit test build | Passed | 45.07 s |
| Candidate strict Clippy | Passed | 30.16 s |
| Candidate `registered_idle_prelude` plus `registered_stop_without_engine` | 4 passed | 15.43 s |

The four focused cases exercise a real Command engine through idle quiescence,
expiry before a Journal write, dispatcher loss while a canceled observer leaves
the turn owned, and the NeverStarted bypass. The empty-workload harness measured
36.47 ms from stopping intake through physical stop observation in one candidate
run. This is a single report-only timing from an in-process test fixture. The
fixture setup supplies no command rows and the printed row count was not
independently measured. The [independent review](../reviews/peer-candidate-idle-command-stop.md#first-implementation-review)
records the bounded approval.

The installed started-engine process, its stock reopen with retained terminal
results, the full subeffect-cut matrix, and large retained histories remain
unqualified. This increment did not change the release build, GitHub Actions,
or the existing registered no-engine acceptance criteria.
