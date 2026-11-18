# Staged Peer invitation inspection

This is an alternative to the first-Welcome admission dependency in the
[Peer invitation design](peer-invitations.md). It keeps the pinned upstream MDK,
the canonical Nostr identities, and the existing Peer authority and CLI rules.
It proposes a new handshake and a separate encrypted MDK store for each
accepted Peer room. The first scope is an isolated native proof using temporary stores and a
local transport fixture. Production design approval remains separate.

## Invitation and consent

Use distinct, versioned NIP-59 control rumors for Offer, Consent and Delivery.
All use the canonical account keys. The receiver verifies the gift wrap and its
recipient, the signed seal and matching rumor author, the computed rumor ID, the
explicit inner recipient, and a strict bounded schema. Use configured relays
only, including for retries. With Arti configured, an unavailable Tor route
leaves work pending or failed. Every leg, including any replay and publication,
uses the configured Tor path without direct or default relay fallback.

1. An eligible server persists a random offer ID, both canonical keys, roles,
   room ID, exact proposed relay set and expiration before sending Offer.
   The initiating profile checks current execution grants before issuing or
   retrying an offer. The receiver caps and validates the offer before recording
   the exact authenticated revision. The pre-offer gives the user a canonical
   npub and intended room and route. The eventual native signer and roster are
   verified after consent.
2. Off, blocked, expired, superseded and over-capacity offers yield no key
   allocation, reply or route. In manual mode, store only the bounded
   authenticated offer and present its exact ID for confirmation. An unknown
   standard Welcome cannot stand in for an offer. In auto mode, an existing
   explicit opt-in records the same per-offer decision after all the same
   checks. It says nothing about subsequent offers and grants no execution.
3. After a durable decision, allocate a fresh local MLS device and one
   SQLCipher store for this offer. The canonical Nostr key signs the fresh
   native KeyPackage. Send the exact signed kind-30443 event privately in
   Consent, along with the offer binding and selected device and KeyPackage
   identifiers. Do not publish this slot to directory relays. Initialize and
   persist its lifecycle before sending. One exact decision and signed
   artifact survive duplicate delivery and restart.
4. The server verifies the Consent's sender, event signature, canonical
   account proof, protocol/ciphersuite, lifetime and KeyPackage event ID and
   MLS reference. It creates locally in a dedicated Peer store with the
   intended initial authority and exact relay set, then persists the signed
   native Welcome and sends it inside Delivery. Delivery is a distinct
   authenticated control rumor binding the same offer, exact Welcome digest
   and KeyPackage. It carries the original, standard native NIP-59 wrapper
   for the canonical recipient. Creating the group does not implicitly publish
   that wrapper. Keep the native outbound Welcome obligation attached to its
   original ID and keep the stage quarantined while Delivery is pending.
   After persisting the receipt that the exact Delivery met the configured
   relay acknowledgment policy, mark the nested ID delivered via
   `AccountDeviceSession::mark_sent_welcome_delivered`. On rejection,
   cancellation, expiry or revocation, withdraw any outstanding obligation
   before enabling an adapter or cleaning up the store. Recover between
   these writes by replaying only the retained carrier or finalizing the
   native mark from the exact durable receipt.
   The client treats a normal relay delivery of the nested artifact as
   unsolicited. The server rechecks the grant before sending Delivery and
   before using the resulting room.

The nested carrier is new Marmish Peer control traffic. It preserves the
standard native Welcome as input to MLS, but peers implementing only the
ordinary Marmot Welcome inbox cannot participate in this handshake. A secondary
Nostr npub would make inbox separation easier and would change the account key
that MLS authenticates. Separate MLS device keys and stores keep both
canonical npubs in the initial roster.

## Store and transport ownership

For the first proof, model independent ingress with the actual signed
artifacts and the pinned adapter contract. Production dispatch remains
a separate gate.

