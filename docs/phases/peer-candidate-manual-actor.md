# Private Linux candidate Manual receiver actor

This phase supplies the actual actor for the reviewed
[daemon composition](peer-candidate-daemon-composition.md) and
[Pending view](peer-candidate-pending-view.md). The private Linux AMD64 build
qualifies Manual recipient mode for a Ready Client or Both profile on one
configured, explicitly trusted private WSS origin. Both contributes the same
recipient authority as Client. Founder, Auto, approval and other write IPC,
Active, Group application traffic, heartbeat and public/default relays stay
outside this phase. Default binaries, standard installed checks and normal CI
keep their existing behavior.

## Ownership and start

Use the daemon's one readiness handshake. The existing run path passes the
original lease Arc, exact identity material, sealed `RegisteredReceiver`,
worker start and sole Pending view producer to one owned actor thread. The
thread uses a current-thread runtime for WSS and owns every Peer SQL/native
operation. The daemon holds actual join and stop controls. Peer does zero
cursor, stage or WSS work until kernel listener registration and ordinary
readiness both succeed. Startup failure, a listener exit or daemon stop
invalidates that view generation and is terminal for this actor.

Consume the existing registration once. Retain a narrow read-only verifier
from the original `RegisteredDispatch` for the daemon and actor after
`RegisteredReceiver::attach`. It can verify or latch loss of the exact
process dispatcher under the existing registry lock. It cannot register,
open or mint quota again. Keep the original checked profile and member
directory descriptors, Ready/manifest and qualification, account, config
and route bound to this same lease and material. From that authority derive
the local SQL source and later the existing port admission. The future
non-test `ReceiverRouteAdmission` must consume the checked route and exact
signer scope from this registration. Fixture roots, a generic origin input,
caller-supplied founder or wall, and a newly reacquired lease confer no
authority here.

Open and verify one keyed stock cursor with exact installed schema ancestry,
then recover charged inventory before connecting. Refactor the concrete
`RecoveredOwner` and `PendingReceiver` constructors so the actor carries
this same owner into the bridge. The evidence fixture's
`PendingReceiver::open` currently requires a live port before opening the
cursor. Keep local recovery and port use as distinct steps. Maintain
disjoint borrows of the attached registration/quota, scope, exclusive
cursor owner and port inside the thread. A held or joined tuple retains the
same exact quota identity used for its original and snapshot turns. Keep the
existing reserved cursor, stage, journal and snapshot limits. Avoid a second
cursor connection for projection, provisioning or retry.

A startup budget of 15 seconds from the second readiness edge bounds
local admission and WSS authentication together. Use monotonic remaining
time clamped by qualification and retained TLS validity where applicable.
Startup uses one WSS attempt. After qualification, take finite turns on
one authenticated physical port, at most one complete checked carrier or one
bounded exact page application at a time. Park idle on readiness, deadlines
or the next maintenance time. Clamp checked network waits to the next
maintenance wake or an existing stricter transport limit. Use one serviced
native turn at a time, at most five seconds including its preflight,
checkpoint and postcondition, further bounded by signed deadlines and live
qualification. Check stop/readiness and time before each turn, immediately
before native or durable effects and before releasing any proof. Pass the
existing concrete fence into the one-polled native operation. This bounds
admission and acceptance, since synchronous SQLite/OS calls can return
late. Keep the lease and actual join alive while such a call finishes and
report incomplete shutdown while it is outstanding. Disconnect or lost
exact physical scope closes the actor for this first qualification.

## Durable work

Carry forward checked v2 Offer, Consent and Delivery in their existing
exclusive cursor order. Per-selection policy derives from the current
checked Manual config and the authenticated sender. Whatever one selection
holds stays charged through ambiguity and across restart. Existing
authenticated cursor dispositions, exact durable one-shot Consent send and
deferred Delivery recovery still govern any ACK loss. Reading relay history
does not authorize replay of an effect. No new relay subscription or role
extends the actor's checked recipient lane.

Live package provisioning is an explicit source gate. The existing WSS
proof reaches Held through fixture `ProfileSlots`, and the prepared Consent
fixture attests using an unmetered temporary directory. Extract the exact
installed v2 transitions, including the durable Unknown reservation, into
the exclusive cursor owner's turn. Qualify fixed-name exclusive provisional
creation after current durable reservation, and use only a managed keyed
stage for native package creation and a separately charged exact snapshot
for package attestation. Bind each to the original registration, clean
inventory, stage/generation/inode and package seal. Reuse the existing
checked package and checkpoint types. Verify both managed operations and
recovery ordering before advancing the actor to physical v2 Consent. If
the available native aperture cannot prove the intended capped operation,
the actor stops before that effect and the affirmative mount remains
unqualified. It cannot obtain a fixture attestation or a fabricated charge.

For a current durable Held row reverify the exact retained signed v2
Delivery in a fresh port scope. The concrete capped bridge prepares from
an independently charged snapshot, closes that file turn, starts an exact
durable JoinStarted in a fresh fence, and consumes its private one-use
Welcome proof on the original keyed stage. JoinStarted on restart permits
only exact readback and the already qualified managed journal recovery
where required. Never reconstruct Welcome-ingest authority. Qualify an
exact current native readback before the same-generation Pending CAS.
A refused or ambiguous mutation publishes no view. Keeping expired or
uncertain custody charged is part of this gate. There is no generic
retirement sweep, Leave or charge release for this installed actor.

