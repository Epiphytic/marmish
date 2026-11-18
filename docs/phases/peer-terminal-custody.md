# Decline and expire joined Pending Peer invitations

This increment adds a durable terminal decision and custody-aware retirement
without native or relay egress. It uses the existing owner and supervised
test-only daemon path. Native SelfRemove and installed enablement each need
the subsequent reviewed design and proof. Keep these decisions separate
from the ordinary Command and Terminal resources.

## Custody epoch and decision

Bump the checked custody overlay from epoch 2 to epoch 3. Initialize only on a
fresh profile or an exact Prepared Consent inventory with no custody overlay.
Reject the existing epoch 2 or a partial/unknown inventory without changing the
database. No installed custody format has shipped. Add exact inventory entries
for a `peer_pending_terminal` table, its constraints, guarded writes, and the
existing custody transition trigger's terminal exclusion. A terminal-insert trigger requires a matching state-2 custody
row and exact owner coordinates. The terminal table has one unique `(class,generation)` and its own bounded storage. It owns the
immutable account, server, origin, Offer identity and revision, Delivery
carrier and Welcome digest, protected stage device and inode, group and epoch
identity, local leaf, decision reason and wall, and retained replay coordinates. Take coordinates from the current same-owner
custody, Prepared Consent, and stage rows in one transaction. The table
survives their deletion.

Keep ordinary custody values Held, JoinStarted, Pending and Activating.
Under one IMMEDIATE transaction insert a terminal row only from the exact
Pending row and its matching live reservation. The custody transition
trigger refuses an update for a generation that has a terminal row. The
ordinary custody read, Manual and Automatic decision queries, and daemon
inventory/due queries also exclude terminal rows, including after reopen.
Thus either Approve or Terminal insertion can win, and neither can reverse
the other's result. Repeat the same selection using a typed terminal lookup;
return the committed reason without touching native state. Expired
Activating never enters this path. It stays closed to native activity
under its existing deadline fence and the activation owner independently
owns its eventual retirement.

Reuse same-UID, one-use local decision authority for Decline, including
the local owner, selection, queue incarnation, caller and deadline.
Check request abandonment immediately before CAS. A failed or lost reply
after a possible commit requires exact readback just as Approve does.
Expiry uses a bounded due query for Pending where `pending_deadline<=wall`,
with no terminal row. It cannot be initiated by IPC. Decline requires
`wall<pending_deadline`: the witness cannot choose the reason after expiry.
Never use the broader signed expiry instead of the custody deadline.
Both paths sample with the role-checked `PreparedConsentOwner::sample()`.
Read the existing consent version and exact cursor-origin wall floors
through `rows::check_wall` and the scoped `check_origin_wall` read. Advance
both through the existing guarded writes in the same write transaction
without checking availability or quarantine. Clock rollback refuses
both decisions and cleanup. Preserve the existing signed replay floor and
the in-use reservation until exact retirement. This no-egress increment
has no additional leave-reconcile deadline.

## Terminal read and custody-aware retirement

Terminal attach always checks the local profile lock, account and schema.
Do not call the admitting `read_custody`, `current_custody` or
`reopen_custodied_delivery` on terminal cleanup. Typed terminal read uses the
persisted immutable coordinates, encrypted metadata and the exact ownership
witness without requiring live invitation policy, held deadline, cursor or
relay readiness. Never treat the stored Delivery as a new admission.
Restart after deadline, turning Off, blocking the founder, or losing the
relay still permits safe local cleanup. These conditions grant no native
egress. Skip cleanup on an ownership or local-clock fault and retain budget.

Mount an epoch-3 joined-custody journal directly from the custody attachment.
Use the terminal row's guarded `Fenced` to `Intent` to `FilesRemoved`
to `Released` progress and immutable outcome `Unconfirmed` from Intent.
A demonstrated witness mismatch may move Fenced or Intent to absorbing
`Blocked` while retaining the charged slot. Ordinary I/O interruption
retains the exact existing progress and retries. Define and validate the
exact guards for terminal insert, progress and release. Until Released,
custody, invitation, outbox and stage rows retain their original valid
shape and references. Freeze the exact stage row against changes below
Released. The checked custody overlay verifies these live pairs plus
Released rows with no custody association. A Released row permits a
higher-generation reservation after applicable cooldown.

