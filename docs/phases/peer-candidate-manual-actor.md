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

## Exact prerequisites

The design base is `de7338f`. The independent inputs are daemon composition
`66d6ea0`, readiness `24dc054`, registration `5d21b62`, Pending view design
`7fa52a2` and source `1f63e9f`, and capped Pending bridge `7d725d0`.
Only the view source is included in this base. Before editing, record the
reviewed integrated successor of each commit, their combined source and
the route and Delivery implementation used by that source. The private
bridge still requires its integrated refusal-matrix disposition.

## Ownership and start

Use the daemon's one readiness handshake. The existing run path passes the
original lease Arc, exact identity material, sealed `RegisteredReceiver`,
worker start and sole Pending view producer to one owned actor thread. The
thread uses a current-thread runtime for WSS and owns every Peer SQL/native
operation. The daemon holds actual join and stop controls. Peer does zero
cursor, stage or WSS work until kernel listener registration and ordinary
readiness both succeed. Startup failure, a listener exit or daemon stop
invalidates that view generation and is terminal for this actor.
The daemon starts the thread through the following private interface.

```text
Runtime::start(lease: Arc<ProfileLease>, material: IdentityMaterial,
               registered: RegisteredReceiver, startup: WorkerStart,
               producer: Producer, control: Control) -> Result<Runtime, StartFailure>
Control::stop(&self)
Runtime::join(self).await -> Result<ActorExit, JoinFailure>
Reader::admit(&self, listener: NonZeroU64) -> Option<Read<'_>>
```

`Control` is revocation only. The daemon and listener hold clones of the
same control with one stop cell, and the actor's handle owns the real join.
Use listener drop and the normal signal/lifetime path to synchronously set
that cell, close readiness and invalidate the view before drain. Runtime
exit distinguishes an ordinary stopped actor, a local Peer refusal and
Indeterminate, in addition to join failure. Signal Indeterminate through
the existing fatal lifecycle while the actor still owns its resources.

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
then recover charged inventory before connecting. First schema creation is
under the fixed 49 MiB cursor reservation after registration, within the
same startup budget. Require the candidate's recovery and operation APIs
to borrow the attached quota, including recovery of an original journal.
Keep `ReceiverQuota::bind_receiver` and the binding variant of
`RecoveredOwner::recover_journal` test-only. Assert the exact attached
quota's identity at all managed operation boundaries. Refactor the concrete
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
Sample the second edge on completion in the shared handshake so actor
scheduling delay cannot reset that budget. A late synchronous open
cannot authorize a later step. Startup uses one WSS attempt. After
qualification, take finite turns on one authenticated physical port, at most one complete checked carrier or one
bounded exact page application at a time. Park idle on readiness, deadlines
or the next maintenance time. Clamp the idle `wait_live` deadline
to the next maintenance wake.
A complete page or physical publish uses the existing checked transport
deadline and yields at the end of that operation. Never cancel a partial
exchange and reuse its socket as clean. A stale view during a bounded
exchange is allowed to expire. Use one serviced native turn at a time, at most five seconds including its preflight,
checkpoint and postcondition, further bounded by signed deadlines and live
qualification. Check stop/readiness and time before each turn, immediately
before native or durable effects and before releasing any proof. Pass the
existing concrete fence into the one-polled native operation. In candidate
turns its synchronous `wall` check observes both the shared stop cell
and the live `Ready::is_current()` watch state, including inside native
callbacks. Do not rely on the actor polling a stop future while blocked.
This bounds admission and acceptance, since synchronous SQLite/OS calls can return
late. Keep the lease and actual join alive while such a call finishes and
report incomplete shutdown while it is outstanding. Disconnect or lost
exact physical scope invalidates the view and closes the actor for this
first qualification. The old cached Pending cannot survive this loss, and
the same process does not reconnect.

## Durable work

