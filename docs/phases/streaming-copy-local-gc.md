# Local Get receiver lifecycle and indexed collection

Status: scoped design revision 4; independently approved by Claude Code
(`claude-sonnet-5-5`) after four actual review rounds. This completes Phase 3's indexed client receiver collection contract.
The remote Get helper is a sender: its terminal status never proves local worker
shutdown. A free flock alone never permits GC. No unindexed filesystem scan,
plaintext path cache, PID signalling or daemon CommandEngine is introduced.

## Encrypted local index and receiver location

Add `copy_local_receivers`, `copy_local_gc_claims`, and bounded completion receipts
to the existing format-7 SQLCipher journal (`CREATE IF NOT EXISTS`). The receiver
namespace key is client/server/direction(Get)/transfer, excluding group. Store the
bounded closed Launch, intended Binding/group, revision, physical last stream,
receiver location, normalized daemon `limits.copy_receipt_retention`, checked
expiry, and lifecycle `reserved`, `active`, `closed`, or `unknown`. Cap rows at
1024/profile account and 16384 total; claims at 8 outstanding/profile and one per
namespace; reject excess with finite Capacity. Cap compact operation replay records at 1024/profile. Reserve, Activate, Release,
GC Claim, GC Complete, Forget and automatic unactivated Prune each produce one
record (request ID/timestamp, digest, finite result); exact duplicate operations
share their existing record. Index lifecycle/claim state is separately durable,
never removed by replay-record eviction. At capacity, atomically evict the minimum-request-timestamp record
(including the incoming record, because Prune records are backdated) and advance the profile local acceptance floor to its timestamp, using
MAX(existing_floor, timestamp). All request IDs at or below the floor are rejected
as stale (the local comparison is <=, independently of remote classify_new_id
which uses <), including a newly generated ID in the same timestamp bucket; retry with
a fresh ID/timestamp is a distinct operation evaluated against current state.
Release, Prune, Activate, GC and Reserve are never refused for replay-record
capacity. Index/claim caps still bound actual owned lifecycle state. No row contains secrets
beyond already-encrypted transfer paths and identity claims.

The local receiver base must not drift with the next CLI's cwd. Before reserving,
the CLI captures an absolute canonical base and dev/inode identity off the actor.
The worker receives that explicit base for Receiver; it does not reread `.` later.
A successful Receiver event reports its pinned base identity and destination
parent dev/inode, plus physical last stream, after checkpoint establishment.
Release stores these confirmed identities. For absolute destinations, still store
and check the actual pinned destination parent; the lexical base is not used to
resolve the operand. The immutable raw Launch/Manifest paths are never rewritten.

Get reclamation receives the indexed absolute base, expected base/parent
identities, Launch and physical last stream. Reopen and verify that base and pinned
parent *before* testing whether the private namespace is absent. An absent tree
is cleaned only under the matching recorded parent. A missing/replaced base or
parent is IdentityChanged/unknown, not successful cleanup. Extend the existing
filesystem API with an explicit `ReceiverSide::Get` versus `ReceiverSide::Put`,
not by dropping its direction check. Server Put behavior and Get-sender index
short-circuit remain unchanged. Reclaim never touches the final destination.

## Same-UID IPC reservation and lifecycle

All operations have closed bounded schemas and ordinary same-UID peer checking.
Lease identity is the fresh Binding.stream plus namespace/revision, avoiding a
second opaque token. Same-UID malicious callers remain outside the isolation
boundary; this protocol prevents accidental concurrency and crash confusion.

`CopyReceiverReserve { launch, binding, group, base, base_identity }` runs before
remote Open or receiver filesystem access. The actor verifies Get, current
profile client identity, binding/Launch accounts and transfer, bounded absolute
base, normal retention, and immutable equality with any existing row. It creates
a durable reserved intent or returns the exact prior result for the same stream.
The reply includes normalized daemon retention for the Receiver job.
A reused stream with changed fields is Conflict. Store request digest/result so
an IPC timeout can retry the identical reservation. The actor derives retention
from its config, never a lower CLI assertion. Expiry never shortens.

Reservation matrix:

| Attempt | No index row | Closed prior row | Reserved/active/unknown prior row | Claimed row |
| --- | --- | --- | --- | --- |
| Create | Reserve bounded intent; FS still refuses any existing unindexed tree | Conflict | Busy | Busy |
| Resume | Reserve intent; shared FS Resume refuses missing/unindexed-invalid state | Reserve matching immutable intent | Supersede with new intent; physical Resume flock decides Busy or recovery | Busy |

Superseding Resume does not authorize GC or declare the old worker stopped. It
changes the lease revision, preserves confirmed physical location/last stream and
sets lifecycle unknown until the new receiver is actually established and later
released. An old worker's stale release cannot mutate the new revision. Physical
flock plus the existing parent admission lock serialize Resume with a still-live
worker. A failed Busy Resume remains unknown and can be retried later; free flock
alone is still never deletion authority. This preserves Get crash Resume rather
than permanently stranding active/unknown rows.

