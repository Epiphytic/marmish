# Private Peer status codec validation

The standalone native test target compiles the codec and deterministic
receiver against the production engine policy types. The
[status phase](../phases/peer-heartbeats.md) tracks native Peer authority,
durable projections, and live publication separately.

## Deterministic checks

```sh
cargo test --offline --locked -p marmish-native --test peer_status_wire peer_heartbeat_core -- --nocapture
cargo clippy --offline --locked -p marmish-native --test peer_status_wire -- -D warnings
rustfmt --edition 2024 --check crates/marmish-native/src/peer_status_wire.rs crates/marmish-native/tests/peer_status_wire.rs
```

All 12 core tests passed in 0.08 seconds. Clippy and formatting checks
passed. The tests cover the engine's policy validator and exposed effective
layers, a fixed SHA-256 vector, canonical JSON and base64, bound and clock
refusals, replay fences, freshness and reissue, conflicts, and seven-day
retention. The high-capacity test holds one of 64 pages back, verifies the full
368,640-byte reservation and withheld permission view, and checks exact
completion after the missing page arrives.

The maximum canonical valid heartbeat fixture encoded to **1,996 bytes**.
A page with maximum issue timestamp, generation ordinal, and page index
encoded to **8,024 bytes**. The suite also exercised the raw 4 KiB heartbeat
and 8 KiB envelope boundaries including whitespace.

Ripwire's baseline comparison reported 18 gating rows from the standalone
test target. One marked a receiver enum used by the module and tests as
dead code. The other rows matched guard and encoding shapes across separate
protocols, including helpers scoped privately to the terminal proof and
stream crate. The independent review and compiler checks exercised the
concrete implementations.

## Local benchmark

```sh
cargo test --offline --locked -p marmish-native --test peer_status_wire peer_status_codec_benchmark -- --ignored --nocapture
```

Linux x86-64 on an AMD Ryzen AI Max+ 395, rustc 1.97.1, unoptimized Cargo test
profile. The ignored benchmark times individual calls with `Instant`.
Permission samples include receiver creation, all page decoding and staging,
digest verification, canonical parsing, and final view lookup. The measured
bytes are complete application envelopes for an operation. The heartbeat
fixture contains synthetic username, uptime, load, one disk, and two
documentation-range addresses. The representative permission fixture uses
four pages, and the at-capacity fixture uses all 64.

| Operation | Samples | Wire bytes | p50 (µs) | p95 (µs) | p99 (µs) | Ops/sec | MiB/sec |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Heartbeat decode | 2,000 | 346 | 9.38 | 11.86 | 14.78 | 102,417 | 33.79 |
| Heartbeat receive | 2,000 | 346 | 9.63 | 12.96 | 14.10 | 94,714 | 31.25 |
| Permission, ordered | 80 | 27,992 | 1,038.30 | 1,451.26 | 1,919.67 | 908 | 24.24 |
| Permission, shuffled | 80 | 27,992 | 1,019.40 | 1,100.96 | 1,679.03 | 953 | 25.44 |
| Permission, shuffled at capacity | 25 | 511,414 | 21,757.37 | 24,870.21 | 27,207.36 | 45 | 22.08 |

These development-machine measurements describe local processing cost.
The upper percentiles for the 25-sample run are particularly sensitive to
machine scheduling. End-to-end relay delivery and encryption measurements
belong to the live transport validation.
