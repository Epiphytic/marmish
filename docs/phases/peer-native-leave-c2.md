# Peer native leave C2 local effect and recovery

This increment follows the [Phase B design](peer-native-leave.md) and
[C1 hold](peer-native-leave-c1.md). The base is C1
`98a55dfda41863893a2451c41c1a5ae6aaf940d9` with upstream MDK
`122ad45ece73c1d6f687a4e92a541b9b2febde34`. Qualify an exact terminal
owner invoking native Leave once on a genuine joined Pending stage, and
cold recovery of the same durable native proposal. Every C2 outcome keeps
the stage Fenced and its reservation charged. The implementation and
harness stay under `cfg(test)`. Recheck the exact runtime and bundled
SQLCipher contracts on the integrated upstream master lock before code
qualification.

## The locked native contract

- `AccountDeviceRuntime::confirm_host_change_without_publish_with_audit_context`
  is a commit acceptance API and its `supports_deferred_commit_publish`
  excludes Leave. Use the existing public
  `send_host_change_with_audit_context(SendIntent::Leave, ...)` with a
  existing `JoinRuntime = AccountDeviceRuntime<RefusingIo,
  StaticTransportRouting, RefusingIo>` construction in
  `peer_stage_joined.rs`, with no group route and `require_unused()`.
  The signed Offer's route and endpoint descriptor authenticate the
  verifier's expected target. The runtime gets no connector, relay
  registry, transport task or socket.
- At the engine, `do_send_leave` calls
  `record_sent_openmls_message_with_leave_request`. The SQLite transaction
  writes the exact signed Sent message, unblocked LeaveRequest and distinct
  own-echo marker together. Native OpenMLS preparation precedes that
  transaction and may modify its storage. An absent committed pair cannot
  prove that no native mutation occurred. Runtime publication follows
  after native acceptance. The [unrouted proof](../validation/peer-pending-unrouted-leave-proof.md)
  observed one missing route before any transport call and recovered the
  same retained message. A transport-shaped error or an interrupted
  future can therefore coexist with a committed native Leave.
- `MessageStorage::list_messages_in_states` uses an epoch lower bound and
  materializes all matching records before returning. Initial selection
  is permitted only on this known fresh single-group fixture with the
  exclusive owner and bounded native storage. Check the complete returned
  inventory, exact epoch and limits, including any marker. This phase
  does not qualify discovery on an installed history or claim the API
  imposes a SQL row or byte limit. Recovery after a seal uses the stored ID
  with native `get_message` and compares the same authenticated evidence.

## Durable decisions and owner boundary

Add a separate test owner for exact epoch 6. Install the immutable C1
epoch-5 layers and the epoch-6 layer atomically from the already supported
exact Prepared Consent inventory, or reopen exact epoch 6. Retain the old
C1 exact opener and all earlier expected guard SQL and fixture checks.
The C2 owner rejects standalone C1, partial inventory, wrong scope and
modified guards before writes. The new `peer_terminal_leave_effect`
table is the only C2 state. Key it by the exact C1 hold foreign key with
one row per hold and at most the existing two hold slots.

| State | Durable evidence and permitted owner action |
| --- | --- |
| No C2 row | C1 hold exists. The exclusive owner can attempt its reservation once. |
| NativeArmed | Reserve wall and SHA-256 of the exact clean original image whose guarded snapshot proved stable authority and no prior LeaveRequest or Sent rows. Bound to the exact C1 hold key. Only the original committing turn receives a non-cloneable, process-local start token. Cold inspection can seal exact retained proof or report indeterminate. |
| Captured | The immutable reserve coordinates, capture wall, canonical signed event ID (also the exact native SignedOpenMlsWire ID), SHA-256 of exact signed bytes, and the single bounded signed event. A cold owner may verify an exact readback. |

