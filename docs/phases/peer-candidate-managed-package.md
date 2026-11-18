# Meter the Manual recipient's native KeyPackage

This is the package prerequisite for the
[approved Manual actor](peer-candidate-manual-actor.md). The plan's
public blob at `8bcc2b0` matches exact approved `e450de00`. Start from
`8bcc2b0`, whose lock resolves MDK
`bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`. The exact integrated
Pending and refusal source is `f7bb53f` (`7d725d0` proof lineage)
with managed rollback `6a8846e`, guard matrix `515557a`, and clean
charge proof `72aef7a`. These supply tested behavior under fixture cfg. Produce two private
operations for the Linux AMD64 candidate's single durable Unknown slot:
mint a native package on the original file once, and attest an existing,
durably signed package on a separately charged copy. Keep the actor,
Consent publication, stock builds and public installers dormant for this
increment. No new schema, native protocol, relay behavior or role.

## Shared owner and fixed narrow operations

- The sole recipient cursor owner in `peer_receiver_inventory.rs` uses
  the authenticated installed-v2 Offer, current `Source`, and exact
  `Reservation` from the existing Manual admission. Keep the same
  `Arc<ProfileLease>` and the borrowed `ReceiverQuota` from
  `RegisteredReceiver::attach` and `LocalReceiverRegistration::quota`
  throughout. Test-only `ReceiverQuota::bind_receiver` and
  `ProfileSlots` cannot supply candidate authority. Registration and its
  forwarding check precede ordinary and Peer native opens. Borrow the
  actor's one existing keyed cursor. Stage state, offer and walls stay
  in `peer_stage_journal.rs`.
- This increment owns the minimum candidate/test Linux AMD64 promotion
  of the exclusive `peer_receiver_inventory` mount and its concrete
  `RecoveredOwner::recover` plus the existing borrowed-quota
  `rollback_journal`. The wrapper `recover_journal` that binds its
  own quota stays test-only. Candidate rollback uses the original
  registered quota and omits the test interrupt argument. Propagate
  the candidate fence through managed recovery and `CleanFiles` checks.
  Promote the required `CleanFiles` and `CleanPermit` mount/re-export,
  `Dispatch::file_turn`, `ReceiverQuota::receiver_files`, and
  `ReceiverFiles`. Include `SlotClass::Snapshot`, `SNAPSHOT` and
  the already candidate-scoped `RECEIVER_SNAPSHOT_NAME`. The old fixture
  `SNAPSHOT_NAME` stays test-only. Include the registered rollback
  path's `RollbackFiles` checked file/header helpers and re-export,
  `RecoveryRequest`, `recover_unknown`, `admit_recovery` and its
  `admit_checked` branch, and the corresponding `RecoveryImage` and
  recovering state in the VFS. Keep crash injection, fixture binding
  and general rollback calls outside candidate cfg. No broader scope
  is supplied by the existing fixture proofs.
- Mount the package coordinator as a private child of that inventory.
  Derive
  `Paths` once, explicitly check actual member directory identity and
  fixed original and snapshot names against the attached quota's
  `ReservedPaths`, and repeat those checks under exclusion. Add
  `ReceiverFiles::generate_package` using the existing private
  `measured_turn` with `SlotClass::Unknown` and `snapshot=false`,
  consuming the same-checkpoint `NativeStart` with exact `BoundStage`,
  measured `CleanPermit` and file witness. Add a private
  `ReceiverFiles::attest_package` wrapper around `inspect_snapshot`
  with the distinct copy permit (`snapshot=true`).
  Split the production-shaped signed generation/read subset out of
  `peer_stage_native.rs` before moving its cfg. Keep withhold, finalize
  and fixture replacement test-only, including their entry points and
  signing switches. Candidate generation uses the fenced publisher
  via MDK once and then signed-only readback. The existing direct-sign
  fallback inside `OpenStage::package` grants no candidate operation. Derive path, key, signer,
  origin and slot from the checked owner and `Binding`. No arbitrary
  path, raw quota, caller-supplied time or general callback crosses this
  boundary. Keep the existing fixture operations and selectors separate.
