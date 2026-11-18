# Peer native leave C1 durable hold

This is the integrated schema recheck required by the
[approved Phase B design](peer-native-leave.md) and its
[review](../reviews/peer-native-leave-design.md). The source examined is
`65e127bf7079debea9945ad9c04586357359a491`, including private WSS activation
R10 and upstream MDK `122ad45ece73c1d6f687a4e92a541b9b2febde34`. C1 adds a
test-owned encrypted journal for the terminal decision's bounded hold. It
qualifies durable fencing and recovery accounting without starting a native
Leave, reopening native storage, retiring held files, or allowing egress.

## Integrated storage

- In `peer_pending_custody_schema.rs`, add a `#[cfg(test)]` fixture epoch kind and
  exact expected inventory. The existing custody epoch-3 marker and M5
  publication epoch-4 marker remain historical layers with unchanged DDL
  and row checks. A new immutable epoch-5 marker and exact inventory
  distinguish the composed schema. The dedicated owner may install the layers
  atomically from an exact supported Prepared Consent inventory, or reopen an
  exact epoch-5 inventory. Reject epoch 3, epoch 4, incomplete or modified
  inventories, wrong account or origin and weakened hardening before any
  installation writes. Existing epoch-specific owners still reject inventories
  outside their contracts. Check all guards are restored and all rows match
  before the install commits.
- Keep `Epoch::is_publication()` true for the composed fixture. It controls
  the actual custody Active projection and admission to M5 checkpoint
  operations. Run the unmodified M5 checks for publication, Active, founder
  enrollment and acceptance on epoch 5. Current M5 permits publications only
  for exact Activating custody and rejects any corresponding terminal row.
  An epoch-5 terminal hold must remain in exact joined Pending custody. Never
  add a synthetic Active row, permit, receipt, activation state or generic
  publish API to support it.
- Mount `peer_terminal_leave_schema.rs` and its checkpoint under
  `#[cfg(test)]`. The only hold-table columns are `class`, `generation`,
  `origin`, `account`, `founder`, `offer_id`, `offer_revision`,
  `consent_carrier`, `consent_revision`, `delivery_carrier`,
  `delivery_digest`, `welcome_id`, `welcome_digest`, `device`, `inode`,
  `group_id`, `epoch_number`, `epoch_identity`, `own_leaf`, `route`,
  `hold_deadline`, `recovery_state`, `recovery_attempts`,
  `recovery_reserved_at` and `recovery_not_before`. An exact foreign key
  to the terminal primary key `(class,generation)` and uniqueness by class
  bound the journal to two held stages. The insert trigger enforces a Fenced
  parent, exact joined custody, the initial recovery coordinates and the
  deadline equality. Check `consent_revision` and `welcome_id` against
  joined custody because the terminal table does not carry them. The stage
  descriptor follows the owner's existing binding and path rules. Reuse the
  historical signed Offer verification in `Context::exact_consent` to check
  the route and verify it against the stored value on each exact live reader.
  SQL can independently enforce the byte shapes and equality of all direct
  row coordinates. Signed route and hashed Delivery verification remain in
  the owner layer. Do not treat an arbitrary stored route as authenticated.
  Verify live association, scope, immutable values, recovery state/count/floor
  and signed wall bounds. Preserve the existing stage, cursor and release
  charges. Disallow direct INSERT, UPDATE and DELETE by installed deny
  triggers. The owner opens narrowly scoped apertures only inside checked
  IMMEDIATE transactions, restores them before verification and rolls back
  the whole change on error. There is no independent C1 prune of a held
  row. Future charged-to-released and replay-prune ownership must preserve
  the parent terminal journal and its retention until the same exact prune
  transaction. C1 has no permit, candidate, proposal, commit or native
  evidence columns. Later integrated proof can replace this provisional
  fixture schema with a fresh exact inventory. Old scratch databases fail
  the inventory check and are disposable. No migration or reuse of an
  earlier database is authorized.
- Marker and guard mismatch is storage failure. A supplied or old owner
  witness is not durable authority. Re-read exact rows and compare owner
  identity, selector, source, generation and full previous state before
  every mutation. Produce only opaque local observations and recovery
  reservations. A cold reopen must reacquire the profile lease and bind the
  exact origin to obtain fresh observations. A read must not grant egress.

## Terminal decision and scheduling

- In `peer_pending_terminal_checkpoint.rs`, reuse the existing verified
  Pending Decline/Expire transaction and local authorization callback. A
  separate private, explicit epoch-5 fixture decision selects the hold.
  Default Decline/Expire creates no hold at any epoch. Once the original
  decision exists it preserves reason, scope and hold choice. A repeat can
  read the existing result but cannot add a hold retroactively or change
  deadline. Create terminal row and hold together after the exact authorized
  decision and before commit, using checked conversion to positive
  SQLite seconds. Require SQL and Rust equality
  `hold_deadline = decided_at + 86400` and `hold_deadline > decided_at`.
  Use both global and exact origin wall floors. The first phase is empty of
  proposal evidence. A local reservation or parked row is never proof of
  membership removal.
- The epoch-5 terminal progress trigger forbids unconditional Fenced to
  Intent or Blocked when a hold exists, including after expiry while an
  ordinary path lacks exact recovery/sidecar authority. A Rust writer checks
  the same rule. Leave the existing files-removed, release and prune
  transition rules in force for no-hold decisions. Factor the terminal DDL
  generation only enough to add the epoch-5 condition without changing
  any prior expected SQL text. The read-only retirement eligibility recheck
  belongs immediately after `terminal_check` at the filesystem owner's
  entry, before opening a retirement descriptor or collecting a witness.
  An ordinary retire operation receives no write aperture for a held stage.
