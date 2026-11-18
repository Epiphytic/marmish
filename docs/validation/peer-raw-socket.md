# Private Peer raw WSS validation

The test-only relay binds an ephemeral IPv4 loopback port and uses an
ephemeral TLS trust chain and two disposable Nostr identities. No public
relay is contacted. The owner exercises the real SDK WebSocket connector
with one slot and the local signed Offer codec. Production application
wiring, cursor commits, Tor, and public relay conformance remain separate
integration work.

## Checks

```sh
cargo test --offline --locked -p marmish-native --lib peer_raw_socket_core
cargo test --offline --locked -p marmish-native --lib peer_raw_socket_native_wss -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
cargo test --offline --locked --release -p marmish-native --lib peer_raw_socket_bench -- --ignored --nocapture
```

On x86_64 Linux with Rust 1.97.1, six offline checks, nine private WSS
cases and all-targets Clippy passed. The core and WSS sets completed in
0.01 and 1.06 seconds after compilation. The WSS set passed ten additional
consecutive runs. The offline checks cover binding failures before a
connection attempt, strict JSON array and event headers, exact body-size
boundaries, resource counters, clock boundaries and invalid prepared
carrier bytes. The native WSS cases check real
signed AUTH, stale and wrong acknowledgments, the exact live and
historical filters, Pong, terminal relay and malformed WebSocket messages,
separate owner and connector caps, ordered finite pages, independent
live and page bounds, predrain, and receipt and connector retirement.
The Offer runs through signed publication, exact byte capture and
authenticated live and historical decoding. A refused publication keeps the
connection ready. An ambiguous or cancelled publication retains its original
prepared bytes while its socket generation is retired.

## Local latency

Release-profile runs on an AMD Ryzen AI Max+ 395 used 12 samples after two
warmups. Readiness includes TCP, TLS, NIP-42 authentication and the first
live EOSE. Publication measures a signed prepared event through matching
positive OK. Delivery measures receipt of a forwarded event through the
signed Offer codec. The latter two start on authenticated sockets. These
are loopback timings.

| Workload | p50 | p95 |
| --- | ---: | ---: |
| Authenticated readiness | 0.474 ms | 0.554 ms |
| Publication and matching OK | 0.083 ms | 0.112 ms |
| Authenticated Offer delivery | 0.147 ms | 0.162 ms |