Carry forward checked v2 Offer, Consent and Delivery in their existing
exclusive cursor order. At candidate admission map the exact sealed
`Receive::Manual` to `Policy { mode: Mode::Manual, reserved: empty,
blocked: empty }` and `ReceivePolicy::Manual`. The current closed schema
has no Manual lists. Every admitted founder takes the one Unknown slot.
Signed recipient, origin, selector, cooldown, deduplication and slot
constraints still apply. Do not interpret Command grants, or the
Auto-only founder list, as Manual policy. Abort Peer admission before
registration for originate intent, Off, Auto or nonrecipient role in
this qualification. No caller may supply a replacement `Policy`. A
test with a competing founder must prove refusal to attach to the first
founder's occupied slot, and verify that the only private founder
authority is the independently configured test sender. Whatever one selection
holds stays charged through ambiguity and across restart. Existing
authenticated cursor dispositions, exact durable one-shot Consent send and
deferred Delivery recovery still govern any ACK loss. Reading relay history
does not authorize replay of an effect. No new relay subscription or role
extends the actor's checked recipient lane.

Live package provisioning is a required source and installed gate. The existing WSS
proof reaches Held through fixture `ProfileSlots`, and the prepared Consent
fixture attests using an unmetered temporary directory. Extract the exact
installed v2 transitions, including the durable Unknown reservation, into
the exclusive cursor owner's turn. Reuse the `peer_stage_binding` sequence
`bind_stage`, `start_stage`, `record_prepared` and exact stage witness.
After the durable Unknown reservation and binding, obtain inventory-derived
charge before fixed-name exclusive creation. Bind the created file's
inode in durable NativeStarted before one exact one-polled native package
creation. For later attestation use a separately charged exact snapshot.
Use the existing 16 MiB closed-main cap, 49 MiB cursor reserve and
strictly sub-128 MiB total charge including original, journal and snapshot. Bind each to the original registration, clean
inventory, stage/generation/inode and package seal. Reuse the existing
checked package and checkpoint types. Verify both managed operations and
recovery ordering before advancing the actor to physical v2 Consent. Both operations must pass for source and installed qualification. If the
native aperture cannot prove either capped operation, split the required
work into a reviewed prerequisite, keep the actor unavailable and leave
this phase open.

| Cold crash cut | Next permitted turn |
| --- | --- |
| Durable Bound without creation | Recheck exact current reservation and quota, then admit exclusive creation. |
| File created before durable NativeStarted | Retain charge. Resume only if existing ownership checks prove the exact allocated empty file; refuse an ambiguous replacement. No native key creation before durable inode binding. |
| Durable NativeStarted without attestation | Retain charge. Recover the exact keyed image under the current distinct recovery aperture if necessary, then read one exact durably signed package from it. A missing or unsigned artifact remains charged and unavailable until independently proven recoverable. No new generation or repeated key creation. |
| Durable Prepared before Consent | Read the exact package from a separately charged clean snapshot and check the stored artifact, generation and current signed bounds before entering the existing Consent turn. |

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
if a native turn remains within its separate five-second ceiling. Sample
the IPC-compatible `Instant` before constructing the native fence. Also
cap the maintenance proof using the fence's continuous clock from the
beginning of the same observation. Either expired clock withholds
publication. The ticket origin stays fixed through snapshot closure and
exact re-read. Report p50/p95/max and saturated-view refresh gaps on at
least 20 maintenance turns with the largest valid near-cap test image
on the qualification host. Require p95 at most 500 ms and that every
counted publication is qualified in less than one second. Late turns
are counted separately and leave reads unavailable when the earlier
view expires. The installed gate needs a successful repeated positive
read/refresh before the old view expires, including a capped fixture
defined by actual measured bytes. A timing failure leaves this gate open.
Do not relax the clocks or add an unproved cache to achieve the target.