- Use checked `wall < deadline` for live native eligibility and
  `wall >= deadline` for the later Unconfirmed path. At epoch 5,
  `terminal_cleanup_due` excludes every held row using a correlated
  `NOT EXISTS` on class/generation in `WHERE` before the existing decision
  time/class ordering and limit. The same exclusion applies before, at and
  after the deadline and regardless of recovery state. No C1 held row
  becomes selectable for physical cleanup on expiry alone. An expired
  hold can be found by an exact journal read, but remains charged because
  no native/sidecar proof has been produced. This is intentionally a
  test-only intermediate state. The subsequent approved owner increments
  admit an expired row to selection and the established Unconfirmed Intent
  writer only after exact descriptor and sidecar verification. They also add the
  distinct guarded Removed outcome. C1 does not change the meaning or code
  of existing outcome zero. A detected exact-file mismatch does not open a
  C1 Blocked aperture for held rows.
- The local recovery substate is Idle, Reserved or Parked with no filesystem
  actions in C1. Idle has zero attempts, no reservation timestamp and a
  floor of `decided_at`. Separate from the write-denial guard, a
  `BEFORE UPDATE` trigger makes all binding coordinates and the deadline
  immutable. It permits Idle to Reserved for attempt one or Reserved to
  Reserved for attempts two and three, increasing the attempts by exactly
  one and requiring the new reservation time at or past the previous floor.
  The first reservation sets its floor to reservation wall + 1 second,
  the second to wall + 2 seconds, and the third keeps the previous floor
  because no fourth reservation exists. The first two changes strictly
  advance the floor. The third grant spends attempt three, and the next
  Parked transition preserves that count and floor. A qualified journal
  transaction commits each reservation or park. An interrupted first or second
  reservation can be spent and retried at or past the floor; an unresolved
  third becomes Parked on the next qualified turn. The only park transition
  is from Reserved with attempts=3 and unchanged recovery coordinates.
  A Parked row is immutable in C1 and retains its charge.
- The private recovery due query selects only scoped account/origin, Fenced
  terminal rows joined to an exact hold, with Idle/Reserved and floor at
  or before the sampled wall. Filter out Parked and future floors in
  `WHERE` before ordering by `decided_at,class` and limiting to one.
  Include an unresolved third as a due park turn. Exact re-read and floor
  comparison decides the one reservation or park; either spends the
  owner's single bounded turn. Use an explicit exact-selection lookup for
  future parked recovery, with no C1 success writer or way to reset the
  budget. Applied to a future fixture owner, a verified stage can complete
  the same reservation and any allowed explicit recovery using the approved
  sealed proof. That owner must recheck recovery/native inactivity before
  file retirement. At/after expiry any recovery remains local, and the
  only future outcome is Unconfirmed.

## Ownership and validation

- Own `peer_pending_custody_schema.rs` and its inventory tests,
  the mount in `peer_pending_custody_checkpoint.rs`,
  `peer_pending_terminal_schema.rs`, `peer_pending_terminal_checkpoint.rs`,
  and new test-only `peer_terminal_leave_{schema,checkpoint,tests}.rs`.
  Mount only from the existing private custody module. Own the single
  retirement-entry precheck in `peer_pending_terminal_retirement.rs` and
  private fixture constructors/selector plumbing where necessary in
  `peer_prepared_consent_owner.rs`, `peer_activation_checkpoint.rs`
  and the existing fixture modules.
  Avoid changes to M5 activation/receipt tables, WSS scopes and gates,
  Command/Terminal code, installed Peer config/daemon/IPC/CLI, transport,
  quotas, target dependencies and CI. The private fixture may use already
  reviewed M5 transport only to test M5 regression, independently of the
  held scope.
- Run deterministic checks of atomic fresh install and cold reopen, older
  and partial schema rejection with table and row state preserved, mutated
  guards and wrong account/origin, all denied write apertures, and forced
  rollback between terminal and hold inserts. Verify immutable re-reads,
  stale local owner and generation, genuine original signed carrier and
  route, arithmetic overflow, deadline minus one/equal/plus one,
  regression against global and origin wall floors, physical-retirement
  entry rejection for a held row, and SQL rejection even if the terminal
  progress guard is temporarily opened by the owner. Exercise hold-row
  immutability and the independent attempt/floor trigger with the hold
  UPDATE aperture temporarily opened. Use both bounded classes to show a
  live or expired hold cannot starve an unrelated no-hold retirement or a
  new Pending expiry. The existing terminal worker returns after cleanup,
  so run these two alternatives in independent bounded turns, each with
  the held row older than the competing row.
- Test the exact 1 s/2 s backoff, interruption and cold reopen after each
  reservation, third stop and attempted fourth, parked wrong-selector
  recovery, a parked class-0 hold yielding to due class 1, direct SQL
  tampering, and retained charge across expiry and restart. Use
  deterministic private SQLCipher fixtures, no sleeping and no
  assumption that a journal row proves a filesystem recovery occurred.
- Drive existing Phase A no-egress decline/expire and M5 exact publication
  tests through selected epoch-5 fixtures with no hold. Keep epoch-specific
  rejection tests and older fixture paths. Default CI stays on the bounded
  deterministic suite. Any same-build WSS confirmation remains explicitly
  opt-in, with single-root ownership of warm targets and separate resource
  qualification. Record exact source/lock, tests and independent code
  review before completing C1. Actual local SQLite sidecar recovery,
  persisted native proposal/commit evidence, eligible deadline fallback
  and Removed outcomes remain under the parent Phase B acceptance.
