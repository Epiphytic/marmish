# Provisional SQLCipher quota validation

The opt-in proof ran on Linux AMD64 as an unprivileged user (effective UID
1001) with bundled SQLCipher 4.17.0, MDK `9a29d1e`, and the checked-in lockfile.
This reports the sum of the database and DELETE rollback journal's logical
lengths. The managed stage cannot create WAL or SHM. After each I/O operation that could change the file size, the wrapper checked
both paths with independent metadata under the same lock. It recorded a
peak only when that check matched the tracked extents.

| Case | Limit (B) | Peak logical bytes (B) | Result |
| --- | ---: | ---: | --- |
| Actual MDK fresh open, generation, encrypted reopen | 4,194,304 | 2,429,504 | Signed artifact reopened with the same ID. Wrong cipher key rejected. |
| Actual MDK fresh open and retry | 0 | 0 | Both attempts refused during migration. The private main file stayed empty. |
| Keyed journal exhaustion | 57,344 | 53,780 | One rollback-journal write refused with `SQLITE_FULL`. |
| Keyed main-file exhaustion | 65,536 | 61,968 | One main-file growth refused with `SQLITE_FULL` after journal writes succeeded. |
| Keyed hot-journal recovery | 65,536 | 53,776 | Abrupt exit after a synced journal and a non-header DB write. Same-cap reopen restored the committed row. |
| Exclusive WAL attempt | 4,194,304 | 49,672 | The VFS refused the WAL open. No WAL or SHM file was created. |
| Exact combined write and truncation boundaries | 32,768 | 32,768 | With 8,192 journal bytes already allocated, the main file reached the exact limit. One-byte overflow by main write, journal write or main truncation was refused. |

Both keyed exhaustion cases retained prior contents through same-cap reopen
and allowed a smaller committed write. Direct sibling and ATTACH opens were
refused. Directory aliases resolved only to the charged stage if admitted.
An over-cap file was rejected before VFS registration. No WAL, SHM or separate
database remained in the fixture directory. The harness forwards only fixed
progress markers and numeric reports. The parent process VFS was unchanged.

The three opt-in tests passed in 15.10 s. The prebuilt test binary also passed
all 22 provisional and retirement regression cases in 84.07 s, excluding the
latency benchmarks. Strict native all-target Clippy, touched-file formatting
and `git diff --check` passed. Full-crate formatting reports a pre-existing
difference in `native_setup.rs:541` at the base commit. The test binary
excluded the subprocess-only test from direct invocation.

```sh
cargo test --offline --locked -p marmish-native --lib peer_quota_vfs::tests::quota_ \
  -- --include-ignored --skip quota_child --nocapture
# Use the native library test binary printed by Cargo above.
"$NATIVE_TEST_BINARY" peer_invitation::provisional::tests \
  peer_invitation::retirement::tests --skip local_latency --test-threads=2
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
rustfmt --check --edition 2024 --config skip_children=true \
  crates/marmish-native/src/lib.rs \
  crates/marmish-native/src/peer_quota_vfs.rs \
  crates/marmish-native/src/peer_quota_vfs_tests.rs
git diff --check
```

macOS qualification is pending. Abrupt-exit recovery was tested through the
keyed SQLite fixture on the same VFS and library. The actual MDK stage was
tested for clean encrypted reopen. Physical allocation, hostile local
filesystem writers and production Peer activation are outside this proof.
