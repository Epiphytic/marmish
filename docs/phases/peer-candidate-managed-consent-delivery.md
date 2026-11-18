# Connect the managed package to Manual Consent and Delivery

This is the local owner portion of the approved Manual actor contract
(`5cd55a8`, parent composition owns its publication) and the [managed
package](peer-candidate-managed-package.md) at author base `adb8c6f`.
The applicable actor contract is one registered Manual Unknown receiver,
one checked direct port, one held profile and checkpoint, actual route
observations with sticky global failure checking and no new authority
on uncertain transport, native or post-effect state. This leaf remains
private Linux AMD64 candidate and test only. Daemon, IPC, view, scheduling
and Welcome/Pending integration are separate. A signed v2 control grants
no active Peer permissions.

## Exact native evidence

- `receiver_inventory::package::PackageProof<'lease>` retains the exact
  original `FileExtent` and encrypted digest from the charged copy,
  `(Binding, FileWitness)`, `Artifact` and `ProvisionalPackage`.
  Replace `BoundStage` in the proof with these coordinates and a sealed
  checkpoint owner witness. `BoundStage` remains in issuance calls to
  `bind_stage` and `record_prepared`. Issuance mints a proof only after
  `record_prepared` succeeds. The queued constructor uses a separately
  sealed SQL challenge with the same coordinates without `BoundStage`.
  Retain the actual borrowed `&'lease ReceiverQuota<'lease>` from
  `RecoveredOwner::package_quota` after exact pointer equality with the
  caller's quota. Each file-effect check compares the proof quota there;
  the checkpoint verifies the owner and row and does not compare quotas.
- Keep the proof non-Clone with private fields and constructors in
  `receiver_inventory::package`. Re-export the type narrowly within
  `peer_invitation`. Checkpoint signatures used by both call sites receive
  a private-field borrowed `PackageEvidence<'a, 'lease>` with an
  exhaustive managed variant and a test-only `NativeAttestation` variant.
  `Checkpoint` verifies the managed owner seal before comparing stage,
  artifact and package lifetime and borrowing package bytes. Share the
  pure comparison with the fixture variant. No provider trait or caller
  construction from coordinates, digests or bytes.
- Split the existing charged-copy read from `RecoveredOwner::attest_package`.
  Provisioning still binds and records Prepared and may recover an
  interrupted NativeStarted journal as approved. A queued outbox forbids
  `stage_transaction`, including `bind_stage` and `record_prepared`.
  The queued helper opens a separate read-only installed aperture.
  `PackageTarget` privately carries an opaque checkpoint owner witness
  from the same checkpoint, exact Prepared binding/witness/artifact,
  and appropriate deadline. Derive the witness from the authorized
  `BoundStage` for issuance or the current scoped read for queued work.
  In `read_installed_consent`, require exact candidate/reservation or
  `Context::exact_consent` matching the original typed `Queued` with
  signed parent, exact outbox and retry/ACK metadata as applicable.
  Verify `State::Prepared` matches the typed input in that transaction.
  Reject custody or terminal ownership before a queued challenge.
  Fresh/saved Delivery feeds existing typed ancestry and the retry
  challenges preserve exact revision and prerequisite. Both floors are
  checked. No SQL write, wall update, journal recovery, stage update,
  new package or signed flight occurs in queued attestation.
- In successful readback check clean original identity, extent and charge
  under `CleanFiles::begin`. It rejects any retained sidecar and
  `classify_unknown` must match the Prepared SQL file witness. Copy/open
  via the existing separately charged permit and close guard. Cleanup
  and reverify the unchanged original. Before each later file effect,
  recheck exact owner/quota and obtain the same measured clean extent.
  Use `PackageDirectory::original_digest` with the exact no-follow file,
  checking stream extent, same inode before/after, exact digest and current
  fence. Close the file turn before any SQL write or network await.
  Recheck exact owner/route/readiness/time before and after side effects.
  A hard kill during copy leaves ambiguous charged inventory fenced on
  restart, including queued attestation, with no cleanup proof inferred
  from the copy or unmetered recovery.
- Use prejoin proof only before Held. Held repeats use authenticated
  retained Delivery, and JoinStarted/Pending use joined evidence. A
  consumed KeyPackage cannot qualify post-Held state.
- Preserve the existing historical check for exact expired saved retries.
  Fresh work uses current native/verification wall and its signed
  authorization end. After expiry, the current saved observation may
  carry sealed `expires - 1` from `PreparedSaved` for historical
  `read_provisional` and `verify_custody_delivery` only. The disposable
  reader still operates in a fresh bounded Ready/stop/route/fatal window.
  Its exclusive cap for terminal-only retirement is the current route
  qualification and physical bound, independent of the expired event.
  Use actual time for every filesystem, route, floor and SQL effect.
  A historical proof is a separate expired-retry-only type consumed by
  exact canonical saved-row deletion. It cannot construct live managed
  evidence, sign, send or commit Held. If expiry crosses during the live
  attempt, refuse late work and let the later expired retry authenticate
  the existing row separately.

