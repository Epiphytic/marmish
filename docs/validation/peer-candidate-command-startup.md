# Candidate Command startup validation

Source `a992a181315457f0f41e44924afdb3d361ff077b` on Linux AMD64.
The [phase design](../phases/peer-candidate-command-startup.md) and
[independent review](../reviews/peer-candidate-command-startup.md)
record the approved boundaries.

| Check | Result |
| --- | --- |
| Candidate `cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib --no-run` | Passed, 46.57 seconds |
| Exact candidate libtest `command_startup --test-threads=1 --nocapture` | 5 passed, 63.11 seconds |
| Exact candidate libtest `command_engine::tests::fatal:: --test-threads=1 --nocapture` | 11 passed, 1.26 seconds |
| Candidate `cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --all-targets -- -D warnings` | Passed, 29.74 seconds |
| Stock `cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings` | Passed, 30.38 seconds |
| `cargo fmt --all --check` and source diff whitespace check | Passed |

Candidate commands used
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`.
Cargo checks used the exact source Git ID as `MARMISH_BUILD_COMMIT`, two
build jobs, no incremental compilation, and dev and test debug info disabled.
The archived libtest SHA-256
`46326733bf22a038b2abe461af4aa9da6f2073bfe1063cfe203d9ab99649585a`
identifies the exact binary exercised by author and reviewer.

The fixture uses disposable profiles and the real initialized MDK owner,
Command SQLCipher journal, and Command-engine thread. Each explicit
frontend cut checks the durable configuration and binding prefix. The
late case trips the shared admission from the spawned worker and verifies
the frontend returns the candidate refusal, retains the engine, and can
physically join it. The fixture supplies a private development-layout path
to the same lazy resolver used by production. The client case invokes both
wrappers. Separate stock cases show successful real worker startup and that
failed layout resolution occurs after journal configure and bind.

The adapter remains unmounted from installed candidate startup at this
increment. The fault fixture uses a controlled admission verifier. Actual
default-dispatcher displacement, assembled daemon startup, Command execution
over WSS, and macOS behavior were outside this run.
