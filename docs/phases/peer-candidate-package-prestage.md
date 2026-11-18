# Metered schema prestaging for Manual KeyPackage issuance

This amendment to [registered Manual KeyPackage issue](peer-candidate-managed-package.md)
preserves its five-second original issue, signer and snapshot-attestation
windows. On the Linux AMD64 candidate, actual first open of a fresh metered
SQLCipher DELETE image took 5.689 seconds before the signer was reached.
Optimizing the bundled SQLite C crate alone gave 5.663 seconds and the
same closed refusal. An instrumented run attributed 6.563 seconds of a
6.61-second issue turn to 189 migrations. Upstream MDK bdf9833e applies historical migrations
in the first keyed open before giving the caller a usable handle. The
candidate needs one separate schema preparation turn on the actual
charged image.

- Continue to allocate exactly one empty private original and commit
  `NativeStarted` with its exact inode before any keyed original open.
  Preserve the current policy, reservation, source, registered profile lease,
  member, quota, route/grant, both floors and signed/qualification expiry
  checks. Do not add a durable state, twin original, copied schema, stock
  allocator, alternate dispatcher or recovery-token reconstruction.
- In the same live owner, consume the non-Clone `StartedPackage` in a
  separate `prestage_package` turn. Use a distinct opaque schema-only
  window with the same current Ready, stop Arc, route cancellation token
  and anchored live authority. Its exclusive deadline is the minimum of
  15 seconds and the unchanged signed and qualification wall ceilings.
  This gives just over twice the measured longer schema migration run.
  This type cannot be
  passed to issue, attest or the publisher. Use the existing measured
  `ReceiverFiles` original turn, zero positive busy wait, exact no-follow
  checked parent/inode, full durable SQLCipher DELETE settings and caps.
  Admit only the zero-length original with no sidecars. Open via the
  public MDK exact keyed storage API and apply the catalogue schema.
  The schema aperture supplies the local database key, fixed file, with no signer, runtime, transport or lifecycle write.
  Verify no groups, no owned KeyPackages and absent slot lifecycle.
  Drop every open handle, verify same regular/private nonempty inode
  and clean sidecar-free family under existing measure, and check the
  schema window. The wall is an admission and completion fence.
  A synchronous SQLite step already entered can return after expiry.
  Keep the file charged and withhold the completion witness on such
  a return. No filesystem preemption is assumed.
- Successful completion checks the signed Offer ceiling again and moves
  the exact existing one-use `NativeStart`
  into a private non-Clone `PrestagedPackage` bound to the same durable
  reservation, started inode, owner, measured closed extent and exact encrypted digest. Preserve the prior sampled clock
  floor and authority incarnation at handoff. Rechecking an elapsed
  original signed/qualification bound or changed Ready/cancellation
  cannot produce a fresh issue window. Only the immediate same-owner
  serviced turn may consume the witness, after entering the existing
  five-second issue window. Both successor windows derive authority and
  clock floor from their checked predecessor, including the original
  wall and route/Ready/stop incarnation. The issue turn uses the same
  wall bound across the handoff gap and rechecks the Offer ceiling. Under fresh measured original
  exclusion, require the same prepared inode, measured extent and exact encrypted digest,
  no sidecars, exact same durable `NativeStarted`, exact owner/route and
  current grant. Cold-open with the existing no-migrations check,
  verify the empty native purpose and perform the existing single
  fenced local lifecycle/signing operation. Keep all signed bytes and
  Prepared authority behind the existing independent snapshot proof.
- If a crash, drift or stop occurs during prestage or between prestage
  and issue, release no witness and keep the charged durable
  `NativeStarted`. Cold recovery may apply only the existing exact
  DELETE-journal recovery and signed-only snapshot attestation.
  A schema-only, partial or empty original cannot authorize another
  generation or Consent. No dangling sidecar or wrong-key recovery
  may advance. Retain the original sign/close/crash cuts.
- Qualification uses the actual cold Linux candidate binary, default
  full SQLCipher durability, current MDK and independent subprocess
  fixtures. Use distinct schema bootstrap, signing issue and signed readback
  entry points. Schema preparation must complete within 15 seconds.
  On the representative cold Linux candidate, require issue from the
  migrated closed image through signed readback to finish within 4.5
  seconds to preserve 10% headroom under the five-second gate. If it
  misses this acceptance sample, keep the candidate closed and revisit
  the model without extending the signer boundary. Record fresh
  schema-open and migration time separately
  from the cold keyed no-migration reopen, checked issue and attestation.
  Exercise no sign/fallback on early/later stop, wrong phase, changed
  inode or unrelated window, after successful prestage with the
  witness dropped, and a real SQLite DELETE interruption. A successful
  uninterrupted issue must produce one signed local artefact and
  cold attestation must reproduce it with zero new signing. Keep the
  release-default and current ordinary VFS path unchanged.