Route Peer controls at the authenticated canonical inbox edge, before
`MdkOwner::receive_one` forwards ordinary deliveries to its runtime. Accepted
Peer transport routes are dispatched by exact native group transport ID to
their sole owning store. No Peer Offer, Consent, Delivery or Peer group
message reaches the established Command/Foundation SQLCipher provider.
Require a verified ordinary-author entitlement and an ordinary local
KeyPackage before passing raw Welcomes to the ordinary path. Ordinary-trusted
senders can still construct a raw Welcome addressed to an ordinary public
KeyPackage. Its inner purpose only becomes visible through MLS, and the
ordinary ingress compatibility behavior for this case is outside this
alternative. If the proof shows that a raw Peer artifact can reach ordinary
MDK in the new flow, the gate fails. An additional transactional guard would
be needed to enforce Peer-purpose rejection even for forged raw ordinary
Welcomes from already-trusted ordinary senders.

For the proof, open an `AccountDeviceRuntime` over its own SQLCipher provider.
Use an inert, recording adapter and inert router, a real Nostr publisher
for local signing only, and a provisioned slot. Leave the device-group
setting unset in the fresh stage so the device tag is settled
(`marmot-session/src/runtime/device_tag_key.rs:64`).
Prepare the package through `prepare_fresh_key_package` and leave the
signed artifact in `pending_replacement`. Reuse the native pending-package
fixture for this setup when possible. Use `session_mut().ingest_delivery`
under MDK's test-only `test-support` feature. Assert zero adapter calls and
zero KeyPackage publication calls during preparation and inspection. A normal
build must not depend on the test-only accessor. Proving an equivalent
production ingest seam is a separate gate.

Validate the outer control and pass its exact nested wrapper bytes through
the pinned native peeler. Separately decode the same bytes and call SDK
NIP-59 `extract_rumor_async` with the canonical local signer. Recover the
authenticated relay tag from that rumor. Require exactly one nonempty tag,
apply pinned MDK's relay validation and configured-route checks, and compare
the computed rumor ID, seal sender and KeyPackage event ID to the peeler's
results. MDK then authenticates and installs the candidate in quarantine.
Require a native `GroupJoined` receipt for the exact wrapper and
`welcomer` equal to both seal signers and the offer issuer. Read native
authority and require Current profile, exactly two active leaves with the
two canonical accounts, the issuer as sole initial administrator, exact
initial metadata/components and room purpose, and agreement between
authenticated hints, MLS route and configured relays. Require an active
local leaf. Verify that the pending lifecycle record marks consumption
of the exact approved KeyPackage reference, and retain the native
`own_leaf_hash` as the local leaf baseline. The public roster exposes
account and leaf index.
Reject any replacement, additional leaf or route mismatch. Record the
checks before any group subscription, public Peer package, maintenance,
application acknowledgment or status publication. Discard an invalid stage
and require a fresh offer with a fresh package. The ordinary account package
and group state stay untouched. This proposal does not preserve a rejected
stage's isolated package for another attempt.

Use the same independently encrypted store when the room becomes live. No
database copy, MLS export, state transplant or promotion into the ordinary
account store. Release the stage owner before opening a live owner. The
native adapter's activation and group synchronization are keyed by canonical
`MemberId`. Do not let individual same-account runtime instances call those
operations against a shared adapter and overwrite the ordinary route set.
One profile transport owner registers the aggregate validated route set and
owns the single canonical inbox. Peer adapters are scoped to their bound
room and route and must be unable to register or publish beyond it. A
subscription, replay delivery or acknowledgment must retain the correct
owning room. Prove the exact adapter seam. If it cannot be scoped without
duplicate account registration or cursor ambiguity, the alternative
is blocked for production. The first proof need not build the aggregate
adapter.

## Durable transitions and limits

Use a minimal test-only SQLCipher registry separate from the Peer store and
ordinary journal. Seed it with a disposable fixture key. Generate a random
local stage ID and SQLCipher key. In the proof store the stage key inside
the encrypted registry and authenticate each binding with a separate,
random fixture MAC key. Cover the local profile, both canonical accounts
and roles, decision mode, exact offer digest and revision, expiration,
stage and device IDs, KeyPackage event ID and ref, nested native wrapper
digest, room ID, and, once available, the native joined group, epoch,
source and complete verified authority and route. Never trust filesystem
presence or a human-readable marker for activation. Production key custody
is a separate design gate.

