# Terminal bootstrap admission owner

This increment turns the [scoped room proof](terminal-room-enrollment.md)
into a callable enrollment owner under the
[interactive roadmap](interactive-peers-roadmap.md). It covers direct WSS
admission to a fresh native room and an exclusive, verified continuation
for selection.
The Command actor supplies verified bindings, the local daemon supplies
attachment selections, and the supervisor owns tmux. Those integrations
remain separate. The first qualification uses private deterministic TLS,
real upstream MDK and the actual ordinary Command room, with no public
relay traffic or new CLI route.

## Authenticated input and relay policy

Define a move-only private `Carrier`, with no Clone or Deserialize, built
only in the Command actor from an `AuthenticatedCommandContext` and the same
turn's read-only `Arc<AttachmentFence>`. The context gains a bounded
`relay_endpoints: Vec<String>` with a read-only accessor. The private actor
checks the exact canonical accounts, role, UID, MLS and transport IDs,
session, and authority digest. It obtains the canonical endpoints from
that same check's validated `NOSTR_ROUTING_COMPONENT_ID`, never from an
IPC argument or general metadata. `check_group_for` includes the raw
routing component in the current authority digest by component ID and
length. Compare the full frozen context when revalidating the fence.
A route change retires the same generation. The paired fence uses object
identity as generation ownership. Bind it to the digest on construction.
One retired fence cancels every admission or native room using that
carrier. Compose startup with the command preparation and fence contract
in the [interactive roadmap](interactive-peers-roadmap.md).

Use a same-profile canonical signing capability bounded to the local
account and Command binding. Alias derivation and the NIP-59 seal stay
inside the capability. The acceptor receives a bounded operation interface
and the resulting ephemeral alias keys. It receives neither an export
of the canonical private key nor a handle to the Command MDK database.
Derivation and seal calls use the separate Terminal executor and never
take a Command actor queue turn. Require the capability public key
and current local UID to agree with the supplied context.

Install a single-URL scoped policy from the sorted, deduplicated
intersection of protected route endpoints and the resolved profile
`RelayPolicy`. Ignore endpoints outside that intersection. Use direct
WSS only in this increment. A clearnet URL under a mode requiring Tor
and an onion URL under Auto yield `terminal_transport_unavailable` if
there is no supported intersection. Never change the configured mode or
accept a redirect to another origin. Build a lane-scoped recording
NIP-42 authenticator around the SDK's `SignerAuthenticator`; keep only
four bounded `(relay, challenge, auth_event_id)` entries while the
connection starts. The public `Authenticator::make_auth_event` returns
the signed event after recording its exact ID. Observe these events in
one ordered MDK frontier reader using the exact endpoint and replay
epoch. A source becomes live after all four conditions hold in order.

1. The exact relay sends an AUTH challenge in that epoch.
2. The recording authenticator creates the event for that relay and
   challenge.
3. The same epoch carries the positive OK for that exact auth event ID.
4. The same epoch carries EOSE for the exact locally issued REQ ID after
   that OK, with no later CLOSED for that ID.

The raw SDK carries these relay messages even though its
`Authenticated` status does not propagate to the MDK frontier. Discard
previous-epoch observations. The usual leading auth-required CLOSED may
precede positive AUTH and the SDK's retried REQ. Never send a Probe or
consume a Challenge as live before all conditions hold. A negative OK
for the exact recorded authentication event, or absent evidence within
the bounded window, rejects the source. A relay that never challenges
qualifies as `terminal_transport_unavailable`; an incomplete exchange at
the deadline is a timeout. The positive evidence is the relay's own
acceptance, which can only establish that relay connection's liveness.
Enable the loopback CA only in tests.

