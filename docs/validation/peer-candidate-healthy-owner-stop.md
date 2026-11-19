# Registered healthy owner stop validation

Validation used Linux AMD64, Rust 1.97.1, and locked offline dependencies
with mainline MDK at `bdf9833e`.
Source `7701bb3d6c14f3dda7cfcc8870691e524fbc0362` builds on
`7978fbb`, which merges the composed ordinary physical cut and the
checked active Command effect boundary. The candidate libtest link took
47.52 seconds. Strict candidate Clippy covered all `marmish-native` targets
in 30.94 seconds. The SHA-256 of the archived test executable was
`0b61232be9eea56f8b64a98af83ee79ea41622b3ecc7bec0d9db19058f023d89`.

Run the focused tests from the isolated source with the existing candidate
build options:

```sh
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'
export RUST_MIN_STACK=33554432
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib mdk_adapter::healthy_stop::tests -- --test-threads=1
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib mdk_adapter::physical_cut::tests -- --test-threads=1
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib mdk_adapter::physical_cut::tests::ordinary_owner_physical_cut_over_private_wss -- --ignored --exact
cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --all-targets -- -D warnings
```

| Scope | Observed evidence |
| --- | --- |
| Engine custody | Three tests passed in 0.41 seconds. Two real Stop sends advance one incomplete turn to a joined worker. A sticky cut transfers the in-flight engine and retains it until its held worker exits. Legacy conversion refusal and a deliberately failed blocking observer preserve uncertain ownership. |
| Healthy owner and physical barriers | Thirteen tests passed in 71.33 seconds. Native cleanup succeeded with no engine and with a real started and quiesced engine. A joined but unquiesced worker, inconsistent owner authority, and sticky cuts during preparation or final cleanup each withheld healthy cleanup. Existing runner and forwarder uncertainty tests held the lease. The opt-in WSS test ran separately. |
| Real private WSS fallback | One test passed in 6.98 seconds. The existing physical path reaped the worker and runner, closed the exact raw socket, and preserved Journal and MDK contents under the dispatcher cut. |

The healthy-stop tests use disposable native state. The private WSS regression
runs with a loopback relay and disposable keys. The changed Rust files
passed formatting checks apart from an existing assertion layout in
`command_engine_admission.rs`, reproduced on the baseline. The Git diff
whitespace check passed. The mode-specific tests exercise
the owner adapter. The daemon retains responsibility for actual listener
completion, external stop reporting, and final process disposition. The
lease is intentionally retained on failed or uncertain cleanup. The ordinary
daemon entry and platform support are qualified by separate integration.
