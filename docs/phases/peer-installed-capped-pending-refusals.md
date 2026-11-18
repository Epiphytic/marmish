# Capped Pending refusal evidence

This addendum to [the capped Pending bridge design](peer-installed-capped-pending-bridge.md)
addresses independent review `e3c8b15e` using accounting source `7d725d0`.
It remains opt-in Linux AMD64. Reuse the three existing private NIP-42
WSS scenarios, one opt-in uncertain variant of the same topology, and
the independently signed fixture used by
`peer_installed_custody_reopens_signed_prepared_consent_and_held_delivery`.
No installed runtime mount, permission, or wire change.

For every runtime case measure `Effects::read/assert_since` or the same
thread-local counters around the named guard, and require the stated error.
Counters track actual MDK Welcome future polls, closed successful native
turns, and committed changes by the installed Start/Pending CAS only.
Verify exact prior custody after a refusal when the guard allows it.
For legacy entry points also compare durable rows because no installed
CAS counter observes those entry points. Preserve the existing relay
publication-attempt delta across the same WSS windows.

In this plan `CheckpointError` denotes `consent_checkpoint::Error`,
`CursorError` denotes `store::Error`, and unqualified `Error` denotes
`peer_invitation::Error`.

The refusal author owns local fixture access. Expose only the private
read-only `current_installed_delivery` call under the existing
Linux AMD64 test configuration. A narrow test adapter within
`v2_pending` checks `Enrollment::from_delivery` with the signed pair
constructed by the existing store test. Use focused test methods on
`PendingReceiver` for the legacy Start check, exact saved epoch edit, and
the one-shot loss. Do not expose its checkpoint or owner to the WSS test.

Both persisted field edits run only on fixtures. In one immediate
transaction read the original SQL for `peer_custody_next` and
`peer_custody_deny_UPDATE` from `sqlite_schema`, drop those exact
triggers, update the named field for one exact Delivery carrier, restore
both SQL definitions, and commit only if exactly one row changed.
`Epoch::verify` rechecks the exact inventory on every custody read, so
restoring both triggers before the normal guard is essential. The
operation must leave schema inventory unchanged and fail atomically.
The independently signed store test uses the invalid stored frame for
its last refusal. The native WSS child uses the changed epoch for its
last check on that disposable profile.

## Local guards

- **Legacy or uncapped authority.** Call `start_custody_join` on the same
  real installed Held and require `CheckpointError::Layout`, zero polls, and exact
  retained custody. The first checks in `commit_custody_pending` and
  `commit_custody_activating` also refuse installed layout before using a
  legacy `JoinedReadback`. Cover their refusal dynamically only if the
  existing legacy test already has this sealed witness to lend. The
  installed checkpoint takes distinct types with private constructors and
  a same-quota pointer guard. The by-value installed token cannot be
  cast from a legacy/uncapped witness. The existing WSS
  `peer_installed_delivery_recovers_after_lost_ack` refuses a signed v1
  Offer. A v1 Held cannot reach `Enrollment::from_delivery`, where missing
  negotiation is also explicitly refused.
- **Exact signed ancestry and v2 negotiation.** Reuse the real signed
  Held pair built by `peer_installed_custody_reopens_signed_prepared_consent_and_held_delivery`.
  Verify that pair yields v2 negotiation and build a separately signed
  competing Delivery from its same Consent and Welcome, changing signed
  Delivery expiry or timestamp so the signed revision differs. Open it
  using the same codec and saved Consent, assert the changed negotiation
  revision, and pass it with the original Held to
  `Enrollment::from_delivery` (test child of `v2_pending`). Require
  `Error::Binding` and zero effects. This covers the mismatched signed pair;
  the exact guard may fail earlier on the distinct signed carrier. The
  same store fixture can change the retained signed frame with
  the exact fixture edit above; the checked reopen must
  refuse lexical/signature drift before any native work. Existing exact
  Offer ancestry/drift tests and WSS competing-carrier and signed swapped
  KeyPackage reference tests cover independently verified inbound ancestry.
  A changed selector is only row selection evidence (`Error::Missing`).
- **Policy and deadline.** Reopen the same signed Held cursor as
  `ReceivePolicy::AutoAny`, using the existing `open_custody` fixture.
  Call `current_installed_delivery` and require `CheckpointError::Policy`. Open
  as `Off` and require `CheckpointError::Policy` at `read_custody` if reachable.
  This tests Manual policy; physical role eligibility is independently
  covered by `receiver_eligibility_reads_only_its_lease_config`, which
  refuses server-only and changed lease/config. Read one genuine owner-local Held at its real wall before testing the
  read-only `current_installed_delivery` call: `deadline - 1` accepts with the known
  signed validity, exact deadline returns `CheckpointError::Expired`, both measured
  separately if wall and signed bounds permit. The continuous fence uses
  `TurnFence::new` with its existing fixture wall/ticks. Never pass a
  synthetic future wall to `read_custody`, `read_installed_join_custody`,
  `start_installed_join`, `commit_installed_pending` or any cursor writer.
  Even `read_custody` advances the persisted wall floor.

