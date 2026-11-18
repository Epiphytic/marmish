# Installed Peer control v2 offline qualification

Source `7c407f2372b0f6d9a5bc1bf4c3bb51ed83e8c286`, upstream MDK
`b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1` from the existing lock.
Linux x86_64, locked offline builds with two jobs, unoptimized profile
with debug info disabled. The fixture uses disposable, in-memory SQLCipher
native engines and requires no relay.

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --offline --locked -j2 -p marmish-native \
    --test peer_control_wire --test peer_accept_receipt_wire --test peer_cursor_store
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo clippy --offline --locked -j2 -p marmish-native --all-targets -- -D warnings
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --offline --locked -j2 -p marmish-native --test peer_control_wire \
    peer_control_benchmark -- --ignored --nocapture
```

| Test target | Passed | Ignored | Run time |
| --- | ---: | ---: | ---: |
| `peer_accept_receipt_wire` | 4 | 0 | <0.01 s |
| `peer_control_wire` | 11 | 1 | 3.79 s |
| `peer_cursor_store` | 55 | 4 | 51.21 s |

The existing v1 receipt ID and signed Offer body stayed stable. A v1
Delivery has no status-room candidate and the explicit v2 decoder refused
a v1 receipt with matching coordinates. The v2 round trip generated a
recipient key package and a founder Welcome with the real MDK engine,
then carried the exact Welcome through both the direct and recovered
Consent paths. Both endpoints derived the same app-room candidate and
enrollment coordinates. The v2 receipt matched those coordinates.

The re-signed negatives exercise unsupported versions, missing, duplicate
and wrongly typed fields, noncanonical JSON, mismatched Offer and Consent
bindings, mixed-version parents and children, and recovery from a transcript
with the wrong signed version. The independent room vector and all six
receipt coordinate mismatches are pinned. The Welcome proof exercises
native generation and exact offline carrier verification. Installed
activation and authenticated MLS receipt acceptance remain separate owner
work.

Strict Clippy passed for all native package targets in 30.46 s.
The changed Rust files passed focused rustfmt and the exact source diff
passed `git diff --check`.

## Offline codec benchmark

Each version ran 10 warmups and 100 measured iterations over the same
fixture shape. Preparation and verification include local cryptography
and encoding. Each Delivery wraps a fixed-size synthetic signed Welcome
for repeatable timing. These are single-host codec measurements. Relay
latency, native group creation and MLS application acceptance are outside
the measured interval.

| Stage | v1 p50 / p95 | v2 p50 / p95 | Frame bytes |
| --- | ---: | ---: | ---: |
| Offer prepare | 1.661 / 2.307 ms | 1.658 / 1.942 ms | 2,585 |
| Offer verify | 1.225 / 1.808 ms | 1.229 / 1.500 ms | 2,585 |
| Consent prepare | 16.224 / 17.846 ms | 16.107 / 17.695 ms | 5,997 |
| Consent verify | 15.509 / 16.928 ms | 15.383 / 16.472 ms | 5,997 |
| Delivery prepare | 4.403 / 6.317 ms | 4.370 / 5.793 ms | 11,457 |
| Delivery verify | 3.319 / 4.384 ms | 3.360 / 4.278 ms | 11,457 |

The opt-in benchmark completed in 10.17 s.
