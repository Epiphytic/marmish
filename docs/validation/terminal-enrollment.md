# Terminal room enrollment proof

The test-gated Terminal owner ran on Linux x86_64 beside an ordinary
SQLCipher-backed Command session, with independently generated test identities
and one local TLS WebSocket relay. The frozen baseline was `9520e77`. The
worker was built from the same source as the test. The relay fixture validated
NIP-42 and recorded exact subscription IDs. No public relay was contacted.

```sh
cargo build --locked -p marmish-native --bins
cargo test --locked -p marmish-native --lib enrollment::core_tests
cargo test --locked -p marmish-native --lib enrollment_routes_only_authenticated_terminal_frames -- --ignored --nocapture
cargo clippy --locked -p marmish-native --lib --tests -- -D warnings
```

The four targeted core cases cover strict package schema, author, role and
capability verification, signed age, the transcript vector, the receipt
sequence and grant generation. The opt-in end-to-end test used real protected
Command traffic to establish the carrier, four protected bootstrap records,
an authenticated Package with its unpublished native artifact and a real native
Welcome. The relay delivered the Welcome before Plan. The client held it
without joining, accepted the verified Plan, verified the installed MLS room
and joined the exact registered route. Client and server exchanged all three
protected receipts and eight alternating 4 KiB frames. The fixture compared
native-delivered bytes and independently decoded every Terminal record. It
revoked the active server grant and verified that the existing owner could no
longer send or receive. The relay saw one REQ per exact alias or room
subscription and no remaining live Terminal subscriptions after teardown.

An unmodified native worker ran a real `/bin/cat` Command over the same
relay before and after enrollment. Independent read-only SQLCipher connections
reopened both ordinary accounts and compared native deduplication and
processed-event records, groups, the native KeyPackage lifecycle, Command
rooms, invocations, requests and source records before and after the exchange.
The compared records and relay kind-30443 write count were unchanged. Native Terminal lane state stayed in its own
in-memory SQLite account for this test.

| Warm debug test observation | Value |
| --- | ---: |
| Complete loopback test | 24.73 s |
| Concurrent room owners open and client native package preparation | 2.592 s |
| Room owners start until both roles admitted | 2.834 s |
| Public native KeyPackage bytes before base64 | 452 B |
| Alternating 4 KiB frame send to receiver | 37.040 ms p50, 76.559 ms p95 |
| Frame latency samples | 8 |

These measurements describe one debug run on local WSS. The eight samples
use lower median and nearest-rank p95 and do not characterize public relay or
Tor performance. This proof uses the enrollment deadline for the short
post-Ready sample, and the later daemon owns attachment lifetime and the
live cross-purpose collision index. The tested success path deliberately
receives the Welcome before Plan. Release request/response latency, a
broader live denial matrix, PTY integration, and multi-relay or Tor
enrollment are separate work. A sole
remaining dynamic SDK socket may close before its queued `CLOSE` is written.
The fixture checks the relay's actual live subscription state after shutdown.