## Borrowed turns

Use one `RecoveredOwner`, its original `Checkpoint` and original `Source`.
There is no new cursor, checkpoint owner, mirror database, native lease
or quota registration. The new `receiver_inventory::managed_consent`
child owns `ConsentTurn<'turn, 'lease>`, which borrows the mutably held
owner, its exact registered quota and current window. Only its narrow
reserve, before-flush, settle and wake methods are available to the
transport wrapper. The wrapper cannot name or use internal owner fields.
The new `receiver_inventory::managed_delivery` child owns an analogous
`DeliveryTurn` for each bounded checked fresh/saved Inspect. It cannot
open a socket.

The actor owner's `PeerRecipientPort::use_current_native` produces
`NativeRouteWindow` from the actual checked account/origin, physical
wall projected from whole seconds to `Duration`, absolute monotonic
route horizon and its private cancellation/fatal state. Bind the
managed native window to that exact port incarnation, privately checking
the pairing before an effect and on the later checked observation.
Keep cancellation private. Cap `publish_control_with` at the minimum
of the physical, existing five-second native and signed limits using
a monotonic remainder from the native window. BOOTTIME still applies
over suspend. Never refresh a window to complete an attempt or ACK.

1. **Offer and queue.** Keep installed signed v2 ingress, page/live
   semantics and the same Manual Unknown reservation. First read the
   exact installed queued state without advancing floors. An outbox
   survivor uses only the queued path. Absent outbox permits original
   bounded provisioning. On Prepared read the exact candidate, derive
   and re-digest before signing and again before queue commit. Reuse
   the queue CAS and before-commit in-memory fence. After commit,
   re-digest and verify the exact recovered transcript before returning.
   Stop or failed postcheck releases no proof, including after commit.
2. **Consent.** Reserve one exact current attempt after charged snapshot
   and original re-digest. Reuse `publish_control_with` and
   `ControlSendPermit`. Before flush compare exact durable attempt and
   re-digest. Per-poll authorization uses memory-only attempt, scope,
   and revocation/time. No file, digest or SQL operation runs inside the
   poll closure. At accepted ACK re-digest and consume the exact current
   `CheckedAck` in the same attempt CAS. Reuse the narrow settled token
   and one-use scoped Delivery wake. Report a failed wake by fixed
   category while retaining successful settlement and unchanged due
   retry. Track the existing not-sent, possibly-exposed and settled
   outcomes. Never infer ACK from history or retry on a retired port.
3. **Delivery.** Reuse checked page/live classification and exact saved
   retry. On Inspect attest the selected charged package. Requalify the
   same immutable checked carrier or exact saved revision before
   original re-digest and committing. Keep grammar, lexical conflict,
   receipt equality, revision CAS, both wall floors, deferred capacity,
   expiry, page completion and no-progress handling. Return only the
   existing durable authenticated Held, with no Welcome permit here.
   Invalidate output on stop/route failure/deadline even if a durable
   effect completed. Actor composition brackets errors with retained
   independent forwarding proof.

## Source cuts and gates

This leaf owns the following package, Consent and Delivery items. The
actor owner owns the checked native window producer, scheduling and
view, installed Pending and Join. The parent coordinates adjacent
`lib.rs` and `peer_invitation.rs` mounts. Preserve portable test coverage
where present by extending `#[cfg(test)]` to
`any(test, all(marmish_peer_receiver_candidate, target_os = "linux", target_arch = "x86_64"))`.
For formerly Linux-only items use
`all(any(test, marmish_peer_receiver_candidate), target_os = "linux", target_arch = "x86_64")`.

1. **Package and read-only challenge.** In `peer_candidate_package_owner.rs`
   own `PackageOperation`, `PackageProof`, shared copy/read and original
   check. In `peer_receiver_inventory.rs` own the package re-export/child
   mount. In `peer_stage_journal.rs` own `BoundStage` and checkpoint
   seal comparison, never a public Arc or constructor. Share only
   `Candidate`, `Candidate::read`/stage getter, `Queued` and the outbox
   decode mount from `peer_prepared_consent.rs`. Share exact `Transcript`
   decoding needed by the outbox read from `peer_prepared_consent_wire.rs`.
   In `peer_pending_custody_checkpoint.rs` extend `Context`'s route, signer
   and codec, their `custody_context` construction, `Context::exact_consent`,
   `read_installed_consent` and its exact scope, and the terminal-owner
   predicate used by that read. Add the sealed challenge inside that
   checkpoint family. `CleanFiles::{begin,snapshot_permit,snapshot_closed,finish}`,
   `OpenStage::read_provisional` and `PackageDirectory::original_digest`
   already exist. No new signing, publication, ACK or Held write.
