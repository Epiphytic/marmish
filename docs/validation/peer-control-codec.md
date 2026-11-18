# Offline Peer invitation control codec validation

The Rust integration target builds the private codec without linking it into
Marmish's client or daemon. The suite sends no relay traffic. It uses generated
Nostr keys, a current-profile MDK KeyPackage from in-memory test storage, and an
opaque synthetic signed Welcome at the codec boundary. Native Welcome peeling,
package consumption, installation of a room, and relay acceptance remain in
their respective integration phases.

## Local checks

The following commands passed with Rust 1.97.1 on x86_64 Linux. The core test
selection finished in 1.39 seconds excluding compilation.

```sh
cargo test --offline --locked -p marmish-native --test peer_control_wire peer_control_core
cargo clippy --offline --locked -p marmish-native --test peer_control_wire -- -D warnings
cargo test --offline --locked --release -p marmish-native --test peer_control_wire peer_control_benchmark -- --ignored --nocapture
```

The eight core tests check all three authenticated flights, distinct sender and
recipient roles, strict layer and body schemas, exact signed-byte preservation,
per-field cross-stage binding, configured routes, independent clock and retention
cover, causal windows and the native KeyPackage lifetime. The size tests accept
a 20,480-byte synthetic signed Welcome, reject 20,481 bytes, and demonstrate
sender and receiver rejection at a tightened outer cap. A separate pinned NIP-44
check seals a 28,672-byte raw rumor below the 65,536-byte outer cap and observes a
size error at 28,673 bytes.

## Local codec benchmark

Release-profile measurements on an AMD Ryzen AI Max+ 395 use 100 samples after
10 warmups. Consent uses a single prepared MDK KeyPackage. Delivery uses a
signed synthetic Welcome with 2,048 bytes of opaque event content. Each prepare
operation signs and seals a new frame. Verification measures only the local
codec. These timings exclude relay and MLS room operations.

| Flight | Prepare p50 | Prepare p95 | Verify p50 | Verify p95 | Serialized outer bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Offer | 0.163 ms | 0.175 ms | 0.110 ms | 0.119 ms | 2,585 |
| Consent | 0.312 ms | 0.334 ms | 0.252 ms | 0.270 ms | 5,997 |
| Delivery | 0.238 ms | 0.256 ms | 0.169 ms | 0.182 ms | 11,457 |
