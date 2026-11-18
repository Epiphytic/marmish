# Single-origin live Terminal enrollment

The opt-in native fixtures use a local TLS WebSocket relay, two disposable
Nostr identities and an ordinary SQLCipher-backed Command room. The enrolled
Terminal peers use an ephemeral native MLS room. The writer claim is a test
fixture, so these tests validate admission and frame delivery without
allocating a PTY.

```sh
cargo build --offline --locked -p marmish-native --bins
cargo test --offline --locked -p marmish-native --lib enrollment_
cargo test --offline --locked -p marmish-native --lib terminal_live_enrollment
cargo test --offline --locked -p marmish-native --lib terminal_bootstrap::core_tests
cargo test --offline --locked -p marmish-native --lib signed_auth -- --ignored --nocapture --test-threads=1
cargo test --offline --locked -p marmish-native --lib refuses_incomplete_auth_and_retires_lost_exact_wss_source -- --ignored --nocapture --test-threads=1
cargo test --offline --locked -p marmish-native --lib enrollment_routes_only_authenticated_terminal_frames -- --ignored --nocapture --test-threads=1
cargo test --offline --locked -p marmish-native --lib transport::
cargo test --offline --locked -p marmish-native --lib terminal_
cargo test --offline --locked -p marmish-native --lib benchmark_signed_bootstrap -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native --lib --tests -- -D warnings
```

Use the same Cargo target directory for the build and tests. The native worker
resolves its sibling `marmish` executable at startup, so build both before running
the loopback Command controls.

The final frozen slice passed all four opt-in WSS cases, 97 tests selected by
`terminal_`, 21 tests selected by `transport::`, and strict Clippy. The
independent Claude run rebuilt both executables and reproduced all four WSS
results, including exact-source teardown without a panic.

The authenticated path requires the signed NIP-42 event's exact positive
acknowledgement followed by EOSE for the exact inbox subscription. The server
receives a signed Probe published before its subscription existed. The relay
acknowledges an exact retransmission without redispatching it, and the fixture
replays the four signed bootstrap records after both native room owners are
installed. The verified path joins after Plan. The earlier owner fixture also
holds an authenticated Welcome received before accepting Plan.

The test compares the two parties' expected binding and independently
decodes eight alternating 4 KiB frames. It rejects crossed, expired and
empty claims. The final frame arrives after the original enrollment
deadline. Native `cat` invocations verify exact binary and special-character
input and output before enrollment, while the attached Terminal owner is
live, and after teardown. Independent read-only SQLCipher connections compare
ordinary MDK and Command rows around the Terminal work. The relay's
KeyPackage write count stays constant.

Negative fixtures cover a missing AUTH challenge, wrong-ID and negative AUTH
acknowledgements, loss of the exact subscription or WSS connection after
admission, bounded replay backlog and grant revocation before native join.
They check retired source behavior, exact subscription release and permit
recovery. In two replay runs, the spool stored 33 of 40 size-eligible events
and dropped seven. It excluded one 40 KiB alias event before spooling.
The fast checks cover elapsed listener and flight budgets, clock drift,
replay charge, suspect and stale authority, one-shot writer leases and
room capacity held until cleanup.

## Benchmark

The following Rust 1.97.1 debug measurements use a Linux x86_64 AMD Ryzen
AI Max+ 395 host. Cold setup and warm cryptographic exchange use one process
with no sockets. The WSS enrollment fixture uses one local TLS relay.
It measures from the start of Terminal admission after the ordinary
Command room is verified. The frame timing ends at receipt by the native
owner and excludes the deliberate wait before the last frame.

| Sample | Observations | Result |
| --- | ---: | ---: |
| Cold canonical key and alias setup | 1 | 1.469 ms |
| Warm signed wrap and verification | 32 | 2.664 ms p50, 3.437 ms p95 |
| Warm four-record cryptographic exchange | 32 | 11.340 ms p50, 12.678 ms p95 |
| Cold WSS admission through native owner installation | 1 | 2.877 s |
| Cold WSS admission through native Ready | 1 | 3.101 s |
| Native KeyPackage before base64 | 1 | 452 B |
| Alternating 4 KiB MLS frame delivery | 8 | 35.274 ms p50, 76.687 ms p95 |

The fixture samples single-stream latency over one loopback origin. Warm
WSS admission, throughput under contention, additional platforms and
relay origins, and public interactive activation remain separate validation.
