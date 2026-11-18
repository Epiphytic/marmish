# Capped recipient Welcome and joined readback

This test-only prerequisite adds one concrete checked adapter for the
installed v2 recipient custody owner. The exact companion design is
`a2745b93:docs/phases/peer-installed-recipient-pending.md`. The source
baseline `5f74aae` composes `b2cdf74` and the unmounted dispatcher,
subsequently published in `81b968b` with its design at
`docs/phases/peer-quota-unmounted.md`. Both use upstream MDK
`bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`. Keep legacy fixture
methods and distributed builds unchanged.

## Adapter and ownership

Use a private `CappedJoin` in a test-only child of
`peer_prepared_consent_owner.rs`. It owns the existing fixture
`ProfileSlots` on the same borrowed profile lease. The recipient owner
retains route, full configuration, policy, expiry, cancellation and the
checkpoint. The adapter takes that owner's `InvitationState`, durable
`HeldEvidence`, exact authenticated recovered `Delivery` and bounded
`TurnFence`. Expose the existing test-only `TurnFence::new` and
`TurnFence::wall` as `pub(in crate::peer_invitation)`; their private
clock and error types stay within that module. Add a crate-visible
`sample() -> std::result::Result<u64, StageError>` alongside `check()`
for the native Join seam. Reuse the existing wall checks and error
mapping. The exact owner mints a new fence for each synchronous native
operation from its wall/continuous clock and sticky stop. Give each
the existing five-second continuous ceiling and the earliest applicable
absolute deadline. Resample the same operation's fence at each cut,
and require fresh owner checks before each SQL CAS. A timeout after
JoinStarted stays charged and cannot rearm ingress.
Confirm the exact profile lease and slot owner, recovered signed
Delivery and carrier, package digest, binding and file witness before
touching native state. Use the already-verified wire version and exact
route. Derive the SQLCipher key and label from the stage. Provide three
operations with only sealed typed inputs and outputs:

- `prepare` verifies the exact authenticated unjoined package on a
  guarded copy and returns opaque, non-cloneable `PreparedJoin` with the
  exact slot owner, stage, carrier and closed original digest. It works only from Held.
  It grants no right to open or ingest on the original. The caller
  revalidates owner, route, policy and deadline and commits the one
  `JoinStarted` CAS afterwards.
- `join` consumes the unique non-cloneable `JoinStarted` and the
  prepared proof. Use the existing `JoinStarted::matches` against the
  pre-CAS Held observation. The token's owner Arc, exact slot owner and
  profile lease identity, and the current original path and file match
  the prepared proof. Compare the exact prepared digest and sample the
  fresh fence immediately before opening the original.
  Recheck the finite wall/continuous budget and stop inside the operation
  before calling MDK ingest. Capture the refreshed carried sample there.
  Return only a closed native result. A refused or uncertain result after
  JoinStarted remains charged and can never repeat this ingest.
- `read_joined` uses only authenticated current JoinStarted or Pending
  custody and a fresh fence. Reconstruct the existing `JoinedReadback`
  on a new guarded copy using the real persisted exact Welcome receipt and
  `OpenStage::read_joined`. Return a distinct non-cloneable
  `CappedJoinedReadback` only after resource closure, unchanged original
  and valid fresh owner fence. Its private constructor is in the adapter.
  The checkpoint can borrow its exact `JoinedReadback` through
  `proof()`. The successor Pending CAS accepts only the capped type;
  generic legacy `JoinedReadback` cannot authorize it. The owner
  independently revalidates current config, route and deadlines before
  Pending CAS. Recovery only reads and cannot repoll ingest, publish,
  mount a group or accept an invitation.

Do not pass a generic operation closure, raw path or charge through the
owner-facing adapter. Keep stock v1 fixture paths and C2 Leave unchanged.
The quota scope owns `peer_quota_dispatch.rs`, narrowly affected quota
tests, the new adapter and its mount, the private fenced Join seam in
`peer_stage_joined.rs`, and the fence visibility/sampling methods and
minimal adoption of the shared snapshot reservation in
`peer_terminal_leave_owner.rs`. This increment needs no change to
`peer_quota_vfs.rs`. The installed v2 owner may invoke this adapter in
its own test-only scope and owns the successor Pending CAS. Coordinate
shared mount and checkpoint references before edits.

## Native scope