Use closed INSERT, UPDATE and DELETE guards with checked single-row
apertures equivalent to C1. SQL fixes the scope by the C1 foreign key,
requires an exact Fenced terminal parent, initial idle recovery state,
a checked time strictly below the unchanged deadline, and exact shape
and immutability for armed and captured data. Preserve root cursor and
per-origin wall floor semantics by checking and advancing them in the
same IMMEDIATE transaction as reservation and capture. The owner reads
exact C1 and custody evidence and compares the identical local generation,
source, prior row and current checked clock on transitions. Saved values
cannot mint a start token. If commit or its reply is uncertain, continue
only through the cold disposition. Consuming token once creates one
native dispatch opportunity in its owner turn. A second call, cold reopen,
stop, invalid clock, owner mismatch or failed gate cannot create another.
C1 recovery attempts serve only their existing filesystem accounting
purpose. Decline/Expire replay cannot retroactively add a hold or change
its state.

The local-only `LeaveOwner` in `peer_terminal_leave_owner.rs` opens
through the existing `Paths`, hardened cursor and `ProfileLease`.
Add a narrow epoch-6 test constructor for the existing
`PreparedConsentOwner` and use the same `InvitationState` fields, source
and wall gates. Keep it separate from the generic WSS opener and its
normal recovery work. The genuine Pending stage is one of the two fixed
`peer/provisional-{known,unknown}.db` paths, with the existing nonzero,
monotone stage generation. Require C1 Idle, Fenced, joined Pending,
authenticated stored Offer/Consent/Delivery and exact Welcome binding.
Their historic enrollment expiry does not serve as new authorization.
Default Decline/Expire, admission, activation and cleanup cannot build
this owner. Its sole stage token serializes the full turn. Cursor
transactions and new wall checks run outside the VFS thread-local scope.
Only owned proof data crosses from native to checkpoint; no SQLite
handle, runtime, pending future or caller callback escapes.

Use `PreparedConsentOwner::available_sample` plus the C1 local and
exact-origin wall floors. Each turn also has a process-local,
suspend-inclusive monotonic ceiling using Linux `CLOCK_BOOTTIME` and
macOS `mach_continuous_time` as in the native sampling proof. Clamp a
five-second test-owner turn to the remaining wall hold. Missing,
regressed, stopped or expired readings deny progress. Inject both clock
readings in deterministic tests. Check the wall and ceiling before
reserve, before native open, immediately before Leave and after the
scope closes before capture. The within-scope check uses only in-memory
wall/monotonic/stop reads, never opens the cursor. If wall or stop crosses
after dispatch, keep Armed. The engine's `LeaveRequest.requested_at_ms`
is its local convergence monotonic counter, not UNIX wall, and cannot
prove correlation across restart. The baseline and exclusive one-call
scope supply that correlation. Do not use
`AccountDeviceRuntime::with_maintenance_sources` as an engine clock.

### Guarded inspection and the one original dispatch

Use the existing `SnapshotArea::with_snapshot` copy discipline for
both the pre-reserve check and every post-dispatch or cold inspection.
This includes a digest-pinned original, distinct one-link mode-0600
copy, capped actual copy, no sidecars and recheck of both identities
and original digest after use. MDK readback can hydrate and write even
on an apparently clean stage. Only the one dispatch ever hands the
original to MDK. The checked reservation binds the pre-reserve image
digest. Recheck that digest immediately before opening the original,
under the same exclusive lease and with no alternate original writer.
Do not compare the post-dispatch image to the pre-reserve digest to
infer equality of logical state. Compare exact native authority and
expected inventories on a new guarded snapshot instead.

Mount `peer_quota_dispatch::mount_for_proof` before SQLite opens in
each C2 owner subprocess. It opens only a genuine prebuilt Pending
profile from the local fixture setup. Use current managed `ProfileSlots`
rules for the original and one concrete, private C2 snapshot scope.
The snapshot gets an exact reserved fixture-only filename and its own
file witness under the same protected profile. It cannot use either
durable provisional slot. Count the original and all occupied extents
outside the active scope once. Apply the same combined main + DELETE
journal cap of `128 MiB - charged other extents` to the snapshot's
native use and retain the 16 MiB closed main cap. Debit the bounded
copy before creating it and give no other operation a concurrent
allowance. Keep the copy and any sidecars charged if exact checked
cleanup fails. The snapshot scope does not alter any durable slot
generation or usage. No new general-purpose quota callback or path
allowlist. A previously abandoned snapshot obstructs another turn
until exact fixture cleanup and does not authorize touching the
original.