Stage matching pre-EOSE EVENT samples for that exact source in a
separate bounded spool. Inspect at most 256 samples or one MiB of
serialized data, whichever comes first, then count and drop excess.
Apply exact route and size checks before buffering and run the same
signed classifier and freshness check before use. Release candidates
to normal admission only after the source becomes live, delivering at
the receiver's bounded pace instead of flooding the post-EOSE queue.
Discard the spool when the generation becomes uncertain. Keep this
path for a Probe or Challenge published before the peer's REQ was live.
An immutable retry can receive a duplicate OK without any relay
redispatch. An idle listener drops a post-EOSE bounded queue overflow
with a saturating counter. Back off and re-open after eight overflows
within ten seconds. Loss of a source serving an active enrollment is
terminal for those consumers, as is a CLOSED, generation change, lag
gap or room queue overflow. Room delivery and publication
use the verified selected URL, correct delivery plane, terminal member
and exact live transport group. Bootstrap and enrollment caps remain
32 KiB and 64 KiB, applied before decrypting untrusted contents. Above
32 KiB, inspect alias data only while the source has an active matching
enrollment consumer. Use the existing volatile room with no durable
MDK handles, discovery or Terminal KeyPackage publication.

## Listener and outbound ownership

Run the admission owner outside the ordinary Command actor executor.
Reconcile at most 32 listener entries by canonical peer order, keeping
newest eligible live bindings first for each peer, then additional
bindings in fair passes. Rotate the starting peer across reconciliations
when demand exceeds capacity. Rotate only idle entries. An entry
supporting a live flight or selection is ineligible for rotation. In each
pass give eligible peers one entry before allocating additional entries
to the same peer. Break
ties on canonical binding and URL. The unit is one binding generation
and verified relay URL. Removing eligibility immediately fences the
entry, including connections still opening. Count closing entries
against the 32 entries until transport and task reap. Report only
fixed capacity categories, with no public keys or endpoints in
routine errors. Bound listener reconciliation and work per turn, wake
on actor changes, and apply bounded backoff to unavailable origins.
Start outbound work under an independent pending limit of eight with
a common absolute Start deadline of 60 seconds, including the ordinary
15-second preparation budget. Outbound alias sources close or transfer
into a separately counted native room before the pending permit is released.

Take supported origins in canonical order, at most four. Use one
immutable, authenticated Probe per exchange. The first Probe is
constructed when the bootstrap budget starts. There are at most four
origin windows, starting at offsets 0, 6, 12 and 18 seconds. Each lasts
at most five seconds including connection, AUTH, exact REQ acceptance,
send and expected Challenge. A failed or timed-out source must be
fenced and reaped before the next offset. A slow reap can end Start.
Move down the ranked origins at each failure until exhausted, then
reuse the last eligible origin and exact encoded Probe. Retransmitting
the same capability is safe even if its previous relay acknowledgement
was lost. An unresolved publication whose local send task cannot be
reaped stops the exchange. Once the client verifies the expected
Challenge, pin the exact URL, connection and REQ generation and stop
fallback. The server may answer a pre-Confirm copy with the same cached
immutable Challenge on each eligible live source, without creating
more challenges or extending any deadline. The first matching Confirm
pins the server's source. Limit the later cross-origin response cache
before enabling this path. The first single-origin slice sends its
Challenge once and uses relay-stored backlog for any reopened client
source. It never republishes Challenge for an exact Probe retry.
Delay or disappearance of the first relay cannot force a new
capability on the second in the later multi-origin slice. Ready and enrollment use only
the pinned origin. Test a slow first relay and healthy second relay.

| Client Start elapsed | Absolute bound |
| --- | --- |
| Ordinary Command preparation | 15 s |
| Bootstrap, measured from preparation complete | 30 s, including the four origin windows and Ready |
| Native room enrollment | The earlier of Ready + 30 s and Start + 55 s |
| Selection and reply reserve | 5 s before Start + 60 s |

The worst case preparation and Bootstrap leave ten seconds for room
enrollment with the fixed selection reserve. At the last window's end
at Bootstrap + 23 s, seven seconds remain for Confirm and Ready.
An earlier Ready permits more native enrollment time. Each bound is a
ceiling, and misses produce a fixed timeout without retrying Start.
The server uses the first accepted Probe + 30 s and Ready + 30 s caps
without inventing a local client Start time.

