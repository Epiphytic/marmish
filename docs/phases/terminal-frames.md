# Lossless Terminal records and delivery state

This increment follows the [interactive roadmap](interactive-peers-roadmap.md)
and the [ephemeral Terminal route](terminal-ephemeral-route.md). Implement
a private, test-gated codec and deterministic delivery state for a single
already-authenticated attachment. Leave bootstrap, grant decisions, session
selection, transport scheduling, and PTY activation to their own owners.
Ordinary Command, Copy and Stream keep their current wire formats and limits.
The in-memory route supersedes the older durable confidentiality experiment
and its extra inner seal, kind and session limits.
The test-only scope allows the independently reviewed Terminal room fixture to
prove full native envelope sizes before product registration.

## Caller boundary and wire encoding

The caller supplies an expected 32-byte room transcript digest, 16-byte
random attachment ID, nonzero generation, writer or observer role and
expected direction. The generation is fixed for each attachment. A takeover
creates a new writer attachment ID and generation. An existing observer keeps
its own binding while its output source stays valid.
Construct those only after verifying the protected room purpose and transcript,
the current room epoch and its two exact ephemeral accounts, and the native
authenticated leaf and application author. A valid frame grants no local
permission. The supervisor verifies the current configured grant, lease and
attachment immediately before each write or resize. Reattach and takeover
supply a fresh attachment ID and drop old record state. An observer has no
input or resize actor. Broadcast and observer synchronization have their own
review, and the output record format can be used independently for each
observer.

Wrap one padded standard base64 record in one existing `MarmotAppEvent` under
app kind 24862 and empty application tags. Use native 60-second expiration.
Reject a protected application payload over 6144 bytes before JSON parse.
Require inner kind 24862, empty tags, a valid inner event ID and an exact
author match to the native authenticated sender. The largest raw record is
60-byte header plus 8-byte sequence plus 4096 bytes, or 4164 bytes. Cap its
encoded content at 5552 characters before decoding. Require strict canonical
base64 by decoding and comparing with a fresh encoding, following the pattern
in `command_wire.rs`. Keep kind 24862 out of ordinary app decoders. The binary record has a fixed magic (ASCII `MT`),
version octet 1, variant octet, transcript digest, attachment ID and an
unsigned 64-bit big endian generation. Consult only an exact match for all
fields. Unsigned integer bodies are big endian and have no optional extensions.
Check the encoded length before allocating decoded data. Check exact body
lengths, enum values, bounds and directions. Drop malformed or mismatched
records without mutating the current lane. Only decoded, correctly bound
violations listed below close an attachment. No compression or screen diff
is defined in version 1.

| Variant | Index | Direction | Body after fixed header |
| --- | ---: | --- | --- |
| Input | 1 | authenticated writer to server | nonzero sequence u64, then 1–4096 bytes |
| Output | 2 | server to attachment | nonzero sequence u64, then 1–4096 bytes |
| Resize | 3 | authenticated writer to server | nonzero sequence u64, rows u16, cols u16 |
| Ack | 4 | reverse of its ordered lane | cumulative committed sequence u64, where zero means none |
| Keepalive | 5 | attachment to server | strictly increasing nonzero pulse u64 |
| Close | 6 | either | no body |

Keepalive and Close have a control reservation independent of ordered payload
buffers. An Ack acknowledges Input and Resize on the client-to-server lane,
or Output on the server-to-client lane, as determined by direction. Observers
may acknowledge Output, keep their own lease alive, and Close their own
attachment. They cannot renew another attachment's lease. Reject observer Input and Resize
before lane admission and never create an observer write actor. Validate rows
2–250 and columns 2–500. Take Close only from the exact current attachment. Close
fences it immediately, discards pending bytes and is idempotent; it does not
acknowledge delivery. This increment does not specify shell exit/status
reporting. Close is a best-effort disconnect signal and loss is recovered by
the independently enforced lease.

## Ordered records and acknowledgments

A receiving lane starts at sequence 1 and stores at most one current ordered
record and the next seven distinct records. Retain at most 32 KiB of data and
give the oldest unresolved sequence 10 seconds from the first queued record
that exposes a gap or from first presentation of the expected record. Caller
polling supplies monotonic time. Reset that timer when a complete commit
advances the expected sequence. Re-evaluate a newly exposed gap at that point.
If the missing record arrives before expiry, it has the time remaining on that
gap timer to commit. Each accepted record keeps its own original signed-age
deadline and must carry the authenticated inner creation time and corresponding
native retention decision. Inject wall
and monotonic clocks independently. Require the native expiry to equal
creation plus 60 seconds and require receive time to be before that expiry.
Allow 5 seconds of future skew inclusive. Reject signed age at 15 seconds
or later so every admitted record has positive remaining time. Give ordered
records and acknowledgments a local 15-second signed age limit (10 seconds desired transit plus 5 seconds
skew) and use the same limit for Close and Keepalive. Elide any expired record
without qualifying as activity. Convert the remaining age to an absolute
monotonic admission deadline, bounded to 15 seconds, and close the attachment
if a queued ordered head expires before complete commit. A backward wall
clock adjustment cannot extend that deadline. If clock state is unavailable
or time arithmetic overflows, drop the record. Pending duplicates do not renew age. Expire
at the exact deadline. A conflicting body for a pending sequence, a forward
sequence beyond the window, a queue byte overflow, a counter overflow or an
unrecoverable gap closes the attachment. A sequence at or below the committed
watermark is ignored and may repeat the current Ack. It cannot repeat an
effect or renew a lease. The lane reports whether admission was fresh without
the caller having to inspect payload content.

