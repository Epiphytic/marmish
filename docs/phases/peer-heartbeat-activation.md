# Live Peer heartbeat and durable status

Extend the accepted directional Peer owner from [Peer activation](peer-live.md)
with the [reviewed planner](peer-heartbeat-planner.md),
[native collector](peer-heartbeat-sampler.md) and
[status codec](peer-status-codec.md). This plan supersedes the earlier status
proposal where it enabled publication by default, shared a sample across
audiences or exposed a remote permission refresh request. The current
proofs cover exact private WSS enrollment and application exchange and
a local sampler-to-projection fixture. The Offer/Consent checkpoint is a
separate increment. No production component currently provides the accepted
room and durable authority required here. This phase neither creates that
owner nor independently routes a Peer room.

## Ownership and configuration

The future accepted Peer owner supplies a non-cloneable transaction-
scoped `AcceptedStatus` read capability. Its stable binding contains
profile, local account, direction, canonical server and client, native
group ID, app room ID and nonzero local room generation. Its authority
snapshot includes a nonzero authorization revision, accepted protocol
version and status capability, current accepted/terminal state, execution
grant, immutable status-field and effective-policy configuration identity,
exact peer leaf and epoch, native connection and subscription generation,
and the exact confidential route. Fixture constructors remain private to
tests. In production only the accepted owner constructs the capability
from its authoritative SQLCipher row plus the exact current native scope.
An unavailable native proof produces no active send or projection handle.
A separately typed terminal scope can synchronize retirement only.
Both-role profiles use separate directional handles. A local generation is
allocated by the accepted owner, never read from a remote payload. The
founder waits for the durable authenticated native acceptance receipt.
The client waits for its accepted native state and current authority
checks. Consent, relay ACK, a usable KeyPackage, mere membership and
ordinary Command activity do not establish status authority. The status
module borrows the serialized owner and its existing native lane. It
opens no room, relay subscription, key package, listener or daemon.
An accepted owner whose application version cannot negotiate this
status marker retains its normal behavior and gets no status sends.

Add a closed, profile-local `peer_status` config section in the same
owner snapshot as `CommandConfig`. Absent or disabled status sends
neither host samples nor permission pages. When explicitly enabled, the
default interval is 3,600 seconds with a 300–86,400 second accepted
range. Default enabled fields are effective username, uptime and
1/5/15-minute load. Publish disk labels and byte counts only for explicitly
selected, locally configured filesystem paths. IP collection stays off
until configured interface and family selection allows it and the exact
route qualifies. Enforce the sampler's eight-disk, eight-interface and
string bounds. Per-key settings may narrow or disable global choices
and may disable policy description. They cannot enable a globally
disabled field or expand the global disk/interface selection. Set
`share_permissions` true by default inside enabled status and allow
disabling it globally and per key. An enabled status may intentionally
send a heartbeat with all host fields omitted. The client can receive
the service on an accepted channel regardless of its own sending
configuration. No inbound refresh operation is added.

Current `execution_clients` membership is necessary on the server for
all status egress. Derive the permission description from the engine's
validated effective policy and the exact local role and current execution
and terminal grants. Preserve policy layer order, absent versus empty
allow lists, intersected allow layers and additive denies. Set interactive
and shell-command capabilities explicitly to Unsupported, Denied or
Granted according to the actual implementation and the applicable
principal's admission policy. Status is advisory and has no path into
policy, grants, connection setup or command evaluation.

Before sampling, classify the complete accepted signed route and exact
locally configured transport policy using the existing relay policy. A
verified, exclusively direct set, every currently authorized endpoint and
the actual chosen transport must all qualify before using
`Route::AddressSafe`. Forced Tor, any configured or signed onion route,
mixed routes, or Tor-selected endpoints always suppress addresses. A
verified confidential route that uses Tor maps to `Route::Confidential`.
An incomplete or unverifiable authority is unavailable for all status.
Reject statically incompatible explicit address-sharing configuration.
For dynamic compatibility changes, omit addresses and retire any
prepared status that no longer qualifies. Never resolve an onion,
probe an external address, switch a route or reopen on a fallback. A
revocation prevents future publication. Values already delivered stay
under the recipient's control and relays can correlate encrypted traffic
timing.