2. **Offer and Consent.** Extend the installed Offer mount without
   `admit_offer_fixture` or test helpers. In `peer_installed_offer_ingress.rs`
   share `InstalledCarrier`, `OfferIngress`, `classify_installed_carrier`,
   `record_installed_offer`, `admit_installed_custody_offer`. Promote in
   `peer_prepared_consent.rs` only `Unsaved`, `read_live_prepared`,
   `inspect_prepared_consent`, `sign_prepared_consent`,
   `queue_prepared_consent` and `verify_queued_consent`.
   Share only the exact capped v2 Consent preparation body from
   `peer_prepared_consent_wire.rs`. Candidate requires the sealed operand
   and installed v2 scope. Keep the old fixture wrapper test-only.
   In `peer_prepared_consent_attempt.rs` share `Attempt` and its accessors,
   `change_queued`, `queued_origin`, `exact_row`, `reserve_prepared_attempt`,
   `check_installed_prepared_attempt` and internal `settle_attempt`.
   Keep legacy `MatchingAck` handling and `check_current_attempt`
   test-only. Extend the settlement mount for `SettledConsent` and
   `settle_installed_prepared_attempt`. In `peer_recipient_port.rs` the
   leaf owns the `installed_consent` child and `PublicationOutcome` export.
   Share only checked permit/use-accepted core and `CheckedAck::exact_attempt`
   from `peer_installed_consent_port.rs`. Retain the fixture import/wrapper
   under test, using a closed variant if needed to reuse the same core.
3. **Delivery.** In `peer_recipient_port.rs` the leaf owns observation
   gates for `CarrierLocation`, `CarrierCoordinates`, `CarrierWitness`,
   `CheckedCarrier` and its impl, `use_page_carrier`, `use_live_carrier`,
   and the saved observation mount/re-export. Share `CheckedSavedDelivery`,
   `SavedPortWitness` and `use_saved_delivery` from
   `peer_recipient_delivery_port.rs`. Extend the two installed ingress
   and retry mounts and all their exact installed methods/helpers besides
   `fixture_delivery_state`. Share `Selector::from_delivery`, only the
   `Transcript`/wire/codec parsing and verification used by installed
   classification and retained exact replay, `MARGIN`,
   `HeldInput::{InstalledObserved,Saved}`, `CustodyConsent`,
   `ConsentAuthority`, Held evidence and its read/load helpers,
   `verify_custody_delivery`, `commit_custody_held_input` and the
   existing exact row helpers used by installed ancestry and replay.
   Leave `HeldInput::Observed`, direct `custody_receive` and
   `custody_replay`, raw fixture signer/unqualified input, legacy
   coordinator, join, approval, publication and leave/effect methods
   under test or separately reviewed actor gates.

The package and read-only aperture is an independently reviewed source
cut. An explicit candidate compiler check precedes each later cut's
review. Any additional compiler-discovered gates outside these families
are a review finding. Keep `SnapshotArea`, `PreparedConsentOwner`, test
`JoinedReadback` construction, indefinite fixture writers and test hooks
under their existing gates.

## Validation

Reuse the existing opt-in candidate subprocess in
`peer_candidate_package_tests.rs` with actual registered quota,
SQLCipher and stock profile lease. Check issued and cold queued proof
against the same exact file across owner restart. Compare full attempt
and wall floors after repeat reads. Exercise wrong owner/quota,
same-inode digest or extent change, replaced witness, retained journal,
hard-kill during queued copy and restart fence, cancellation and native
deadline, no artifact/queue changes or charge release on refusal.
Measure fresh and queued snapshot and later re-digest separately,
including a valid larger encrypted image within the documented cap
where feasible. Passing live effects must finish within the actual five
seconds. If measured work exceeds it, require bounded refusal with no
eligible output and record workload extent and elapsed time. A wider
budget needs separate approval.

Reuse the local WSS Opt-in cases in `peer_installed_consent_wss_tests.rs`
and `peer_installed_delivery_wss_tests.rs` for actual checked-port sends,
ACK CAS, scoped wake, before-flush and ACK-time local refusal, fresh,
saved, expired and Held replay. Use a candidate mapped test turn with real
registered cursor/quota before claiming managed end to end. Expired saved
tests include exact boundary and older than package lifetime, present
or missing ACK, current stale checked port and no unexpected Held.
Rollback both floors for failed deletion and verify same-row admission.

Run `cargo check --offline --locked -p marmish-native --all-targets` and
`cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings`
in stock and explicit candidate mode. For the latter set
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`
and pass `--target x86_64-unknown-linux-gnu` before the Clippy separator.
Use warm paths and archived exact executable with no shared writable
profile. Run candidate negative compile probes for fixture proof
construction, `PreparedConsentOwner` and `Fixture::open`, plus stock
symbol audit. Record exact review/validation under `docs/reviews/` and
`docs/validation/`, including per-case wall time and proof of no stock
mount. The installed daemon/Pending benchmarks belong to actor qualification.
