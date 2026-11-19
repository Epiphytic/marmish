# Expired managed Delivery on a registered private Peer port

Exact reviewed source `15fb03fd7ceb98d748b3b28e4c8d985a1a91c630` with
upstream MDK master locked at `bdf9833e`. The Linux AMD64 candidate uses real
private WSS with disposable identities, the registered quota, SQLCipher,
and a measured encrypted MDK package copy. It saves one independently
signed Delivery while live. After the cutoff, retirement authenticates the
retained signed bytes and copied package at exactly cutoff minus one.
Both cursor and Consent clock floors advance with the exact SQL deletion.

The private fixture compares stage bytes, quota extents and charge, native
original-open and Welcome counters, signing count, and relay traffic.
The temporary copy is removed. Changed port scope, stale revision, wrong
saved cutoff, missing or corrupt stage and cancellation leave the deferred
row in place. A stop or cursor name replacement after the tentative DELETE
rolls back both floor changes. The other row is byte-identical after the
successful turn, and can be selected next. It is a test-seeded FlightVerified
row with a distinct ID and prerequisite. The current registered inventory
supplies one eligible Unknown stage, so the fixture establishes SQL selection
and preservation for the later row.

| Private WSS case | Bounded native retirement ms | Full child seconds |
| --- | ---: | ---: |
| Live cutoff and refusal matrix, then exact retirement | 396.412 | 23.62 |
| Hermetic current window after both retained lifetimes | 381.311 | 22.19 |

Both private subprocesses passed in 45.85 s. Full child time includes
first-time stage schema initialization and the scripted cutoff wait. The
five-second operation budget starts at the checked turn and covers copy,
readback, cleanup, re-digest and SQL retirement. These timings are individual
Linux debug-profile measurements.

All seven existing private managed Consent and Delivery subprocess cases passed
in 120.77 s. The native cursor selector passed 15 tests with one opt-in case skipped in
8.43 s. Its deterministic expiry regression covers exact equality at the SQL
boundary. One classifier test covered 13 result cases. An intentional bad
SQLCipher page HMAC is evidence from the corruption case.

The preserved executable embeds build commit
`15fb03fd7ceb98d748b3b28e4c8d985a1a91c630`. Its SHA-256 is
`39c4f36d02d7931a35e16579f0300eb802919266addefacbd210b579e0179df6`
and its ELF build ID is `a1235de378df1b1989311db1d8e2415420451e4e`.
Strict native all-target Clippy passed in candidate mode (31.02 s) and stock
mode (39.25 s). Scoped Rust formatting and diff checks passed.

The leaf is available to future candidate composition. No daemon actor,
stock route, wire or installer behavior changes in this phase. This fixture
qualifies localhost Linux AMD64 only.

## Integrated qualification

Source `a48aeb30eab417d505b6e58e49f816bb00c6c473` combines this leaf with the
private Manual listener and upstream MDK lock. The immutable libtest embeds
that source ID and hashes to
`a2b4e36d09ded88c179098a6e74f0d8f3f0d93ea5e0720328ead35ad7da845ce`.

The two retirement subprocesses passed in 43.82 s. The seven existing private
managed Consent cases passed in 119.92 s. The Manual read-half test passed in
3.99 s, the 25 read-path tests passed, and the cursor check passed 15 tests in
8.32 s with one opt-in benchmark skipped. An expected HMAC error in each
corruption fixture verified refusal. Strict all-target native Clippy passed
for stock in 33.84 s and for the candidate in 31.07 s.

## Reproduce

```sh
export MARMISH_BUILD_COMMIT=$(git rev-parse HEAD)
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUST_MIN_STACK=33554432
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib managed_expired_delivery_private_wss -- --ignored --nocapture
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib managed_consent_single_port_private_wss -- --ignored --nocapture
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib peer_cursor
cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --all-targets -j2 -- -D warnings
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS
cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings
```
