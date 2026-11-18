# Recipient quota registration and measured inventory

Linux AMD64, exact source
`d3ad63e298a950c51704fb6ac284f7aa9f789c66`, locked bundled SQLCipher.
The test-only receiver registration binds one exact set of fixed names
to one live profile lease. The measured clean allowance retains other
file charges, this main's physical excess, and the one-byte strict
aggregate margin. No installed receiver or managed recovery is enabled.

| Check | Result |
| --- | --- |
| Recipient inventory and repeated dirfd-relative scan | 4 passed, 0.43 s |
| Opt-in receiver/fixture registration in both orders | 1 passed across 2 fresh subprocesses, 0.11 s |
| Existing opt-in native dispatcher and ordinary stock routes | 1 passed across its isolated subprocesses, 7.88 s |
| Joined retirement dirfd regressions | 3 passed, 0.15 s |
| Existing opt-in capped Welcome native proof | 1 passed across 4 subprocesses, 49.40 s |
| Existing raw bundled SQLCipher quota cases | 3 passed, 5.73 s |
| Strict native all-target Clippy with `installed-checks` | Passed, 40.33 s |
| Scoped Rust formatting and staged whitespace | Passed |

```sh
cargo test --offline --locked -p marmish-native --features installed-checks --lib peer_receiver_inventory
cargo test --offline --locked -p marmish-native --features installed-checks --lib dispatch_receiver_registration -- --ignored
cargo test --offline --locked -p marmish-native --features installed-checks --lib dispatch_native_checkpoint_and_ordinary_routes -- --ignored
cargo test --offline --locked -p marmish-native --features installed-checks --lib paths::retirement::tests
cargo test --offline --locked -p marmish-native --features installed-checks --lib peer_capped_welcome_native_proof -- --ignored
cargo test --offline --locked -p marmish-native --features installed-checks --lib peer_quota_vfs::tests::quota_ -- --ignored --skip quota_child
cargo clippy --offline --locked -p marmish-native --all-targets --features installed-checks -- -D warnings
```

The author reused the exact built test executable for non-inventory
selectors. Registration negatives covered a different profile, fresh
lease on the same profile, changed checked directory, and named stock
VFS displacement without replacing the default VFS. Ripwire identified
historical churn, test-only symbol visibility, and the expanded recovery
module's size. Native execution with this receiver's measured allowance,
macOS behavior, and the installed startup and relay path remain for
their later proofs.
