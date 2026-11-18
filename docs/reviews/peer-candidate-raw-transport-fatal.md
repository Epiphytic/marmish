# Candidate raw Peer transport fatal design review

Claude Code, Claude Sonnet 5.5, Auto permission mode, reviewed the
[phase design](../phases/peer-candidate-raw-transport-fatal.md) against
source baseline `9344290`. The subsequent physical-route publication
`0498e267` changes only documentation relative to that baseline.

| Round | Exact design | Session | Verdict |
| --- | --- | --- | --- |
| R1 | `b9c9bda8e0163c40ab7ea47b646860d539128f99` | `7f01b245-e6bb-4029-ae8d-cf4a3202443b` | Request changes |
| R2 | `34953903a753f1aca5cd965db43443cbfb845b60` | `246583ad-2d60-45d3-9f2d-eb435274a9fa` | Approve |

R2 fixes all five R1 findings. The sole socket permit is acquired
before returning the drain and goes into the underlying stream before
TLS. Checked access observes fatal even when application data is
buffered. Private WSS tests must distinguish raw EOF/reset, a WebSocket
close, and Nostr subscription CLOSE. The deterministic drop-order test
checks the actual slot is still held during raw I/O destruction.

The implementation must preserve stock and Terminal permit behavior
and Linux AMD64 candidate gating, and treat a closed semaphore as
incomplete. A later production registry issuer must bind the process
signal to the exact registered route or retained verifier before the
daemon mount. The design alone establishes no production issuer or
runtime result. The independent review was read-only.

## Completed code review

Claude Code Sonnet 5.5 reviewed exact Linux AMD64 source in Auto
permission mode against the approved R2 design.

| Round | Exact source | Session | Verdict |
| --- | --- | --- | --- |
| R1 | `dc851af05761e37f6881d8326c04e9a980e9f932` | `23c47146-47fb-43a4-8466-3480c0f4f7dc` | Approve |
| R2 | `78359dbb6a1e6c3e4f223150feee8aec27a41d83` | `627d04db-2e80-4853-8f24-8d671e25674e` | Approve by source inspection |
| R3 | `5b319577a1cd5606eb236b5e304069a468907e5a` | `bbcc7961-9fb4-4f0f-be94-5539515ae351` | Approve |

The R2 tests put the fatal cut directly through the real TLS-backed
WebSocket read and write halves with profile cancellation still live.
R2 also preserves existing Ping flush behavior for unregistered routes.
The compiler required an explicit unit type for the pending unregistered
Ping waiter. R3 supplies that type as its only change from R2.

The final reviewer inspected all eight Rust files and independently
verified their formatting, the diff and the exact candidate test binary
digest. Both raw I/O unit tests passed in that independent run. The
reviewer left the opt-in current private WSS run to the author to avoid
overlapping validation. The [validation record](../validation/peer-candidate-raw-transport-fatal.md)
contains the author results. No blocking findings remain.
