# Terminal listener pacing and rotation validation

The deterministic tests ran in the existing `terminal_runtime_core_` selector.
They drive the 64-scope profile ledger with a paused Tokio clock, assert the
initial and capped retry schedule, and test eviction while waiting. At the
cap, each selected listener attempts registration every two seconds while
eligible, averaging 0.5 attempts per second under sustained refusal. The old
25 ms task-recreation cadence allowed about 40 attempts per second per room.
These are scheduler intervals. They do not measure live relay throughput.

The exact registered-listener tests exercise a 32-fence registry, including the
30-second boundary, 16 held fences, successful entries following a soft
capacity refusal, and delayed progress after old snapshots drop. The offline
fixture additionally creates 34 signed native MLS Command rooms across two
accounts in one SQLCipher server profile. It verifies the real room selection
and authority registration path at two scheduler times. Retained snapshots
initially block both new rooms. After idle clones drop, both later rooms appear
and the eight held active fences remain live with identical identity. This
fixture uses no relay subscriptions. Normal attachment capability is verified
separately by the bundled private WSS writer fixture.

The WSS fixture uses temporary profiles, NIP-42, real native workers and tmux.
It exercises ordinary Command alongside interactive output, byte handling,
resize, Ctrl-C, Busy, New and reattach in the debug Linux x86_64 runtime.

| Check | Result |
| --- | --- |
| Focused core selector | 16 passed, 0.01 s |
| Native MLS and SQLCipher rotation | 1 passed, 33.93 s |
| Strict workspace all-target Clippy | Passed |
| Private WSS writer and Command | 1 passed, 77.80 s |

The WSS writer run provides one observation per workload on the same host.
Echo measures the native PTY to the client's local output record.

| Workload | Observation |
| --- | ---: |
| Cold Ready | 8.820 s |
| Warm Ready | 9.563 s |
| Single-key echo | 181.192 ms |
| Ordinary Command before Terminal | 7.529 s |
| Ordinary Command during Terminal | 3.987 s |
| Ordinary Command after Terminal | 5.463 s |

```sh
cargo test --offline --locked -p marmish-native --lib terminal_runtime_core_
cargo test --offline --locked -p marmish-native --lib \
  terminal_runtime_native_rotates_registered_command_listeners -- --ignored
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo build --offline --locked -p marmish-native --bins
cargo test --offline --locked -p marmish-native --lib \
  terminal_installed_runtime_native_writer -- --ignored --nocapture
```

Build the workers and run the WSS test with the same Cargo target directory,
as a nonroot user with tmux installed. The two longer tests are opt-in. The
core selector remains free of relay and tmux dependencies.
