# Peer raw WSS connection ownership

Exercise the [Peer live wire contract](peer-live.md) in a private WSS fixture.
Mount `peer_raw_socket.rs` as a `#[cfg(test)]` module of `mdk_adapter.rs`
alongside the existing loopback relay proofs. Its nested `#[path]` module
uses the [signed codec](../validation/peer-control-codec.md) and its
`Prepared` type. The codec's existing unit tests compile in both the lib test and
standalone integration binaries, including the ignored benchmark. Its
unit tests cover its remaining methods under strict Clippy. Add a crate-local `#[cfg(test)]` direct
connection attempt getter in `transport.rs`. These are the only existing
source files this proof touches. It has no production transport, daemon,
ordinary delivery, native Welcome or authorization effects. The
[cursor proof](peer-cursor-store.md) owns durable effects. Per-profile and
per-origin rate buckets and production relay conformance are outside
this fixture. Timings are informational.

## Ownership and authentication

Use one mutable, serialized owner for one canonical `wss://` origin, one
canonical local recipient and one socket generation. Before connecting,
verify the WSS origin exactly matches the singleton policy and the
recipient exactly matches the entrusted AUTH signer account. A separately
authenticated sender can publish an event for another recipient from its
own connection. Give each connector distinct absolute state and cache
paths, one configured relay with only the loopback fixture's trust root,
and one slot. Bootstrap each connector before connect. One absolute
deadline wraps bootstrap, connect, AUTH and live EOSE, with existing
connector limits also in force. Drop both split halves on retirement.
Reconnect on the same connector after both halves drop, provided it was
not explicitly cancelled. Mint a fresh random 128-bit namespace for each
successful connection attempt and put its lowercase hex in the live and
page IDs. The ID shape is `mp/<namespace>/l` or
`mp/<namespace>/p/<decimal sequence>`. Checked sequence increments
prevent reuse within the generation, and every ID is at most 64 bytes.
On a healthy explicit stop send CLOSE for the live subscription, then
flush and close the sink under the operation's absolute deadline. Stop
and Drop release the slot without cancelling the connector.

Use the default 1 MiB connector cap, which also limits outbound frames
and buffers. The owner enforces its own 64 KiB plus 256 bytes limit for
every inbound text message before array parsing. The exact raw EVENT
value is limited to 64 KiB before header parsing. A frame over the
connector cap fails at the transport. A text message over the owner cap
but below the connector cap retires at the owner. Test both caps. The
64 KiB exact/plus-one carrier cases are synthetic raw-layer tests.
Accept Nostr payload only in text WebSocket messages. Invalid UTF-8,
binary, socket close or socket error retire. Ping uses the split
transport's normal tungstenite auto-pong via normal reads and flushes,
up to the 125-byte control payload bound. Flush the queued Pong after
observing Ping within the current deadline. Prove actual Pong observation.
Unsolicited Pong carries no authority. Allow at most eight control frames
per owner call, with at most 24 total inbound frames in any one call.
Predrain is additionally bounded to 12 immediately readable frames.
A ninth control frame, twenty-fifth overall frame or thirteenth
immediately readable predrain frame retires. The stricter control and
record reserves limit every admissible current wire transcript to
fewer than 25 frames. Test the additional total cap directly.

Accept exactly one AUTH challenge of 1 to 256 bytes without ASCII
controls. Sign kind 22242 with exact challenge and relay tags (the
canonical URL including trailing slash). Track only a positive
same-socket OK for the exact signed ID. Early OK, negative AUTH OK,
wrong ID, repeated AUTH or any other application message retires.
After the positive matching AUTH OK, send live REQ with exactly this filter
`{"kinds":[7435],"#p":[recipient],"limit":0}` with no other keys.
Require the first EOSE for its live ID on the AUTH-established socket.
CLOSED, negative or unsolicited OK, wrong EOSE or any premature live
EVENT retires. The proof relay emits EOSE first. A real relay that races it
requires qualification before this policy can be used.

## Live and historical receipt

Use a bounded JSON array visitor with exact arity and borrowed raw
values. For the ready state, the only accepted inbound Nostr verbs are
EVENT on the live or current page ID, EOSE on a pending page ID and OK
on the pending published event ID. CLOSED, NOTICE, AUTH, unknown verbs,
unexpected IDs, duplicate EOSE or trailing elements are terminal.
For header fields use a strict derived deserializer that recognizes
every codec outer event field, rejects unknown and duplicate keys,
requires the codec's field types, and verifies exact bounded tag form.
For a recognized EVENT, charge raw bytes and count before inspecting
those fields. Check canonical ID (64 lowercase hex digits), unsigned
integer timestamp and kind 7435. Require exactly two two-field tags in
the codec's order. The canonical `p` comes first and equals the
subscription recipient. The expiration follows as unpadded unsigned
decimal in a nonzero 21,600-second bucket. The canonical outer public
key differs from the recipient.
Leave complete signature and nested event verification to the existing
codec. Preserve the original raw JSON value including serialization.