## Durable authority and send reservations

Use versioned status tables on the accepted owner's hardened SQLCipher
metadata connection and in its transaction, alongside the exact accepted
registry row. This is the Peer owner connection represented by the
`HardenedPeerConnection` pattern from the cursor proof, not the ordinary
Command journal or an extra runtime writer. Reuse the profile lock, native
credential-derived key, hardening, checked schema and sidecar protections
from the eventual accepted owner. No plaintext or separate key fallback.
A standalone storage test may mount the interface using a disposable
hardened connection and fixture authority table. Migration and registration
in the shipped owner belong to the activation increment. Never initialize
a fresh store in place of an unsupported schema, failed open, wrong account,
ambiguous room binding or failed readback.

Store a one-to-one foreign-key relationship to the exact accepted binding
and enforce uniqueness per direction and current peer. Compare the room
IDs, profile, role, principals and local generation through the trusted
registry before each state read or mutation. The status row always stores
its own positive authorization revision and compares it against the exact
authoritative registry revision. The accepted owner increments and
persists the authoritative revision before making changed facts active.
Status synchronization copies that value in the same accepted-owner
transaction. Every observed durable change to the facts covered by
`AcceptedStatus` must advance it, including a changed authorized native
epoch, signed route, role, selection or policy capability. At fresh
process ownership, start with a closed egress registry and durably advance
and synchronize each restored room's revision before activation. New
in-process eligibility also gets a new planner epoch. Transient route or
subscription loss closes the in-memory gate and invalidates in-flight
work immediately. A successful exact
revalidation can reopen on a fresh in-process epoch. Durable revoke,
disablement, replacement, role/selection change or final route change
advances the revision transactionally. Config changes that remove
authority close the gate before any database wait. If storing or validating
the new revision fails it stays closed. Re-enable needs a newer revision.

Within each binding keep the next available send sequence, its qualified
heartbeat wall floor, highest local
policy ordinal, current effective policy digest or disabled marker, and
the last issued signed wall time of the current ordinal. Counters are
nonzero values encoded as exact-width big-endian 8-byte blobs to preserve
the entire wire range. Represent unused counters explicitly with null.
Advance checked with exhaustion fail-closed. Under the exact active
revision reserve the next heartbeat sequence before planner release, and
burn it on cancellation, failed release, expiry or a crash. Require
`prior binding heartbeat wall floor <= sampled_at <= qualified reservation wall`.
The qualified reservation wall must also meet the profile high-water for
rollback detection. In the same transaction persist it as both the
binding heartbeat floor and the profile high-water. A different binding's
heartbeat never invalidates an otherwise current sample. Pass the
planner's fixed sampled time without rewriting the timestamp. The fixture
planner needs a narrow typed way for the owner to read the ready pin's
sample time before it consumes the pin. Persist and
restore the last reservation as the planner floor. Only allocate when a
sample is ready. Do not accept a cached reservation from another room or
an older registry revision.

Use the existing codec digest over the real normalized permission plan.
Allocate the next policy ordinal transactionally when the effective
canonical digest changes or when sharing is enabled after being disabled,
before exposing it on a heartbeat or permitting a page. A disablement
records the disabled marker and retains its ordinal floor. Leave the
ordinal unchanged when non-policy settings or the route change. Bounded
temporary loss of transport does not advance it. If a crash left a new
binding unresolved, retry from the durable state rather than allocating
from volatile config. Reserve one strictly increasing issue creation
time under that exact ordinal and revision before encoding any page.
Burn the time on abandonment or failure. Apply the current wall
floor and persist the accepted issue time in that reservation transaction.
The planner must consume the owner's durable issue reservation or move
its start operation inside the owner's transaction. The reviewed in-memory
`start_issue` needs an explicit durable-time input before use in this
owner. A current wall value earlier than the durable
floor defers issuance and does not erase or move the floor.

