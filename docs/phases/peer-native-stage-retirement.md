# Retire a Peer stage before export

This phase of [Peer pending activation](peer-pending-activation.md) releases a
stage that has never exported a native KeyPackage. Use the checked
[native stage binding](peer-native-stage-binding.md) as the predecessor.
A locally declined or expired candidate may free its class slot after local
custody has been retired. This increment implements the local effect in the
existing private native fixture. Daemon routing and publishing remain gated by
the broader pending-activation design. This phase leaves the native storage
limits and relay contract to their separately reviewed activation work.

## Authority

- Local decline requires the existing authenticated local UID `Local` and
  profile-owned `RetirementTarget`. An internal expiry operation takes no
  time parameter. It uses the existing owned `provisional_sample` clock, cross-checking
  UTC within 120 seconds before applying cursor and consent
  monotonic-wall checks. Tests
  can inject the fixture's owned clock, never a remote value. The timed
  operation acts only on the exact stored stage it resolves by binding. It
  has no CLI, remote entry point or timer in this fixture. Read the original
  Offer with a retirement-specific loader instead of `live_provisioning_offer`
  and resolve its recorded configured route on each transition. Reauthenticate
  the exact frame with `check_signed_offer` at checked `row.expires - 1`,
  comparing the returned signed author, route, selector, carrier, expiry
  and replay expiry against the row. Expiry compares the independently
  sampled wall with the verified signed expiry. Missing or corrupt rows and
  an origin that no longer has a local configuration remain fenced. Local retirement does not depend on live relay
  I/O, quarantine or current acceptance policy.
- Start, resume and release require exact class, generation, origin,
  selector, stage and no Consent outbox for that selector. A specific
  **retirement epoch 1** fixture schema proves a stage had no KeyPackage
  publication, Consent outbox, Welcome ingress or native join. Atomically
  replace `peer_provisioning_reject_claim` with byte-different DDL that
  preserves its conflict behavior and carries a
  `retirement epoch 1: never exported` marker comment. The old v1 proof
  and production attach reject that trigger through their existing exact
  SQL comparisons. A separate immutable export-incapable epoch marker is
  included in the inventory. A later export-capable variant must be
  distinct and fails this path.
- Open the hardened SQLCipher connection and use a dedicated
  existing-data-only attachment. In one IMMEDIATE transaction verify
  the complete inventory before any table initialization, account and
  configured origins, cursor and Consent versions and two intact slot
  rows. Only a complete v1 stage schema can migrate. A complete retirement
  epoch can reopen. Blank, partial or unknown DBs fail. Never call the
  previous CREATE IF NOT EXISTS initialization path. Construct the
  expected manifest by executing the current creation DDL in a fresh
  in-memory SQLite database and reading its stored SQL text. Require exact
  equality of the whole sorted main SQLite schema (object type, name,
  owner, SQL including NULL auto-index bytes) including names outside
  the Peer namespace. Reject all temp schema objects. Check the epoch
  inventory, marker, binding and lack of outbox within each relevant owner
  transaction before any wall or state mutation and before file effects
  and release. No ordinary init can recreate a deleted outbox/fence.
- All new DDL, schema checking, constructors, retirement code and hooks
  in existing `admit`, `live_reservation`, `acquire` and stage methods are
  under `#[cfg(test)]` and take effect only for an attached retirement
  epoch. The earlier v1 fixture and production keep their current behavior
  and queries. Reuse the existing test-only provisional fixture gate with
  an exact retirement-epoch selector. Once an intent commits, both
  `bind_stage` and `stage_transaction` reject that lane with a distinct
  Retiring error before placeholder allocation, native file opening or
  signer dispatch. Keep the SQL progression trigger as a second fence.
  Legacy approve/discard remains denied for a bound stage. Retirement
  opens no native DB, invokes no native leave and has no relay effect.

## Commit sequence