Share one inbound flight across eligible origins for a binding. Refuse
a different Probe while a pre-Ready flight is in progress. End a flight
when the enrollment owner takes the terminal capability, independently
of later room reap. The proof's limit of one fresh Probe per minute
prevents immediate reattach and the approved Busy/choice sequence.
For the later multi-flight slice, use one bounded in-memory bucket per
eligible canonical peer across its bindings and origins. Burst four
fresh authenticated Probes and refill one token per 15 elapsed seconds.
Keep inbound and outbound charges in separate directional buckets so
an unsolicited inbound burst cannot starve the local caller. A bucket
survives generation rollover and remains in a capped 32-peer counter
table until fully refilled, even if the peer lost eligibility.
Do not remove active or depleted counters for newly eligible peers.
Refuse new admissions while all counters are retained. Rate denial is
sanitized locally and silent for remote Probes.

Track accepted consumed Probe rumor IDs independently of Exchange state,
including after flight expiry or replacement. Retain each for at least
45 seconds of checked suspend-aware monotonic time from first sight.
Before admission and expiry sweeps, compare wall time to the advance
since the last trusted wall/monotonic sample. Keep that anchor at
profile scope across binding and connection replacement. Only advance
the anchor within tolerance, and never rebase its projected wall time
backward. A divergence
greater than two seconds makes admission clock-uncertain and suppresses
new Probes silently without evicting any ID. Hold this condition for at
least 41 seconds since observation and until the wall clock returns
within two seconds of the projected last trusted anchor. Continuing
drift keeps the condition in force. This recovery condition also covers
a backward jump large enough that an old signed Probe would become
timely again only after 41 seconds. Within tolerance, use a freshness
clock that never falls below the projected trusted wall time minus
two seconds. The retention covers five seconds of signed future skew,
35 seconds of inclusive accepted age, two seconds of first-sight
tolerance, two seconds of floor tolerance and one strict margin
second. Test a first sight at the upper tolerance and maximum future
skew, followed by lower tolerance and replay at 43 and 44 seconds.
Check the old signed ID after its ordinary sweep would evict it,
with injected backward steps both smaller and larger than retention.
A large forward divergence stays suppressed until the same recovery
condition holds or the profile restarts. Restart clears the in-memory
anchor and consumed IDs; a relay-only replay on restart still cannot
satisfy the new random Challenge.

All first-slice duplicate Probe IDs are inert, including exact
current duplicates. The later multi-origin slice can retransmit only
the cached Challenge for an exact current flight, within its original
window. Other duplicates remain inert. A
record becomes consumed only when a fresh valid Probe is charged,
including later known failures. Check dedup before charging. The
production bucket plus expiry permits at most seven consumed IDs per
peer/direction. Cap at eight, and refuse rather than evict a live ID.
Store the binding digest with the ID in the peer counter, keeping it
across actor generation changes. Preserve the stricter per-binding rate
in the first single-flight slice. That slice still tests consumed-ID
protection separately and refuses a second enrollment until the first
native room is reaped.

Before passing a Confirm to the existing mutating Exchange transition,
validate only its authenticated exact expected Challenge and origin.
While that check is still current, acquire the room permit and pass the
server grant and Command fence. Then advance and cache Ready. Refuse
duplicate Confirm after denial or exhaustion without emitting Ready.
Recheck the grant and fence before founder or join mutation and before
transferring a ready room. A Probe reserves only in-memory bootstrap
state. It cannot allocate MLS or cause a tmux effect.

