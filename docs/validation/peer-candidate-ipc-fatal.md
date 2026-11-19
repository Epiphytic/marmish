# Registered candidate listener validation

Exact source `8dea700`, Linux AMD64. Strict all-targets Clippy passed for the
engine and native packages in stock and candidate configurations. The candidate
uses `--cfg marmish_peer_receiver_candidate` via target-specific Rust flags.

| Immutable native libtest filter | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Stock, `ipc::` | 60 | 0 | 2 |
| Candidate, `ipc::` | 83 | 0 | 2 |

The candidate checks use real same-UID Unix-domain sockets. The listener
rejects requests already queued before a fatal start, then separately proves
a live Status response and drains a pending Command exchange and partial
Terminal frame on fatal without an explicit drain receiver. The latter also
checks that the owner response receiver is gone and the path refuses new
connections. An already transferred Terminal socket outlives the listener
until the fixture drops the retained handoff. The healthy explicit drain
leaves the latch untripped.

A controlled writer admits only the four-byte length prefix before tripping
the shared cut. The frame body and write shutdown never reach the writer.
This test controls the boundary between socket polls. The installed daemon
still needs integrated tests with real dispatcher displacement and its
retained owner and worker obligations.

See the [independent review](../reviews/peer-candidate-ipc-fatal.md).
