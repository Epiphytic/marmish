# Native candidate 53f94f9: macOS verification failure

[Candidate 42400376357/1](https://github.com/Epiphytic/marmish/actions/runs/42400376357)
used source `53f94f99d06942a58f52fd4a76048a26ced1a99d`.
[macOS job 128486633561](https://github.com/Epiphytic/marmish/actions/runs/42400376357/job/128486633561)
ran 18:15:35–18:35:39 UTC on 2026-11-07 and failed native verification.
Remaining Linux builds were cancelled after that failure; no installed cohort
was dispatched, no candidate qualified and no release was published.

The [sanitized machine-readable record](streaming-candidate-53f94f9-failure.json)
contains API/log provenance and exact hashes. Counts below are per target;
nested isolated children are not additional tests.

| macOS target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| Engine library | 148 | 0 | 1 |
| Engine encrypted_storage integration | 2 | 1 | 0 |
| Engine engine integration | 2 | 12 | 0 |
| Engine native_storage integration | 1 | 4 | 0 |
| Native library | 626 | 1 | 3 |
| Native process_limits integration | 1 | 0 | 0 |

Cargo explicitly warned that engine and native both emit
`target/debug/marmish-worker` and its dSYM. Engine fixtures stage that worker
alone; the native worker requires sibling `marmish`. Launching that worker
would fail before claiming work, consistent with the observed mostly
NotStarted/InputClosed engine failures. The actual winning executable bytes
were not retained. The collision predates this change, so its removal is an
actual build-contract correction and a hypothesis test, not a captured
historical root-cause proof.

The native failure was
`stream_runner::tests::final_handle_drop_releases_full_unacked_output_after_exit`
asserting `!uncertain`. That test observed full stdout but did not establish
its advertised leader-exit premise before dropping the last handle. The log
does not identify the failed cleanup operation or underlying Darwin reason.
Passing neighboring cancellation tests do not waive it.

The [reviewed corrective design](../phases/macos-candidate-worker-and-cleanup.md)
gives the development worker a distinct name and preserves both legacy bundle
consumers. It retains the original race, adds deterministic post-exit and live
full-window cases, and captures test-only operation context. Existing deadlines,
physical-cleanup assertions and process-identity fences remain required.
Focused diagnostics cannot qualify the release; all-platform candidate and
installed qualification still apply.