1. Capture filesystem witnesses side-effect free while holding the checked
   profile lease. Open a checked private profile/peer directory handle,
   verify owner/mode and its device against the original checked cursor,
   and retain filesystem (device, inode) for the profile and peer directories. Inspect only
   the stage's fixed class entry and `-journal`, `-wal`, and `-shm` using the
   held descriptor. Every sidecar must be absent. For Bound, the main entry
   may be absent or a checked zero-length placeholder with recorded device
   and inode. For NativeStarted and Prepared it must be the exact recorded
   regular, private, owner, single-link and same-device file. A missing
   NativeStarted/Prepared entry at first intent is ambiguous. Persist the directory and optional leaf witnesses and prior state
   with the exact binding in an IMMEDIATE transaction. This committed
   intent is the tombstone, including its original action, observed wall
   and cooldown values. An explicit decline writes its inviter cooldown
   in that same transaction, before unlink. Expiry alone preserves the
   existing ten-minute slot charge and requires no extra decline cooldown.
   A new explicit decline may be recorded after signed expiry. Requests
   arriving after a durable expiry or decline resume that same action
   and do not upgrade expiry to decline. The stage-less legacy
   retirement path is outside this increment and adds no cooldown.
   Verify the filesystem witness agrees with the immutable stage row.
2. `Retiring` is the committed fence. Use the checked `ProfileLease`
   throughout this turn, including the file operation. Reopen or retain the
   private directory descriptor, require identity to equal the journal
   witness and verify the current profile path still resolves to that
   descriptor. Use descriptor-relative no-follow checks and unlink of one
   fixed basename, then recheck the current directory and bounded entries and
   synchronize the held directory descriptor. A matching present stage with
   unchanged witness may be unlinked; absence after a recorded present leaf
   is acceptable only after the intent commits, since a prior attempt may
   have already unlinked it. An initially absent Bound leaf must stay absent.
   No fallback path-based unlink. A swapped directory or leaf, added
   sidecar, non-regular entry, reappearing leaf after a later durable mark, or
   failed synchronization retains the binding. If the original filesystem
   observation is ambiguous, commit a blocked intent after the same
   authenticated database checks. It records the decision and fences all
   preparation but cannot authorize unlink, FilesRemoved or release. An
   absent initial witness or a hot SQLite journal needs separately
   reviewed repair before the class slot can become available. The private lease serializes
   all legitimate profile writers. A local principal with the account UID
   already has authority over profile bytes and is outside that trust
   boundary. Nevertheless check the directory and entry immediately before
   unlink and verify the directory identity and absence afterwards. No
   SQLite transaction stays open during unlink or sync.
3. Commit `FilesRemoved` under the same exact validation. Recovery at
   `Retiring` repeats the file operation and directory sync, including after
   an interruption before sync. Recovery at `FilesRemoved` still requires
   verified durable-directory identity, all fixed entries absent, and an
   exact unchanged binding, but neither removes a reappearing path nor
   allows a subsequent release from an ambiguous tree.
4. In one IMMEDIATE transaction, recheck full schema, no outbox, exact
   binding and absence of all fixed entries in the original dirfd. No
   unlink or sync is performed inside that transaction. Preserve the
   existing signed replay fence and decline cooldown. Clear only the
   exact active class slot, preserving its generation and ten-minute
   charge. Delete the exact Invite and Stage, and transform the active
   tombstone to a compact immutable Released receipt with the exact prior
   binding, action and a 24-hour retry horizon from release. No filesystem
   effect follows slot release. Recover an exact matching receipt as
   `AlreadyReleased`; a fresh local handle may resolve that same full
   selector and prior generation through a read-only receipt lookup under
   the same profile and route. A receipt cannot match a different attempt
   or grant native or remote authority. Once the receipt has expired a
   retry returns stale/missing. A stale completion cannot affect a new
   generation. An interrupted transaction leaves recoverable FilesRemoved.

