# Peer raw WSS proof reviews

Claude Code reviewed the private native WSS design and implementation in
automatic permission mode with Claude Sonnet 5.5. Scope is the
[test-only socket owner and loopback script](../phases/peer-raw-socket.md).

## Design

| Review | Claude session | Disposition |
| --- | --- | --- |
| Parent Peer design R7 | `d6775d31-9d40-4a76-9d17-f572a632bd9c` | Approved the bounded raw WSS lane and finite-page recovery design. |
| Raw design R1 | `6c94cb14-08ec-4976-98e4-9d9512d62827` | Requested a tighter socket contract and verifiable fixture boundary. |
| Raw design R2 | `b0539650-9e4a-481d-93ad-43c2a016636f` | Requested exact test-only wiring, separate transport and owner caps, and explicit departure from the parent design's page failure behavior. |
| Raw design R3 | `95b29dd5-4d48-42d9-b779-8c76810b8069` | Approved with no implementation blockers. |

R3 checked the SDK WebSocket transport and caps, the loader's test-only module
paths, raw tungstenite framing, and the signed Offer API against the design.
The implementation uses a private receipt constructor, deadline-bounded Pong
flush, an exact publication recipient from the flight, and reconnect after both
socket halves drop. Synthetic event headers cover deterministic ties and order.

## Completed code

Claude code review R1 in session `081d1b1a-96de-4fa9-ac0e-250d02800d29`
found the ownership and wire transitions consistent with the design and
requested stronger tests for exact inbound bytes, a full live reserve during
publication, validation of carrier fields and prepared writes, deadline
boundaries, and real control frames after readiness. It also requested a
private build target after another snapshot replaced one test executable in
the shared Cargo target.

The follow-up preserves relay-reserialized bytes, tests cancellation and
overflow after the publication write, and verifies both live and historical
decoding of the original signed Offer. Receipt retention is tested against a
different JSON representation of the same event. The deadline wrapper is
tested with a paused clock. The WSS cases hold AUTH and EOSE and exercise
actual Ping/Pong during a live wait. The separate total-frame budget is a
unit check because the socket's stricter current state, control and record
bounds prevent a valid transcript from reaching 25 frames.

Claude code review R2 approved the exact revised implementation in session
`15fd35ef-6e96-4523-ab88-e5373ba91320`. It independently passed six offline
core tests, nine private WSS cases, the broader filter with fourteen passing
tests, and strict all-target Clippy in the revision's dedicated Cargo target.
R2 read the owner and wire implementation in full and reported no correctness
or coverage blockers. It noted that the receipt parser leaves inbound
signature verification to the signed codec and that an uncertain local send
error is conservatively ambiguous.

Claude approved the same exact snapshot in the focused fixture and test
review `216f6d72-c61c-4bed-b20e-090b3ff7011b`. It read both new files in
full and passed the combined 23-test filter locally. The review verified
the assertions added after R1 and found no substantive findings. Its
optional notes were to widen short timing windows if needed on slower
runners and to extend direct coverage of valid boundary values. These
WSS cases are opt-in and passed ten consecutive repetitions.