The Receiver worker first verifies its opened base identity equals the reservation;
Resume requires the same indexed canonical base and identity or returns Conflict.
A successful Receiver event must report that same reserved identity. Absolute
operands retain this conservative base check as well as their actual parent check.

`CopyReceiverActivate { stream, revision }` after remote Open and construction of
CopyIo, but before Start or any Receiver job, verifies the exact
persisted client-side NativeCopy Get Open, Launch/digest/group/binding and reserved
intent and the currently active client command room for that session, peer and
MLS group. Fresh activation and cached successful activation both recheck that
room has not been revoked; the persisted Open alone does not retain authority.
Only then does activation permit the local Receiver job. An exact replay remains
idempotent while current authority holds; mismatches or withdrawn authority refuse. Reservation refusal happens before Open. A definite
Activate refusal after Open terminates the helper by bounded protocol Error+EOF
(as deterministic local failure), never a Cancel message. Uncertain IPC failure
or attachment loss follows normal disconnect semantics and leaves the intent
fence; it never invents remote termination.

`CopyReceiverRelease { stream, revision, disposition }` is sent only after
physical worker shutdown. The finite disposition is `NeverSubmitted`,
`Established(location)`, or `Attempted`. Frontend-owned state records whether the
Receiver job was ever submitted, independently of its cancellable protocol
future. Established includes verified base/parent identities and physical stream;
it marks closed and extends expiry to checked completion time plus retention.
Attempted without a successful Receiver event preserves prior physical identity
and marks unknown: filesystem side effects may already exist. NeverSubmitted
proves this attempt could not touch receiver state: delete a fresh intent with no
prior location, restore a superseded closed row to closed, or retain prior
active/unknown state as unknown. Store the predecessor disposition in the lease
so this rollback cannot invent physical shutdown. An identical release is
idempotent; altered release or stale revision refuses. Transport terminal status
never changes local lifecycle.

Activate is the mandatory gate before every Receiver job. Therefore a fresh
reserved intent that never reached Activate and has no predecessor physical
state can be pruned on restart or after its bounded reservation TTL (15 minutes),
with a retained replay record. A delayed Activate is refused by its missing
intent/revision check; the acceptance floor separately fences the old Reserve. A reserved lease
superseding an existing row restores its predecessor state under the same rule.
An activated intent on restart becomes unknown, preserving physical identity and
bindings. Resume may supersede unknown under the matrix; restart never proves an
activated worker dead. Closed rows remain closed. No lifecycle bypasses full
immutable manifest/prefix verification or exclusive flock.

The frontend owns Worker and the lifecycle lease outside the cancellable protocol
future, passing the worker by reference. It records successful Receiver events
in frontend-owned bounded state. On every exit, drop that future and both job
sender and event receiver, then poll the retained JoinHandle.is_finished() with a
bounded deadline off the actor; only join after finished. Dropping events lets a
worker blocked on its last result exit. Release only after successful join.
Timeout/panic is unknown, never physical cleanup proof. Explicit user signal
requests remote cancellation independently. Disconnect never sends Cancel; it
closes the local attachment and releases local files only after its worker exits.

## Bounded lazy GC claims

Before reserving the target transfer or opening remotely, request up to eight
local claims, excluding the intended target namespace derived from Launch. This
order permits collection even when the receiver-row cap is already full. Prefer expired closed rows in the same recorded
parent, then other eligible rows, ordered by retry-not-before/expiry/key. This is
bounded lazy GC: no new Get means no local sweep. It works in client-only daemons
without an engine/filesystem thread. Register signal handling before reservation
and GC. SIGINT during this pre-Open work is local interruption, not remote Cancel.

`CopyReceiverGcClaim { request_id, exclude_namespace }` persists the exact claim
and request/result atomically. An identical request returns the same grant after
reply loss. A claim holds namespace, physical location/stream, revision and a
fixed checked collection timestamp; it fences Reserve until completion. A
previous unresolved claim may be reissued unchanged to a later local executor,
including after restart. This is safe only because it originated from closed,
expired indexed state and all executors take the same admission lock/flock and
perform the complete descriptor/identity checks. This rule does not relax the
existing conservative server UnknownLaunch GC policy.

The CLI executes the claim in one bounded off-async filesystem job. The claim
survives cancellation or client death; no remote operation has started yet.
Repeat execution must be idempotent under the same fixed receiver identity. A
partially deleted namespace whose remaining metadata cannot prove that identity
is uncertain, not an absent-tree success. Do not attempt blanket cleanup of it.

`CopyReceiverGcComplete { request_id, claim, revision, outcome }` matches the exact
claim. Atomic completion stores its idempotent result before removing the claim.
Repeated identical completion succeeds even after successful row removal. The
bounded request/receipt caps above prevent unbounded durable bookkeeping.

