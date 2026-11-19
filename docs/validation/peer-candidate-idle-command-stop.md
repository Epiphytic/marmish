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

## Worker liveness follow-up

Exact source `0429ed1b46ca2b88b61e9806520303b8d428842b` passed a fresh stock
unit link (42.02 s), stock all-target strict Clippy (28.30 s) and candidate
all-target strict Clippy (28.73 s) using the flags above. The immutable stock
binary passed two `idle_cleanup_` engine tests and three
`registered_idle_prelude` ownership tests. The latter finished in 11.53 s.
The empty-engine case independently counted its retained rows through the
Journal and measured 38.07 ms with zero rows. This is one report-only
sample. This follow-up has no candidate runtime rerun or installed-runtime
qualification. See the [independent source review](../reviews/peer-candidate-idle-command-stop.md#worker-liveness-follow-up).

## Composed Linux AMD64 runs

Exact composed source `918b92cbcf74d79483cc1219ded34bd0ffe356a3`
passed stock all-target strict Clippy (28.12 s), candidate all-target
strict Clippy (36.94 s) and a candidate unit link (48.04 s). The author
ran nine focused cases covering the idle prelude, engine-liveness and
Copy GC refusals, the no-engine bypass and physical ownership.
The independently counted empty engine completed its prelude and
physical stop in 50.88 ms in one in-process sample.

Claude independently ran 32 cases from the immutable candidate runner
in 54.85 s, including real in-process engine stops and Command service
regressions. The candidate executable SHA-256 was
`69524796731e10e7243cc5c52dda3fe52f417faad983307bccf82456e789d91f`.
The author also ran the existing stock GitHub Actions selectors locally
on Linux AMD64. All 36 selectors completed in 94.81 s with 160 passes
and four opt-in cases skipped. This duration includes a warm local
build cache and the test links, and is not a runner time estimate.

The exact [independent review](../reviews/peer-candidate-idle-command-stop.md#composed-registered-daemon-review)
covers the source integration and its local runtime tests. Live
installed Both with retained results, effect-boundary loss, macOS and
Linux ARM64 have separate validation.
