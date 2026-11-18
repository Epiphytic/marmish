# Durable Peer provisioning reservations

This local SQLCipher increment implements the offered to provisioning reservation
in [Peer pending activation](peer-pending-activation.md). The reviewed
Offer/Consent checkpoint is the source of authenticated identity, exact offer
ID and signed revision, qualified relay origin, signed expiry and replay fence.
This increment implements slot acquisition, exact recovery and pre-native
retirement. It introduces no native effects, physical stage, CLI, daemon,
acceptance, permission, group membership or launch behavior. Preserve the
existing independently qualified encrypted Consent replay and publication logic
for selectors without a provisioning owner.

## Authority and transactions

Add a private provisioning module under `peer_consent_checkpoint`. Expose
`reserve_provisioning(source: &Source, key: Selector, wall)`,
`recover_provisioning(source: &Source, key: Selector, wall)` and two cleanup
operations to Marmish's internal serialized owner. The cleanup operations are
`retirement_target(local: &Local, key: Selector) -> RetirementTarget` and
`retire_provisioning(local: &Local, target: RetirementTarget, wall) -> ()`.
Both return the existing checkpoint Result. No provisioning Reservation can
authorize retirement. Cleanup reconstructs a separate, opaque, exact capability
from the authenticated local owner and the current SQL row, regardless of the
Offer expiry or the current mode, block or socket state. There is no external IPC. Reuse
`Source`, which carries the current Checkpoint allocation owner and exact
explicit qualified WSS origin, and `Local`, which is already bound to the
verified profile owner UID. Recovery re-mints an opaque reservation after reopen
from the exact same selector, source and current policy. Both opaque capabilities
carry the owning checkpoint Arc. Retirement validates the supplied Local and
target owner independently. A different instance or stale target cannot retire
a later generation. A reservation stores
the exact origin, Nostr server key, Offer ID and revision, class and generation.
A generation is a persistent checked positive counter in its fixed class within
the profile database. Complete stage identity will include profile, class and
generation. SQL updates compare the full selector and its class and generation.

Access uses `CursorStore::with_wall` with one immediate SQL transaction and
the existing profile-global consent wall fence. Check `Policy::permits`
before issuance or recovery. The Checkpoint is a client-side interface with a
signed Offer recipient that must match its signer and differ from the inviter.
The later production activation must only open the owner for a configured
client role. Policy is locally constructed, with `Mode::Off` and explicit
blocks failing closed. Manual and Automatic
authorize the same bounded reservation. `Policy::reserved` is the locally
supplied set of canonical pins and eventually prior local application acceptance.
This increment's test policy supplies pins only. Neither a username, email,
Offer claim, native membership nor the invitation's historical priority column
classifies the server. Reclassification applies to new reservations only. An
idempotent request for the already-held exact, live selector preserves its
class and generation, even after local pins change. Exact recovery requires
current policy and a live unexpired Offer. Off and block cannot create or
recover work, but verified owner cleanup is still possible. Check current
configuration origin through `Invitation::load(tx, key, source.origin)`.
For live issuance and recovery, share the signed verification already used by
`queue()`. Call `open_offer` with the local signer, stored frozen frame, expected
inviter and exact route URLs. Recompare the full selector, signed carrier,
expiry and replay limit, and require strict wall less than signed expiry.
Read the exact cursor origin's quarantine marker inline during issuance
and recovery. Do not open a nested cursor transaction. Retirement skips the
quarantine gate and requires a monotonic wall. Reject a
legacy outbox before acquiring a new slot. Existing authenticated admission and
replay rules stay unchanged.

Reservation does not consult or write legacy `decision_at` as an approval.
An Auto-admitted row or previously manually marked row without a legacy outbox
can reserve. The reserving transaction clears its legacy decision atomically.
Future pending acceptance must use distinct generation-bound durable state.
A successful exact recovery or idempotent reserve commits current origin and
profile-wall progress without changing class, charge or generation. First load the exact row under the transaction. If wall has reached its
persisted expiry from verified admission, commit the origin and consent wall
advance and return Expired before attempting a signed reopen or checking
quarantine. The signed decoder can itself reject old Offers, so checking that
decoder first would roll back the necessary expiry fence. Within the valid
stored window, verify the exact frozen signature and read quarantine. You
cannot recover an observed expired Offer by rolling back the clock. Current
Off and block remain early policy refusals without a wall update. As in the
cursor, the caller qualifies the wall and forward jumps can block work until
it catches up.

## Fixed storage and rate