## Maintain, qualify and answer

Maintain on the actor independently of IPC, targeting the approved 500 ms
pace. Before a durable or native change invalidate the view. The sole
producer issues a one-use ticket before the earliest covered dependency
read. Use a fresh admission sample and exact durable plus native
readback within the same turn after a successful Pending CAS. If a
separate maintenance turn sees durable Pending, use a read-only exact
checkpoint aperture and a clean charged bridge readback. Closed-scope
disposable snapshot use is part of maintenance. Rollback, native Join and
checkpoint mutations belong to the invalidating actor path. Pending
readback refuses a journal that needs repair; it cannot use the
JoinStarted recovery aperture. A failed or late turn cannot extend any
view. Enforce the approved strictly under one-second proof lifetime even
if a native turn remains within its separate five-second ceiling.

Only the current fenced bridge/owner can mint a non-Clone proved result
bound to its producer ticket, actor and listener epoch, exact selection,
row generation, signed ancestry, native authority, Held/Pending bounds,
current verified wall and effective floor. Keep those constructors private
to the checked paths. Convert it to a redacted observation via the existing
producer. A separate checked inventory and exact SQL pass can prove
healthy-empty only when current scope, applicable authenticated floors and
all relevant retained ownership prove absence of a row and outstanding
charge. Missing, ambiguous, Reserved, Held, JoinStarted or expired retained
evidence cannot seed Empty. The reader never gains access to a proof
constructor. Preserve the existing one-way view invalidation, exact
Show equality and expiry latch.

The single same-UID IPC listener holds only the memory reader. List/Show
uses the existing four-slot nonwaiting permit through reply admission and
the existing final generation, monotonic and wall checks. It has no
message sender to the actor, write operation or cursor/port/quota access.
The only result for startup, invalid view, contention, expiry, stale
proof, listener loss or actor stop is the existing unavailable surface.
Use the approved exact healthy-empty/List and exact Show semantics.

## Failure and cleanup

Run the exact global verifier at actor startup and around managed turns,
including their error paths. Verify global forwarding independently when a
local scope or stage check fails. Path drift, quota or SQL/native refusal,
qualification/route loss and integrity errors confined to Peer stop Peer
and preserve ordinary work when global verification succeeds. Default
dispatcher mismatch, changed descriptor or poisoned registry latches
Indeterminate, clears the view and signals the reviewed process-fatal
physical-only shutdown. Do not remount, unregister, clean evidence or
continue ordinary SQLite through an unverified default. Stop listener
admission and invalidate readers before waiting for a blocked actor. On an
ordinary shutdown, stop new actor effects, drop any unused permits and
native file handles, stop/drop the exact port, close cursor/quota ownership,
and physically join before releasing the one lease. The fatal path performs
only verified physical closure as defined by the daemon composition.

## Compilation and exit evidence

Use `marmish_peer_receiver_candidate` solely on Linux AMD64. Promote only the
installed v2 cursor/Offer/Consent/Delivery glue, concrete receiver inventory
and capped Pending leaves and checked route/permit producers shared by the
candidate and the qualification harness. Use `any(test,
marmish_peer_receiver_candidate)` at those leaves with their existing
target constraints. Keep legacy fixture parents, `ProfileSlots`, Founder
and third-role constructors, temporary attestation, fault hooks, metrics
counters, injected clocks and test relay authority gated by test. Retain
the reviewed trust and registry mounts. Keep candidate daemon/view linking
at existing run and IPC boundaries. Source-only tests derive their
coverage from the same shared checked functions.

1. Compose approved registration and readiness with an unavailable view.
   Prove no Peer I/O before both readiness events, both edge orders,
   aborts, no fallback to another owner, typed local and global faults,
   stop-before-drain and lease retention through actual join. Exercise
   ordinary Command and retain existing stock/Terminal behavior.
2. Qualify actual candidate source for same-cursor recovery, managed package
   issue and attestation, checked Consent and Delivery, and the exact
   capped Welcome/readback bridge. Cover charge refusal and custody
   ambiguity, no repeated ingest, postchecked deadlines, and corruption
   that withholds positive projection. Keep installed reads unavailable
   until this evidence and the observation composition are reviewed.
3. Install the explicit private non-test Linux AMD64 binary. Use disposable
   identities and a private WSS relay/founder fixture outside the receiver
   authority. Demonstrate foreground/background readiness, a real signed
   v2 exchange through durable Pending, cold new-process same-profile
   readback and exact List/Show. Exercise healthy-empty versus unavailable,
   an expired, stale and wrong selection, a parked actor and saturated
   IPC, and ordinary Command while Peer is available and unavailable.
   Confirm the candidate has no write or active Peer surface and the stock
   package has no candidate surface. Report startup and actor turn timing,
   view refresh age, availability and local IPC latency without treating
   measurements as substitutes for the enforced ceilings. Keep these
   private benchmarks and the full private proof opt-in.
