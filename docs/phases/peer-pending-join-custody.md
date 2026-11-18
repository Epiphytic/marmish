# Peer Delivery custody and pending native join

This test-only milestone extends the private WSS M1 Delivery receiver and
Prepared Consent owner integrated at `4bb566ecd19c331f5102d7d150fe7d5ddd4fde32`.
[M1 validation](../validation/peer-private-wss-enrollment-m1.md) records the
exact Linux AMD64 rerun and [Claude approval](../reviews/peer-private-wss-enrollment-m1.md)
of the identical Rust source at `5ce1727051a64b4708e4f783e568f3fb724e2984`.
The standalone public runtime preflight has separate Claude design approval
`e7007a23-787d-42b6-b437-73a933ed9476`. Run it and record its reviewed result
before implementing the joined custody owner below. The exact standalone
source `69a1edca0499b6f14ccffd53fe320d7d30bca756` passed all five public
runtime tests and standalone strict Clippy on Linux AMD64. Claude Auto
session `717e3379-ae1b-4e9e-b0b2-98c11e88ba92` approved it and independently
reran the standalone proof, which passed in 25.55 seconds.
The native resolution is upstream master
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`. Keep the mainline branch
manifest. Verify all MDK git entries in Cargo.lock resolve to that commit
before the first build and use `--locked` for every run. This increment
ends with the durable Pending or Activating decision. Later work owns join
egress, accepted receipt, rotation, status and CLI/daemon activation.
The native stage remains the single durable copy.

## Schema and authority

Upgrade the existing exclusive InvitationState and hardened SQLCipher
Checkpoint in place. The current Prepared Consent schema opens from
`retirement::schema::expected(false)`, which is the pre-retirement
provisional schema. The retirement fixture and Prepared Consent epoch 1 are
separate branches from that inventory. Do not migrate the retirement
fixture into Consent or custody. The only new accepted upgrade is Prepared
Consent epoch 1 to custody epoch 2. Keep the immutable epoch 1 singleton
and add an immutable epoch 2 singleton. Run the hardening, authority,
exact inventory and row-graph checks before and after upgrade in the same
immediate transaction. Compose custody with the existing M1 owner in
the same connection. If the component needs an epoch-aware inventory
check, make the expected inventory a privately sealed owner value and
retain all epoch 1 row/guard checks in the epoch 2 owner. Do not make epoch
1's old constructor accept epoch 2. Prove both older owner constructors
reject epoch 2 and that the retirement fixture rejects Consent epoch 1,
before either alters schema or rows.

Add one strict custody table. Use positive generation with the existing
class values and unique exact Consent carrier as a foreign key to the
Prepared Consent outbox. Verify account, server, Offer coordinates, origin,
class, generation, package event/digest/reference and exact positive
settlement against that row both at attach and when changing a custody
record. Never change provisioning stage, invitation, slot, outbox or
legacy `decision_at` through custody. Exact SQL trigger inventory and
write denial gates cover all custody mutations. Use the custody owner's
transactional writer apertures for insert and exact next-state CAS only.
Never open delete. Retain the charged native stage and replay fences until
the disposable test profile is destroyed. Production expiry and cleanup
require their own current-phase implementation evidence.

Use one immutable column set for the class, generation, selector identity,
Consent carrier, Delivery carrier, signed creation time, expiry, raw exact
signed Delivery bytes (1 to 65,536 bytes), exact replay horizon, Welcome
id/digest, verified package binding (event/ref/slot), package digest, signed
package validity bounds (not-before and not-after), admission mode at Held and Held
deadline. Carrier uniqueness and the horizon serve as the replay fence.
Keep storage to a single copy of the outer signed carrier. Derive the
nested Welcome from fresh authentication of those bytes. In-memory receipt
capabilities never serialize. Derived immutable columns serve as lookup
and equality checks and do not substitute for revalidating the carrier.
At Held insertion derive the immutable package binding and both bounds
from the same freshly verified package witness used for the original
Delivery, and compare its event/ref/slot and digest to the exact stage
and outbox. On later reads, require the exact outbox foreign key and
check those same bindings before exposing any readback evidence.

| State | Required added committed fields | Allowed transition |
| --- | --- | --- |
| Held | Immutable columns and held timestamp | JoinStarted |
| JoinStarted | Started timestamp | Pending |
| Pending | Verified native group ID, epoch pin, own leaf, qualified timestamp and pending deadline | Activating |
| Activating | Local approval kind and activating timestamp | None |

Use column widths and nullable-field checks for each exact state, immutable
field checks and positive monotonic timestamps, and prohibit skips,
rewinds, deletion and resetting the class. At each transition lock the
exact row and compare prior state, replay identity, native binding where
available, clock and current profile policy. Unavailable is a read result
for invalid or unknown native evidence. It has no durable state value.
Expired rows stay inert and charged, including JoinStarted when its
closed image cannot qualify before the Held deadline. Use `provisional_sample` for the
qualified application wall clock. Compute the Held and Pending deadlines
from the lesser of the signed Delivery and bound package lifetime with
checked margin subtracted. The Pending deadline can only narrow the Held
deadline and cannot exceed the Delivery expiry. Retain the original bounds
after restart. Use a fixed five-minute margin equal to the existing
Peer admission clock-skew bound. Set `PENDING_WINDOW_SECONDS = 86_400`
to match the lifetime of a Peer control flight. Use the earlier of the
Held deadline and qualified Pending time plus that window. Use checked
addition and subtraction. Require the applicable deadline strictly in
the future for adjudication. MDK records
its own sampled join time and delegates tree leaf validity to OpenMLS
`ProcessedWelcome::into_staged_welcome` with the default current-time
`LeafNodeLifetimePolicy::Verify`. OpenMLS `6100df3` uses system time
for that policy. Start disposable packages relative to the real clock and
cover synthetic application clock changes separately.

## Ingress order

Place the custody owner within `crate::peer_invitation`. Held,
JoinStarted and Pending have a separate transaction path from
`live_provisioning_offer` and `stage_transaction`. Those paths require
the original Offer to be unexpired and read the legacy outbox. M2 can
process a still-live exact Delivery after Offer expiry. It uses the
new exact Consent settlement and the Delivery/package deadline instead.

At ingress require the current socket and check the exact trusted origin,
quarantine and qualified clock. Verify the signed outer carrier and bind
its ID and signed creation time to the indexed ID and stamp before any
valid-carrier disposition. Compare the complete original event JSON bytes
(excluding the relay message envelope), not a normalized serialization.

1. Before opening the stage, look up a custody row by the exact origin,
   authenticated carrier ID, stamp and original bytes. In the retained
   replay horizon an exact match returns its already-applied disposition
   even after the original Delivery expires. This is an index disposition
   only, without native access. Any local resume is a separate owner turn
   under current policy, clock and saved provenance. Keep or release
   deferred work only with the corresponding exact raw record. A signed
   event whose ID matches a retained carrier but has different stamp or
   raw bytes is an invalid disposition, never a replacement. Replays of
   retained carriers at or after their exclusive horizon are invalid.
   A signed event for the same exact consumed Consent with different carrier ID
   must pass seal and exact signed Consent coordinate checks, then gets
   a stable invalid disposition without opening the stage. Gate the
   Prepared Consent queue, reserve, publish and native preparation entry
   points on custody. A matching held owner yields typed no-work there,
   including after restart. No path may attest a consumed package as
   though the stage were still prepared.
2. For a first carrier, authenticate the inner seal/flight, exact Offer,
   exact recoverable outbox, and freshly attested package as for
   `Transcript::verify_delivery`. Check positive ACK **after** successful
   flight authentication. Only that authenticated flight may defer for an
   unsettled ACK, with its exact signed Consent carrier as prerequisite.
   Extend the commit-trusted owner callback with the typed Held outcome
   and commit the first row with exact generation, outbox positive ACK,
   immutable package witness and replay identity in the same transaction.
   Atomically clear any matching deferred copy. Never transfer authority
   from an in-memory M1 history value without this transaction.

On restart verify the saved signed outer frame, signed seal, both
principals, room, signed flight times/type, Welcome and Consent
coordinates and the restored exact signed Offer and outbox transcript.
The stage after joining may have consumed its KeyPackage bytes. Use a
private, non-serializable verification capability that only the
checkpoint's hardened exact Held row reader can mint after verifying the
saved stage, Consent and package bindings. Require the immutable verified
package binding and both bounds from that row. A narrow post-Held codec
entry point checks the signed outbox frame and reuses the Consent and
Delivery lifetime checks using those saved bounds, then revalidates the
full Delivery and the original Held deadline. Pass no caller-supplied
`VerifiedPackage` to it. Keep the first-carrier verification and its
live native package check intact. Recovery compares the exact original
origin against the trusted cursor, Offer and outbox rows. It never
authorizes another origin, advances a history cursor, acknowledges an
unsettled local attempt, or qualifies fresh ingress.

### Closed cursor disposition table

Apply this table in order after transport authority, clock and quarantine
checks. Use typed source outcomes for local dependencies instead of
treating a generic verification error as permanent or transient.

| Condition | Owner action |
| --- | --- |
| Stale socket, quarantined origin, invalid index page, uncertain clock/write/read integrity or temporary stage inspection error | Return an origin error with the page cursor unchanged. Keep any existing deferred row. |
| Well-shaped index tuple with bad outer signature or malformed carrier | Commit invalid for the raw tuple. |
| Valid outer event whose signed ID/stamp disagrees with the index | Reject the page as an origin contract error. |
| Exact retained custody duplicate | Commit trusted AlreadyApplied without reopening the stage or extending authority. |
| Authenticated stable mismatch, conflicting carrier at owned Consent, intrinsically invalid flight, known terminal/blocked/Off/expired authority, or no eligible exact outbox | Commit invalid. A correctly bound flight cannot name an outbox whose signed Consent does not yet exist. |
| Flight verified with an eligible outbox and exact unsettled ACK | Defer with `Auth::FlightVerified`, checked exact expiry, and that Consent carrier as the prerequisite. A capacity/Backlog error stops page advancement without dropping records. |
| Flight verified with current exact positive ACK and fresh package attestation | Commit Held and the replay fence, clearing only the matching deferred record. |

For a mixed page require every raw tuple to have its own commit receipt
before advancing the history cursor. `commit_page` remains separate
from those transactions. An already carried-through disposition remains
safe if a later tuple fails and the page is replayed. A deferred retry
uses the owner's serialized `due()` turn and the existing capped 60-second
retry schedule. ACK wake is an optimization. Each retry revalidates the
exact retained tuple and its prerequisite, including current policy and
expiry. Retry does not reuse an earlier socket as transport authority.
Native custody progress below is a separate serialized owner turn.

### Native owner turn

3. Validate the original closed stage, exact class/generation, signed
   package, event/digest/ref and file identity. Use the approved encrypted
   snapshot inspection of the original. Carry the short-lived pre-ingest
   image digest in memory and rehash the original immediately before the
   JoinStarted commit. Neither an empty stage nor a stale operation can
   progress.
4. Commit JoinStarted before the first mutable original file open. Use a
   non-Clone stage operation bound to the same owner like `NativeStart`.
   Hold the existing profile and path guards across the stage operation.
   File identity and digest checks detect accidental substitution.
   Same-UID tampering with a path or process is outside the fixture's
   authority model. Use the original device/inode and profile binding
   after the native operation which can change the digest.
5. Open and call `AccountDeviceRuntime::ingest_delivery` once. Explicitly
   select `SqliteStorageOptions { journal_mode: Delete, .. }`; upstream
   default is WAL (`storage-sqlite/src/base/connection.rs`). Runtime open
   can hydrate and mutate, so it runs inside the already-started turn.
   Construct the `TransportDelivery` exclusively from the authenticated
   embedded Welcome, authenticated singleton source and a fixed fixture
   subscription id. Never turn an independently relay-observed kind-1059
   event into this native input.
6. Drop all original native handles. Check device/inode, protected file
   identity, clean sidecars, and the existing 16 MiB file ceiling. Break
   the one-turn operation on any error. For readback or cold recovery,
   open only an encrypted disposable snapshot of that exact closed
   image. A hot journal, oversized image or unexpected file is typed
   unavailable. Reuse `ProvisionalFile` and the `SnapshotArea` copy, inode,
   digest and cleanup guards with a distinct joined-image verifier. The
   prepared `read_provisional` check rejects a consumed package and is not
   a joined-image verifier. A recovery turn cannot roll back a hot journal on
   the original, send, create a package or call ingest again.
7. Require the exact durable and still-current readback before CAS to
   Pending. A JoinStarted row without complete proof remains charged.
   Start from the public API in `tests/peer_public_runtime.rs`'s
   `ExpectedJoin::verify`. The staged MDK proof's `runtime.session()`
   depends on MDK test support and is outside this boundary.
8. A local manual or eligible automatic decision can CAS Pending to
   Activating after rechecking independent native proof, role, policy,
   clock and block state. Persist only in custody and emit no app ACK.

## Native proof and local approval

Require the public runtime-level ingress API. Use a refusing adapter with
per-category counters exposing no connector or I/O handle. Keep the native
publisher disabled. Capture the result so a native error after durable
join leaves JoinStarted until fenced readback. A successful call requires
one matching GroupJoined effect, the expected ingest outcome and no
publication fanout, reports, failures or pending publication work. In
`marmot-session/src/runtime.rs` the public call drains the session publish
queue into AccountDeviceEffects and has no `publish` field. Check all `AccountDeviceEffects` fields other than durations. Require
`IngestOutcome::Processed`, no unpersisted object, exactly the
expected single GroupJoined, no other events, no publication,
application, pending or convergence effects, and the default maintenance
disposition. All refusing adapter and KeyPackagePublisher counters must
remain zero. Count all transport and package methods, including transport
readback and commit and size operations, and the package adapter's
`legacy_slot_id` and `prepare_key_package`. Drop application effects
locally. Test the public runtime API with an
adapter that returns errors and a fault cut immediately after an actual
committed join returns. Runtime-open or ingest drains must preserve the
required stored pending application receipt. MDK writes GroupJoined in
the native join transaction. Engine hydration lists those rows. Its
pending-output upgrade only rewrites a subset of MessageReceived.
The SQL read enumerates stored rows without a deadline, and the explicit
acknowledgment deletes by message ID. Prove repeatable readback on a closed
image. This scope never acknowledges that receipt or invokes history prune.

Read back an exact singleton pending application receipt of type
GroupJoined. Require its `via_welcome` to equal the exact staged Welcome's
native message ID. Obtain the native group ID from that stored receipt, distinct
from the signed app room ID. Require the only live group to have that
native ID. Require the MLS-authenticated `welcomer`, `via_welcome`, and
`explicitly_confirmed == false`. Validate `Current` protocol profile,
non-removed exact two principals, sole founder administrator, exact
purpose name and description, component, route bound to the signed room,
public own leaf and typed epoch pin.
Match the consumed native package reference and current stored lifecycle.
Recheck exact signed Welcome evidence separately. Preserve this scoped
receipt and suppress it from application projection. Absent, duplicated
or changed authority leaves the row unavailable.

Persist the locally selected mode at Held. Auto requires that value and
the freshly rechecked local mode to be Automatic, a current Pending
deadline, and exact same authority and generation. After a restart recheck
those same durable facts. A Manual Held row stays Pending on a subsequent
automatic configuration. Manual requests use a genuine same-UID local
socket, the exact selector and generation, and current authority. Share
one CAS for these decisions and never call the legacy `approve()` method.
No remote action can mark Activating.

## Validation

The standalone SQLCipher fixture exercises epoch attachment, exact graph
and writer guards, legacy sweep, and storage replay. The integrated owner
must execute the actual signed Held transaction, including deferred
cleanup and same-turn matching, and recover and qualify the closed native
stage through Pending and Activating. Exercise the exact native proof
binding, deadline boundaries, wrong owner, and manual and automatic policy
through the integrated owner before closing this milestone. Direct SQL
fixtures alone cannot establish those authority boundaries.

Start with one no-network case using the package and founder helpers
from `tests/peer_public_runtime.rs`. Close the prepared stage before
founding the room. Use explicit Delete mode from preparation through
both snapshot reads. Reset the refusing adapter and package counters at
the end of package setup. Require `migration_summary().0 == 0` during join
and both readbacks. Check the original inode and digest, and prove it and
the independent clean copies have no journal, WAL or SHM after all handles
drop. Check the original digest is unchanged by each snapshot readback.
This preflight's copy uses local fixture guards and its authenticated
Welcome carrier is fixture-local. It cannot qualify kind-7435 or the
custody owner's production-shaped file guards. Then run the integrated
M1 private WSS path into the real runtime for a disposable Manual and
Auto profile. Verify both pre- and post-restart
Pending. Test manual request with the real local socket, current and
wrong generations, manual-to-auto policy change, and one eligible auto
decision. Exercise stopped, blocked, wrong socket, signer, revision,
package, route, welcomer and wrong stage conditions, raw 1059 rejection,
hot journal preservation, sidecars, corrupted row, wrong key and older
owner refusal. In every custody state, probe delete and update against the
stage, slot, invitation, and Consent outbox. Both older constructors must
refuse the epoch 2 profile. Check expiry, duplicate exact and conflicting carrier,
other-class isolation and immutable schema. Test exact duplicates in Held,
JoinStarted, Pending and Activating, including cold rescan, after carrier
expiry within the replay horizon, and with a consumed package. Test an
authenticated different carrier for the same Consent without reopening the
stage. Cover both orderings of ACK versus Delivery, ACK-independent due
retry, invalid neighbors, full backlog and mixed page replay. Verify
post-Held reserve, queue, publish and preparation calls never execute
native attestation. Confirm recipient does not
rejoin on recovery. Inject fault cuts around Held, JoinStarted, native
ingest return, Pending and Activating. Label these owner call boundaries
rather than process or power-failure simulations. Check a started
stage with no durable join stays fenced.

Assert zero native physical traffic, no command/terminal grants,
acceptance send or application projection. Distinguish M1 wire counts,
M2 stage and native-processing measurements. The exact opt-in fixture,
standalone cursor tests, strict all-target Clippy and scoped rustfmt must
pass. This fixture is outside the core CI subset. Qualify shutdown and
post-join sidecar cleanup with the explicit Delete journal mode. Record the
platform and exact MDK/owner source in evidence.