The runtime uses `OpenStage::open_provisional` (explicit DELETE and
existing-only), with zero pending migrations, current profile key,
signer, slot label, generation and file witness. Verify the stage
mode, positive size, one link, same device and clean sidecars before
the open. Add concrete private scope operations for Leave and
snapshot inspection with their own bounded result types.
`ProfileSlots::native_turn` currently accepts only Generate/Reopen and
returns `(ProvisionalPackage, bool)`. Keep this interface unchanged.
Use `resolve_once` with the existing scoped calling thread for the
new operations. It must be proven under the exact lock that the native
Leave and verifier futures complete in one poll with local signer and
`RefusingIo`. Unexpected Pending drops future and handles in scope
and leaves the phase indeterminate. The first implementation must
prove that property before proceeding; if it fails, retain closed
behavior and report the blocker. Do not switch to an unscoped executor
or turn on network. After every managed call check the default VFS,
no live handles, exact stage identity and sidecars. A different
inode, unexpected sidecar, exceeded cap or uncertain close prevents
a seal and retains charge. This increment does not reopen a hot
original or perform the separate C1-accounted sidecar repair.

## Native qualification and restart table

In `peer_terminal_leave_native_tests.rs` reuse the offline setup in
`peer_postjoin_proof_tests.rs`: signed Offer and Consent with a local
fixture ACK, founder-side `peer_enrollment_founder` using the exact
native KeyPackage and route, genuine signed Delivery passed through
`receive_custody_record`, and `advance_custody` to sealed Pending.
Close enrollment before the dedicated owner subprocess. Verify held
device, inode, group, full epoch and own leaf against that original.
Fixtures supply no trusted fabricated custody/native evidence. On the
pre-reserve snapshot and again on the hydrated dispatch runtime
confirm stable current group and epoch authenticator,
exact founder/admin and non-admin recipient roster, local signing key and
own leaf index, exact Welcome provenance and no pending commit,
unrecoverable/disbanded/removed state, queued intent, unexpected fanout
or native LeaveRequest. Require empty matching-or-later Sent inventory and no LeaveRequest
before reserving, then repeat immediately before Leave. A genuine
deferred PostJoin obligation may remain. Its exact native record must
remain unchanged and no work from it may be executed. Refuse wrong
or extra groups and unexplained work. A post-reserve failure leaves
the one opportunity spent without calling Leave.

Check that the unrouted result has exactly one matching local
failure ID, without relying on the human-readable reason for authority.
Require zero publisher/scheduler calls, queued intent, success report
and fanout. The returned result alone cannot establish either native
commit or relay exposure. On a guarded snapshot prove exact stable
authority and the matching unblocked current-epoch LeaveRequest. Require
exactly one SignedOpenMlsWire Sent and one distinct RawTransport Sent
own-echo marker, with no other matching-or-later records. Apply all
three size limits, 262,144 bytes for the encoded signed native record,
65,536 bytes for the signed Nostr event and 65,536 bytes for inner MLS.
Refuse extra data, malformed encoding, wrong epoch, IDs and routes.
Recompute the marker ID from SHA-256 of the exact verified inner MLS
and require the same bytes, group, epoch and Sent state. A raw marker
cannot stand in for the proposal. After sealing, use the two expected
native IDs for exact indexed lookups and retain full owner authority
and bounded-work checks.

Refactor the established test-only SelfRemove verifier itself to
accept the closed runtime type and exact Offer endpoint descriptor.
Reuse its OpenMLS query-only snapshot and exact account, local
credential, signer, leaf, group and epoch checks, plus its signed
outer kind-445 Nostr ID, signature and route checks. Keep assertion
and benchmarking wrappers outside the reusable verifier. The exact
candidate is sealed only after snapshot/runtime handles close and the
same original image, stage and terminal state are revalidated.
Captured means the locally retained native proposal was authenticated.
Armed without the proven pair means possibly mutated and requires
future disposition. Neither state evidences delivery or removal.
Keep carrier bytes in encrypted state and exclude them, paths, keys
and peer identities from diagnostics.