Only the accepted owner removes a retired registry binding. First close
native egress and hide the view, persist terminal retirement, remove
status values and staging, and keep the room generation and native
retirement fence until no native work can run. Never reuse a local
generation. Authority for replacement is a different accepted binding.
Historical status or a signed event from a retired room cannot restore it.
A manual backup restore can roll counters behind already delivered values.
Disable status on restored state and require full accepted-owner
reconciliation with a new explicitly accepted binding before sending.
Peers ignore repeated values and the restored store cannot discover a
remote sequence floor reliably.

## Publication through the native owner

Use a closed Peer-only application kind and marker selected in the
publication proof design and reserved together with the accepted Peer
app dispatcher. Keep the acceptance protocol distinct. Encode the
reviewed status record in the canonical `MarmotAppEvent` from the
accepted server account and use the exact planner `created_at` as the
authenticated application creation time. The envelope marker, exact room,
sender, leaf and epoch all bind independently. Prove the inspected
8 KiB permission envelope plus Marmot/MLS/relay overhead can fit the
actual native path before enabling it. Use the Peer group lane
(kind 445), not the private invitation carrier (kind 7435), ordinary
Command, Terminal, or a public metadata publication.

The reviewed fixture shows an epoch-pinned `SendIntent::AppMessage` and
public retained application effects. Existing `PublicationGate` pauses
general recovery but does not authenticate status provenance. Build the
status-specific check at the accepted Peer adapter's actual last publish
boundary. A permit binds exact native signed kind-445 event ID, exact
application event ID, local binding/epoch, current authorization revision,
purpose, route classification, frozen exact allowed endpoints and absolute
deadline. Its trusted association must exist before the first network
egress and be rechecked for every retry and every destination. At that
point verify status enabled, key grant, route confidentiality, signed
time, native provenance, exact endpoints and current gate. Raw IDs or
wire fields alone never create a permit.

A Prepared heartbeat expires at the earliest local, native or signed
deadline. Derive the signed upper bound with checked arithmetic from
the fixed sample time plus the codec's `fresh_limit(interval) + interval`.
Keep any shorter local freshness or action deadline. Unknown native
work past expiry remains spent and unresolved until disposition. An
expired event cannot be refreshed or automatically handed off again.

The publication proof's starting candidate is a retained public native
storage handle. At the first adapter entry, resolve the request's exact
verified native event ID to the already-written native
`OutboundFanout` and its exact application ID via
`application_message()`. This lookup must be bounded, race-free and use
only supported public interfaces on the existing owner connection.
Match the frozen binding, bytes and target against the owner's prior
write-ahead intent and live gate. Prove native storage has committed this
association before the first adapter call and can read it there without
blocked reentrancy. It must distinguish maintenance effects and unrelated
application sends. Never attribute from timestamp, marker guess, the
current sole send slot or post-return effects alone. If the pinned
public storage/transport path fails, select a separately reviewed exact
public provenance API or keep publication disabled.

Treat publication as one serialized owner operation. A close/revoke
linearizes before the next network send starts. If a transport adapter
launches independently per endpoint, apply the fence at each actual start
or use the single exact accepted origin. A request already in network
I/O when close linearizes can complete as prior release with no retry.
Neither an application queue removal nor endpoint
intersection for new sends closes a pre-existing native fanout.

Use a bounded encrypted write-ahead obligation per room, with eight
across the profile. Each committed stage is explicit.

| Cut point | Owner status transaction or native state | Recovery |
| --- | --- | --- |
| Reserved | In the publication slice, atomically commit authorization revision, sequence or issue reservation, and reserved work identity before exposing it. | Burn reservation and abandon. This stage has no native handoff. |
| Prepared | Commit kind, canonical app event ID and digest of the exact encoded app event, binding/epoch, revision, slot and absolute wall expiry before calling native send. No app plaintext is needed on disk. | Leave network gate closed. Look up the exact app ID in the retained fanout, queued intent and native effects. Missing or ambiguous evidence never grants a permit. |
| Native owed | MDK can now persist a queue record or frozen fanout and invoke the first adapter before returning. Validate the frozen native ID against the committed Prepared row at adapter entry. Record the exact returned native ID/status or uncertainty in the owner DB. | Probe both DBs with publication still closed. A committed Prepared row with a matching native entry is already handed to native, even if no result was stored. Terminally dispose of any old work through the scoped native owner. |
| Terminal | Record exact native completion/suppression and any possible network exposure. Do not mark status delivered from an ambiguous or partial native result. | Keep suppression through exact native disposition and reconciliation. Clear only once supported terminal proof shows no automatic retry remains. |

