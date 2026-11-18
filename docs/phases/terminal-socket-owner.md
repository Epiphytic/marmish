# Same-UID Terminal socket and foreground display commits

Connect the [reviewed local record codec](terminal-ipc-fence.md) and
[foreground TTY frontend](terminal-frontend-tty.md) through a Unix socket.
First qualify the socket pair and real controlling PTY locally with an
explicit fixture receiver. Admission and native delivery remain separate
owners. No production CLI route activates from this increment alone.

## First request and ownership transfer

Use the existing 64 KiB JSON framing with an additional closed
TerminalStart variant. The fields are canonical remote key, an already
selected exact Command session, local relay selection bound to that
session, initial rows and columns, and a closed attachment request enum.
The actor supplies all authenticated context; no digest, generation,
physical socket path or tmux name in the caller's request is authority.
This fixture increment only implements the initial writer choice, returning
a typed unavailable response for the others. Existing command IPC request and response serialization, deadlines and
half-close behavior stay byte exact. Factor a bounded frame write body
that retains existing length/JSON checks. TerminalStart request and
its success reply use the non-shutting body. The ordinary writer then
half-closes as it does today. TerminalStart failure also half-closes. Test both old readers and old writers.

Keep peer credential and profile checks before the first request.
Classify decoded variants explicitly. Ordinary commands and inventory
retain their existing queue. A matched TerminalStart reserves a
separate bounded startup permit without waiting, releases its ordinary
permit, and hands its descriptor and deadline to the Terminal owner.
Full startup capacity returns the typed busy reply without a transfer.
Use at most four pending and eight attached sockets, with one owner
and no descriptor copies. A successful candidate reserves its attached
permit before exchanging the pending permit. That permit lives with
the actual stream through explicit close or descriptor teardown.
Admitted credentials remain scoped to the same UID and profile. A bad
or absent response, deadline, or owner exit closes and releases the
descriptor. Successful initial JSON replies use positive optional
`terminal_stream` with an explicit role, omit unrelated fields, and
keep both write sides open. Error replies use the ordinary closed
response and half-close. All handshake readers use exact read lengths,
so they never swallow binary bytes. The frontend enters raw only after
the success reply is validated and the UnixStream is converted into a
single nonblocking owned descriptor.

## Finite local data movement

Reuse the fixed-size record decoder on both directions. Each owner
keeps one frame encode buffer with an exact partial-write offset and
one read suffix with an exact consumed count. Never perform a write
before polling unless known writable or as one bounded initial try.
On the daemon the socket read never waits for a downstream effect.
The input queue has reserved capacity for the entire cumulative 16 KiB
outstanding credit. Treat any over-credit record as a protocol error.
Permit at most one Resize in flight. New geometry remains in the
CLI as a single latest unsent value while one is in flight. The daemon
queues the Resize at the exact accepted input byte offset and acknowledges
it only after the downstream resize effect. Amend the local writer
codec with daemon-to-CLI opcode 8 (body length 2, version and opcode)
for `ResizeApplied`. It needs no identifier because exactly one Resize
can await it and the stream cannot recover or repeat partial frames.
An observer cannot send Resize or receive ResizeApplied. A second
Resize before the first completion has been fully sent is a protocol
error on the daemon. An unsolicited or duplicate ResizeApplied is a
protocol error on the CLI. No exposed existing wire version includes
TerminalStart, so do not change ordinary IPC or native Marmot records.
An orderly Close may arrive while a Resize is outstanding and cancels
that outstanding operation. Never send ResizeApplied after Close.
A failed downstream resize ends the attachment within the input deadline.
Once the current encode slot is empty, prefer a pending Close, then
ResizeApplied, Credit and Output in that order. Never preempt a partially
written record.
Ack and Input can continue during a Resize, but no input already
accepted is reordered, and later input keeps its place behind the
queued Resize. If the user changes geometry while it is in flight,
send only the final queued size after ResizeApplied. Place it at that
new send-time input offset.
Continue to read and process Ack, Detach and EOF with full accepted input.
The CLI flushes pending Ack and Detach ahead of starting more Input.
Once any frame is partly written it finishes or ends on its deadline.
Only include CLI read interest when the frontend owns capacity for the
next complete record. Write interest exists only for a current unfinished
frame, eligible data or pending control. No permanently writable
idle loop. One event turn and one pending frame cannot block a shutdown;
close/Detach respects frame integrity and the existing deadlines.