Add a STRICT table in the existing profile-bound SQLCipher database and seed
exactly two class rows when that table is first created in the existing checkpoint
attach transaction, including when an existing version-2 consent owner first
gains this table. Determine table presence using the SQL schema first. If it
exists, validate the expected table schema and both class rows. Fail if the
consent metadata or either required class row is missing or malformed. Never
reseed an existing table. The primary key only admits known and unknown
values. Each row has a persistent nonnegative generation counter, a
nonnegative next-start wall and either no owner or the complete exact Offer
selector and origin. An explicit CHECK requires the four owner columns all
NULL or all non-NULL with exact lengths and a positive generation. A unique
server covers both class rows. A restrictive composite foreign key points to
the exact Offer. This avoids SQLite's partial-NULL composite key exemption.
The profile-wide 8+8 Offer counts remain intact.

Acquire in one transaction by checking wall, exact origin/Offer, mode and
current block, pending slot occupancy, absence of any legacy outbox and
the class next-start wall. On success increment the selected class's
generation, fill active ownership, set next start to the checked wall plus
600 seconds, and advance the consent wall atomically. Use checked Rust integers when
incrementing the generation and adding the 600-second window. Reject both
bounds independently without borrowing. Retried exact issuance and recovery
do not increment or recharge. Do not release occupancy at Offer expiry.
Generic sweeps may prune normally expired signed replay fences, and the
invitation deletion statement must explicitly skip owners with NOT EXISTS.
An expired owner must not block generic sweeps of other rows or be replaced. This keeps
the exact hold visible across process death and prevents a generic sweep from
freeing a future stage's occupancy. Expiry denies work. The UID-bound local
owner can construct a cleanup-only target for its exact expired or blocked
claim without a provisioning capability. Retirement releases only by full
selector, origin, class and generation comparison inside the transaction.
It remains available while local provisioning policy is disabled.

Retirement here means only local pre-native abandonment of a storage-only claim
by the verified local owner. Forget selector fields without erasing its
generation counter or rate charge, then remove its exact Offer in the same
transaction. Existing signed replay fences continue through their original window
and their normal expiry. Keep the row and charge on unexpected errors. Before introducing
native stage creation, the later phase must prevent this pre-native abandonment
operation from freeing the reservation under a live native stage through durable
stage-state gating. It must own stage construction, bounded terminal release,
expiry/decline cooldown and permanent application fencing. Deployment remains
disabled until that transition exists and the legacy unreserved stage path is
removed or gated. This increment's methods have
no native route, callsite or side effect. It makes no claim about retirement of
a live native stage or application denial views beyond this storage seam.

Exclude a currently held selector from legacy approvals, approved work,
discard, and legacy consent summaries. Reject legacy acquisition in the presence
of an outbox even if its frame has expired; ordinary checkpoint state can still
clean or retry that row. Legacy reserve/ACK requires an existing outbox. In the
single checkpoint cohort, the reciprocal acquisition and queue exclusions make
a legacy outbox and a provisioning claim mutually exclusive. Check acquisition and legacy state-changing exclusions in the same SQL
transaction as each operation. Add two SQL triggers to reject both states
even when a database write bypasses those operations. `peer_provisioning_reject_outbox`
runs BEFORE INSERT on the outbox, and `peer_provisioning_reject_claim` before
an update filling slot ownership. Claim slots are fixed. Retirement independently
refuses before deleting an Offer if an outbox is present. No change to existing
unowned ciphertext bytes or replay handling. Retain existing old
fixture behavior for unowned rows. The application has no deployed downgrade
or mixed-binary contract for this internal checkpoint. Existing scoped
dead-code annotations on the private non-deployed integration and standalone
test mounts cover local methods while there is no activation callsite.

## Validation

Use real signed exact-origin Offers and a disposable key/profile over SQLCipher.
Test Manual, Auto, Off and current blocks; two origins; hard separation of
classes; reclassification through reopen while active; no legacy bypass,
approval result or package generation; auto-admitted and manually marked legacy
rows clear approval on acquisition; legacy outbox blocks reservation even after
the ciphertext expiry until normal cleanup; durable reopen and charge at 599 and 600 seconds;
exact idempotence; invalid handle / old checkpoint, changed selector, origin,
and generation; existing SQL constraints reject partially NULL ownership;
missing class rows fail attach without reseeding; direct SQL verifies both
directions of slot/outbox exclusivity; recovery after expiry and signed skew
commits a monotonic refusal and a wall rollback returns ClockRegressed;
quarantined origin cannot provision; profile-global regressed wall and invalid
arithmetic; failed or forced SQL transaction leaves
no release, charge, or consent mutation.
Verify sweep past expiry retains ownership until explicit cleanup, legacy
cleanup cannot delete it, and a delayed replay during the signed replay
window cannot regain a retired claim. Simulate a stale generation against an
active claim and also acquire a new signed revision after retiring the previous
Offer with the same inviter and Offer ID. Exercise existing Consent ciphertext
recovery tests as regressions. Run scoped tests offline with locked
dependencies. Benchmark initial reservation and exact recovery over a bounded
set of signed Offers and private profiles. Label results local SQLCipher
latency. Transport and pending/approval latency use their future end-to-end
qualifications.
