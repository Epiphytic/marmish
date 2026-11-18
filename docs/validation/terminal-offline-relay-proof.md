# Offline terminal relay and receive proof

The opt-in native tests ran against pinned Marmot MDK
`8abc51befbf71a1ae3bdbb25eb0860c731e75db2` and the pinned native Nostr SDK.
The relay used a fresh in-memory CA and a single IPv4 loopback WSS listener.
The production build check excludes the private fixture trust constructor and
pre-ingest observer. Run from the repository root with the real worker built in
Cargo's target directory.

```sh
cargo build --locked --offline -p marmish-native --bins
timeout 180 cargo test --locked --offline -p marmish-native --lib \
  terminal_route_proof_tests -- --ignored --test-threads=1 --nocapture
timeout 180 cargo check --locked --offline -p marmish-native --lib --bins
```

Three tests passed in 34.80 seconds on Linux x86-64 with a debug test build.
The preceding test build took 10.12 seconds and the non-test build check took
6.65 seconds on a warm dependency cache.

| Case | Observed result |
| --- | --- |
| TLS trust | The SDK refused the fresh fixture certificate with standard roots and refused a different loopback port with the fixture trust profile. It connected with the matching private CA and URL. |
| NIP-42 reads | The relay refused unauthenticated reads, AUTH with the wrong challenge, wrong-recipient and multiple-recipient reads, and broad filters, including kindless and mixed-kind inputs. A valid, fresh AUTH followed by an exact recipient filter reached `EOSE`. |
| NIP-42 writes | The qualifying relay accepted actual kind-1059 and kind-445 signatures distinct from the connection signer. An unknown signer failed AUTH in known-accounts-only mode. Both kinds failed when outer authors had to match experimental AUTH signers. |
| Cross-room dispatch | Each of two independent Terminal lane adapters received zero copies of the other room's exact events. The tests exercised active wire subscription IDs, the direct adapter path and the raw group-copy path. Matching deliveries succeeded. No Terminal KeyPackage publication attempts or relay kind-30443 writes occurred in the experimental-only test. |
| Same-account generic dispatch | A valid Welcome with an unissued subscription ID yielded one delivery to its intended adapter. This characterizes the pinned API. |
| Durable Command controls | The two real Command owners exchanged Welcome and group events through the SDK and WSS fixture, and the server ran an allowlisted `cat` through `CommandEngine` and the real `marmish-worker`. All four controls returned exact binary stdin on stdout, empty stderr and exit status zero. The server engine was constructed directly because the Cargo unit-test executable has no installed image layout. |
| Durable Command wrong-route dispatch | Exact Terminal events on live Command subscription IDs produced zero native pre-ingest hits in both owners and no matching markers in their live stores. The server's baseline and post-negative-batch counts were both 2 inbox markers and 9 processed transport IDs. The stores remained clear of those Terminal IDs when reopened with their SQLCipher keys. After an independent Command positive, the reopened server had 2 inbox markers and 12 processed transport IDs. |

The live SDK test dispatches production-format signed Terminal events to the
matching native adapter. The in-vitro test separately authenticates both
founder orientations and decrypts application payloads in MDK memory stores.
This increment has no result for retransmission after a cold Command actor
restart, a four-room fixture ceiling, host fault injection, the full durable
artifact scan, installed binary interoperability, external relay admission or
Tor. The loopback fixture exercises transport and staging, with no PTY or
production Terminal entry point.
