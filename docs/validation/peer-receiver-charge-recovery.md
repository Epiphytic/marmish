# Managed recipient rollback validation

The test-only Linux AMD64 source uses the same leased cursor, durable Unknown generation and exact original inode from [the charge-scope design](../phases/peer-receiver-charge-scope.md). The recovery turn uses the bundled SQLCipher and a fixed schema read. It checks a valid journal header and an unambiguous trailer, then corroborates them through the handle that the pager uses. A successful result requires observed playback writes, main sync, exact journal deletion, closed handles and a fresh full physical inventory scan.

## Isolated author source

These results apply to `760933cd477a769115c3d3b3fe71b6cc33f885fc`.
The author measured rollback at 2.47 s. The independent reviewer reran
it separately in 1.91 s. The raw rollback, retirement and bounded Welcome
checks in this table were run on the isolated source.

| Linux AMD64 check | Result |
| --- | --- |
| Bundled SQLCipher, isolated rollback and ordinary stock WAL/SHM | 1 pass, 2.47 s |
| Fixed journal format | 1 pass |
| Inventory and repeated descriptor enumeration | 4 pass |
| Receiver registration and dispatcher with ordinary SQLite routes | 2 pass |
| Existing raw quota crash and rollback | 1 pass |
| Filesystem retirement and descriptor enumeration | 4 pass |
| Existing bounded Welcome on the separate fixture scope | 1 pass, four subprocess trials, 46.00 s |
| Scoped formatting, diff checks and strict installed-checks all-target Clippy | Pass |

The isolated rollback test starts with disposable profiles and independently checks the opened database value after recovery. It observes a first interrupted database write after journal sync, interrupts playback once, then verifies the journal is retained across that cut. A new process repeats recovery and verifies that stock SQLite continues to use WAL/SHM on another thread. Separate cases with a damaged fixed header and first-record checksum retain their charge and file bytes. The wrong-key case returns no clean scope and retains the main charge. Legitimate pager replay may write before SQLCipher rejects the key.

The test retains ordinary fixture paths and credential material only in private temporary directories. No network is used. The inventory holds a conservative charge when file evidence is missing or ambiguous. A successful local rollback permits a fresh exact protected native verification. Windows, macOS, filesystem power-loss ordering and private installed recipient startup remain separate validation scopes. The tests use process termination on Linux.

```bash
cargo test --offline --locked -p marmish-native --lib --features installed-checks managed_recovery_hot_journal_and_stock_wal -- --ignored
cargo clippy --offline --locked -p marmish-native --all-targets --features installed-checks -- -D warnings
```

This increment qualifies the positive rollback and the tested interruptions and
refusals. The full recovery qualification still needs native guard coverage
for stale/inode inputs, post-measurement extent changes, per-file and combined
growth, sidecars and access modes, and malformed trailer or oversized journal
inputs through the exact managed turn. See [the independent review](../reviews/peer-receiver-charge-recovery.md).

## Integrated source

The composition at `6a8846ec2a6f3b363ec9b7cdf8f90faf15fbe188` on
`091c448` passed all nine selected test cases on Linux AMD64. Managed
rollback with concurrent stock WAL passed in 1.79 s. Inventory and
descriptor checks passed four cases in 0.37 s. The fixed header and receiver registration tests passed. The dispatcher and ordinary SQLite
regression passed in 10.93 s, and the abrupt Founder native-write quarantine
passed in 8.50 s. Strict installed-checks all-target Clippy passed in
43.32 s.

The 2026-11-18 log is reproducible with these selectors. Use the
project's warm Cargo cache for offline runs.

```bash
export CARGO_NET_OFFLINE=true
for test in managed_recovery_hot_journal_and_stock_wal dispatch_receiver_registration dispatch_native_checkpoint_and_ordinary_routes founder_hot_native_create_quarantines; do
  cargo test --locked -j2 -p marmish-native --lib --features installed-checks "$test" -- --ignored
done
for test in peer_receiver_inventory recovery_header; do
  cargo test --locked -j2 -p marmish-native --lib --features installed-checks "$test"
done
cargo clippy --locked -j2 -p marmish-native --all-targets --features installed-checks -- -D warnings
```

The positive cold rollback case checks the recovered value and absence
of the journal. An exact post-recovery charge assertion remains open.
The wrong-key case checks a conservative minimum main charge.
The interruption uses process exit 80 while the system page cache
survives. Power-loss ordering and torn writes require separate tests.

## Measured guard matrix

Exact matrix source `990c8ca03b3cf2f6c7ba6c966ea2bded506bb565` passed
the expanded bundled SQLCipher test in 2.50 seconds. A separate
measured inventory regression passed in 0.23 seconds. Across the four
ordinary inventory regressions, all four passed in 0.50 seconds.
The fixed journal format and two adjacent dispatcher tests passed
along with the raw quota rollback regression. Strict installed-checks
all-target Clippy passed in 40.72 seconds. Scoped Rust formatting and
Git diff checks passed.

- Durable inventory rejects a replaced inode, main or journal length
  changed after measurement, and physical WAL and SHM siblings.
- Seven fresh subprocesses exercise stale generation, foreign
  `xAccess`, growth through journal write and truncate, separate
  main and aggregate caps, and a read-only main request through
  the registered dispatcher. Both measured files remain byte exact.
- A real interrupted journal with a super-journal marker and a
  separately oversized sparse journal both retain the files and
  main bytes when the owner refuses them ahead of playback.
- No genuine stock read-only fallback, cross-platform behavior or
  power-loss durability is claimed. The previous wrong-key limit
  remains. [Independent review](../reviews/peer-receiver-charge-recovery.md)

The integrated source `515557a61235f81debdb3deb6d52bdb60a6cafee`
passed the expanded bundled SQLCipher test in 2.66 seconds and all four
inventory tests in 0.43 seconds. Receiver registration passed in 0.12 seconds,
the abrupt Founder native-write test in 8.99 seconds, and ordinary dispatcher
routing in 8.89 seconds. These are separate Linux AMD64 results from the
isolated matrix author run above.
Strict installed-checks all-target Clippy passed in 43.59 seconds, and
the full workspace formatting check passed.