Only the current fenced bridge/owner can mint a non-Clone proved result
bound to its producer ticket, actor and listener epoch, exact selection,
row generation, signed ancestry, native authority, Held/Pending bounds,
current verified wall and effective floor. Keep those constructors private
to the checked paths. Convert it to a redacted observation via the existing
producer. The owner-only `qualify_empty(ticket, fence)` performs a
separate checked inventory and exact SQL pass with the same ticket,
readiness, route, clocks, floor and wall rules as the positive path.
It proves healthy-empty only when current scope, applicable authenticated floors and
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
continue ordinary SQLite through an unverified default. Before an ordinary
owner exists this only drops owned resources. At runtime, the fatal latch
closes Command, stream, Terminal and IPC admission as soon as global trust is
lost, before ordinary stop helpers can queue work. The daemon composition
must prove a worker-side fail-stop check that rejects queued controls
such as CopyGc and late completions before entering SQLite. Keep the
ordinary finite command worker handles and their existing ownership
barriers. Close/cancel retained stream runner handles directly with no
stock journal progress/cancellation or relay report from those shutdown
calls. A normal FIFO engine stop or `command_lifecycle::Frontend::stop`
alone is insufficient. Already executing operations retain ownership
until actual cessation. Keep a failed join incomplete. Stop listener
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

| Concrete leaf | Current gate or seam | Candidate change |
| --- | --- | --- |
| `peer_candidate_registration`, `peer_quota_candidate_registration`, `peer_ready_scope` | Already shared on selected Linux AMD64 | Keep existing mount, add read-only global verifier and same material/route admission. |
| `peer_physical_route::ReceiverRouteAdmission`, `peer_relay_trust` exports | Fixture recipient/founder issuers test-only | Keep fixture exports test-only and add a sealed candidate issuer. |
| `peer_recipient_port` carrier witnesses and use methods, `peer_installed_offer_*`, `peer_installed_consent_port`, `peer_installed_delivery_*` | Raw port shared, installed controller leaves under tests | Mount exact checked carrier/settlement and implemented receiver owners under shared Linux gate; isolate test harness, phase cuts, fixture policy and fixture issuer. |
| `peer_installed_offer_store::attach_installed_v2`, `peer_receiver_inventory`, `peer_receiver_clean_charge`, `peer_quota_dispatch` receiver turns | Installed test owner and measured leaves test-only; registrar shared | Mount the exact checked schema/inventory leaves under shared Linux gate. Promote same-quota checked original/copy apertures; keep second-bind fixture helpers and scalar charge absent. |
| `peer_receiver_v2_pending`, `peer_installed_pending_checkpoint`, `peer_pending_custody_checkpoint`, `peer_stage_binding`, `peer_stage_native` and `peer_stage_joined` | Test bridge and installed-only custody writers behind Linux test gate or inherited fixture parents | Widen shared Linux gate for only the durable v2/capped leaves and concrete package/native operations. Split fixture owner and legacy state transitions at inherited mounts when necessary. |
| `peer_pending_read_view` under `peer_read_ipc`; new `peer_installed_recipient` | View is test-only, actor absent | View shared only with its candidate mount and tests, actor candidate-only. Keep read request/response contract. |

Add configured-item/compile probes plus a symbol audit of unstripped
candidate and stock builds. Candidate must exclude `PeerFixture`,
`ProfileSlots`, `ReceiverRouteAdmission::fixture`, fixture attestation,
fault constructors and legacy write IPC. Stock and standard installed
checks also exclude the candidate actor and Peer read operation. A symbol
audit alone is insufficient for inlined or eliminated items.

1. Compose approved registration and readiness with an unavailable view.
   Prove no Peer I/O before both readiness events, both edge orders,
   aborts, no fallback to another owner, typed local and global faults,
   stop-before-drain and lease retention through actual join. Cut stop at
   each fence sample, listener loss mid-call, and the gap between durable
   arm and actual ingest. Assert charged JoinStarted, no repeated ingest
   and no positive result. Exercise
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
   an expired, stale and wrong selection, a parked actor, saturated
   IPC, and disconnect with exact Unavailable and no reconnect. Exercise
   ordinary Command while Peer is available and unavailable.
   Confirm the candidate has no write or active Peer surface and the stock
   package has no candidate surface. Report startup and actor turn timing,
   view refresh age, availability and local IPC latency without treating
   measurements as substitutes for the enforced ceilings. Keep these
   private benchmarks and the full private proof opt-in.