- The actor supplies a concrete owned native window shared by the
  candidate turn and its local publisher. Refactor the clock/checking
  core behind the test-mounted `TurnFence` into a narrow candidate/test
  companion; do not promote the fixture owner chain. The native window
  is `Send + Sync + 'static`, with the exact cloned readiness watch
  receiver and stop Arc, fixed monotonic and signed/qualification wall
  ceilings, verified anchored clock, and shared floors/irreversible loss
  latch. It confers only revocation checking, never readiness, WSS or
  storage permission. Retain the existing `StageWall` as an immutable
  checked subsecond `Duration` sample captured before the native call.
  Use the same sample for MDK seconds and milliseconds. `WallClock`
  remains infallible and cannot enforce cancellation. Check the actual
  window immediately before the keyed open, local lifecycle bootstrap
  and native preparation call. Check inside the fallible
  `KeyPackagePublisher::prepare_key_package` immediately before signing
  and after signing before handing bytes to MDK for its durable save.
  Recheck after synchronous I/O, before SQL CAS and releasing any proof.
  An internal SQLite step can finish after loss. Keep effects charged,
  discard any authority when the latch is set, and release the scope
  only after the one-polled future/handles have dropped. Never recreate
  `NativeStart` after interruption.
- There are separate serviced turns for bind/allocation/start, original
  generation, and copy attestation plus its Prepared CAS or handoff.
  Sample the existing `continuous_time()` on admission to each turn.
  Add at most five seconds to that fixed monotonic sample and tighten
  it to the remaining signed and qualification wall limits, including
  all preflight and postchecks. Independently obtain the verified
  subsecond wall `Duration` before entering native preparation. Check
  that wall against the native system clock within 120 seconds because
  MDK's OpenMLS expiry uses system time. Carry the fresh single-use allocation
  or start token only to the immediately next phase in the same live
  owner and port; it gets no extra lifetime or recovery authority.

## Original issue and cold attestation

1. Before allocation use checked arithmetic, the verified anchored
   wall and MDK's 30-day native lifetime. Require
   `offer_expires + 600 + 120 <= wall_seconds + 30*86_400`, including
   the existing 120-second clock tolerance conservatively. This only
   avoids impossible work; `record_prepared` still checks actual native
   expiration independently. Run `bind_stage` against the exact
   reservation. Under the same
   registered file exclusion, inspect the durable binding and require
   inventory `Class::Create` for this Unknown generation, clean family,
   correct parents and sufficient total headroom. Create the fixed main
   exclusively with private mode, no-follow, and sync file and parent
   using the checked member parent descriptor and the existing creation
   constraints. Capture identity from the actual created fd and compare
   it with checked path, parent and clean sidecars before minting a
   private one-use allocation witness. Measure the new file's allocation
   against the same inventory and quota before using the witness.
   Release file exclusion before `start_stage` writes the exact inode.
   Do not authorize allocation through the fixture's permissive
   `resume_allocation`.
2. Reacquire measured clean file exclusion. Require `NativeStarted`,
   the exact bound empty inode, the one-use token and no sidecars. With
   the active Unknown quota admitted, open as keyed SQLite DELETE only
   then call the closed, non-withholding form of
   `OpenStage::generate_provisional` once. Require no native groups,
   clean closure, same inode and all caps. Reuse
   `AccountDeviceRuntime::prepare_fresh_key_package` through the existing
   `NoIo` adapter and local publisher, never the native publish method.
   Hold the quota until storage and the local future have dropped. Treat
   the returned artifact as a comparison value, with no Consent authority.
3. Live completion and cold `NativeStarted` meet at the same read-only
   logical attestation. Use `CleanFiles::begin`, `snapshot_path`,
   `snapshot_permit`, `snapshot_closed` and `finish` with the same
   registration. Implement private `copy_for_package_attestation` on
   `CleanFiles`, reusing the file and digest mechanics of
   `peer_receiver_v2_pending.rs::inspect_copy` under the same type
   of measured quota. Keep the enclosing Pending fixture and its
   generic callback outside this promotion. Replace uninterruptible
   `io::copy` in the candidate leaf with a bounded checked loop. Admit overlap before
   create, exclusively create the registered copy as mode 0600 and
   no-follow under the checked parent, capture its fd identity before
   writing, and check the turn between bounded chunks. Sync the full
   copy and parent. Compare exact original identity and digest before
   and after, and equal digest on the copy before opening. On refusal
   close everything first, then remove only the same freshly witnessed
   copy if clean and metadata still matches; verify no sidecars and
   sync the parent. If exact cleanup cannot be proven, retain the
   closed charge. Cleanup grants no further authority.
   Open the copy with the exact stage key, DELETE and zero migrations.
   Require unjoined authority and `read_provisional` (signed-only).
   Reuse `check_pending` and `exact_artifact` for one owned native
   package, local signer, precise endpoint and label, canonical signed
   bytes and ID, valid lifetime, and unattempted/unconsumed lifecycle.
   Close and recheck copied inode, extent and sidecars before checked
   removal. Only the original must retain exact ciphertext after
   readback. Charge all actual allocation and logical extent, including
   sidecars and copy overlap, with the fixed 49 MiB cursor reserve,
   16 MiB closed-stage main limit and strict total below 128 MiB.
