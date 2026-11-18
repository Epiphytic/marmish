# Peer native leave C2 local effect and recovery

This increment follows the [Phase B design](peer-native-leave.md) and
[C1 hold](peer-native-leave-c1.md). The base is C1
`98a55dfda41863893a2451c41c1a5ae6aaf940d9` with upstream MDK
`122ad45ece73c1d6f687a4e92a541b9b2febde34`. Qualify an exact terminal
owner invoking native Leave once on a genuine joined Pending stage, and
cold recovery of the same durable native proposal. Every C2 outcome keeps
the stage Fenced and its reservation charged. The implementation and
harness stay under `cfg(test)`.

## The locked native contract

- `AccountDeviceRuntime::confirm_host_change_without_publish_with_audit_context`
  is a commit acceptance API and its `supports_deferred_commit_publish`
  excludes Leave. Use the existing public
  `send_host_change_with_audit_context(SendIntent::Leave, ...)` with a
  permanently closed publisher, scheduler and absent native group route.
  The group route in the signed Offer remains independently authenticated
  and is used only to validate the native event. An origin descriptor is
  local data and has no connector, relay registry, tasks or sockets.
- At the engine, `do_send_leave` calls
  `record_sent_openmls_message_with_leave_request`. The SQLite transaction
  writes the exact signed Sent message, unblocked LeaveRequest and own-echo
  marker, when that marker's ID differs. Runtime publication follows
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

Extend the private epoch-5 fixture exact inventory before any native use.
Reject the C1-only exact inventory for this new owner and reject partial
inventory, wrong scope and modified guards before writes. C1 scratch
profiles are disposable. Preserve the immutable historical DDL for
epochs 3 and 4 and their existing scoped behavior. A new guarded journal
can use the C1 hold key and an exact foreign key. Keep one journal row
per hold and at most the existing two hold slots.

| State | Durable evidence and permitted owner action |
| --- | --- |
| No C2 row | C1 hold exists. The exclusive owner can attempt its reservation once. |
| NativeArmed | Checked wall at reserve and exact C1 hold key. Only the original successful committing turn receives the non-cloneable, process-local start token. A cold owner may inspect and seal a qualifying retained proposal, or return unavailable. |
| Captured | Original reserve wall, checked capture wall, exact native and canonical signed event ID, SHA-256 of exact signed bytes, and the single bounded signed event. A cold owner may verify an exact readback. No more sends are possible. |

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

The owner uses the real disposable profile lease and current exact private
`Checkpoint` and source binding. It accepts only the selected terminal
and known current generation. Revalidate C1 Idle, Fenced, joined Pending,
authenticated stored Offer/Consent/Delivery and exact Welcome binding
without treating their historic enrollment expiry as a new authorization.
Validate account, founder, group, route, own leaf and original
device/inode under the existing checks. Default Decline/Expire and
unrelated admission, activation, publication and cleanup never build
this owner. Only one stage token is alive. Its serialization covers
checked reserve, native open/use/close, and checked capture. There is no
await from the live native handle to any other owner turn. Sample a fresh
qualified wall and verify the in-process stop and suspend-aware ceiling at
reservation, immediately before native dispatch, after close and before
capture. Use the clocks already available to the private owner where
possible. A missed gate after dispatch retains NativeArmed. Observing or
replaying metadata after the deadline cannot authorize a native operation.

Open mutable original storage only after a committed NativeArmed row or
exact Captured readback eligibility from that same row. Start with exact clean
one-link, mode-0600, same-device file, current path/generation and positive
size within the closed-stage 16 MiB cap, with no journal, WAL or SHM.
Use the existing test-only `ProfileSlots` managed dispatcher in a fresh
subprocess. Add a concrete private Leave operation to its closed scope,
with inert deny-only fixtures, rather than a generic scoped callback.
Maintain the current `128 MiB - charged other extents` cap over the
managed main and DELETE journal. Independently check the fixture's other
files and aggregate charges once per exclusive turn. Keep SQLite opens,
access and closure on the scoped calling thread and finish using
`resolve_once`. Unexpected Pending drops the runtime and leaves uncertainty.
Recheck the exact descriptor, clean sidecars and no live managed handles
after close. A different inode, unexpected sidecar, failed cap or any
indeterminate check prevents a seal and retains the charge. This increment
does not open a hot original or authorize an attempt reservation and
same-value header recovery. The existing guarded retirement stays closed
until a separate proof qualifies that operation.

## Native qualification and restart table

Use two disposable real SQLCipher accounts with genuine Peer Welcome
enrollment and a fresh private cursor under the same owner. For the
original runtime confirm stable current group and epoch authenticator,
exact founder/admin and non-admin recipient roster, local signing key and
own leaf index, exact Welcome provenance and no pending commit,
unrecoverable/disbanded/removed state, queued intent, unexpected fanout
or native LeaveRequest. Freeze the expected known fresh Sent inventory
for this fixture. A genuine deferred PostJoin obligation may remain and
must stay unexecuted. A wrong or extra group or unexplained work consumes
the start opportunity without calling Leave.

Check the pre-send disposition from the unrouted runtime, the retained
transaction's result and absence of every publisher/scheduler call,
queued intent, success report and fanout. The published-error wording
is corroborative evidence only. Durable evidence comes from exact native
readback: the same authority remains current and unchanged except for
the exact LeaveRequest and its Sent records. Require a single exact
current-epoch 64 KiB-bounded serialized native candidate and exact
signed event bytes of at most 64 KiB. Use the established OpenMLS
SelfRemove verifier and authenticated outer kind-445 Nostr event,
including exact event ID, signature, route, group, pinned recipient leaf
and credentials. If the distinct raw marker is present, require the same
content-derived ID and exact MLS bytes, never treat a raw marker as
the proposal. Refuse additional, wrong epoch or malformed records.
The proven native candidate is sealed only after all runtime handles
close and the same stage and terminal state are revalidated. Preserve it
in encrypted state, use redacted diagnostic outcomes, and keep all bytes,
paths and keys out of review logs.

| Interruption or result | Next exact owner turn |
| --- | --- |
| No committed reservation | Fresh C1 hold can attempt the reservation. Assert zero original native dispatch. |
| Armed with exact clean original and no durable native pair | Keep Armed and report unavailable after comparison. No cold start token. |
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
  generation and route, multiple or wrong-epoch proposal candidates,
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
  in-process test faults. Cover deadline-minus-one, exact equality and
  plus one without sleeps. Re-run existing unrouted Leave and C1
  selectors and strict target Clippy and scoped formatting.
- Record exact source and MDK lock, private Linux AMD64 proof and
  macOS ARM64 status. Run macOS when the reviewed native prerequisites
  are available. A passing local fixture on one platform and a clean
  reopen cannot qualify installed Peer or hot-journal recovery.
