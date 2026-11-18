# Candidate fatal Command worker validation

The bounded unmounted worker and keeper tests were run on Linux x86-64
against source `d3646178105f68ec7a7abba03a38144c656c2e2d`. The executable
SHA-256 was `350936e0b48666ab2b7f632c67610cee25f0be0855171658665dd6febbbb6a69`.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |
| `cargo clippy --offline --locked -p marmish-native --lib --tests -j2 -- -D warnings` | Passed, 27.53 s |
| `cargo test --offline --locked -p marmish-native --lib --no-run -j2` | Linked, 42.60 s |
| Exact libtest `command_engine::tests --test-threads=1` | 28 passed, 1 pre-existing opt-in ignored, 14.40 s |
| Exact libtest `command_lifecycle::tests --test-threads=1` | 6 passed, 4.20 s |
| `cargo check --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib -j2` with candidate cfg and exact build commit | Passed, 9.05 s |

Tests covered the refusal before opening storage, saturated control and event
channels, a late finite-operation error, hidden buffered observations, typed
fatal stop, repeated healthy stop after a panic and later fatal join, and
lease retention until an actual thread join.
A disposable subprocess admitted Stop using a real keyed engine, cut before
owner destruction, joined the worker and proved that reopening the same
storage still returned `AlreadyOpen` within the subprocess. In a healthy
candidate worker, Cleanup emitted `Failed` while the live engine still held
its storage lock. Normal Stop released that healthy lock. The stock worker
continued to open, finish its operation and hold its ordinary Cleanup guard
until shutdown.

The opt-in contended output-lock fixture requires a linked native worker
binary and was outside the focused libtest run. These checks exercise local
Command worker ownership, with no installed caller for `start_candidate`.
The candidate daemon, registry recheck, inner SQLCipher opens and partial
destructors, actual relay/Terminal shutdown and deployment qualification
remain separate composition gates in the approved plan.
