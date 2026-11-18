# Durable Peer heartbeat send fences

Implement the independent send-reservation portion of the
[activation design](peer-heartbeat-activation.md). The store validates
ownership and preserves sequence, policy and clock fences. The
accepted Peer registry, planner adapter and live egress check remain
dependencies for production use. This phase stays private and mounted
only in a standalone integration test. It opens no Peer channel,
dispatches no packet and does not checkpoint client receiver state.

## Transaction and authority boundary

Use private `crates/marmish-native/src/peer_heartbeat_send_fences.rs`
that borrows a caller-owned hardened SQLCipher connection. Its standalone
`crates/marmish-native/tests/peer_heartbeat_send_fences.rs` mounts the
module with `#[path]`. The module owns no path, credentials,
network access, profile lifecycle or sidecar configuration. The fixture
uses the existing `storage_sqlite::open_hardened_sqlcipher` helper and
`SqlCipherHardening::live_cache` used by the cursor proof. The fixture
applies `foreign_keys=ON`, `synchronous=FULL`, `journal_mode=DELETE`
and a zero busy timeout after opening. Each module entry requires
autocommit, a nonempty `cipher_version`, `foreign_keys=1`,
`secure_delete=1`, `synchronous=FULL`, `journal_mode=delete`, memory
temporary storage, `ignore_check_constraints=0`, `query_only=0`,
`writable_schema=0` and no other attached database. The owner sets the
busy timeout and the store does not change it. The fixture's zero value
keeps the lock fault deterministic. SQLCipher pragmas are defense in
depth and do not establish that a caller applied a key. The eventual
accepted Peer owner must provide its own sealed, keyed hardened type
under `ProfileLease`, with a separately approved migration and foreign
key to the accepted binding. This standalone proof references no cursor,
journal, installed binary or native dispatch implementation.

A `ProfileName` newtype validates the existing profile name grammar:
one to 64 ASCII alphanumeric, underscore or hyphen characters,
case-sensitive and with no normalization. In this standalone target use
that exact grammar without mounting identity providers and filesystem
lifecycle. The owner only supplies names it has already bound to this
profile lease. Bind the exact name and local server account together in
a version singleton. The full sending scope contains that namespace, the
canonical server and client accounts, the native group ID, the app room
ID and the nonzero local room generation. Require the local account to
equal the server, different canonical account keys, and a one-to-256-byte
native group ID. The presence of a `SendScope` means server direction.
Only the owner allocates generations and checks the actual live room
bindings. Read stored scopes back through the same validation. A stored
generation with different scope is a binding error.

Each operation obtains authority from one fixed, private parameterized
SELECT inside the SQLite `IMMEDIATE` transaction. Resolve only the exact
bound generation in the main database's standalone authority fixture.
Validate exactly one complete row, including profile name, local server
account, client account, native group ID, app room ID, generation,
revision, state tag and optional digest. Use the same typed decoders as
the fence row. Query parameters are bound values. The API consists of
the typed operations below. The fixture controls its separate authority
table outside the operations. The future production adapter replaces
this private query with a reviewed exact accepted-owner projection and
adds its native scope validation. The committed owner row supplies
revision, role, sharing and effective-policy digest. The owner derives
the permission digest from the validated engine policy using the status
codec. The store consumes its result as a typed `Hex32`. Caller inputs
to the store are expectations and cannot install authority facts.

The wrapper validates the exact scope and expected authority revision
against the readback. It commits before yielding a snapshot or a
typed reservation. Roll back pre-commit errors. Any commit failure
yields `CommitUnknown`, no typed result, even when SQLite reports a
likely rollback. Finish failed operations with explicit rollback and
verify autocommit after every transaction end. Any connection still in
a transaction is poisoned and cannot serve another operation. The
wrapper closes its reservations until that scope has a freshly
committed restore, synchronization or retirement. A
reopened wrapper validates the committed store again on attach before
it can reserve. Reconcile using committed floors and burn any
committed work without reusing an old result. A held write lock yields
`Busy` at transaction acquisition. Production restart starts closed
until qualifying native state, advancing the owner's revision and
resynchronizing durable floors.

## Schema and transitions

Use STRICT version/binding and fence tables. The singleton binds schema
version, profile name, server account and a nullable qualified wall
high-water for the profile. Each fence row stores its own nullable
qualified heartbeat wall floor. Use BLOBs of exactly eight big-endian bytes
with nonzero CHECKs for used counters and wall seconds to preserve the
full `NonZeroU64` range. Null means unused. Store typed fixed-size
identifiers/digest, bounded native group IDs and explicit status tags.
All operations validate decoded state, including that enabled sharing
has an ordinal and digest, an issue time requires an active digest and
an allocated ordinal, disabled rows carry no active digest/issue, and
retired rows carry no publishable state. Compare row server to exact
bound account on read. A partial unique index on `(server, client)`
permits one nonretired row per direction, even while disabled. On
replacement the owner retires the prior generation first.