Enforce local unsigned cumulative input Credit plus accepted byte
offsets exactly as specified in the codec contract. Start at zero and
replenish only after a completed downstream input effect. A queued resize never
overtakes already accepted input. Observers emit no input,
resize or credit. Local Output IDs start at 1 for each admitted socket.
The daemon keeps the one outstanding mapping from local output ID to
the ordered native output sequence and its offered byte length. Ack is
cumulative over physically written display
or deliberately discarded source units, and is never queued merely
because bytes were read. The daemon accepts repeating the last committed
ID, rejects regression or a value beyond the last full frame, and does
not reuse IDs. Bounded Close completes a final acknowledged prefix
only on authenticated orderly close. On HUP, attempt bounded reads of
already available socket bytes to disambiguate Close, clean boundary EOF
and truncated EOF. Reuse the codec end-of-input predicate. A typed Close
ends the daemon direction; confirmed boundary EOF after Close is orderly
and gets the existing normal detach-style cleanup. EOF without Close,
malformed record or a partial-frame timeout abandons uncommitted display
and runs the existing interruption cleanup. When no display capacity
exists remove the interest from the descriptor set entirely, including
persistent HUP. Re-register after prior progress or when the progress
deadline wakes the owner.

## Output segmentation and acknowledgments

The native delivery receiver holds its next frame behind the previous
frame's commit. Therefore do not depend on frontend lookahead to finish
a control sequence across an acknowledged frame boundary. Require the
live writer's output producer to use the existing streaming filter before
batching completed display units. Strip policy-denied bytes before
record creation and retain at most one incomplete UTF-8 or control
unit within the parser's existing byte and time bounds. The producer
queues complete independently safe units, packing them into existing
4 KiB or 8 ms output batches. The eight millisecond timer starts with
the first completed unit. An incomplete unit can continue across
PTY reads. A full batch flushes before adding another complete unit
that does not fit. A lone pending escape has no record to flush and
expires under the existing parser bound. A parser time or length error
ends only the attachment with transport. The tmux server and session
survive. The frontend remains a
separate trust boundary. It runs the same sanitization policy and
rejects an unexpected incomplete unit at the frame boundary. Add a
Filter Ground predicate and a Display commit predicate that also needs
the matching entire record consumed and every emitted unit fully
written. A record producing only deliberate discards commits on that
predicate immediately. Keep the filter's partial-write output state so one short
physical write never commits a partly rendered unit.

The foreground socket receives one Output at a time. It sends its Ack
only after the output commit predicate succeeds. The daemon writes the
full local record while the native head is offered but reports
`WouldBlock` rather than `Wrote` to the native delivery machine. It
retains the mapping without rewriting any socket bytes when the same
native head is offered again. On receipt of the exact local Ack, and
only then, report the entire offered length as a single native
`Effect::Wrote`, which commits and releases the authenticated network ACK.
This uses the entire offered source length even when the frontend
deliberately discarded all of its bytes.
Run the local stall deadline five seconds after the first local offer,
clamped below the actual native head and absolute frame expiry. If there
is insufficient earlier headroom for the local record, close locally and
attempt typed Close on the existing native deadline. A typed Close is
best effort and transport loss can prevent it from reaching the peer.
This replaces the earlier provisional 15-second local stall in both the
umbrella and local record plans. Update those references when wiring
this increment.


## Evidence

Deterministic tests cover exact old exchange bytes, capped handshake and
pending/attached accounting, wrong UID predicate, negative replies and
surplus, direction exhaustion, credit, ID progression, ordered resize,
partial writes, backpressure, shutdown and closed-state cleanup. Include
sustained input near credit cap while the other end emits blocked output
and assert that Ack and Detach still progress. Interleave repeated
local resizes with writer bytes while downstream is stalled. Assert at
most one Resize is sent, the final geometry arrives after its receipt,
and control/EOF continue within bounds. Test POLLIN combined
with HUP, EOF after valid Close and no idle readiness spin. Force
the final byte of UTF-8 and accepted ANSI sequences to cross several
producer reads and test record packing boundaries. Exercise long
policy-denied strings, short TTY writes, ordered Close and EOF with
dirty output. The opt-in real local PTY fixture verifies raw input,
physical output commit ordering, resize and restoration through the
actual socket pair. Measure local socket turn, per-frame cost and the
added coalescing delay. Report native network key-to-echo separately
after activation. Keep deterministic core cases in the small
self-hosted workflow. Obtain phase design and integrated Claude Code
approval before production activation.