| Interruption or result | Next exact owner turn |
| --- | --- |
| No committed reservation | Fresh C1 hold can attempt the reservation. Assert zero original native dispatch. |
| Armed with a sidecar-free original and no durable native pair | Keep Armed and report indeterminate, including possible preparation before the record transaction. No cold start token. |
| Native completed before local seal | Reconstruct the exact native proposal from the same original SQLCipher image, strong native facts and C1 binding. Seal once without another Leave. |
| Armed with corrupt, partial, wrong stage or unexpected native coordinates | Refuse and keep Fenced and charged. No fall-through to ordinary storage, renewal or cleanup. |
| Journal present, quota indeterminate or guarded close fails | Refuse without reopening the hot original, replacing or unlinking files, spending recovery attempts, or relaxing admission. Keep Fenced and charged. |
| Seal committed before reply | Recheck the exact ID and native evidence with the original stage before the live hold deadline. Return the same capture observation. No resend or second journal row. |
| Deadline reached, clock regressed, owner stopped or profile replaced | No native operation in that turn. Preserve the phase and charge. |

Future phases own local hot-journal repair, bounded deny-only native
fanout freeze, one-use egress, founder convergence, sealed recipient
ingress, Removed and Unconfirmed Intent, and physical release. C2 never
seeds fanout, resumes native outbound fanouts, acknowledges publication,
drives maintenance, accepts a founder commit, records removal or releases
a file. An opt-in initially local evidence suite requires no WSS, public
relay, endpoint deployment, installed Peer wiring, CI selection change or
production dependency changes.

## Validation and ownership

- Keep edits to private Leave C2 owner, exact checkpoint/inventory and
  fixtures, and narrowly scoped native verifier and test-only quota turn.
  Reuse existing C1, native authority and [quota dispatcher](peer-quota-dispatch.md)
  checks where possible. Coordination is required before shared inventory
  changes. No concrete file should expose generic MDK runtime access.
- Fast SQL tests cover exact fresh/reopen inventory, old and tampered
  checks, direct writes denied, false and stale C1 witnesses, both walls,
  floor regression, a stopped turn, duplicate authorization, rollback
  before reserve commit, and failed capture rollback. Assert unchanged
  C1 recovery attempts, physical-retirement protection, and no dangling
  row or second send grant. Regression-test no-hold terminal behavior.
- In opt-in native tests drive a genuine decline or expiry and exact
  one-turn native operation. Cover stable wrong account, founder, leaf,
  generation and route, missing, additional or wrong-epoch records,
  malformed bounded bytes and marker, native return with no sealed result,
  quota refusal and replacement. Assert no outbound adapter or scheduler
  calls and no native fanout, application effects or published reports.
  Assert a separate unopened attempt when stop or expiry lands before
  dispatch and explicit retained uncertainty when it lands afterward.
- Use subprocess cuts with fresh owners and no retained Rust tokens at
  three committed boundaries: armed before the original open, native
  transaction complete and stage closed before seal, and seal committed
  before result. Reopen exact profile and original file, validate the
  cold disposition, unchanged charge, same event and no additional
  Leave. Inject a real or fixture-controlled journal and confirm
  byte-preserving refusal before original open. Distinguish post-commit
  child exit from a process stopped mid-transaction and from ordinary
  in-process test faults. Exercise a reserved attempt that stops at a
  positively observed scoped journal write using the existing quota
  fault child plumbing. If the cut is during hydration, state that
  precisely. Separately exercise an Armed snapshot with no durable
  pair despite potential native preparation. Do not call that state
  pristine. Cover deadline-minus-one, exact equality and plus one
  without sleeps. Re-run existing unrouted Leave and C1
  selectors and strict target Clippy and scoped formatting.
- Record exact source and MDK lock, private Linux AMD64 proof and
  macOS ARM64 status. Run macOS when the reviewed native prerequisites
  are available. A passing local fixture on one platform and a clean
  reopen cannot qualify installed Peer or hot-journal recovery.