Build a custody-specific runner under the custody attach. The existing
pre-join fixture attachment and intent routines remain separate. Share
the `RetirementDir::{open, capture, unlink, sync_removed, ensure_absent}`
private-inode, descriptor-relative and directory-sync primitives. Match the
terminal witness to the retained same-generation stage row before writing
Intent and immediately before any filesystem effect. No SQL transaction
spans I/O. Permit proceeding from a previously committed exact Intent and
verified absence, including interruption after unlink or directory sync.
A replacement inode, sidecar or ambiguous directory cannot count as
absence, cannot be deleted, and cannot release the reservation. Verify
no stage runtime is attached or otherwise able to recreate files.

Write Intent with Unconfirmed before attempting cleanup. This increment
can take the same safe fallback immediately because there is no native
reconcile owner. After durable directory sync record FilesRemoved, then
verify absence and release in one IMMEDIATE transaction. Delete exactly
the matched custody row, then its Prepared Consent row, then the stage and invitation rows in the order required by their foreign
keys, then clear the exact provisioning reservation through its original
guarded generation update. Within the same transaction lift only the
explicit custody DELETE, Prepared Consent outbox DELETE, stage DELETE and
terminal release guards and restore them before commit. Restore and check
the exact inventory and affected association counts. A failed predicate or mismatched
count rolls back the entire release and leaves the slot charged. Keep
the terminal reason and Unconfirmed outcome, and match an exact
retirement reread after possible commit. Never claim physical MLS
removal in this increment. Reuse the existing `peer_consent_fence` and
`peer_retirement_cooldown` stores and their checked schema helpers.
For epoch 3, extract and share only the existing cooldown DDL, write guards
and `Writers::Cooldown`. Exclude the separate retirement epoch, intent
schema and replacement provisioning trigger. Install the shared cooldown
entries in the custody inventory and use the exact writer at Decline.
Check cooldown in both Offer admission and provisional reservation whenever
the custody epoch-3 or tested retirement overlay is attached. Make the
custody path available without a test-only conditional at these call sites.
Expiry applies no explicit-decline cooldown. File cleanup is independent
of cooldown. No separate cooldown or replay mechanism.
Keep terminal tombstones within the existing bounded retirement history
policy and retain each through at least the original replay expiry. A
full protected history rejects new decisions without evicting active or
replay-protected rows.

## Validation

Test Manual same-UID Decline and timed exact expiry through the current
owner. Include the approval/expiry and approval/decline races, pending
boundary, restart after held deadline, block and Off after the terminal
decision, closed epoch 2, wrong account/origin/generation and abandoned or
lost replies. Native access counters and fixture transcripts must show
zero new egress or native work. Expired Activating and expired unjoined
rows stay out of the terminal due path. Inject restart after decision,
Intent, unlink, FilesRemoved and release. Exercise wrong inode, missing
file before and after Intent, sidecar, mismatch to Blocked with retained slot, bounded
histories and exact slot release. A new Offer from a declined inviter
during cooldown yields SkippedCooldown. Direct provisioning returns
Cooldown under the same authority. Add only fast regressions to core CI. Record independent Claude
Code Auto code approval and publish the reviewed increment before native
leave or production onboarding.

## Subsequent native leave design

The follow-on phase will use the existing typed SelfRemove and unrouted
leave proofs, the public exact retained message lookup, and the isolated
exact Proposal permit. It must independently reconcile pending versus
frozen native state, require an inventory containing only the verified
proposal before each broad native re-drive, and qualify the actual
founder's matching removal commit. The intended stable removal predicate
is `removed && !is_member` with no disband or unrecoverable state,
observed on the current authoritative branch before and after reopen.
Restoration before cleanup invalidates the observation. A disband
observation is separate. Native ACK cannot set a terminal removal result
and terminal native cleanup can delete the retained work. The follow-on
must get its own detailed approval before altering this no-egress path.