Cover a stop before and after every step plus loss of a network result
and failure to persist a terminal result. Owner state and native storage
are independent encrypted databases. A native result alone supplies no
authority to open egress on restart. Never retry by regenerating the
encrypted event or switching origins. A committed Prepared row that
might have reached native remains unresolved unless exact storage
certifies it is absent or disposed. Unknown native work for the status
lane stays closed. A native result with possible exposure is spent,
including partial publication. Do not rewrite it as a never-sent event.
Hold a single in-flight application obligation per room and reconcile it
before another send.

Before status handoff require a converged pinned epoch and no unresolved
same-room native application intent or blocking fanout. Recheck in the
serialized native owner. Even an epoch-pinned send must be measured
against queued-intent outcomes. Treat any unexpected `effects.queued` or
stale newly discovered queued intent as unsent, ineligible for egress and
an unresolved obligation. Bind it by exact application and native queue
identity. Reconcile both fanouts and queued intents at startup and during
retirement. Prove exact deletion through the supported public queued-
intent contract and verify no regenerated send can occur before or after
the deletion. No scheduler call regenerates or adopts a queued proposal
as new status work. Where public inspection cannot classify an existing
queue item, keep the affected room gate closed.

On startup close all old permits, including for unchanged config. Prove
public exact cancellation or terminal disposition across first-send,
partial-send and crash gaps, then fresh same-room decryption across
skipped sender ratchet generations. The public retained list alone,
`recovery_paused`, and test-feature-only
`session().delete_outbound_fanout` do not establish retirement. Public
engine or storage interfaces may be candidates only if their documented
contract and the proof cover supported disposition and continued owner
consistency after possible exposure. If exact cancellation is unsupported,
keep status disabled unless the accepted owner proves whole-room
retirement, zero egress during cleanup, and consent-based replacement.
Ordinary Command ownership and retry behavior remain independent.

## Durable receive and clocks

A client receives only through the accepted owner's exact authorized
connection/subscription, native group ID and verified source. Require the
current client-side binding, expected app kind and marker, active native
sender leaf for the pinned server, current MLS epoch, exact application
author, and a canonical decoded status record for the app room ID.
Check bounds before app parsing and use the codec's authenticated
creation and age rules. Store exact native application source identity
with each changed status checkpoint. No unverified frame can allocate
permission staging or modify a projection, and no decoded status can
acknowledge acceptance, change grants or mark command admission.

Extend the existing receiver with a private versioned typed checkpoint,
checked restore and a plan/apply transition port in the receiver storage
slice. Keep one source of truth for conflict, expiry and replay decisions.
Pass the profile staging admission into that transition with the current
transaction. Derive an incremental mutation for one page or bounded
metadata. Do not clone a whole 368 KiB stage on each input. Apply the
validated live mutation only after SQL commit and require the existing
codec tests to keep their behavior without substitutions. Persist the
seen heartbeat sequence
and first local receipt with its distinct fresh-display sample, current
permission ordinal, digest, representation/conflict/issue fence,
completed canonical rules and their generation and confirmation times,
and one staged issue with exact metadata/first receipt and received
pages. Serialize bounded typed fields only. Store stage pages in
individually bounded rows with exact stage identity and unique indices,
so one new chunk does not rewrite the entire stage. Cross-check canonical
descriptions and stored digest, signed/receipt timestamps, counters,
containment, stage total and page shape on restore. Corruption or an
unexpected schema keeps status closed. Restore lazily by current binding
and validate aggregate budgets before materializing content.

