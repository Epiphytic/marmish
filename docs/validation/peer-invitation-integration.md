# Peer Offer and Consent validation

The client received signed Offers and returned signed, encrypted Consent
over a real TLS WebSocket to a private scripted relay on loopback. The
fixture required NIP-42 authentication under the same disposable Nostr
account that signed the withheld native KeyPackage. A separate disposable
server account opened the Consent and checked its native package against
the client's independently persisted encrypted MDK stage.

The manual recovery test restarts the socket owner and reopens encrypted
SQLCipher state across a negative acknowledgement, an unrelated positive
acknowledgement, a corrupted or missing native stage, and a forced SQL
failure after a valid acknowledgement. It checks byte-for-byte replays,
the persisted 60-second retry schedule and suppression after a durable
acknowledgement. An already expired deadline or retired socket does not
consume the retry slot. Other tests cover the stale generation witness, actual
Unix peer credentials, policy and origin binding, history followed by
live intake, deferred row fairness, stage path custody and unsigned
native recovery without replacing the retained KeyPackage.

## Test runs and measurements

| Target | Passing | Wall time |
| --- | ---: | ---: |
| Invitation core, including cursor and consent primitives | 33 | 63.59 s |
| Invitation WSS integration | 3 | 15.63 s |
| Raw WSS integration and microbenchmark | 10 | 1.95 s |
| Independent cursor and consent integration target | 25 | 45.91 s |

All-target native Clippy passed with `--locked --offline -- -D warnings`.
The deliberately corrupt or wrong-key fixtures emitted expected SQLCipher
page authentication errors and passed their fail-closed assertions.

The table uses the frozen review dependency lock. With upstream MDK
resolved to `0ad3c61088e1acac58e0fcf1a9f1142bae5d997a`, all three private
WSS invitation tests also passed in 22.25 s. The focused unsigned
native recovery test passed in 7.72 s with the same upstream revision.

```sh
cargo test -p marmish-native --locked --offline --lib peer_invitation -- --test-threads=1
cargo test -p marmish-native --locked --offline --lib peer_invitation_live_ -- --ignored --nocapture --test-threads=1
cargo test -p marmish-native --locked --offline --lib peer_raw_socket_ -- --ignored --nocapture --test-threads=1
cargo test -p marmish-native --locked --offline --test peer_cursor_store -- --test-threads=1
cargo clippy -p marmish-native --locked --offline --all-targets -- -D warnings
```

Five sequential debug-build fixture repeats on Linux x86-64 with an
AMD Ryzen AI Max+ 395 and parallel build and review work on the same
host measured these end-to-end turns. Intake starts immediately before
sending the signed Offer and ends after its local durable disposition.
The publication turn includes opening the stage, first native package
generation and persistence, the checkpoint transaction, relay
acknowledgement and its durable update.

| Operation | Samples | Min | Median | Max |
| --- | ---: | ---: | ---: | ---: |
| Signed Offer intake and commit | 5 | 52.9 ms | 90.6 ms | 874.5 ms |
| First native package through acknowledged Consent | 5 | 4.60 s | 6.53 s | 8.89 s |

The raw WSS microbenchmark ran 12 samples per operation. Its medians
were 2.27 ms for authenticated readiness, 0.65 ms for an exact signed
publication through matching OK, and 1.29 ms for authenticated delivery.
The corresponding p95s were 2.81 ms, 1.18 ms and 2.00 ms. These
are local turn measurements and do not estimate public relay latency
or interactive terminal throughput.

Every relay and key was disposable. The phase compiles its internal
owner in production, while the origin constructor remains fixture-only.
These runs cover through relay acknowledgement and server Consent
decryption. Daemon enablement, deployment qualification, Delivery,
Welcome, room activation, and cross-platform relay behavior await later
validation.
