# Native Peer management activation

Keep the [invitation authority](peer-invitations.md) and [staged SQLCipher
room](peer-staged-inspection.md). This phase connects the public-runtime stage
to real daemon roles, local controls and a private relay lane. The existing
Command group, account package, lifecycle, replay and room paths are still
independent. A Peer invitation never becomes a command or shell grant.

## Separate invitation lane

Use Marmish regular stored kind 7435 as a candidate. The published NIP
registry did not list it when checked. Before feature activation, qualify
recipient-scoped AUTH, retention and retrieval on a private fixture and an
explicitly approved relay. A profile without a qualified relay reports
`peer_transport_unavailable`. Existing ordinary Command relay defaults
remain available. The operating relay can see the recipient tags, kind and
timing. The canonical sender and invitation details stay inside the seal.
[Event registry](https://github.com/nostr-protocol/nips)
[NIP-01](https://github.com/nostr-protocol/nips/blob/master/01.md)

The pinned SDK GiftWrapBuilder emits kind 1059 and its inverse accepts
standard wraps. Construct the application outer event with `EventBuilder`,
an independent one-use signing key and pinned NIP-44 v2 around a canonical
signed kind-13 seal. Use Terminal bootstrap's authenticated seal and rumor
checks with an exact application outer parser. Require the ordered `p` and
`expiration` tags. The ordinary Welcome decoder is unchanged. The standard
native kind-1059 Welcome stays inside Delivery after exact Consent and
goes only to the isolated native stage.
[NIP-44](https://github.com/nostr-protocol/nips/blob/master/44.md)
[NIP-59](https://github.com/nostr-protocol/nips/blob/master/59.md)

Randomize outer and seal timestamps independently within 48 hours before
signing. The authenticated inner creation and expiry govern freshness.
Allow at most five minutes of clock skew. Use the outer absolute expiry
as relay housekeeping only. Round inner expiry plus skew up to a six-hour
boundary, then add one or two independently selected six-hour buckets.
This retains the event beyond the admissibility window while avoiding a
precise outer timestamp derived from creation. Forbid use of that outer
tag for deciding whether to accept an invitation. In the offline codec,
verify that it cannot encode the exact inner time by construction and
that the randomized layer timestamps are independent across key use.
Subscribe across a 72-hour plus five-minute lookback and check the exact
inner window. Keep durable replay fences through exact inner expiry plus
five minutes of acceptance skew and ten minutes of clock-step margin.
Retries preserve identical signed bytes.
[NIP-40](https://github.com/nostr-protocol/nips/blob/master/40.md)

| Flight | Authenticated time and linkage |
| --- | --- |
| Offer | Signed creation and exact expiry 24 hours later. |
| Consent | Created only for a live exact Offer revision, before its expiry. Expires no later than its own creation plus 24 hours or the signed native KeyPackage expiry, whichever comes first. Require the package usable at Consent creation and verify this independently when receiving. |
| Delivery | Created after exact Consent and within its window; expires no later than its own creation plus 24 hours, Consent expiry or signed package expiry, whichever comes first. It remains acceptable after the original Offer expires if this exact Consent and Delivery are still live and authorized. |

Use one independently authenticated carrier owner per profile relay, exact
configured-origin subscription and exact owned subscription ID. One per-origin owner
can handle meetings for all eligible peers. No shared seen-event tracker
spans origins in this lane. Establish current successful connection
ownership and subscribe before admitting control. Use one origin selected
from the locally configured exact relay set when persisting each Offer.
The server may select another configured origin for a distinct future
offer after the earlier intent ends. A raw URL or time hint received from
a peer never opens a connection. Require match to this offer's actual
connection, explicit authenticated origin, canonical server and client,
role and revision. Consume raw frames on the owned socket with the exact origin
and REQ identity. A qualified relay enforces authenticated registered writers,
exact recipient AUTH and numeric admission for this kind: 256 total stored
visible records per recipient, at most three per stamped second, at most
a 64 KiB event admission ceiling, 32 accepted records per ten minutes
and at most 32 simultaneously stored
records per registered writer per recipient, and retention until verified outer
expiration, up to 90 hours from admission. When full it rejects further
writes without early eviction. Expiration is the rounded inner window
plus skew and 6 or 12 hours, so a 90-hour retention ceiling does not
truncate a legal event. The relay applies filters to the exact recipient
and returns the greatest matching timestamps first with stable ties.
All these limits apply before parsed delivery, including invalid records
that a registered writer manages to store. Prove actual relay enforcement
for the first explicit loopback WSS origin. This feature reports typed
unavailable on a relay without the proven contract.

Capture one auth-bound live REQ with `limit:0` and no `since` before the
first backlog page and each resumed scan. At the socket, establish exact
challenge, successful client AUTH and that live REQ's EOSE before the
first page. The live lane gives no durable recovery guarantee by itself.
On reconnect, retire all IDs, parser/spool state and confirmations. Complete
the same challenge, exact AUTH acceptance and EOSE on the fresh socket
before receiving for that origin. Do not trust an SDK aggregate Auth
notification or a previously successful socket. Live events have independent
bounded intake, and retire the generation on overflow. Use one recycled
scratch page and distinct bounded live spool within eight records and
512 KiB. A count or size overflow creates no cursor advance and a
bounded reconnect; backoff is independent of replay and authorization.

Anchor a catch-up scan at trusted local wall (T). The inclusive upper
cutoff is (T + 5 min), and the inclusive lower cutoff is
(max(0,T - 72 h - 5 min)), including forward-skewed fresh sends. Persist the
unfinished anchor, original cutoffs, trusted-time reference and current
inclusive `until` boundary. Resume an interrupted pass with that anchor
after opening the fresh live lane. On completion, store the last
completed anchor and start the next explicit recovery from the full
window at its new trusted anchor. It examines at most 256 visible items
per recipient by contract; the completion anchor is telemetry and
proves bounded exhaustion for that pass, not an event-time live floor.
Run a periodic full catch-up no more frequently than every ten minutes,
also bridging relay storage races and events injected after a page that
were missed during an interrupted live handoff. Never use the completion
anchor as the next lower cutoff. On large clock reversal or uncertainty
defer new admissions; choose a new scan anchor only after trusted clock
qualification. Authenticate freshness inside the envelope.

Use fresh exact page REQ IDs, `limit:4`, `since` at the stored lower cutoff
and `until` at the stored boundary. Count raw socket EVENT frames before
SDK event signature, expiry, size or policy filtering. Regular SDK raw
notifications occur too late for a trustworthy count at the pinned
revision. The first qualification uses a bounded dedicated WSS stream
behind the existing policy connector, with explicit NIP-42 handling and
no transparent SDK retry or broad ambient subscription. Enforce the
64 KiB event and 256 KiB page boundaries at raw read/parse. A frame
that cannot be attributed or bounded invalidates the page, including a
frame over the raw socket message size. A page never applies a
subscriber-wide success flag. Close the exact page request on its only
verified EOSE. Any unexpected duplicate EOSE, duplicate raw page ID
within one page, extra beyond the requested limit or stale generation
invalidates that page. The single live REQ remains active on that generation.
No page publishes control or advances authority through a stale socket.

Charge per-record and per-byte tokens before expensive validation, hold
a bounded page while tokens replenish, and service at most eight pages per
origin per pass. At most three records can share the prior boundary; a
full page must contain an older second or fail qualification. Commit
exact disposed IDs at the oldest second and leave that second inclusive.
Only a verified, fully accounted short page whose items have final
dispositions or durable deferred entries proves exhaustion. Route terminal-invalid, deferred-retry
and accepted separately. Invalid unauthenticated inputs only write
bounded scan-local rejection IDs where necessary. A trusted flight is
accepted only in a single encrypted Peer transaction that writes the
exact authenticated origin replay fence and the idempotent state effect
(for example, pending Offer, decision and outbox intent). Persist the
page cursor in a later transaction after all page records have terminal
outcomes. A crash between transactions replays the page and repeats no
committed effect. A missing durable prerequisite, pending capacity,
rate or work budget refusal, a native stage still in reconciliation and
local write error are deferred retry. Keep encrypted per-origin deferred
entries by exact generation-independent event ID with signed serialized
bytes, authentication status, retry-not-before and expiry. Never trust a
record's authority because it appears in this table; reverify it on each
processing attempt. Use the authenticated inner expiry when verified,
otherwise bounded relay envelope expiry. Back off once per event at
1, 2, 4, 8, 16, 32 and then 60 seconds, with an immediate eligible
wake upon the matching persisted prerequisite. Continue scanning other
records after the deferred row is durable and include its ID in the
inclusive boundary ledger. Count completion with deferred rows and retry
those rows independently of periodic full catch-up, coalescing rediscovery
by exact ID. Delete the deferred entry in the same transaction that commits
a final trusted effect and fence, or on verified expiry. An unavailable
database or failed deferred write prevents cursor advancement. The store
reserves 256 slots and 16 MiB per origin, matching the qualified visible
limit and 64 KiB raw ceiling. On saturation, suspend the cursor without
dropping deferred records, keep bounded retry and advertise typed
`peer_backlog`. Reserve service turns for retries even when pages are
active. Outer timestamp ordering never implies flight dependency.
Retries of signed bytes denied by the relay quota for their stamped
second can fail until expiry; report typed publication refusal without
regenerating a flight or selecting another relay.

Use the existing policy connector. A Tor profile stays on its configured
routing policy or reports unsupported status for the feature. Ordinary
Command traffic does not inherit this lane's hop or activation choice.

Use a closed and versioned Offer, Consent and Delivery schema. Reuse NIP-59
common sealing and outer/signature verification helpers where their existing
scope permits, with adaptation of the new kind confined to this lane. Check
all capped decoded layers, computed Nostr and inner IDs, canonical sender,
canonical recipient, bounded times, actual source and stage. Require the
Peer-specific rumor kind and closed Peer domain tag at the inner layer.
Persist a replay fence only after verified decryption, seal and rumor,
origin, role and exact revision. Its key is the exact authenticated
signed flight ID and origin. A pre-decrypt lookup never writes an
authoritative fence. Do not use
the outer randomized timestamp for application freshness. Derive idempotent
recovery from the exact signed immutable flight and authenticated offer
revision. A received payload never chooses package storage, subscriber
state, whether a signed flight counts as approved, or a local directory.
Provisional limits before decryption cap serialized events at 64 KiB and each origin queue
at eight events and 512 KiB. Use an origin bucket of eight events and 64 KiB
per second, bursts of eight events and 128 KiB, and an aggregate profile
bucket of 32 events per second with a 32-event burst. Validate origin, REQ,
kind, recipient, tags and size first. The dedicated raw carrier socket
owns both exact REQs and live spool. Keep the transport's 1 MiB wire cap
and reject carrier events over 64 KiB before event parsing. The separate
native stage SDK never receives these carrier subscriptions. Pinned local
identity keys/aliases and already known peers share eight reserved pending
slots. Up to eight other slots admit an unknown sender with one exact
offer per sender in either class. An exhausted class refuses new
invitations with `peer_pending_full` without acquiring package state.
Reservation never grants trust or consent. Apply the staged storage byte and outbox caps. Before freezing
sizes or enabling Delivery, prove that the largest native Welcome fits
the measured NIP-44 v2 padding, nested expansion, SDK and relay limits.
Keep keys,
private filesystem paths and decrypted content out of diagnostics.

This shape offers a dedicated application outer admission before Native
Welcome handling. The [aggregate proof](../validation/peer-aggregate-ingress-proof.md)
motivates independent receive ownership. Prove construction, real relay
kind filtering and bounded per-origin subscription ownership. Reuse the
already reviewed public-runtime fixture for exact native membership and
persisted transition evidence, and do not introduce a dependency on the
ordinary inbox path.

## State and activation

Create a bounded encrypted state store owned by a separate serialized Peer
owner. Local transitions are typed finite operations, same-UID and under the
profile lease. Persist one durable outbox flight before sending, unchanged
across bounded retries. A relay ACK establishes publication progress.
Only an authenticated application receipt in the verified native Peer room
establishes peer acceptance. Arrival of an authenticated Offer creates a
pending invitation with its exact revision and bounded expiry. Auto policy
follows the same persisted decision as explicit manual accept. Until then
there is no private package, scope, or Peer group to register. Increment
one covers envelope and durable pending/decision control with no active
native room. Increment two gives each Peer stage its own SDK client and
adapter, separate from Command, with strict exact room egress. The native stage adapter's effective group
relay set is the singleton exclusive authenticated Offer origin. Publish both
Consent and Delivery only through the same exact originating lane and
count only its ACK. Refuse a native Welcome or group relay hint outside
the singleton before stage ingestion. If the client never listens on this
origin there is no authenticated decision; report waiting then expiry at
the signed Offer deadline, with no implied rejection or automatic hop.
Prove two
isolated stage owners and an ordinary Command owner use the same relay and
canonical identities concurrently while an ordinary command completes.
Persist the exact origin ACK for the exact signed Delivery carrier, then
withdraw the corresponding native Welcome ID and recipient with
`AccountDeviceRuntime::withdraw_welcome`, then persist the result as its
own transition. This supersedes the staged draft's
`mark_sent_welcome_delivered` step. An absent exact obligation after an
interrupted persisted ACK is already-withdrawn success after checking the
stage and exact ID. Before an ACK persist, retry only the immutable carrier.
After an ACK persist but before withdrawal, repeat the exact withdrawal.
After withdrawal but before result persist, verify absence and persist the
result. Keep native stage transport inactive during startup reconciliation
and keep a stage-specific raw Welcome egress prohibition even after
activation. No raw native Welcome retry can escape through either normal
operation or recovery. Other owed native group obligations are left in
place and separately reconciled by exact carrier or withdrawal records.
Expiry, block, forget, revocation, rejection or failure remove only the
matching stage's native obligations and close before egress. An
authenticated application receipt is separately durable. A crash test at
each ACK/withdraw/result gap verifies zero raw Welcome publication when
the stage is reopened and transport becomes active.

A normal init, default manual client and server with no outstanding state
retain ordinary startup behavior if the Peer relay is unavailable. Do not
start one Task per configured principal; reuse a bounded scheduler and
owned waiters. Profile shut down, block, forget, revoked grant, expired stage,
restart during any phase, failed underlying stage, and budget pressure have
closed transitions and exact retry behavior. Cap live stage stores as in the
staged plan, lazily open active state, revalidate SQLCipher metadata and
current authority before registration, and reconcile interrupted work
before enabling native egress. Keep separate client/server direction axes
for a both-role daemon.

Provide `marmish peer invite CLIENT`, `marmish peer accept SERVER OFFER_ID`,
`block`, `unblock`, and `forget`. Provide bounded local `marmish servers list`
and `show SERVER`, plus outgoing status on the already shipped
`marmishd clients list/show`. Extend the existing private config with
`peer_invitations` and safe setup options for `manual` (default), `auto` and
`off`; installed older profiles retain their ordinary command behavior.
In `marmish`, recognize these exact pairs only after clap has produced the
external `Remote` variant and only when the invocation has no `--` target
delimiter. Keep aliases including `peer` and `servers` valid and use
`marmish peer -- ...` or `marmish servers -- ...` to disambiguate matching
words. `marmishd clients` keeps its own inventory. Check the parser with old
configs, global options and values equal to management words, and rsync's
existing target/command split. An operator who uses a verified identifier
still confirms the full npub.
Permissions and heartbeat enter through the separate status phase. Their
statuses are unknown until authenticated records really arrive.

## Acceptance and review

Exercise the installed entry points with separate keys, throwaway
profiles and private WSS: manual pending without native room, wrong offer
revision, canonical accept, exact native room and acknowledgment, auto
accept, both roles, blocked/forgotten/restart states, inaccessible relay,
ordinary Command in parallel, profile inspection, relay isolation and
configured egress constraints. Check bounds and crash recovery for all
persisted transitions. Record cold and warm offer-to-pending/accept-to-ready
latency, events and bytes for each flight, and inventory cost. Keep
loopback and benchmark cases opt-in. Run strict closed codec, permission
and journal projections on the small self-hosted CI subset. Review the
new outer kind on the actual pinned SDK and relay witness before
selecting it. Verify AUTH-scoped retrieval of a legitimately randomized
48-hour-old outer event, recognized expiry, and matching configured origin.
Prove `limit:0` accepts subsequently published backdated events with
no `since`. Prove inclusive `until`, stable ties, the three-per-second
and total and per-writer quotas, exact 72-hour plus five-minute boundary,
expiry behavior, sustained deferred older-flight progress, over-limit relay failures and repeated
page IDs. Exercise invalid or oversized raw records before SDK filtering,
an out-of-order flight deferred while its prerequisite arrives,
post-reconnect AUTH/REQ ownership and the three withdrawal crash gaps.
The distinct kind reduces the anonymity set available under 1059. If that documented contract cannot be validated, settle a
separate exact receive lane design before writing product integration.

First extract scoped envelope limits and the common pinned seal and rumor
checks from the reviewed Terminal wrapper. Preserve Terminal wire behavior
with its existing tests and keep each outer kind's tag and scope policy
local. Qualify the codec without live routing and keep activation unavailable
until the state and relay acceptance criteria pass.