A receive transaction checks the current authority revision and runs the
receiver's read-only transition planner. It atomically writes changes,
including any poison, expired reservation or advanced ordinal, with the
current exact source. Apply that typed mutation to the in-memory receiver
only after commit. Commit failure discards the uncommitted mutation.
Authenticate no-ops using the same current authority. An `Ignored`
result may also have changed expiry state, and `Deferred` can fence an
older ordinal. Commit changes in either case. Never acknowledge a
`Deferred` native app event until a terminal codec decision or an
independently reviewed bounded durable defer record. The first version
keeps the exact native app event pending and retries within signed expiry.
Cap authenticated unresolved native status inputs before staging at eight
per room and 128 per profile, including one committed-but-unacked slot
per room in the cap. At pre-ingress, charge every candidate encrypted
input for the exact Peer stage against native pending capacity until
native authenticates and classifies it. Backpressure an occupied stage
using the accepted lane's bounded recovery contract. Reserve control and
disposition service independently where the owner can prove the routing
before decryption. If the native entry cannot enforce the bound and keep
accepted-room control progressing, leave status unavailable. When the
signed age expires, commit a
terminal refusal of an authenticated deferred input and free its slot.
Use fair retry turns and preserve unrelated-room control and Command
capacity. Invalid authenticated records
with an unchanged receiver get a terminal refusal after current binding
checks. Clock, budget, authority-read and storage faults are retryable
without any receipt. Return durable relay/native progress only under the
accepted owner's own ingress rules. Use one bounded committed-disposition
slot per receive room. Persist a
terminal status disposition with the exact native application ID in that
slot before deleting that pending item via
`MessageStorage::delete_pending_application_events`. Do not commit another
terminal disposition in that room while the slot is unresolved. A crash
before commit does not ack. After commit, replay of that exact pending ID
retries deletion through the current owner and does not rerun the
codec or refresh receipt. On ambiguous deletion, confirm exact absence
before clearing the slot and before processing the next event for that
room. The pinned API lists pending events globally in a stage. An absence
probe needs a proven pre-ingest bound of 16 pending native application
events per separate Peer stage including status, one scan per retry turn.
Otherwise use a narrowly proved exact public lookup or keep the gate
closed. Different rooms retain independent service. Other native IDs with
duplicate or older authenticated app data follow the ordinary codec
fences. No unlimited source log or independent retained raw event copy
is necessary. When native work is definitively retired,
apply the accepted owner's source disposition without reopening status.

Use local Unix seconds and the suspend-aware monotonic clock. Persist
a per-profile qualified wall high-water mark in each sender reservation
before any native handoff and in each successful state-changing receive or
terminal receive decision. Preserve it across normal recovery. On zero
wall time, rollback or unreadable clock, close status egress and suppress
host age, host values and permission values from current status views.
Retain fences and retry when trusted time catches up. Add an offline
operator-qualified clock rebase as a separately reviewed recovery path.
Require the profile lease with all native status transport stopped and
all native obligations reconciled or terminally suppressed. Require a
trusted local wall assertion with confirmation of canonical local
identity. In one owner transaction close and advance status authorization
for every affected binding, persist the explicitly accepted lower wall
floor, burn all active issue times, discard exposed/cached values and
abandon stages while preserving all receive and send replay fences.
Allocate new monotonic policy ordinals before issuance in bindings
where old wall issue fences would otherwise block. The dedicated offline
path must be closed when the owner cannot establish trusted time or clean
native retirement. Automatic rebase is forbidden. Until it is implemented
and qualified, time recovery is to keep status closed until the original
floor catches up.

Record native monotonic receipts only in the current process time domain.
On restart do not synthesize a receipt or compare monotonic counts from
different runs. Restore with explicit unknown prior-process monotonic
receipts. A completed projection is stale/cached at best until a new,
actually observed eligible confirmation. A replay cannot restore
freshness. An incomparable wall clock produces unknown. Restore first
qualified wall receipt for offline age and use it for seven-day pruning
after restart. Abandon interrupted staging transactionally before
accepting new pages after restart, advancing that ordinal's issue fence
to the abandoned issue time if greater and deleting its pages. A newer
same-generation issue can recover. Keep replay fences when retention
drops values. An invalidated native source or authoritative room
retirement hides state immediately and cannot regain it by revalidation
of the retired binding.

