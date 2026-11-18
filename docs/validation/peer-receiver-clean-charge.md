# Recipient clean quota turn validation

Source `75dc953396e5ce79608b1f64ed90b8f36d32f328` atop `25748b2` qualifies the clean receiver charge checks on Linux AMD64. The test process binds a fresh exact receiver mount and uses its leased SQLCipher cursor to recover the witnessed Unknown image. It creates a real keyed SQLCipher copy exclusively and authenticates a schema query through the measured dispatcher scope. The adjacent managed rollback regression exercises the bundled SQLCipher pager on a real hot journal.

| Check | Result |
| --- | --- |
| `measured_clean_turn_reserves_and_rechecks_exact_copy` in a fresh subprocess | 1 passed in 0.44 s |
| `managed_recovery_hot_journal_and_stock_wal` | 1 passed in 2.87 s |
| Deterministic receiver inventory cases | 4 passed in 0.52 s |
| Deterministic quota VFS cases | 2 passed in under 0.01 s |
| Native dispatcher and ordinary route proof | 1 passed in 8.83 s |
| Receiver dispatcher registration subprocess | 1 passed in 0.12 s |
| Strict native all-target Clippy with `installed-checks` | Passed in 17.66 s |
| Scoped rustfmt and Git whitespace checks | Passed |

The copy case verifies exact quota and lease ownership, full copy overlap before file creation, independent allocation before and after the copy turn, preserved original identity and extent, strict aggregate headroom, and refusal before the exact copy is removed. It also exercises a second profile lease, a second quota owner, exclusion against rollback, the original inode and permit in place of the copy, a stale copy extent, and charge changes after the initial reservation. It accepts a reservation one byte below the aggregate cap and refuses one at the cap. The clean scope held by the driver stays separate from its custody transaction.

```sh
cargo test --offline --locked -p marmish-native --lib --features installed-checks -j2 --no-run
cargo clippy --offline --locked -p marmish-native --all-targets --features installed-checks -j2 -- -D warnings
```

The Linux test executable for the recorded runs had SHA-256 `21cde54dbee3b476743dcde21d1dd80e9c4aff51aab0645b574f4b7a6f765397`. Run the two ignored parent cases individually with `--ignored --exact` after the full namespace `peer_invitation::receiver_inventory::tests::`. The unfiltered namespace runs the four deterministic cases and leaves the subprocess cases ignored by default.

The [capped Pending bridge](../phases/peer-installed-capped-pending-bridge.md) owns the later native Welcome proof. This increment establishes local quota measurement and the sealed copy scope for the fixed prepare and readback operations. Original Welcome effects, actual Pending readback, cross-platform qualification, and installed behavior remain separate acceptance tests.

## Composed Linux validation

The same five Rust files were replayed unchanged onto `d26c570` and validated at `b2a0ccc2f2157c93ce58bac523db66ba3499b521`. The exact native libtest linked in 1 min 39 s and had SHA-256 `52e91679fd106b15969327e84fabec2304213bf41df1c7acbcae8ba49603605e`. The keyed copy passed in 0.44 s, managed rollback in 2.69 s, four inventory cases in 0.56 s, and two quota VFS cases in under 0.01 s. Native dispatcher and ordinary routes passed in 8.15 s and receiver registration in 0.11 s. Strict all-target Clippy with `installed-checks` passed in 48.17 s. Scope and qualification limits are unchanged. [Integrated Claude review](../reviews/peer-receiver-clean-charge.md#integrated-review)