4. Release file exclusion. Recheck the exact lease/origin/selector/
   generation/inode and current live binding, original identity and
   digest at the checkpoint fence. Compare the live artifact if one
   exists. `record_prepared` commits the existing ID, SHA-256 digest
   and expiration, retaining the Offer expiry plus 600-second minimum.
   Keep the sealed package bytes private. On a subsequent Prepared turn
   acquire a fresh copy proof and compare all stored artifact coordinates
   before offering a capped witness to the existing v2 Consent builder.
   Keep fixture `NativeAttestation` and unmetered `SnapshotArea` unable
   to satisfy that candidate boundary. The handoff is read-only package
   evidence, with provenance tied to the registered owner and exact
   sampled stage. Queueing or publishing Consent belongs to its owner.

| Durable cut at cold start | Permitted next operation |
| --- | --- |
| Bound, clean family absent | Recheck exact charge, then exclusively allocate. |
| Bound, any file present, even an empty main | Retain charge and fence. The current inventory cannot prove that a pre-CAS file belongs to this allocation. |
| NativeStarted, clean exact inode with signed package | Run separately charged attestation and Prepared CAS. |
| NativeStarted, exact managed rollback candidate | Recover with the existing keyed recovery under this attached quota, then remeasure and require signed-only attestation. A cut during generation may roll back to unsigned or empty, which remains unavailable. |
| NativeStarted, missing, empty, unsigned, stale or invalid stage | Retain charge and fence without generation, finalization, replacement or cleanup. |
| Prepared, exact clean main | Re-attest and verify the same durable artifact if still current. Progressed custody or stale reservation follows the existing owner. |
| Stray or ambiguous snapshot, sidecars or failed cleanup | Retain charge and fence. Clean only the exact copy still witnessed by this turn. |

The package supplement specifies the first actor's recovery limit. Current
inventory provides no durable proof of a file created before NativeStarted,
so the actor's conditional resume is unreachable. After an interrupted
generation, only a complete signed and reverified package can advance.
An ambiguous snapshot or an empty/unsigned NativeStarted can therefore
leave the sole Unknown slot unavailable across restarts. A hard kill
while the copy exists can strand a valid signed original. A readiness
or deadline loss after MDK durably stages unsigned material can also
strand the slot, including when the signing callback refuses normally. This increment
adds no reclamation authority. The separate [schema prestage](peer-candidate-package-prestage.md)
adds a schema-only strand interval of up to 15 seconds between the
durable start and issue. A stop, crash or overrun may leave that sole
slot unavailable without creating a signed package. Incorporate these
limits explicitly when
composing the approved actor plan.

## Evidence before integration

On Linux AMD64 use one disposable profile, actual registered,
checked-default quota dispatcher, actual installed cursor and v2 signed
Offer, and real upstream SQLCipher MDK at the exact reviewed lock.
Run fresh original generation, cold restart and exact snapshot readback
over the same persistent files using a newly checked registration.
Assert registration before first SQL open, no network or native publish,
one original native generation and signing, zero migrations/signing on
readback, exact native event/digest and signer/endpoint, no native groups,
unchanged original through copy readback, caps, no journal residue and no
snapshot after success. Record bounded timings for issuance and
attestation independently. Repeat with actual cold cuts at Bound,
file-created, durable NativeStarted before first open, unsigned durable
state, signed before Prepared and Prepared. Use a separate process for
cold restart. Build the unsigned cut with a test-only injector absent
from candidate. Reuse the
[charged recovery harness](../validation/peer-receiver-charge-recovery.md)
for a real interrupted generation write leaving a managed DELETE
journal. Assert the recovered unsigned/empty image remains charged and
fenced, with no regeneration. Interrupt the actual bounded copy and
prove an ambiguous residue also fences cold while a checked live copy
can be cleaned. Reuse the
[clean-charge harness](../validation/peer-receiver-clean-charge.md)
for separate copy measurement. Check mismatched quota paths and owner,
replaced inode, wrong key, artifact mismatch, signed lifetime threshold
one second on either side before allocation, deadline/stop/readiness
loss including inside signing, and real cap/copy refusal. Count one
native generation and record its duration against the five-second
ceiling. Audit configured items and symbols that stock lacks the
coordinator and the listed candidate-only inventory, copy, managed
rollback and dispatch items. Verify no candidate witness can be
constructed from `bind_receiver`, `ProfileSlots`, fixture attestation,
withhold, finalize, direct signing fallback or fixture replacement.
Run normal stock and private candidate checks separately. This local prerequisite
does not qualify the installed actor, whose packaged test must still
show the entire authenticated flow.
