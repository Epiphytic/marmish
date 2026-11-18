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

## Integrated validation

The exact Linux AMD64 test executable was built from source
`1dfe9d0a3e4da5904ba6cc9cc52925dd27b65bfb`, tree
`e13bebca98e114e2e1fe0dbc13442263c510cd1c`. Its SHA-256 was
`62a6baeedb81437391bb6299f8eeb90b4cbecb353040334d570a3863cb445c14`.
The integration merge `5d0f33c4488c12bda252028f4f3b071a622c7cc8`, tree
`cbbaa55d5bd4b328a83820c335387f01ba4fbb8f`, kept the complete tested `crates/`
tree and synchronized the previously published documentation. The nine
registration files also match the independently reviewed author source.

| Check | Result | Time |
| --- | --- | ---: |
| Registration subprocess harness | 1 passed, including 4 fresh fixture processes | 21.61 s |
| Ready scope and Pending memory view | 10 passed | 23.99 s |
| Cursor and checkpoint store suite | 76 passed, 4 opt-ins ignored | 56.30 s |
| Private WSS Consent and Delivery, with native dispatcher proofs | 8 passed | 217.80 s |
| Default non-test native check | Passed | 10.52 s |
| `installed-checks` native binaries | Passed | 13.75 s |
| Default strict native library and test Clippy | Passed | 27.95 s |
| Candidate all-target check | Passed | 23.28 s |
| Candidate strict all-target Clippy | Passed | 27.66 s |

The combined opt-in WSS run includes both explicit child entry points, which
return when invoked without fixture parameters. The ordinary Consent and
Delivery wrappers launched real private-relay child processes. The existing
native dispatch tests exercised ordinary forwarding and staged-registration
ordering. This run establishes regression coverage for the integrated source.
It does not mount the registered receiver in an installed daemon. Exact
registration harness build time was 46.05 seconds. Formatting and whitespace
checks passed. See the [integrated review](../reviews/peer-candidate-preflight-registration.md#integrated-review).

## Forwarding verifier after attachment

Exact source `e8bf71f3ccbd8c23facbb8865d1585b17339c42b` extends the
registration with a read-only global witness. The witness can check the
same registry entry, SQLite defaults, and stored forwarding descriptor
after the registered authorization is consumed by attach. Attached checks
bracket the original profile, material and reserved topology with the
independent global verification. Local drift cancels only Peer scope.
Displacement permanently latches the global failure, including when the
default pointer is restored.

| Exact Linux AMD64 check | Result | Wall time |
| --- | --- | ---: |
| Bundled SQLCipher registration libtest build | Passed | 39.45 s |
| Isolated registration harness | Passed in all five cases | 17.77 s |
| Default strict native lib and tests Clippy | Passed | 27.60 s |
| Default binary check | Passed | 8.87 s |
| Explicit candidate strict all-target Clippy | Passed | 27.93 s |

The five cases cover preflight refusal, attached config drift, attached
directory replacement, sticky displacement after attach, and forwarding
after lease drop. The local-refusal cases use real SQL forwarding after
Peer refuses. Claude independently reran the focused harness with the
same libtest executable, SHA-256
`64e6396e0edab01cee37e0d8c057fe5aa94ebb49601c043124a842a76027e2b0`.
The verifier is available to later candidate composition. This increment
keeps it outside the active daemon run path.
[Code review](../reviews/peer-candidate-preflight-registration.md#retained-verifier-review)