Reuse the fixture's checked profile-local snapshot reservation and file
checks from the C2 Leave proof. Reuse its private, fixed, registered
`terminal-leave-snapshot.db` slot, distinct from both original stage slots. C2 and Join keep
distinct typed operations. All require the slot and its sidecars absent
before copying or opening the original for a Join. An exact
process-wide snapshot reservation covers observation, exclusive create,
the guarded native open, close and cleanup. Release it only after the
final fence. The same guard excludes overlapping original
Generate/Reopen/Join/Leave through their close checks. Contending or
reentrant operations fail without blocking. A poisoned or unwound
reservation fails closed and retains charge if safe cleanup cannot be
proved. Keep the guard separate from the SQLite turn lock. C2's existing
`inspect_slots` acquires and passes that typed reservation. Its
existing `leave_turn` and Generate/Reopen's `native_turn` take the same
exclusion before inspection or open. Preserve their existing serial
behavior and proof logic. Charge the complete bounded candidate extent
before exclusive create. Recheck
copy and original byte digest and device/inode against 0600 regular
one-link files on the same local device. Use the existing protected
sidecar fences, check each before and after, and require exact digest
on prepared copy success. A successfully joined copy may change during
hydration. Require its original source unchanged, same copy inode and
valid extent throughout. A stale path, failed cleanup or unmanaged
sidecar fences that scope. Never open a copy outside the registered VFS.

For each copy or original, hold one dispatcher turn with the aggregate
128 MiB cap charged to all other present reserved extents (main and
sidecars) and the fixture's external charge; keep the independent 16 MiB
closed main bound. The new typed scope uses exact stage and copy witness
and slot class. Do not use Generate/Reopen's older fixture scalar-only
admission for the new operations. Clear one scope before another starts.
Hold all handles within the non-Send synchronous dispatcher turn. Any
failed close check retains the existing quarantine behavior.

Reuse concrete `OpenStage::open_provisional` (DELETE journal, zero expected
migrations, hardened keying and in-memory temp store) and
`require_prepared_join` and `read_joined`. Add a narrow private
fenced Join next to `join_once`. Keep the existing signature and
caller behavior, delegating the shared native body internally. The
fenced Join samples immediately before ingest with the runtime already
open and passes that sample as its native receive time. Use
`resolve_once` for the concrete Welcome future and preserve the
deny-all `RefusingIo` with its unused proof. At the pinned MDK,
`AccountDeviceRuntime::ingest_delivery` reaches
`AccountDeviceRuntime::ingest_delivery_with_observer`, then session
ingest and effect publication in `marmot-session/src/runtime.rs`.
Audit the particular authenticated Welcome path, signer, routing and
SQLite opens for no detached work, auxiliary SQLite paths or callback
after return. Verify bundled worker limit zero, memory temp storage and
DELETE journal through the existing disposable harness probes and
locked source. A callback that requests egress is a failure even if the
adapter refused it. Unexpected Pending destroys the future and captured
runtime on the same thread inside the quota guard and yields no receipt.
No wait, spawn or retry within a managed scope.

## Qualification

Run each case in its own exact opt-in child process, including the C2
rerun. The VFS is registered once per process and fixes one profile.

- Drive exact real recipient Welcome from an encrypted prepared stage on
  Linux AMD64. Prove only one real native ingest, zero egress, closed
  original, then a separate capped copy and exact durable Welcome,
  founder, roster, group, epoch, local leaf and package coordinates.
  Repeat readback after a fresh native open with original bytes unchanged.
  Reject mismatched Delivery, stage generation or file identity and
  already consumed or swapped package. Assert the standalone adapter
  returns the distinct capped type and exposes only a borrowed legacy
  proof.
- Exercise an unjoined copy at the hard aggregate bound, then real MDK
  original-join write refusal using an accounted budget that allows
  opening the prepared main but prevents its transaction growth. Verify
  the specific managed VFS refusal, logical main/journal ceiling and no
  extra native effect. If the bounded budget misses the write, report
  that cut as inconclusive. Cover an occupied/displaced snapshot,
  original image changed between preflight and effect, exact deadline,
  and cancellation before open or mutation. Recovery after a durable
  JoinStarted without native proof remains inert.
- Deterministically force the one-poll Pending error in the private test
  path using a fixture-only future that owns a real managed handle.
  Verify synchronous destruction, cleared TLS and zero retained handles.
  Check attribution and closed behavior for success and negative cases,
  including no later managed open; retain ordinary VFS forwarding
  coverage and rerun the existing C2 snapshot use.
- Keep the source and opt-in proof review independent from full v2
  recipient integration. Existing real v1 fixture can prove this isolated
  adapter without claiming v2. The later composed gate must run the
  actual private WSS v2 Offer/Consent ACK/Delivery and restart flow
  through this same adapter to Pending. Audit the successor Pending
  signature for `&CappedJoinedReadback` and the successor owner for no
  calls to `inspect_joined`, `join_custody_stage` or `SnapshotArea`.
  Run focused tests, normal
  compilation and strict all-target Clippy with `installed-checks`.
  Report exact source, lock, timing, quota counters and Linux-only
  evidence. This phase keeps the installed registrar and receiver closed.