Cap both directions at 64 status bindings per profile, or the accepted
owner's lower limit. Maintain at most one stage per receive binding,
64 chunks of 5,760 decoded bytes, eight active reservations, 4 MiB
aggregate stage storage/memory, and 32 MiB aggregate completed canonical
content. Keep only the one newest completed representation per room. It
may be an older cached generation while the current generation stages,
and an atomic replacement removes it when a new description completes.
At 64 times 368,640 bytes the maximum is below 32 MiB. If a store budget
would still be exceeded, fail closed and defer the change without
evicting replay fences. Count reservations and their
replacement inside the serialized transaction, not merely live caches.
Reserve only after authenticated framing and freshness. Do not display
partial policy rules. Bound native pending-work admission separately
before enabling. Retain authenticated values up to seven days in the
encrypted profile, including explicit heartbeat, too-large and completed
receipt times. Scope secure deletion to the database configuration and
do not promise physical erasure from old backups or delivered records.

## Scheduling and local views

Run low-priority round-robin status work from the existing owner tick
after command/stream work. Use the dedicated sampler worker, one sample in
flight per process and one planner per bound room. Apply the planner's
two-second sample deadline, 30-second sample attempt/retry floor,
deterministic room jitter and latest-view coalescing. Invalidate receipts
and tickets on route loss, policy change or revocation. No catch-up burst
after suspend, restart or outage. The one-hour default precedes jitter.

Never await a status relay acknowledgment within the ordinary Command
owner or daemon's serialized tick. The accepted Peer owner needs a
bounded supervised asynchronous work lane for native status operations
with one serialized native runtime worker per owned stage at most, no
new task per configured key and no second writer for any runtime. Pass
only a closed typed status work item after the durable reservation.
Maximum four native status actions in flight across the profile, one per
room, at most one new action per 100 ms tick and four per rolling
second including retries. Reserve one slot per second for a due
heartbeat. Each started native action gets at most one second including
network call and relay ACK, or the earlier app deadline. Timeout
marks uncertainty and holds that room's obligation until exact native
reconciliation. No detached retry can outlive the owner. The status proof
must demonstrate cancel-and-close bounds and continued normal Command
service with a nonresponsive Peer relay before this lane is accepted.
Peer owner commands and invitation control retain independent work budgets.

Send permission pages after activation and effective generation changes.
Schedule at most one same-generation issue per hour from its last
reserved issue, including restart and abandonment. Require terminal
disposition of older native work first. The hour exceeds the previous
240-second send window, the 900-second accepted signed age, and the
900-second fixed receiver stage, with room for the codec's 120-second
future skew at each end. Keep the same
issue identity throughout one attempt. A newer authenticated ordinal
frees the previous remote stage but must still obey current authority
and shared budgets. Coalesce to the latest generation.

Permit up to eight prepared issues and one page per room per second.
Widen the fixture's 60-second issue window to 240 seconds on both
clocks in the publication slice. Admit an issue only if a bounded
per-profile admission ledger can assign enough eligible future page
slots within that 240-second window, honoring earlier reservations,
the one-page room rate, remaining in-flight deadlines and one heartbeat
slot per second. Allow at most three permission starts per second. Under
continuous one-second-complete service, 512 pages need at least 171
seconds of aggregate slots. The remaining window is headroom, never
a guaranteed service rate or a reason to wait inside the ordinary owner.
Pause admission if health or external load makes reserved slots
unavailable. Deadlines always win. An expired attempt is abandoned,
uses its durable issue time and backoff above, and never immediately
restarts at page zero. Measured scheduling-delay and command
responsiveness under contention are production gates. No remote request
can trigger work.