| Filesystem result | Journal action |
| --- | --- |
| Verified cleaned/verified absent under matching parent | Remove index; retain bounded completion receipt |
| Busy exclusive flock/admission lock | Keep row closed; clear claim; set retry-not-before at least now+60s |
| Verified checkpoint not expired | Keep row closed; raise expiry/not-before to checkpoint expiry; clear claim |
| IdentityChanged, Conflict, CommitUncertain, HashMismatch, SourceChanged or Protocol | Keep row unknown; clear claim; report fixed reconciliation diagnostic |
| Missing/non-directory parent or UnsafePath | Classify IdentityChanged; keep unknown; clear claim |
| LocalIo, DiskFull, Quota, PermissionDenied or interrupted/unknown completion | Retain unresolved claim with bounded retry budget and backoff |
| Unsupported or NotFound outside verified-absent matching-parent proof | Keep unknown; clear claim; report fixed reconciliation diagnostic |

Each initial grant/reissue durably increments its attempt counter before replying.
Transient or interrupted claims use exponential retry delay: 60, 120, 240, 480,
then 960 seconds. After five grants without a successful finite completion,
retire the claim into unknown with `local_gc_retry_exhausted`, preserving the row
and physical metadata. An expired unresolved claim counts too; no completion is
needed to advance this bounded recovery policy. This prevents abandoned or
persistently failing claims from permanently pinning all eight claim slots.
Reissued unresolved claims are eligible after backoff, not always the first row. New claims and retries are round-robin/fair under the eight-job sweep
bound. Never collapse all errors into `cleaned=false`; the filesystem API must
return this finite classification and checkpoint expiry where applicable.


All lifecycle and GC IPC operations are non-command requests (150s existing
transport deadline); their actor work is SQL-only. CLI retries an identical
request at most once within a 15s total wall-clock budget, then treats the result
as uncertain. Request IDs include a creation timestamp, checked within the
existing replay horizon; actor persistence maintains a monotonic acceptance
floor. Replay records may retire after that horizon or earlier at the bounded cap,
atomically advancing the floor in either case. IDs at/below the floor are rejected,
never reinterpreted as new operations. An unretired replay within the horizon
returns the exact stored result. Automatic Prune uses its lease timestamp as the
record timestamp and cannot lower the floor. Reservation TTL pruning
retains this receipt and advances neither physical state nor remote replay floors.

Provide same-UID local index inspection plus `ForgetLocalReceiver { namespace,
expected_revision, acknowledge_retained_state: true }` for unknown/orphan rows.
It removes only the encrypted index/claim after explicit acknowledgement, never
any file. Warn that retained unindexed private trees still need same-UID manual
inspection/removal and are never discovered or adopted by GC. Exact revision
prevents deleting a concurrently replaced active row; active/closed rows with a
live lease/claim require explicit reconciliation disposition, not silent forget.
A crash after physical Receiver establishment normally permits Resume without
Forget. The narrow crash after Activate but before filesystem establishment may
leave an unknown row with no state for filesystem Resume; index-only Forget may
then be required. Repeated unactivated supersedes preserve the original physical
predecessor disposition, rather than nesting unbounded lease history. A stale
release after supersede cannot close the current row, even if the old worker
completed: the row may conservatively remain unknown. Exhausted transient-GC
rows likewise require successful Resume or explicit Forget.

An unresolved claim on the intended target cannot be swept destructively by that
Get. Reserve may advance an expired target claim's retry budget without issuing
filesystem deletion, and retire it to unknown after the same five-grant budget;
otherwise it returns Busy until a later eligible sweep or explicit reconciliation.
This never authorizes deleting the target during its own Get.

Local lifecycle GC does not delete remote Open receipts, alter server copy replay
floors, or affect Exec/rsync. Incoming Marmot messages cannot Reserve/Activate a
local receiver. Once physical receiver state is collected, existing Resume file
checks refuse its absence; no remote replay creates a CLI receiver.

## Validation

Journal/IPC tests cover the full matrix, restart then Resume, stale release,
request/result idempotency after reply loss, row/claim/receipt caps, non-shortened
retention, NeverSubmitted rollback and unactivated TTL pruning, row-cap sweep
before Reserve, replay-record cap filled followed by GC/Release/Reserve, old IDs
rejected at/below the local floor, delayed Activate rejected by intent/revision
after pruning, backdated incoming Prune record ordered by minimum timestamp, exhausted retry claim-slot release, matching Open activation, wrong side/accounts/group, claim-versus-
Reserve atomic races, and finite outcome/backoff fairness. Exercise a client-only
daemon with no CommandEngine. Verify forget changes only indexed metadata.

Filesystem tests cover Get anchored from cwd A and collected while the new CLI
is in cwd B; replaced base/parent must retain the row even when the wrong parent
has no namespace. Test active flock, expired closed state, last-stream/manifest
mismatch, CommitPrepared uncertainty, repeated claim, and partial deletion.
Final destination bytes must stay intact. No parent-scanning discovery exists.

Frontend tests hold the worker on a barrier and prove release is absent before
physical completion, including a signal dropping the protocol future. Cover
worker/event send blockage, interrupted reserve/claim replies, crash during Get,
Resume while its old worker lives (Busy) and after death (success), deterministic
Activate refusal Error+EOF versus transport loss, and exact retained-state
warnings. Existing raw/zlib Put/Get, partial Resume, server Put GC and ordinary
Exec/rsync qualification must continue to pass.