Pin both parent rows and the active stage until the last step. Key the
retirement table by (class, generation) and add a manifest-pinned partial
unique index on class across all active states, including blocked.
The new retirement and cooldown tables default to BEFORE INSERT/UPDATE/DELETE guards
that reject general direct SQL, including both forms of REPLACE with foreign
keys and recursive triggers disabled. A checked owner-only IMMEDIATE
transaction verifies pinned DDL and each expected row transition, removes
only the exact relevant guards transactionally, and restores identical DDL
before commit. The final transition additionally removes the original stage
DELETE guard within that same transaction. Keep exact existing Invite/Slot
guards intact; removing the authorized stage first makes its specific parent
rows releasable. All guard drops, data writes and restores are atomic.
A second connection cannot write during this transaction or observe the
temporary DDL state. Use one-row checks for each exact transition and fail
on an unexpected table or trigger difference. Install a separate immutable
BEFORE UPDATE stage progression trigger over the exact active Retiring or
FilesRemoved binding. It aborts even while the original v1 transition
would accept the write. Keep both that trigger and all existing claim
trigger guards installed during file activity. Existing Stage uniqueness
guards and the new receipt guards reject both forms of REPLACE.

## Replay and cooldown

Reuse the cursor's existing `peer_consent_fence (origin, revision,
replay_until)`, which outlives the Invite; do not introduce a second
replay table or prune it in retirement. While the signed replay deadline
remains active check that the exact authenticated fence with the same deadline
still exists before release. Preserve unrelated fences and existing legacy
encrypted Consent replay behavior.

Persist 24-hour decline cooldown keyed by canonical inviter across origins
from the checked start-of-intent time. Check the SQLite-range deadline
before committing the intent. A resumed release never changes that
deadline. Expiry uses no separate inviter cooldown, and its existing
ten-minute class charge remains anchored at original reservation. The
active retirement stays pinned until release in both cases. Suppress new signed Offer admission,
classification/reuse and final reservation under active cooldown, after
verified inviter authentication. Apply the same decision under the immediate
transaction used to charge a slot so no later projector creates the effect
from stale policy. Prune only expired cooldowns with persisted checked wall.
Bound each collection of live cooldown and retained Released receipts
to 512 rows. Each 24-hour window covers at most 290 rate-charged starts across two classes with a
ten-minute charge including the endpoints, plus at most two older stages
already occupying slots at its start. Bound active
rows separately to two. Prune Released receipts only upon their checked
expiry and never prune an active tombstone. Equality follows the existing
cursor convention. Capacity or arithmetic failure keeps custody fenced. A
later turn can retry after expiry pruning without changing the intent.

## Acceptance

Use the real SQLCipher cursor, disposable signed Offers and native stage
storage with no network. Exercise both classes and distinct Bind,
NativeStarted and Prepared retirement; signed expiry/equality, explicit
decline, wrong UID or owner, policy Off cleanup, missing source, corrupt
Offer, signed replay and same-inviter cooldown of fresh IDs across origins.
Test selective decline cooldown without blocking another inviter or lending
it the other class, preserving exact rate limits including equality and SQL
maximum. Recover across every durable boundary, including interrupted
unlink and directory sync, rollback after each SQL writer effect, and
original exact stage reappearing before versus after FilesRemoved. Reject
symlinks, hardlinks, wrong modes, replacement leaf, switched directory
including the case where the original leaf was removed, foreign device and
all sidecar variants. Exercise unexpected table, mutated Consent DDL, generated index, out-of-prefix
trigger and the export-capable marker as denied inputs. Check exact receipt
retry across process restart, expired receipt and stale generation.
Test SQL write guards with FK and recursion disabled
and verify with a second writer that the transaction cannot expose a guard
gap. Verify absence of any signer, transport, room and command effect and
byte-exact unchanged legacy Consent replay. Assert old v1 and ordinary constructor Binding rejection of this
fixture on reopen. Run the ordinary non-test build with production
storage, and scan exact production rmeta and rlib for absence of the new
fixture constructor and retirement/cooldown symbols or table names,
alongside a positive cursor symbol. Test the Rust lane error before
allocation/opening/signing with an active retirement. Document the
hot-journal and stage-less path limits in the agent specification.
Check replay/cooldown capacity. Record separate timings for local decline and restart recovery;
keep new tests outside the narrow core until the daemon exposes a real
activation entry point. Obtain Claude Code Auto review on the exact
completed code and fixture.