The same split stream is read by each serialized call. Before sending a
page or starting a publication, poll and handle immediately available
input in socket order, with no wait for fresh activity and the
predrain cap above. One pending request at a time shares the call's
total budget. An already observed terminal condition always retires
first. Each of the pending page and queued live deliveries has its own
four-record and 256 KiB reserve, or eight records and 512 KiB
combined. The caller can take live bodies from the reserve between
operations. A live duplicate of a historical event ID counts against the separate
reserve. A fifth record, excess bytes, duplicate event ID in a page, increasing page
time or bad inclusive bounds retires the socket. The split budgets and
whole-owner retirement after page errors intentionally narrow the
parent design, where the live spool has its own eight rows and some
page-only failures keep the live REQ. No raw item is silently skipped.

Send a historical REQ with exactly the filter
`{"kinds":[7435],"#p":[recipient],"since":lower,"until":upper,"limit":4}`
using requested inclusive unsigned bounds with lower at most upper.
Until its first EOSE, up to four exact raw values constitute a page.
At EOSE, end receipt for that ID and write its CLOSE successfully within
the deadline before returning. No other inbound messages are processed
during that write. A later frame for that ID is unknown and retires
when observed. A page deadline before EOSE or while writing CLOSE
retires and yields no page. A deadline already expired at call entry
returns without I/O or a change to a ready owner. Live wait returns no
record at entry or on a receive timeout and leaves the owner ready. If a
frame was consumed or a control flush was incomplete at the deadline,
the owner retires to preserve receipt ordering. A predrain or validation error
before any publication write retires without starting that publication.

Return opaque `Page` and `LiveRecord` from an inner owner module. Their
constructors and fields are private even to the sibling tests.
Both carry the exact origin, local recipient and a shared generation
identity. Page also carries the request ID, requested bounds and short
versus full status. Live records carry the live ID. A generation token is
shared with the owner through a retirement cell the owner retires before
dropping the socket, including in Drop. Dropping a pending owner
operation after it has entered the socket invalidates its generation and
releases the slot before the next operation. A dropped publication may
have reached the relay. The caller keeps its prepared bytes and treats
that cancellation as ambiguous. The owner's synchronous authority
check requires a matching live token and context. A future serialized
owner must call that check immediately before a synchronous cursor
commit or live admission. Transport staleness here means the state the
owner actually observed. A returned item carries attributable evidence
and confers no independent authorization.

The fixture treats history as a finite newest-first stream with stable
ties, exact inclusive bounds and count limit, through its first EOSE.
It sends no normal historical records after EOSE. It verifies a read
request's exact shape and that the single `#p` equals the authenticated
reader. Live limit 0 sends no stored matches before its EOSE. New inserts
may arrive on the live ID while history receives its finite prefix.
CLOSE has no acknowledgment in NIP-01. A successful write precedes
return, and a later fixture observation proves receipt. A finite EOSE
prefix gives no proof of completeness against concurrent backdated
inserts. Periodic full catch-up covers that case. Production use
requires a qualified finite relay contract or a reviewed bounded
confirmation protocol. TLS success alone is insufficient.

## Publication and evidence

Accept one existing `Prepared` whose immutable `frame()` is the event
JSON. Verify the signature and match the raw header against the event ID
and recipient in trusted local flight metadata. Splice its exact bytes inside the outgoing
Nostr EVENT array. The AUTH signer is the registered writer, and the
carrier's one-use outer author and recipient may both differ from it.
The only definitive acknowledgment is the matching same-socket OK
boolean. Negative OK is refusal with the owner retained. Never expose
relay text, including duplicate descriptions. Unsolicited, stale or
wrong OK retires. Any loss after beginning the write through matching
OK, including deadline expiry and live overflow, is ambiguous and
retires. The owner borrows `Prepared` so the caller retains exact
original bytes and origin for any later explicit retry.
No implicit republish, native delivery or cursor update.

Use a separate bounded scripted WSS relay fixture and reuse the
existing local ephemeral TLS trust pattern in its own fixture. Its frame
cap exceeds 1 MiB for the connector-cap negative case. Allow the authenticated
writer to submit a valid kind 7435 record with a distinct one-use key.
Verify signed AUTH and exact read filters including stable historical
tie order and live EOSE. Inspect the relay's observed CLOSE. Inject
specific negative transitions over tungstenite, using an explicit
raw text frame for invalid UTF-8. Fixture tasks and channels have
finite message counts and deadlines. Authenticate a real signed Offer
after both live and page receipt, and compare its canonical ID. Exact
byte identity is required for the sender's offered and retained bytes,
and preserved on each inbound message as it was received. A relay
can reserialize a valid signed carrier.

Synthetic carriers exercise exact tie and ordering cases since prepared
carriers randomize their timestamps. Offline core cases cover exact array grammar, duplicate keys and
headers, raw length boundary, stamp order, tie/order-independent IDs
and reserve limits, stale-token checks, and zero direct attempts on
scheme/origin/recipient mismatch. A paused-clock test exercises the same
absolute deadline wrapper used for close and flush, including a pending
operation and a completion at the deadline. The opt-in real WSS test covers
actual signed AUTH against two disposable identities, mismatched/early
or negative OK, empty/too long/control challenges, repeated AUTH,
wrong/duplicate EOSE, premature live EVENT, CLOSED, late page ID,
binary, invalid UTF-8, real Ping/Pong, owner versus connector caps,
interleaved page and live data, deadline and ambiguous loss, byte
retention, explicit stop and singleton slot release/reconnect with
fresh IDs. Include positive cases preserving exact publication and
the raw receipt body. Measure connect through authenticated readiness,
publication acknowledgment, and signed carrier receipt on the local
link. No public relay publication is included.