## Real Held/Joined guards and cut

- **Foreign lease, owner or family.** With the existing real Held, call
  `PendingReceiver::open` with another same-UID lease and the original
  quota and require `Error::Path`, without registering another quota.
  Rename the original stage file aside, put a distinct inode at its path,
  run regular `arm` and require a typed pre-effect refusal at the checked family/extent boundary.
  Determine whether the unclassifiable family is refused as `Error::Path`
  during inspection or `Error::StageIdentityMismatch` at the exact inode
  lookup from the observed guard. A wrong variant is not evidence that the
  desired guard ran. Restore the original by rename. For old
  checkpoint authority, use the pre-effect scenario where one exact
  `ArmedWelcome` already exists. Drop receiver #1, reopen receiver #2 on
  the same profile and call normal `ingest` with the stale token. Require
  `Error::Binding` at `JoinStarted::matches_installed_turn`, before the
  normal Welcome poll. The preexisting wrong-generation and retry checks
  retain their separate zero observations.
- **Physical retirement.** At the end of an existing child after the other
  cases, retire its actual physical port via `stop` or change a held scope
  file and trigger ordinary recheck. Invoke the normal bridge read/arm
  on the now-retired port and require `Error::Checkpoint(CheckpointError::Source)` with zero effects.
  The pure Manual policy refusal above separately covers the admission
  predicate. The unmounted proof does not promise lifecycle role-loss
  detection by a future daemon.
- **Native roster, epoch and package.** On final cold Pending replay
  first observe exact positive readback. With the disposable profile
  about to be discarded, use the exact fixture edit above to change only
  `peer_pending_custody.epoch_identity` for the exact carrier. Run the
  ordinary checked Pending readback and require
  `Error::Checkpoint(CheckpointError::Cursor(CursorError::Stale))` at the
  retained-vs-native
  coordinates compare with zero effects. Verify it did not rewrite the
  changed expectation. If storage validation blocks before that compare,
  it is a different test and must be reported as such. The first
  JoinStarted readback learns its epoch from authenticated native
  storage. The shared roster/admin guard in
  `peer_stage_joined.rs::joined_authority` is exercised positively by the
  same native readback. Exercise a negative third-member or wrong-admin
  group only if the existing fixture can reach the guard, or record the
  negative by inspection. The signed swapped KeyPackage WSS ingress
  case and unjoined cold readbacks are executable. The latter fail before
  the consumed-bit check. A joined image with only native `consumed_at`
  cleared on a disposable SQLCipher copy is an optional focused negative
  if the existing native storage setter is available. A genuine reached
  check must return `StageError::Invalid`, not `Unavailable`.
- **Uncertain after actual Start and one actual Welcome poll.** Add one
  one-shot thread-local fixture flag in `peer_receiver_v2_pending.rs`.
  Consume it after `files.finish`, unpacking the exact successful
  native result and rechecking the physical port, before returning that
  result. Its only change is inside `Ok((result, metrics))`. Replace
  inner `Ok(())` with `Err(Error::Checkpoint(CheckpointError::Source))`
  and keep the real native metrics. Permit only the new WSS scenario to
  arm that fixture flag and confirm it is consumed. Keep the existing
  `joined` success-return scenario untouched. Add one separate ignored
  `uncertain` variant of the same WSS topology, sharing the same
  `qualify` and `replay` branches as `joined`.
  Assert `(1, 1, 0)` for actual polls, closed successes and custody CAS in
  the injected loss window, the exact `JoinStarted` row and rearm refusal
  (`Error::Binding`, zero new effects). Subsequent processes see no old
  token or result and qualify only if independent cold verification
  matches the exact founder coordinates. The effect is deliberately
  ambiguous to the custody caller. This does not model MDK returning
  Pending or VFS close failing. A measured one poll with zero closed
  success alone would not establish that nothing persisted.

Managed rollback is qualified independently by
`managed_recovery_hot_journal_and_stock_wal`, source `6a8846ec` and
[measured evidence](../validation/peer-receiver-charge-recovery.md).
It executes journal-sync/main-write interrupted native SQLite and keyed
playback with the same managed quota/owner primitives. The clean WSS
process cuts do not establish hot-journal recovery.

The refusal author owns narrow test-only WSS and existing signed-store
fixture additions, local children of the bridge/checkpoint as needed,
and the uncertainty seam. The sibling's accounting measurements stay
unchanged. Use the existing warm exclusive target when available. Review
the exact composed source and measured evidence independently before
counting this proof.

## Design review

Claude Code Auto approved `b2d2ec9995d6b44446115dd8dcc8625f735f7f05`
in session `e67e5b3b-e4b1-4817-ae05-dcbbf38724b1`. The final review
checked unchanged trigger inventory, exact nested errors, real counters,
owner access and read-only synthetic clock use. The first two reviews
requested changes in sessions `421dadc0-0134-429d-b796-325593253f75`
and `3205e458-4c06-4c13-a757-a21a37a81e79`.
