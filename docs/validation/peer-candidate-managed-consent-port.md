# Managed Consent publication on one registered Peer port

The Linux AMD64 candidate implementation publishes the retained Consent
frame through the same qualified and NIP-42 authenticated recipient
port that observed the managed Offer. The port owns the five-second
native turn and accepts an exact relay ACK before the existing durable
settlement. That settlement allows one attempt to wake a matching
already deferred Delivery. The recipient actor does not call this
method yet.

## Private WSS matrix

The ignored native fixture generates disposable keys, qualifies a
loopback TLS relay, opens the registered quota and creates a real
charged encrypted package. The exact owner persists and publishes
managed v2 Consent. The wake cases use an authenticated founder
Delivery with native-generated Welcome bytes. The test lets its own
retry advance through the 1, 2 and 4 second waits until a future retry
remains available. It then checks cold SQLCipher readback under the
original registered lease. Each isolated child uses a 40-second
watchdog.

| Case | Wire and durable evidence |
| --- | --- |
| Positive ACK | One exact frame; the matching retry becomes due with a higher cursor revision and the same ancestry. The separate retry and other scoped state are unchanged. Repeating the call is unavailable after durable ACK. |
| Staged image changes before flush | Zero relay frames. The exact attempt and original retry remain durable without an ACK or new cursor work. |
| Relay rejects | One exact frame and a correlated negative response; no settlement or wake. |
| ACK lost | One exact frame reaches the relay. The original deadline retires the route and leaves the exact retry due time intact without settlement. |
| Stop after ACK before settlement | One exact frame. The scope loss leaves only the durable attempt. |
| Stop after durable settlement before wake | One exact frame and durable ACK. The matching Delivery and its cursor revision remain unchanged. |
| Wake transaction interrupted after UPDATE | One exact frame and durable ACK. Both database wall floors retain the earlier ACK time after rollback. The retry, revision and ancestry remain unchanged. |

Every case checks the native Welcome counters and checks the relay
frame count against its initial observation. The frame comparison
checks the authenticated sender, exact recipient, carrier, length and
byte digest without reporting the payload. Cold readback checks durable
attempt and ACK disposition, the retained retry and both SQL wall
floors. A settled Consent is ineligible even at that retained retry
time. The rollback case intentionally observes a later whole-second
wall without starting a new native turn.

## Scope

The test uses an installed-shaped private Linux AMD64 candidate, real
TLS and MDK state. It invokes the same candidate port entry point
directly in test processes. Actual installed daemon mounting, bounded
actor scheduling, later Delivery processing and platform or public
relay qualification require their own evidence.

## Exact source and results

All checks below use `24571ae7cb8b1921a81589c3cca2766b3c4be764` on
Linux AMD64 and its locked upstream MDK. The seven-case WSS suite
passed in 124.28 s. The existing five-case checked ACK WSS proof passed
in 67.10 s and measured local correlated relay ACKs from 48.259 to
58.616 ms. The standalone SQLCipher cursor suite passed 77 tests with
4 ignored in 66.23 s. The focused native cursor fence and due/wake
coalescence tests also passed. The earlier private WSS regression for
Delivery recovery after a refused wake passed in 74.80 s and covered
the actual 60-second retained Consent retry.

| Managed turn | Caller elapsed ms | Remaining five-second budget ms |
| --- | ---: | ---: |
| Positive ACK and wake | 552.353 | 4447.647 |
| Precommit wake rollback | 925.590 | 4074.410 |
| Stop after durable settlement | 561.361 | 4438.639 |
| Staged drift | 483.941 | 4516.059 |
| Relay rejects | 485.907 | 4514.093 |
| Lost ACK | 5006.240 | 0 |
| Stop before settlement | 511.784 | 4488.216 |

Caller elapsed includes package attestation, reserve, socket exchange
and settlement or wake as applicable. The rollback case includes an
intentional real wall-second wait. The lost ACK result includes the
return from the expired turn. These are single local observations in
an unoptimized test profile. Network throughput and latency percentiles
remain unmeasured.

The locked candidate all-target Clippy check for both native and
engine passed in 28.47 s with warnings denied. Stock all-target
Clippy passed in 44.88 s including cache-lock wait. The separate stock
compile check passed in 22.32 s. Scoped Rust formatting and the patch
whitespace check passed.

Reproduce from the exact source with `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
`MARMISH_BUILD_COMMIT` set to the full source SHA. For the Linux AMD64
candidate commands, set
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`.

```sh
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib -- --ignored --nocapture --exact peer_invitation::receiver_inventory::package::tests::checked_ack::joined_port::managed_consent_single_port_private_wss
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib -- --ignored --nocapture --exact peer_invitation::receiver_inventory::package::tests::checked_ack::managed_consent_checked_ack_is_exact_and_single_use
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --test peer_cursor_store -- --test-threads=2
cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native -p marmish-engine --all-targets -j2 -- -D warnings
```

For stock validation, unset the candidate Rust flags and omit the
explicit target when running the same Clippy command and
`cargo check --offline --locked -p marmish-native -p marmish-engine --all-targets -j2`.