The aliases are deterministic per authenticated Command binding and
role. One reader per exact source owns raw receive, replay dedup and
acknowledgement. Even in the first slice it separates bootstrap records
from the current enrollment record. Pass exact current Probe, Challenge,
Confirm or Ready duplicates to the existing Exchange duplicate check
and leave them inert after Ready. Ignore old-flight or stale bootstrap
records. Retire only authenticated conflicts tied to the current
transcript. There is no multi-consumer dispatch in the first slice.
For the later multi-flight slice, introduce a private
`classify_alias` that verifies the exact outer recipient, sender seal,
canonical inner author, kind, freshness and strict signed structure
once. It returns a typed `ProbeOrHandshake` with validated rumor ID,
or a typed `Enrollment` with Ready rumor ID, record and signed rumor ID.
Dispatch those typed records through existing bounded capacity. Only
the exact later validation of Ready, parties and digests can advance
a live per-flight owner. An exact duplicate is inert. A different
authenticated record of the expected kind with that same Ready ID
retires the conflicting enrollment owner. Another valid transcript
and unverified or stale traffic cannot retire a healthy room or
cross into ordinary Command delivery. The alias reference held by
that room ends after selection/handoff. The server listener may
continue to own its subscription. Each listener source retained
by a room still counts under the 32-entry limit. Room and routing IDs
remain collision-reserved until their associated transports reap.

## Deadlines, handoff and retire

Use checked suspend-aware time, with an injected clock for deterministic
expiry tests. Distinguish listener lifetime, Probe lifetime, 30 seconds
after verified Ready for Room enrollment, and attached lifetime. Separate the lane gate from the proof's boolean
`Authority`. Use Live, Suspect and Retired, and a single owner-driven
phase transition from enrollment deadline to attachment guard. Before
each individual authoritative effect, wait for Live within that
operation's absolute bound. The bounded receive/validation path may
drain while Suspect and preserve queue age and replay state. Use
a ten-millisecond wait poll for this increment and measure the pause.
Retired and uncertain clock or source state stop effects. Never hold
the Command mutex across I/O. An already-issued publication keeps
its task and permit until its bounded completion or transport reap.
Recheck authority again before processing its result. Check each
receipt transition individually instead of wrapping all three effects
in a single authority check.

Handle ClientHello on the exact native lane and produce a move-only
ready continuation with its transcript, authenticated canonical pair,
local direction and UID, Command generation and digest, current native
leaf and epoch, and pinned delivery generations. Carry the existing
room reservation and cleanup ownership. A caller cannot construct a
continuation from identifiers or cancel the enrollment authority on its
own. At this boundary room work still expires with enrollment authority.
The continuation supports only the owned selection exchange and a single
claim handoff. The subsequent integration installs the grant/lease and
server-issued claim after exact selection and before ending enrollment.
Recheck the full immutable binding and live source generations under
both authorities on the same owner turn. Selection failure or expiry
closes both scopes. Exercising handoff in this increment uses a fake typed
claim interface alone. Production selection, termios, forwarding, local
IPC and PTY are handled by their own increment. Never restart the original
deadline or use the test proof's 30-second authority as an attachment
lease. Keep the reviewed limits of 16 native rooms including retiring,
eight admitted attachments at the next layer, 24 hours per room and
65,536 send or ingest attempts per room.

Keep the owner structured across shutdown and errors. Each owner has
an ordered cancellation, exact CLOSE/deactivation and joined reap path.
Dropping the admission or ready token cancels effects immediately and
returns ownership to that path. A permit releases only on positive local
close, retired source generation and joined owned tasks and transport.
A cleanup timeout may report failure while a bounded reaper job retains
the permit and its task and transport handles. Reserve one reaper slot per
created handle set before opening it. Bound SDK pools to 72, including 32 listener pools, eight pending
outbound alias pools and up to two independently counted pools for
each of 16 rooms. A listener borrowed by a room still consumes its
listener entry; an outbound source counts against its room only after
the scoped transfer. Allow at most three application tasks per pool
(owner, read and reap) plus one coordinator. Confirm exact SDK socket closure
and join the ingress task; requesting shutdown alone is insufficient. If it cannot prove reap, refuse further
affected capacity and report the fault. Other bindings and ordinary
Command work continue. Never put secret values, payloads or remote text
in logs, persistent state, IPC status or metrics. Use fixed categories
including `terminal_transport_unavailable`, `terminal_room_capacity`
and `terminal_budget`. Keep stable admission meanings distinct from
the transport and MDK details.