Persist allocated stage intent before opening an owned store. Persist one
exact candidate and its fence before native ingest. Use `Ready` only after
rechecking authority, the registry revision, current role/policy, block
list, grant where applicable and route configuration. Commit `Ready` in
the registry before deleting the stage outbox join receipt. The databases
have separate transactions. A restart after `Ready` commits and before
receipt deletion revalidates the binding and can clear the same native
receipt idempotently. A consent response or Delivery receipt does not mean
the room is ready. On a crash after isolated join and before
`Ready`, reopen only the recorded store. Require its durable native
initial-join receipt and authority to match the recorded exact candidate
and policy. Roll forward to `Ready` only when the binding is complete.
A missing/malformed seal, missing artifact, mismatched store, unexpected
additional group, stale consent, revocation, uncertain group state or local
read error leaves the room inactive. Transient failure can retry within
expiry. Confirmed terminal failure first fences the failed offer and disables
its route. It then removes the stage and key before accepting fresh work.
An orphan without an authenticated registry binding never opens as live.
Revoke routes before deleting stores. Report encrypted store/key removal
as logical cleanup, without promising physical media erasure.

Use proof admission limits of 16 pending offers, one per sender, a 24-hour
offer lifetime, and 64 KiB per control envelope. Partition the 1 MiB retained
ciphertext budget into 512 KiB for Offer and 256 KiB each for Consent and
Delivery. Charge actual retained bytes and leave an existing offer intact
when a partition is full. Bound work before decrypting and again by
authenticated sender. Test two concurrent quarantine stores and up to eight
`Ready` rooms across both roles, with one owner per store. Measure a target
of at most 64 additional descriptors and 128 MiB of combined DB, WAL and
SHM across those stores, including peak replay and reopen. The pinned
storage options lack a hard size cap. Refuse new admission at the measured
thresholds and report any peak overshoot. These are fixture bounds.
The later heartbeat phase remains outside the proof.

## Native proof and independent review

- The pin is `8abc51befbf71a1ae3bdbb25eb0860c731e75db2`.
  `transport-nostr-peeler/src/peeler.rs:229–305` verifies the wrapper and
  seal, returns non-Welcome rumors separately and exposes the computed
  rumor ID and claimed package event ID for a Welcome. Prove relay-hint
  validation by running the second SDK extraction on the same retained
  bytes, including duplicate tags, malformed values and configured route
  mismatch. Pinned relay validation is at `peeler.rs:487–530`.
- `marmot-session/src/runtime.rs:1392,1534` prepares an exact signed
  KeyPackage artifact and persists it before I/O. `session.rs:620` and
  the provider expose lifecycle state. `traits/src/maintenance.rs:505`
  recognizes a signed pending publication. Marmish already creates the
  signed event in `crates/marmish-native/src/mdk_adapter.rs:776` and
  validates kind/signature/identity and attaches `KeyPackageSource` near
  line 1511. Prove that a privately conveyed, never-published artifact can
  create a real signed native Welcome while its signed lifecycle record
  remains pending and before any publish or promotion. Assert that a
  different event ID or MLS reference fails and leaves the valid candidate
  usable in the stage.
- `marmot-session/src/session.rs:155,1374` accepts an already opened
  provider and ingests without an adapter when used through the test
  accessor at `marmot-session/src/runtime.rs:681`.
  `cgka-engine/src/group_lifecycle.rs:1494–1557,1688–1778,1935–1977`
  checks recipient material and native authority, then retains the native
  joined event and consumes the KeyPackage in that provider.
  `marmot-session/src/runtime.rs:1171–1205` exposes explicit transport
  activation and group synchronization. Marmish's ordinary ingress near
  `mdk_adapter.rs:2350` currently mutates before app projection. Exercise
  the interposition and independent databases. Reopen the outbound store
  against the real native adapter using a loopback relay and trigger startup
  recovery and retry. Assert zero raw Welcome publication after a durable
  Delivery relay acknowledgment and native completion, and after native
  withdrawal. Before either disposition the quarantined store may use only
  its inert adapter. Test a crash after persisting the carrier acknowledgment
  and before marking the native obligation delivered.