Attach inside one `IMMEDIATE` transaction without DDL before discovery.
Find all main and temporary schema entries whose names or table names
start with the case-insensitive reserved prefix
`peer_heartbeat_send_fence`. Compare a literal prefix using `substr`
of lower-case text, with no SQL wildcard semantics. Include unexpected
objects and internal indexes on the reserved tables. If none
exist, create the known schema and insert its bound singleton. Otherwise
require the known singleton table, first validate its single namespace
and version row, then check the exact complete object shape. Compare
type, name, table name and stored canonical SQL text against fixed
constants, including STRICT, CHECKs and partial index, and require the
expected implicit indexes if any. Reject any temporary match and extra
main triggers or views in the reserved namespace. Verify complete shape
after fresh installation as well. Qualify every store SQL table with its
main schema to avoid temporary shadowing. Check at most 65 stored rows
on attach, reject a count above
64, and validate every read row under the exact bound account and
profile high-water. Before any operation recheck the full main and
temporary reserved shape, including triggers and views, so post-attach
drift fails closed. Failure never recreates, repairs, resets or migrates an
existing schema. Test fault injection between DDL statements and persist
no half schema. Future migrations get their own review.

Keep up to 64 rows per store, including terminal rows. A new generation
that would exceed the bound refuses with `Capacity`. The fixture
never removes rows, so this is a lifetime generation limit in the
proof. Eventual production removal needs the separate native-retirement
reconciliation and durable terminal registry disposition. On an unseen
terminal generation at capacity, `retire` returns `Capacity`.
The registry's terminal state remains authoritative. Existing rows can
close even when the cap is full. No raw config, permission rule, host
sample, private key, endpoint, native receipt or plaintext packet is
stored. No arbitrary generation order is imposed across unrelated rooms.

The authority state is a closed tag `Disabled | Enabled(Option<Digest>)
| Terminal`. First sync clones only typed owner facts into the row.
Greater current revisions synchronize them and preserve the sequence
floor. Equal revision is idempotent only for exactly identical stored
facts. A lower revision refuses. The rule for active permission state
is as follows.

| Change | Policy ordinal and issue floor |
| --- | --- |
| First disabled or enabled without sharing | Both unused |
| First sharing | Allocate ordinal one, no issue yet |
| Same sharing digest on newer non-policy revision | Keep ordinal and issue time |
| Changed digest | Advance ordinal and clear current issue floor |
| Sharing or status disabled | Keep ordinal floor, clear current digest and issue |
| Sharing enabled again with either digest | Advance retained ordinal and clear issue floor |
| Retired | Keep ordinal and sequence floors, clear active digest and issue |

Counter exhaustion can refuse new reservations or new active permission
generations. It cannot prevent disabling either feature or retiring.
No overflow rolls into a new generation inside the same room.

- `synchronize` accepts a current nonterminal authority revision.
  Return the synchronized read-only floors after commit. A live
  scope becoming terminal must use `retire`. The same-revision
  `restore` confirms an exact existing synchronized scope, authority
  and state without allocating anything. Restoring or synchronizing a
  terminal binding or retired fence always yields `Retired`.
- `reserve_heartbeat` requires matching enabled authority and synced
  revision. Its expected advertised `Option<Generation>` must exactly
  match the durable active ordinal and digest, including `None` when
  sharing is disabled. Require
  `prior binding heartbeat wall floor <= sampled_at <= qualified_now`.
  Require `qualified_now` to meet the profile high-water separately.
  Reserve the next checked sequence, initially one, and store
  `qualified_now` as both the per-binding heartbeat floor and the
  profile high-water, all in the same transaction. Activity in another
  binding cannot reject an otherwise current sample if current wall
  time meets the profile high-water.
  Return the sequence, advertised generation and fixed sample time.
- `reserve_issue` requires enabled sharing, synchronized revision,
  and the exact expected active ordinal and digest. Take the owner's
  qualified nonzero wall seconds as the exact app creation time. This
  value must meet the profile high-water and strictly exceed the
  current ordinal's prior issue time. In one transaction store the
  issue time and profile high-water. A clock regression or future
  sample refuses with no change. The caller handles monotonic time,
  OS-clock qualification, stage deadlines and same-ordinal one-hour
  cadence. This slice has no clock-reset or operator-rebase API.
- `retire` requires committed terminal authority and a strictly newer
  revision for an existing live row. A repeated exact retirement is
  idempotent, and a later terminal revision can refresh the tombstone
  without reopening it. A lower revision refuses. A matching scope
  can never reactivate that row. An unseen scope may create a tombstone
  within the same capacity bound.

