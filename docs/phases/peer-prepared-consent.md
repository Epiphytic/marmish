# Own Peer Consent from the exact prepared stage

This increment adds a private handoff from the
[native stage binding](peer-native-stage-binding.md)
checkpoint to immutable Consent. It uses Marmish
`95ce7683d0bcc0ed8720011be5b187e1bbb6610d` and its locked upstream MDK
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`. The fixture proves ownership,
recovery and ordering with a non-network sink. The output is a typed seam
for a later owner. Daemon, CLI and ordinary Command routing remain closed
to provisional stages. Storage quota, relay qualification, Welcome
ingestion, Peer acceptance, and joined-stage leave are separate phases.

## Existing boundaries

`Checkpoint::reserve_provisioning` and `recover_provisioning` own a
profile-bound `Reservation` and its immutable `Binding`. The binding covers
class, generation, authenticated source and full signed Offer selector.
Each of two classes has one occupied slot and a ten-minute reservation
charge. Only the configured local canonical identity policy supplies the
class. `InvitationState::prepare_provisional` drives
`bind_stage`, `start_stage` and `record_prepared` to retain the exact signed
MDK KeyPackage, its lifetime, digest, and its checked file witness. The
same `ProfileLease` must span the owner turn and encrypted stage reopen.

Legacy `InvitationOwner::publish_approved` takes a caller-selected
`StageSlot` and the old `approved_work` / `decision_at` route. Its
`reserve_attempt` and `record_acceptance` provide exact-origin,
attempt/bytes/carrier ownership, retry timing and ACK compare-and-swap.
The existing SQL claim and outbox triggers deliberately prohibit mixing
that outbox with a generational slot. Preserve those triggers and the
legacy behavior. Reuse `Codec::prepare_consent`, exact signed Offer checks,
the exact-native-artifact verifier, imposed limits and the retry contract.
The provisional export path never uses a caller-selected slot, sets
`decision_at`, or grants application authority.

## Prepare, queue and verify

1. Add a distinct export fixture epoch. Every new constructor, type,
   SQL literal, writer and sink is `cfg(test)`. Start from the complete
   v1 provisional-stage schema, using the checked SQLCipher cursor and
   expected account and origins. Validate the full sorted main schema
   including auto-indices with NULL SQL, reject the temp schema, and
   verify versions, account, origins and both slots inside one IMMEDIATE
   transaction. Use an immutable marker and byte-distinct claim-trigger
   DDL that retains the legacy conflict behavior. Keep export separate
   from both old modes, including the retirement epoch flag. Export has
   no retirement tables, freeze trigger or cooldown. The export
   attachment reopens its exact inventory. It rejects a blank, partial,
   unknown or retired schema before DDL. Legacy `Checkpoint::attach`,
   `attach_for_provisional_proof` and `retirement::open` reject export
   with Binding; neither newer fixture upgrades into the other.
2. The serialized leased owner supplies `queue_prepared_consent`,
   `recover_queued_consent`, `reserve_prepared_attempt` and
   `settle_prepared_ack`. First queue uses the exact owner-issued
   `Reservation`, configured source and owned checked time. It verifies
   a live Offer and exact Prepared stage in the existing
   `stage_transaction`. Obtain exact native bytes with the sealed
   snapshot read described below. Revalidate native event ID, signed
   bytes, lifetime and artifact digest. The capability stays opaque
   outside this owner. The lease serializes witness use, preparation
   and publication. Replaced or missing native state cannot authorize
   repair.
3. Existing queue lookup and restart use `recover_queued_consent` before
   any live-Offer check. Its dedicated query joins the occupied slot,
   original invite, exact Prepared stage and committed Consent row on
   the full binding, including selector and authenticated origin. It
   never calls `live_provisioning_offer`, `stage_transaction`,
   `recover_provisioning`, `bind_stage`, native generation or signing.
   Reverify the original Offer using `check_signed_offer` at its stored
   signed expiry minus one, as retirement does. This reconstructs the
   cryptographic Offer only. Advance the owned monotonic wall and check
   current profile, client role, policy, quarantine, source and Consent
   deadline independently. The exported generation stays pinned through
   Offer expiry and the exact still-live Consent can retry. Queue
   without an existing row still requires a live Offer. The restart
   native read uses the same checked witness and bounded snapshot, with
   no transitions or writes on the authoritative native stage.
4. Use an independent guarded outbox. Require `PRIMARY KEY(class)`,
   `CHECK(class IN (0,1))`, a positive generation on every query,
   `UNIQUE(server)` and `UNIQUE(carrier)`. This permits at most two rows
   including ACKED and expired rows. Persist the canonical profile
   account, configured source, server, full Offer selector and revision,
   generation, exact stage artifact ID/digest/expiry and exact encrypted
   signed carrier ID/bytes/digest. A permanent BEFORE INSERT guard
   requires an occupied exact slot, the full matching invite and a
   Prepared stage (state 2) with the identical artifact ID, digest and
   expiry. SQL shape guards require frame length 1..65536 and a positive
   Consent deadline and bind state to attempt, retry and ACK: QUEUED is
   (0, NULL, NULL), IN_FLIGHT is (positive, present, NULL), and ACKED is
   (positive, present, present with ACK wall before Consent deadline).
   Fields that establish the binding, transcript and carrier are immutable.
   All direct INSERT, UPDATE and DELETE are denied unconditionally by
   dedicated write guards. In an IMMEDIATE owner transaction, remove
   and restore only the exact relevant deny guard as retirement does.
   All other guards, including exact reference and shape checks, remain
   installed. Use plain INSERT or UPDATE, require exactly one changed
   row for each write, and validate the full identical epoch before
   commit. Bind reverse references so release, rotation, sweep or either
   REPLACE form cannot strand an outbox. Verify with foreign keys and
   recursive triggers disabled. Keep the exact late row and replay
   fences pinned after expiry or revocation until separately reviewed
   export-aware cleanup. The earlier no-export retirement path cannot
   touch this epoch.
5. Build the private signed encrypted Consent synchronously outside the
   final IMMEDIATE queue transaction using `Codec::prepare_consent`, the
   verified original Offer, local signer and re-read exact native
   package. Require live Offer at sign and at commit. Use the minimum
   of verified MLS package `not_after`, stored package event expiry,
   checked `Consent.created_at + 86400` and the legacy Material cutoff
   (owned wall + 86400, also limited by the package event expiry).
   Preserve all codec validity checks and reject any empty window.
   Recheck exact profile, origin, policy, quarantine, generation, artifact
   and owned monotonic wall under the final transaction. Exact file
   identity is checked before that transaction and after commit. Discard
   undisclosed frames on failure; no sink obtains bytes before commit.
   If the exact row exists, return its authenticated recovered transcript
   instead of creating a signature. A retry may never replace the native
   KeyPackage or committed carrier.
6. Preserve the signer-produced inner transcript separately from the
   outer carrier ID and digest: persist inner Consent revision, creation
   time, expiry and native package binding (key package event ID,
   package reference and slot), and rederive or compare the remaining
   Flight fields using guarded row, signed Offer and re-read native bytes.
   Persist the transcript together with the exact frame while the local
   signer owns the sealed Consent. On recovery verify outer signature,
   ID, exact bytes/digest, recipient, canonical transcript coordinates
   and exact native package again. A sender cannot decrypt its own
   outbound gift wrap. The disposable receiver separately decrypts
   published fixture bytes and compares the entire inner transcript to
   the owner record. A later codec-only step converts the verified
   transcript into the expected `Consent` used by `open_delivery`.
   It can reconstruct the canonical inner rumor revision using the
   exact stored creation time with the verified fields and compare
   that revision, while leaving the encrypted carrier unchanged. No
   fixture authority here accepts an incoming Delivery.
7. For a due attempt, recover and re-read the exact native artifact before
   the reserve transaction. Recheck the full binding, current policy,
   quarantine, origin, expiry, custody and exact epoch. Reserve one
   attempt with checked bounded counter/backoff arithmetic and the
   existing persisted 60-second retry. No attempt is reserved for an
   unavailable or retired sink. Verify file identity before the
   transaction, after commit, and before and after the sink call. Bind
   the witnessed native image digest to the opaque in-process attempt.
   Give immutable committed bytes to the bounded connector-free in-process
   sink on the exact authenticated origin. Only the same sink may issue
   a positive typed ACK for this attempt, route and carrier. Settle checks
   the exact file witness and attempt-bound image digest before its
   transaction and uses guarded SQL checks for the same attempt, exact committed bytes, profile, origin,
   policy and expiry. Settle needs no native handle. Verify exact
   identity after commit. Commit ACK using one matching CAS. The ACK
   permits a later Delivery claim and grants no application authority.
   No cursor transaction spans native, signer or sink work.
8. Recover without implicit effects. A cut before sign or after sign
   but before queue commit leaves Prepared with no sink access. A cut
   after queue restarts the same carrier even past Offer expiry while
   Consent is live. A cut after reserve pays the retry interval even if
   no send occurred. A cut after a positive sink ACK and before its
   durable CAS allows exact byte retry. A cut after the durable ACK
   leaves it settled. Negative, unrelated, wrong-origin, duplicate or
   stale ACK cannot advance the row. Stale generations cannot touch a
   later row. Expired or revoked owners obtain no new attempt or
   application state.

### Native read custody

Upstream `SqliteAccountStorage::open_encrypted_with_options` opens
read/write/create and checks migrations before returning.
`OpenStage::open_provisional(..., false)` rejects a nonzero migration
summary after that return. Keep this MDK open away from the original
Prepared path.

For queue, recovery and reserve, reopen and check the original using
`Paths::reopen_provisional` with its exact stored file witness and
profile lease. Use the witness-checked read descriptor and the existing
stage image limit of 16 MiB. Copy one bounded exact encrypted image
to a create-new 0600 regular file inside an owner-private mode-0700
disposable test fixture directory. Check the original path, open inode,
expected absence of sidecars, original image digest and the copy digest.
Open only the copy with the same derived SQLCipher key, exact native
slot label and origin via `OpenStage::open_provisional(..., false)`;
then use `read_provisional`. Close MDK before checking that copy bytes
and original witness and digest still match, with no sidecars. Remove
the copy before exposing any recovered capability. Require the copy to remain the same existing inode and reject a
nonzero migration summary or any changed bytes. Native
SQL accepts the conventional path-based open on this disposable image
only. No image travels to the sink or leaves the fixture directory.
Allow one active image per fixture owner. Interruptions use a fresh
private fixture directory and the test harness owns cleanup of the
bounded superseded directory; no production storage or quota feature is
built here. Settle verifies the original checked witness and the
captured digest against that attempt, without copying or opening MDK.

Within this single increment, implement and validate schema guards first,
then queue/recovery, then reserve/settle. Retain one completion gate.

## Later Pending and activation

The later Delivery owner must require a durably acknowledged Consent
for the same origin, inviter, offer, generation, native artifact and
unclaimed Delivery. Use the reviewed codec seam that reconstructs a verified expected
`Consent` and supplies it to `Codec::open_delivery`. Verify the exact
carrier and pending KeyPackage/route hints, then record an
exact durable Candidate before invoking the public
`AccountDeviceRuntime::ingest_delivery`. Adapt the run proven by
`exact_welcome_and_ready_survive_public_runtime_recovery`. Require the
stored exact Welcome receipt, inviter/provenance, current native room
authority, two-leaf account/role roster, purpose, route, and consumed
package before committing Pending. Reconcile the same receipt across
crash without repeating native consumption. The default-deny adapter
guards every native egress and fetch surface. The exact Consent sink is
separate from native egress. A native receipt alone conveys no remote
trust, application dispatch, application ACK, permission/status view or
accepted server listing.

An authenticated local UID can request approval on that exact Pending
generation. Auto invokes the same checked Pending-to-Activating transition
only when first admitting a newly validated Pending row under the current
auto policy. Only later, after typed PostJoin/history completion and
authority/expiry recheck, does Active authorize an exact signed
application acceptance outbox. Those decisions and application effects
are unreachable in this phase.

Joined decline and the crash after durable native SelfRemove but before
fanout storage are an independent qualification. At the locked revision
the public `send_host_change_with_audit_context` accepts the native intent
before `TransportRoutingPolicy::publish_target` and fanout staging. A
separate proof can refuse routing before staging, reopen encrypted
`Sent` + `LeaveRequest`, verify exact locally authored SelfRemove against
the current stable epoch, and evaluate
`publish_session_effects` with one verified `PublishWork::Proposal`
through a physically denied adapter. Preserve unconfirmed leave on any
ambiguity. No Leave change or retirement widening is part of this phase.

## Validation and exit criteria

- Use encrypted disposable profiles, real MDK Prepared artifacts and the
  existing exact signed Offer + Consent codec. Assert both Manual and
  Auto can provision under the current policy without `decision_at`.
  Off, wrong role, current block, quarantined origin, stale owner,
  wrong origin/selector/class/generation, expired Offer without an existing
  queue row or invalid package cannot produce an attempt. Fill both classes and verify their
  independent owner/charge bounds.
- Attempt legacy cross-claim/outbox conflicts, unexpected/partial schema
  including retirement and old attachment, direct DML, INSERT OR REPLACE
  and UPDATE OR REPLACE against each key independently, including
  collisions with the other class, with FK and recursion disabled. Check
  reverse parent guards, second connections and profile/origin/policy/row
  corruption. Require identical accepted inventory and rollback after
  cursor write failures. Test both directions of export versus retirement
  rejection and older/legacy attachment behavior.
- Test every boundary with fresh SQLCipher attachments. Include loss after
  native stage preparation, before sign, after sign before queue commit,
  after queue commit, after reserve before sink,
  after sink before ACK commit, and after durable ACK. Substitute wrong
  package, bytes, SQLCipher key, file identity, expired clocks and stale
  capabilities. Assert an interrupted or mutated native read cannot
  change original stage bytes, create a replacement or make a sink
  attempt available. Exercise a stale or tampered source image, dirty
  sidecars, copy mutation/migration and image cleanup on both success and
  failure. Verify all published sink frames are identical.
- Run the non-network sink against not-ready, negative, wrong carrier,
  wrong origin, stale/duplicate and correctly matched acknowledgements.
  Verify retry equality at 60 seconds and row no-op for stale ACK. Assert
  live queued Consent survives the original Offer deadline, ceases at its
  own signed expiry, and never admits a new Consent after Offer expiry. Prove
  absence of any native consume, room, acceptance receipt, CLI, relay I/O,
  or new export epoch, table, writer and sink symbols in the non-test
  library. Verify both inner transcript and outer carrier identity after
  restart against the exact distinct recipient-decoded Consent. Re-run
  prior cursor, stage, retirement
  and legacy Consent suites without adding expensive runs to core CI.
- Measure at least 20 local queued, verified restart/retry and matching
  ACK turns, separating MDK stage prep and sink latency. Record platform,
  lock, timings, sample count and expected report scope. Benchmarks are
  opt-in. Obtain actual Claude Code Auto approval for the exact integrated
  implementation and evidence before folding into a later phase.
