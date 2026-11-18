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