- Use quarantine and discard. The public trait's transaction contract at
  `traits/src/storage.rs:1324` and its private SQLite implementation at
  `storage-sqlite/src/base/connection.rs:663` could preserve a rejected
  stage's own KeyPackage through rollback. This would require a
  single-thread executor and disposal of the tentative in-memory engine.
  The proof accepts the cost of a new isolated offer after rejection.
- Use a real two-party signed/encrypted Welcome with both canonical
  leaves. Run manual accept and configured auto accept, plus off,
  unapproved, blocked and expired cases. Probe wrong seal/MLS signer,
  non-admin founder, changed metadata/purpose/route, wrong destination or
  package, extra leaf/device/admin, swapped provider, wrong profile,
  supersession, duplicate and rewrapped delivery, and delivery before
  consent. Confirm no subscriptions, status, publication obligation or
  command authorization for rejected Peer work. Keep an established
  Command room running concurrently. Assert unchanged ordinary group and
  KeyPackage state, unchanged relevant event/dedup rows and no ordinary
  routing overwrite from Peer work. Exercise ordinary command traffic
  before and after and test refusal of released raw nested Peer artifacts.
- Test crashes after intent/KeyPackage preparation, after delivery custody,
  after isolated join, after `Ready` and before deleting the native
  outbox join receipt, after receipt deletion and before route/ack, and
  during revocation. On a fresh process, verify one valid room, no double joins or
  duplicate publication, correct replay ownership and failure-closed
  recovery with corrupted or swapped registry/store and unavailable
  configured Tor. Measure end-to-end offer-to-ready time, active store
  descriptors/RSS, and reopen time as Ready rooms grow. Use one
  representative signed native fixture in core CI only if it is fast
  and deterministic. Keep the negative/restart matrix and resource
  measurements in the full local suite.


## Gates for any production phase

The first proof does not settle the following product and runtime decisions.

| Earlier invitation rule | Proof behavior and what production must establish |
| --- | --- |
| 64 accepted client channels and 64 outbound intents or active channels | The fixture exercises eight Ready rooms and two quarantines. Establish a supported room target against the execution grant scale and measured OS resource costs. |
| Existing encrypted journal stores invitation state | Use a separate test registry. Decide production registry ownership, key wrapping and derivation labels, and what happens when a custody key is lost. |
| Native Welcome acknowledged by relay | Record acknowledgment of the exact Delivery carrier separately from client acceptance. Define restart and status projections and which configured relay acknowledgments satisfy delivery. |
| Acknowledge after a durable decision, and full queues must not pin the cursor | Prove the production aggregate transport owner can distinguish a durable retriable intake, a disposable rejection, and a local write failure. Verify account-keyed replay ACK and same-account room dispatch with the actual adapter. |
| Validate authenticated rumor time, expiry and clock skew | Pin explicit local clock/skew semantics and line up validity for Offer, Consent, the KeyPackage, Delivery and replay. Randomized outer wrap timestamps do not decide freshness. |
| Replay fence spans the maximum remaining period of possible valid replay | Specify the fence relative to all authenticated expirations and skew, and prove expired-state cleanup and duplicate control recovery. |

Before production review, define the auto opt-in scope and the admission
policy for unknown senders. Demonstrate progress for ordinary traffic under
pre-decrypt load, bounded pending slot occupation and protection against
unknown senders filling Ready slots. Measure the cost of the second peel
on ordinary Welcomes and test the production ingress before deciding how to
handle already-trusted ordinary senders that forge raw Welcomes addressed
to ordinary public packages. Provide a non-test production ingress API
and prove aggregate same-account routing never clobbers ordinary routes.
Specify durable stage expiry, sweep and budget accounting, route revocation,
store close, destruction of the wrapped key, removal of DB/WAL/SHM and
cleanup recovery. Keep invitation code disabled until these gates have
evidence and independent review.
