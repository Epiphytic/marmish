# Native Terminal default and new writer selection

This increment connects verified Command authority, an in-memory Terminal room,
and the native tmux writer supervisor in an opt-in loopback test. The daemon
scheduler, foreground executable activation, observer and takeover follow in
later increments. Reuse the [room enrollment](terminal-room-enrollment.md),
[frames](terminal-frames.md), [private worker](terminal-live-supervisor.md),
and [socket owner](terminal-socket-owner.md). Both enrollment and selection use
volatile MDK stores. The durable registry keeps the existing tmux ownership
metadata. The required baseline is the reviewed
[direct-WSS admission](terminal-live-enrollment.md) increment with
`terminal_live_enrollment` mounted in `mdk_adapter`, the Command-backed
`Authority` modes and `Owner`'s alias watcher and handoff. The reviewed
R7 enrollment code must land before this implementation.

## Selection records and wire limits

Use application kind 24868. Its content is the canonical lower-case hex
encoding of the fixed binary record. Cap the complete encoded Marmot event at
768 bytes before decoding it. Reject non-ASCII hex, odd length, uppercase and
content above 180 bytes before allocating decoded bytes. After reading the
fixed prefix, require the exact content length for the opcode. A record
has a one-byte version (1), nonzero 32-byte room transcript, nonzero random
16-byte transaction ID, and one-byte opcode. The following fields use network
byte order and exact lengths.

| Opcode | Direction | Payload |
| --- | --- | --- |
| 1 Request | Client to server | Choice byte (1 Default, 2 New), u16 rows in 2–250, u16 columns in 2–500 |
| 2 Accepted | Server to client | Nonzero 16-byte claim, nonzero u64 claim generation, nonzero 16-byte stable slot ID |
| 3 Busy | Server to client | Empty |
| 4 Refused | Server to client | Reason byte (1 capacity, 2 permission, 3 unavailable) |
| 5 Confirm | Client to server | Exact claim and generation from Accepted |

Reuse the native app receiver's exact group, origin, REQ, peer sender, MLS
leaf and epoch checks. Require empty tags and use the receipt's existing
`fresh` predicate in the selection decoder after native verification.
Valid signed age is at most five seconds ahead and less than 30 seconds old. Reject trailing bytes, unsupported variant
values, wrong direction, wrong transcript, zero fields and responses for any
transaction except the outstanding client transaction. A client can send one
Request and one Confirm. A server can send one outcome. Selection retires on
unexpected input including frame kind. After handoff, the frame owner rejects
kind 24868. Durable application rooms never interpret selection records.

Busy reveals to its authenticated account that the account's Default slot is
occupied. It carries no slot, claim or inventory. Map the supervisor's owned
capacity error to capacity and its grant-denied error to permission. Map
deadline, stopped, launch, backend, registry, missing-slot, and all remaining
errors to unavailable. Never return local error strings.

## Source, clock and ownership

Use one serialized selection owner with phases waiting for Request, waiting
for native Start, waiting for Confirm after Accepted, and attached or retiring.
Before submitting Start and after each asynchronous boundary, compare the
fresh checked enrollment binding with the original and check the current
server grant. Await the one-shot Start result under the authority's cancellation
and both admission ceilings. If the room or native source retires, drop the
receiver immediately. The bounded raw frontier can queue arrivals while Start
is pending. Its overflow retires authority and cancels the wait. The owner
validates queued records before handing off. A duplicate Request or early
Confirm arriving during Start may encounter an already allocated pane, but
it cannot yield an attached writer or another Start. Only the successful
one-shot Start result can authorize the server handoff. The existing supervisor performs its own grant and fence
checks before allocation and again before physical I/O.

Bind the checked witness to the exact peer MLS leaf index as well as the
existing local leaf, epoch, route and native subscription string. Resolve the
peer leaf from that room's enrolled two-member MLS roster under the verified
client Nostr key. Extend `RoomWitness` with that index and `App` with the
checked inbound MLS sender leaf from `Room::receive_app`. Compare them before
disposition and compare each fresh witness to the first admitted witness.
The native SourceId leaf field is the 32-byte authenticated client account
whose MLS leaf was thus checked. The SourceId epoch is 1 from the checked
native enrollment epoch. The SourceId subscription
is a process-local nonzero monotonically allocated token issued once for each
exact room and native subscription string pair by the single proof owner.
Keep the exact native witness beside the token. Never derive the token by
hashing or parsing the string. Never reuse a token and refuse overflow.
A witness loss or selection cancellation causes the selection owner to retire
Source through its scope guard. At handoff transfer this guard to the attached
frame owner, which retires it on witness loss and on Drop.