The successful reservations are opaque `#[must_use]` structs with
no `Clone` or public constructor. They carry the exact scope,
revision, sequence and optional generation or exact policy generation,
and the associated fixed sampled or issue time. Expose only typed
read access. Each success burns its allocation, including on policy
change, timeout, abandonment or crash. No codec encoding, planner
reservation conversion, publication or egress permit is part of this
module. The later native publication adapter must extend the private
transaction wrapper to commit the scoped Reserved work identity
atomically with its matching sequence or issue reservation. The eventual
owner checks live leaf, epoch, grant, route
and publication gate before reservation, then rechecks before actual
release. A gate close between those operations burns the allocation.
The current planner reads its own time for `start_issue` and cannot
consume the exact durable reservation. A later reviewed adapter must
make it consume and verify the reserved wall and signed times.

## Private interface

Use codec `Hex32` for app room and digest, the codec's `Generation`,
and canonical `nostr::key::PublicKey` accounts. The interface fixes the inputs,
results and errors. Method signatures omit repetitive crate visibility.

```text
Store::attach(connection: &mut Connection, namespace: Namespace) -> Result<Store>
ProfileName::new(name: &str) -> Result<ProfileName>
SendScope::new(namespace, client, native_group, app_room, generation) -> Result<SendScope>
Store::synchronize(&mut self, scope, revision) -> Result<Floors>
Store::restore(&mut self, scope, revision) -> Result<Floors>
Store::reserve_heartbeat(&mut self, scope, revision, expected: Option<Generation>,
                         sampled_at: NonZeroU64, qualified_now: NonZeroU64)
                         -> Result<HeartbeatReservation>
Store::reserve_issue(&mut self, scope, revision, expected: Generation,
                     qualified_now: NonZeroU64) -> Result<IssueReservation>
Store::retire(&mut self, scope, revision) -> Result<()>
```

`Floors` returns synchronized closed state, sequence, ordinal, active
generation, issue time, binding heartbeat floor and profile high-water.
No field contains
a new reservation. `Stale` covers a lower persisted revision, mismatch
against the caller's expected revision, or a reservation before latest
authority has synchronized. `Conflict` covers equal-revision different
facts, a mismatched expected active permission generation and an
active-direction collision. `Binding` covers invalid or different
profile, account, endpoint scope or generation. `Disabled`, `Retired`,
`Exhausted`, `Capacity`, `Clock`, `Unavailable` (no authority or
missing fence) and `Busy` remain distinct. Use distinct `Hardening`,
`Schema`, `Storage` and `CommitUnknown` for a bad connection,
unusable schema/stored invariant, other SQLite faults, and uncertainty
at commit. Return `ReconciliationRequired` while waiting for the same
scope's committed readback and `Poisoned` after a failed transaction
cleanup that leaves the connection non-autocommit. Keep errors free of
sensitive identifiers and SQL values.

## Qualification

Use disposable hardened SQLCipher files, anonymous fixture scopes and
deterministic clocks. Verify exact canonical binding, cross-profile and
wrong-account refusals, fixed authority projection and wrong or absent
authority rows, stale and
same-revision conflicts, directional uniqueness, retirement at and below
capacity, and no overflow preventing closure. Seed near `u64::MAX`
directly in fixture SQL to verify values above `i64::MAX` and exhaustion.
Exercise initial and repeated policy transitions, mismatched optional
generation on heartbeat, wrong policy generation on an issue, and exact
restore. Reopen and assert the next reservation, shared profile wall
across rooms, future/older sample refusal, per-ordinal increasing issue
times and no regression or rollback moving either floor. Verify another
room's newer reservation only fences the current wall and does not
discard an otherwise current sample. Hold a write lock from a second
connection. A fixture-only one-shot `rusqlite` commit hook aborts a real
SQLite commit, with no reservation returned and with confirmation of
autocommit, closed egress authority for the wrapper, unchanged durable
floors and successful exact reconciliation. This target uses the
`hooks` feature explicitly enabled by the locked direct
`storage-sqlite` dependency. No authorizer is needed for the fixed
query. Execute DDL as separate statements, with a test-only failure
point after the first successful creation and real SQLite rollback.
Attach to missing marker plus partial objects, modified CHECKs, extra
schema objects, invalid typed rows and unknown versions. Check
temporary shadow objects and temporary triggers, including drift
after attach. Compare file bytes and absence of sidecars before and
after a refused existing attach. Verify an encrypted file cannot be read
with the wrong key. Check hardened PRAGMA enforcement in the module,
including refusal to use a caller-owned transaction.

Cargo auto-discovers this target for full package/workspace runs. The
ordinary per-push workflow lists explicit core tests. Add one integrator-
coordinated run to `.github/workflows/interactive-core.yml` using the
`peer_heartbeat_send_fences::core_tests::core_` filter on this target.
Keep core cases deterministic, cheap and local. Full qualification
can additionally run the remaining deterministic cases. Narrow unused
annotation to mounted code used by later production wiring by marking
only the integration target's imported status codec where appropriate.
The optional `#[ignore]` benchmark measures
consecutive committed sequence/issue reservations and close/reopen
recovery, reporting only counts and latency percentiles. Run strict
Clippy and formatting for the exact files.