Offer only the next expected record to its sink. Track a partial byte
offset when a nonblocking sink writes a prefix. Check the exact generation
and the current grant and lease in the same owner turn as each write.
`WouldBlock` does not advance the offset. A fully written record advances the
committed watermark once. A Resize shares ordering with Input and commits
only after its syscall succeeds. The frontend reports progress over local
IPC once every byte in the record was written to the terminal or consumed as a
disallowed control or bounded parser state. An approved byte waiting in a
filter output buffer has not progressed. Bounded parser state may consume a
split escape or UTF-8 prefix. A filter failure or oversized control string closes the
attachment with no new acknowledgment. Complete disallowed controls can be
intentionally consumed, and such bytes never reach the terminal display. The I/O owners consume
records through an explicit begin/progress API; merely moving a record into
another buffer never acknowledges it. After ambiguity, expiry or any sink
error close and abandon the prefix. Never translate old unacknowledged data
into a fresh activation.

A sending lane retains at most eight ordered records and 32 KiB awaiting
commit. The oldest outstanding record has a 10-second monotonic deadline
from its first begun handoff to the native sender. The outer preparation queue
also remains bounded by age. Acknowledge only a cumulative sequence through
the highest sequence whose native handoff has begun. An old Ack never retrogresses or renews activity. An Ack past that
range closes the attachment. Prepare one unchanged record for handoff to the
native sender. A transient refusal leaves the same record at the same
sequence. Exactly one unresolved native publication per room remains an outer
runtime constraint. A native ACK can release that publication slot while the
application window retains the bytes. Relay retransmission reuses the frozen
signed event. Never reseal a record after an ambiguous native handoff. A
missed application acknowledgement eventually closes the attachment; this
phase introduces no second durable outbox or application relay retry. A record
can still reach the server after the sender gives up, within the signed-age
window. The sender treats this result as a detach with ambiguous input and
never automatically reissues that input.

Expose fresh ordered progress and fresh monotonic control pulses to an outer
lease owner. The lease begins on admission. It closes at 11 minutes since the
last fresh committed ordered record, advancing application Ack or fresh pulse
for that attachment. A zero or nonadvancing Ack never renews activity. Duplicate
records, duplicate or older pulses, malformed frames, an unconfirmed gap, and
relay ACKs do not extend it. The sender emits one new pulse on a steady idle link
at five minutes by default. Do not let a full ordered queue supersede Close.
Keep the newest pending cumulative Ack and newest pending pulse without
queuing duplicate controls. Each control is still subject to the native
sender's existing route and publish checks. Tune sending, fairness and
payload batching separately on the proven caller API; the codec never waits.

All state and native payloads stay in memory. No debug representation or log
contains a record payload, key or identifier. Wall time is used only to
validate authenticated record freshness and establish the first monotonic age
deadline. Lease and queue expiration are monotonic.

## Qualification

Unit tests cover exact encoding against fixed binary vectors, all directions
and roles, wrong attachment and generation, strict Base64, unknown variant
and trailing bytes, exact cap and one byte over, stale and future boundaries, bad native expiry,
wall-clock regression after admission, skew-clamped monotonic expiry,
clock unavailability and overflow, age checking of Ack, Close and Keepalive,
mutual rejection by ordinary and Terminal decoders, zero and boundary integers,
full receive and send windows, conflicting and old duplicates, stale/forward
Ack, reordering, partial sink write including mid-write expiry, EOF and timeout, resize order, fence
between partial writes, lease pulse monotonicity, resize bounds, wrong-ID state preservation,
idempotent Close without acknowledgment and independent control capacity. Generate bounded deterministic byte patterns including every byte
value so the codec makes no text or UTF-8 assumptions. Use selector
`terminal_frames::core_tests` with a 2-second warm execution budget for
the bounded unit tests in routine core CI.

Using the existing opt-in actual memory room and private relay fixtures,
parameterize the test-only memory-room round trip by app kind without changing
its existing CHAT coverage. Verify maximum-size 4096-byte input and output plus all frame header fields
using a maximum-width generation and sequence. Count the exact encoded
application event, signed outer kind-445 event and accepted relay WSS message.
Assert conformance to the 6144-byte app limit and to the route caps of 16 KiB
protected content and 32 KiB outer event. Check native 60-second expiry. Verify sender,
current room, leaf, epoch, and exact binary payload in both directions.
Confirm wrong-route events cannot reach the lane or durable ordinary ingress.
Use controllable reordering inside the application fixture; never induce
load on the public relays. Benchmark codec throughput with binary and
keystroke-sized workloads, sequence admission-to-ready p50/p95 and protected
event bytes per payload. Separate codec and WSS timings. Record the command,
sample counts, platform and limits in validation. Keep max-size feasibility,
network tests and benchmarks out of routine CI.