Create the supervisor Source with the local Command fence Arc. The two
endpoints each check their own Command generation. Do not put the generation,
uid or native subscription on the wire. Capture the paired Instant and
suspend-aware continuous sample when bootstrap records the matching
confirmation locally, where the existing `confirmed_at` is established.
Set an absolute continuous admission ceiling using this sample and the same
30-second window, clamped by any earlier enclosing client Start ceiling
tracked from client Start. Before accepting Request and at each later
boundary, verify both clocks against their captured ceilings and reject
uncertain time. Where a wait accepts only one clock, convert the earlier
remaining duration using checked subtraction and addition to the current
clock and recheck both after waking. The supervisor applies its existing 10-second Start
budget within that ceiling. Start Confirm's five-second timer before trying to
send Accepted and clamp it to both original admission ceilings. The room
semaphore caps concurrent pending rooms, including unconfirmed candidates.
The old enrollment budget ends only on successful handoff. The attachment
authority and native Source both get an eight-hour absolute ceiling, which is
within the Terminal room's 24-hour ceiling. Idle and authority loss may end it
earlier.

## Uncertain completion and retention

There is no in-room retransmission or cached outcome replay. Lost replies,
duplicates, failed or uncertain send, expiry, and cancellation retire the
room and Source. A dropped pending Start reply notifies the supervisor by
closing the oneshot. Once Start succeeds, immediately put AdmittedTerminal
in the selection scope guard. Source retirement and dropping AdmittedTerminal
ends the worker link and preserves the established pane retention and reaping
semantics. Both Default and New can leave a detached pane after a lost reply
once the manager has committed a retained slot. Retain that pane in the normal
bounded registry, charge it against the slot cap, and keep the retiring link
against the live limit until reaped. There is no hidden retry for New. A later
explicit New is a separate operation. Default can report Busy while the
previous writer is still live and Refused/Unavailable while its source is
retired and cleanup continues. It can resume the retained Default after reap.
Other retained panes remain subject to normal slot selection and retention
rules. At the cap further New attempts return capacity. The proof fixture
releases all its owned slots through exact `LiveControl::release` after the
workers have reaped and checks that no pane survives. Production release
controls belong with the later session inventory and selection CLI.

Derive the frame binding directly from the exact Accepted supervisor claim
bytes and claim generation. Only an exact Confirm in the same current native
room releases the server owner to hand off the admitted worker. Before that,
neither endpoint delivers user input. The server attachment type requires
the guard that owns the real AdmittedTerminal. The independently typed
client result owns no local server worker. Remove FakeWriterClaim and its
constructor from the shared enrollment path. The isolated earlier enrollment
test may use a private test-only transition helper that cannot construct
either selected attachment type. Before reporting installed writer readiness,
retire and join the alias subscription as specified by the enrollment phase
and recheck the native witness. Keep a Busy or Refused room out of the worker
handoff path.

## Proof criteria

Use two independently accepted Command owners, disposable profiles, a real
isolated TLS relay requiring NIP-42, and the actual tmux worker under an
unprivileged test UID. Check byte-exact input and physical echo, new native
room and claim on reattachment, retained Default, second writer Busy, New
separate pane, ordered resize, authority retirement ahead of the next physical
input, endpoint loss, and ordinary Command after terminal retirement. Measure
cold and warm admission, key-to-physical-echo p50/p95, and throughput with
the measured batch size and platform recorded.

Cover wrong direction, transcript, witness, stale grant, zero or mismatched
transaction, Confirm before Accepted, Request again after the first Request,
wrong Confirm generation, cross-kind traffic in both directions, dropped
one-shot reply for New, and lost Accepted notice. Before a valid Request,
none may allocate a pane. After Start, malformed input must revoke without
further effects. Count actual tmux slots at Start, after a duplicated Request
or lost reply and after retirement. In a fresh room, exercise Default during
reap (Refused/Unavailable) and after recovery. Scan sampled relay event JSON and durable test
state for plaintext selection nonce, claim bytes, and selection content
using private generated sentinels. Keep compact deterministic codec and
transition checks in focused CI and make real relay tests and benchmarks
opt-in.
