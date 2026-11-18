# Peer daemon custody validation

Validated on Linux AMD64 with upstream MDK master resolved at
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`, source
`ac373be2893308c3a64f4db730ab7cd1783aec16`. The opt-in native binary ran the
common daemon entry, a same-UID Unix listener, disposable SQLCipher stages
and two independent authenticated loopback TLS WebSocket relays. Each case
kept Peer approval separate from ordinary Command transport.

| Check | Result |
| --- | --- |
| `peer_daemon_wss` ignored selectors, serial | Three passed in 169.44 s. |
| Earlier `peer_private_wss` ignored custody selectors, serial | Three passed in 99.70 s. |
| IPC | 57 passed, two opt-in benchmarks skipped. |
| Daemon launch | 21 passed. |
| Native binaries, strict workspace and native all-target Clippy, changed Rust formatting and diff check | Passed. |

Manual reached durable Pending, rejected a stale selection, recovered its
committed `Activating` row after daemon restart and a lost decision reply,
and refused an expired retry. Automatic reached the same state with its
own explicit cause. The refusal case exercised missing, Off and Server-only
configuration, metadata-only List after the closed database became missing,
Show and Approve refusal, and exact expiry after restoration of the original
image. IPC tests exercised actual listener abort, queue saturation and
malformed frames. Foreign UID rejection is covered by predicate tests.

Stopping only the Peer relay preserved a same-UID local approval. An
ordinary command through the other relay round-tripped binary input containing
NUL and non-UTF-8 bytes, newline, quotes and shell metacharacters. The client
verified the exact output, exit code and invocation history. Shutdown removed
the original Unix socket and released the profile lease. Relay transcripts
confirmed no Peer carriers on the ordinary relay and no ordinary commands on
the Peer relay.

## Observed latency

Each entry is a single Linux debug measurement over loopback with disposable
accounts. These are wall durations, including owner scheduling and database
opening. They are independent of the injected clock used for expiry tests.

| Operation | Time |
| --- | ---: |
| Manual Offer to Pending | 27.177 s |
| Manual decision | 643 ms |
| Same-generation readback after uncertain reply | 368 ms |
| Automatic Offer to Activating | 19.825 s |
| Manual decision during Peer relay outage | 707 ms |
| Ordinary binary command during Peer relay outage | 5.787 s |
| Selected clean daemon stop samples | 53 ms, 75 ms, 77 ms |

An early outage run hit the default Rust test stack. The final fixture uses
its own bounded 32 MiB thread. An earlier teardown helper recalculated the
socket path after cleanup. The final run verifies the socket path captured
before teardown. No environment override is necessary. These measurements
qualify the private test route. Native macOS, Linux ARM64, PostJoin rotation,
physical leave, installed Peer configuration and live heartbeat are outside
this evidence.

See the [independent code approval](../reviews/peer-daemon-pending-owner-code.md).
