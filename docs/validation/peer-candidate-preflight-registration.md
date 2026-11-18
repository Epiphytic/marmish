# Candidate receiver preflight registration validation

Linux AMD64 validation used source `14df37d8fc7c570a9d09f0572b2e6b1324475d67`,
tree `ab1c69daff3ff11a62024fae773b18acb683a770` and locked upstream MDK
`bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`.

The new test starts with a disposable open-ready profile and a locally signed
schema-3 qualification fixture. Four fresh test processes each acquire the
real profile lease. Preparation leaves `peer/` absent. The successful
registration runs before ordinary SQLite opens in the child and forwards a
real create, insert and read through the replacement default. Ordinary open
and delete calls refuse all three reserved names and all nine sidecar names.
The failure cases check a wrong SQLCipher root with the account and device
held fixed, altered qualification records, exact config and manifest drift,
directory mode, symlink and inode substitution. The expired-lease case calls
the dispatcher after the original `Arc` is released, checks the still-valid
forwarding descriptor separately and refuses re-registration. A child that
displaces the default verifies that the indeterminate state stays fatal.

| Check | Result | Time |
| --- | --- | ---: |
| Exact registration subprocess harness | 1 passed | 30.59 s |
| Ready scope | 3 passed | 27.02 s |
| Existing native dispatcher and ordinary-route proof | 1 passed | 17.91 s |
| Existing opposing-order dispatcher registration proof | 1 passed | 0.38 s |
| Default strict native library and tests Clippy | Passed | 28.46 s |
| Default non-test native library check | Passed | 11.20 s |
| `installed-checks` non-test native library check | Passed | 15.33 s |
| Explicit Cargo candidate cfg native all-targets check | Passed | 23.26 s |

The test times exclude the warm unoptimized test compile, which took 42.52
seconds. `cargo fmt --all --check` and `git diff --check` passed. The test uses
no relay connections and qualifies the registrar under the real SQLCipher
library. Daemon mounting, an installed candidate package proof, command
interoperability and other platforms remain separate validation work.

## Reproduce

```sh
cargo test --offline --locked -j2 -p marmish-native --lib \
  candidate_registration_scope_and_irreversible_dispatch -- --nocapture
cargo test --offline --locked -j2 -p marmish-native --lib \
  peer_relay_trust::ready_scope::
cargo test --offline --locked -j2 -p marmish-native --lib \
  peer_quota_vfs::dispatch::tests::dispatch_native_checkpoint_and_ordinary_routes \
  -- --ignored --exact --nocapture
cargo test --offline --locked -j2 -p marmish-native --lib \
  peer_quota_vfs::dispatch::tests::dispatch_receiver_registration \
  -- --ignored --exact --nocapture
cargo clippy --offline --locked -j2 -p marmish-native --lib --tests -- -D warnings
cargo check --offline --locked -j2 -p marmish-native --lib
cargo check --offline --locked -j2 -p marmish-native --lib --features installed-checks
MARMISH_BUILD_COMMIT="$(git rev-parse HEAD)" \
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate' \
  cargo check --offline --locked -j2 -p marmish-native --all-targets \
  --target x86_64-unknown-linux-gnu
```
