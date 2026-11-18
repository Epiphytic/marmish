# Offline Peer cursor and deferred store validation

The Rust integration target uses disposable file-backed SQLCipher databases and
synthetic opaque event bodies. It exercises durable scan and retry transitions
through file reopen. Socket attribution, signed-event classification, clock
qualification, MDK effects and live relay behavior remain integration gates.

## Local checks

The following commands passed with Rust 1.97.1 on x86_64 Linux. The ten core
checks completed in 16.97 seconds excluding compilation. The wrong-key case
intentionally produces a SQLCipher authentication error before the test confirms
that the original database opens with the correct key.

```sh
cargo test --offline --locked -p marmish-native --test peer_cursor_store peer_cursor_core
cargo clippy --offline --locked -p marmish-native --test peer_cursor_store -- -D warnings
cargo test --offline --locked --release -p marmish-native --test peer_cursor_store peer_cursor_benchmark -- --ignored --nocapture
```

The core checks cover inclusive cutoff and timestamp ties across reopen,
periodic cadence and recovery, quarantine and requalification, the visible cap,
missing and mismatched page dispositions and failure atomicity, SQLCipher key
and binding checks, hardened SQLite settings, and trusted effect rollback on
error and unwind. Deferred checks cover bounded admission, exact conflict
replacement, stale receipts and retry proofs, backoff and wake coalescing,
clock regression and rebasing, fresh expiry classification, and retry service
amid continuously arriving records and repeated reopen.

## Local storage benchmark

Release-profile measurements on an AMD Ryzen AI Max+ 395 use 100 samples
after 10 warmups. Both workloads use a local file-backed SQLCipher database
with full synchronous writes and DELETE journaling. The page measurement times
validation and the durable cursor commit after four dispositions are ready.
The retry measurement times a due read and a committed reschedule. Byte counts
are raw input sizes. Timings include local database and filesystem overhead.

| Workload | Raw bytes | p50 | p95 |
| --- | ---: | ---: | ---: |
| Full page cursor commit | 262,144 | 47.935 ms | 77.223 ms |
| Due read and retry reschedule | 65,536 | 97.072 ms | 134.305 ms |