Extend the accepted owner's same-UID, versioned, bounded inventory IPC.
Preserve the existing two-way socket credentials, profile lease and
15-second query budget, without unlocking credentials or hitting relays
from a CLI read. `marmish servers list` pages canonical npubs with
optional local alias, connection/invitation state, heartbeat state and
last observed facts. `marmish servers show NPUB_OR_ALIAS` shows the
canonical npub, status `fresh|stale|unknown`, signed sample time, first
verified local receipt, age when comparable, and present host fields.
Expose permission state separately as `fresh|cached|too_large|unavailable`
with generation plus a bounded rule page for that generation, using the
local inventory's existing safe policy rendering where compatible.
Require the exact generation for continuation and reject a changed
snapshot. Show current verified receipt and actual admission separately
where qualified local evidence exists. Pending, blocked and forgotten
rooms have no status detail. Resolve aliases by current local config
only. Retain the existing remote-argument delimiter behavior.

`marmishd clients list/show` continues to enumerate current
`execution_clients`, including never-connected clients. Append outbound
Peer invitation and last status send state, preferably aggregate list
state and detail on show. A relay ACK does not update last connection or
admission. Preserve separate last authenticated command submit and
engine admission, with their current journal and grant-scoped rules.
Only actually authenticated new Peer application evidence already
qualified by the accepted owner can contribute a separate Peer-access
time. Display explicit unsupported/denied/granted capabilities from
local current config instead of reading them from any received snapshot.
Use short fixed diagnostic categories and bound complete IPC envelopes
by 65,536 bytes. Escape control and bidi marks in rule output. No
authority secrets, paths, routes, stats, permission contents or identifiers
beyond the intended local inventory go to debug logs or public metadata.

## Increment boundaries and acceptance

1. **Offline durable status storage.** First qualify the privately
   mounted [send fences](peer-heartbeat-send-fences.md) in the SQLCipher
   fixture with the typed authority scope and local authority revision.
   A separate checkpoint slice adds the existing receiver's typed
   transition, restore and persisted projection API, without rewriting
   codec semantics. Exercise binding substitution, directional collision,
   failed-write rollback, sequence and ordinal exhaustion, received page
   admission and poison, storage fault rollback before exact native
   acknowledgment, restart and retention. The send fences own the
   per-binding sample and profile rollback floors. The separate
   publication proof's storage adapter adds the Reserved identity
   atomically with its sequence or issue reservation, plus the Prepared
   obligation and reconciliation state. No production migration or
   existence of the accepted owner is assumed for either offline slice.
2. **Native publication and ingress proof.** Use the same explicit fixture
   authority contract with two real native rooms over private WSS and an
   independent Command owner. Prove exact kind-445 status provenance on
   first send and verify no stale or regenerated egress after revocation,
   route or process changes. Exercise the write-ahead and ACK crash gaps,
   maximum record size, native queue/fanout cleanup, bounded async lane
   and current room recovery after blocked work. Exact safety and bounded
   continued operation are both required before activation.
3. **Production mounting and installed validation.** Requires the
   implemented and reviewed production accepted Peer owner from
   [Peer activation](peer-live.md), with the exact authority capability and
   separate native stage contract above, plus approval of steps 1 and 2.
   Add transactional migration, profile config, bounded tick integration
   and local views. Test old profiles, opaque status with no room or
   endpoint fallback, unchanged ordinary command/copy/rsync traffic,
   pre- and post-commit process termination, permission limits and both
   directional rooms for one remote. Verify same-user IPC and refusal of
   wrong-side writes. A working Command stream remains independent of
   unavailable status. Qualification requires installed Linux AMD64,
   Linux ARM64 and macOS ARM64 pairs on self-hosted runners. Keep only
   core deterministic storage/codec decisions in required CI. Expensive
   installed cases, live WSS and benchmarks remain opt-in.

Record signed publication-to-projection and native ACK latency, collector
and storage transaction p50/p95/p99, cold recovery and large permission
transfer sizes/timings, idle load and command latency under status
contention. Use disposable identities and report only aggregate counts
and timings. Activate only after actual code review and end-to-end
evidence for the exact paths. This design does not expose a production
flag or authorize a release.