## Implementation boundaries and validation

Keep new production-owner logic in `terminal_live_enrollment*`. Reuse
the reviewed bootstrap parser/state, volatile native room and exact
adapter; make narrowly reviewed changes to share admission prerequisites
where the single-proof owner needs separation of authority or inbox
routing. Do not change wire schemas, durable room discovery, ordinary
Command behavior or the dependency pin. Coordinate the private actor
carrier, fence and later selection contract before connecting this owner
to an entry point. There is no public activation in this increment.

Deliver the first real WSS slice with one direct origin, one server
listener and one client dial, one live-or-retiring native room per
binding, and only the default writer enrollment role. It uses
authenticated context and endpoint evidence, exact NIP-42/REQ/EOSE,
bounded consumed-Probe replay state, grant and capacity before
Confirm mutation, tri-state effect checks, exact reap, and typed fake
claim after real ClientHello. It refuses a second enrollment until
the existing one reaps. It uses one alias reader and classifies late
bootstrap duplicates. It has no concurrent alias fanout because
there is no second consumer. Exercise ordinary Command on the same
relay and verify the volatile storage boundary. Implement and review
the fairness, per-peer bursts, multi-origin fallback, transcript
fanout and saturation follow-up before invoking any multiple-room
CLI path.

Run fast simulated clock and source scheduler cases through one
self-hosted CI selector. Cover stable fair ordering, endpoint
normalization, unsupported policy, global/pending/live/retiring
exhaustion and recovery, bounded reconciliation, duplicates across
origins, old-generation delivery, token refill and binding rollover,
immediate reattach and separate Busy/choice starts, two same-binding
transcripts, overflow, Suspect resume, permanent retirement, handoff expiry, and
close/drop/reap. Verify no room factory call at Probe, unsigned copy,
stale or mismatched Confirm, denial or unavailable capacity. Replay
the first Probe after starting a second flight and after the first
flight expires, including an injected wall clock step. Duplicate a denied or capacity-exhausted Confirm
and assert no Ready bytes were sent. Assert
no active operation continues after an injected revocation or uncertain
clock.

Retain an opt-in bounded real WSS fixture with actual NIP-42 AUTH, true
Command admission and ephemeral native peers. For exact source admission,
exercise the initial auth-required CLOSED and retried REQ to same-ID
EOSE, AUTH refusal, CLOSED after acceptance and connection epoch
change. Track emitted server messages and bounded pre-EOSE backlog. A wrong AUTH OK event ID and a valid AUTH OK
from an old epoch cannot authorize the current REQ. A stale flood
ahead of the fresh record may exhaust the spool and ends in a fixed
timeout. Record that drop count in the qualification output.
Add a relay fixture mode that stores each event once, returns
`OK true duplicate:` for repeats and never redispatches them. In the
first slice publish Probe before the server REQ exists and prove normal
admission from pre-EOSE backlog. Deliver an exact late Probe, Confirm
or Ready after Ready and check that the room remains live. Exercise
exact Plan-first and Welcome-first and grant revocation before join.
Use an injected monotonic clock or wait beyond the original 60-second
per-binding interval for any first-slice warm reattach. Qualify
pre-Challenge origin fallback, post-Challenge loss without fallback,
same-pair concurrent rooms, active-alias conflict and old- vs new-source
delivery during the later slice. Force room and retiring
permit saturation without leaking sources. Prove a real ordinary
command before, during and after Terminal admission and check exact
durable row equivalence and zero KeyPackage publications as in the
reviewed fixture. Show real ClientHello followed by permitted fake
claim work beyond the original enrollment clock, and exact rejection
when the claim, source or Command authority changes. Use local fresh
test keys and synthetic data. Keep all untrusted bytes and identities
out of diagnostics. Report cold and warm admission percentiles,
bootstrap and enrollment round timings, maximum queued bytes, and
listener/close work under concurrent admissions. Report runner, TLS
locality, measured sample sizes and qualifications. Obtain independent
Claude design approval before code and Claude auto review of the
completed scoped implementation.
