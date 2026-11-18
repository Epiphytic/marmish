# Peer heartbeat receive transitions

This is the first, standalone code seam in the reviewed [heartbeat activation
plan](peer-heartbeat-activation.md), increment 1. The existing private codec is
mounted by the offline `peer_status_wire` test target. This increment keeps
that scope. A separate increment needs the checkpoint schema, restore, durable
source disposition and accepted Peer owner before any production mounting.

## Scope and contract

Split the current receive path into read-only planning and an infallible
in-memory apply. The test entry point `Receiver::receive` keeps the same
signature and behavior by planning and immediately applying. Add a private
codec `plan_receive` that takes an exclusive receiver borrow, authenticates the
same caller-supplied app binding and record room, performs the same decode and
signed-time checks, and computes the exact outcome and changes without
modifying the receiver. Native provenance is still the caller's responsibility.

A prepared transition carries that exclusive borrow and owns its typed delta.
The caller can inspect its outcome and read its fields from the codec without
changing live state, then consume it on successful commit. Dropping the plan
does nothing. A failed prepare creates no plan. The exclusive borrow prevents
another plan, a sweep or a receiver replacement before consumption, and a
prepared transition cannot target a different receiver or be applied twice.
A future owner serializes its authority and native receipts independently.
This increment only guarantees local receiver exclusivity.

Keep the existing single source of truth for replay, ordering, conflicts,
stage admission, expiry, and receipt times. A working projection copies only
small current-permission metadata and borrows the existing stage, sample and
completed description. A stage delta either keeps or clears the stage, starts
one from its initial page, or appends one bounded page. Do not copy the
existing stage or completed description into a prepared transition. To finish
a multi-page description, verify the existing canonical encoding and digest
on one bounded contiguous assembly with the candidate page overlaid, and
prepare the verified replacement or poison. This bounded full copy is
allowed only when a page completes the description, since the existing
canonical decoder needs contiguous input. A single-page completion uses the
page directly. Defer allocating the live stage backing buffer for a new
nonfinal stage until apply.

Treat outcomes and deltas independently. An `Ignored` plan can clear a
timed-out stage and set its signed issue fence, and `Deferred` can advance the
ordinal or expire a stage before budget prevents a new reservation. Rejected
record, binding, and clock checks do not reach transition preparation.
Watermark can advance the seen sequence without replacing the fresh-display
sample. Keep peer checkpoint structures private to the codec boundary and
leave protocol and view behavior as existing tests specify. Keep current
retention sweep behavior using the same stage-expiry predicate. Preparing a
record does not change sweep policy.

## Validation

Run all existing status wire behavior tests with their original assertions.
Add deterministic tests inside the private receiver module so they can inspect
the prepared delta. Cover a discarded heartbeat plan and its original receipt
on retry, a discarded ignored expiry and its issue fence after commit, and a
discarded deferred newer ordinal followed by successful retry under its new
fence. Prove ordinary one-page append does not mutate, replace, or copy the old
stage buffer until apply, and dropping a fully assembled completion or poison
preserves the original reservation. Exercise prepare after a sweep and reuse of
an input after discard. Check exclusivity by attempting, in a disposable
negative compiler probe, to sweep the same receiver while retaining and later
applying its plan. The probe must specifically fail for the conflicting
receiver borrow. Validate formatting and strict Clippy for the offline test
target. No network or installed fixture is needed.

## Exact delta and equivalence

| Receiver field | Prepared mutation |
| --- | --- |
| Seen sequence and its first receipt | Keep or replace after a strictly higher sequence |
| Display sample | Keep or replace only for a fresh eligible heartbeat |
| Current permission | Keep or replace small ordinal/digest, issue fence and representation metadata |
| Stage | Keep, clear, start with one owned page or append one owned page |
| Completed permission | Keep, clear matching poisoned generation, replace with verified description and preserved same-generation heartbeat, or refresh just its heartbeat stamp |

Observe the generation before expiry for permissions, and perform later
representation and fence comparisons against the projected state. A fresh
heartbeat can advance the permission ordinal or poison a same-ordinal digest
conflict while its outcome remains Heartbeat. A stale watermark never observes
permission. Use a single read-only page classifier for duplicate, conflicting,
pending and completing pages. The apply path only writes the prepared page;
it does not classify it again. Transfer the decoded page buffer into the delta.
Resolve final length from the candidate when terminal and the stored length
otherwise. Release completion scratch before returning the owned delta.
Transient peak for completion can include the original bounded stage, one
bounded assembly and one bounded decoded description. A future owner must
budget this separately and serialize completion within each profile.

The plan is must-use and non-cloneable. It owns the delta, exposes the outcome,
whether any receiver field changes, and the projected stage reservation.
Apply consumes the plan, materializes new backing storage before any live
field assignment and needs no logical validation or fallible assertion. Stage
append may use a mutable borrow derived from the exclusive plan, so write
only to the classified in-bounds range; avoid fallible lookups or assertions.

## Additional deterministic cases

Capture a baseline golden transcript of outcomes and full receiver-state
fingerprints from the original in-place implementation before replacing it.
The committed test replays that transcript with a model alphabet of heartbeat
sequence, fresh vs watermark, matching, advancing and conflicting permission
ordinals/digests, permission expiry, budget, duplicates and conflicts. On each
step compare a snapshot before and after dropping a plan, compare prospective
reservation with the applied reservation, and verify the golden outcome and
state after applying. Derive case snapshots from bounded canonical fixture
fields, using synthetic inputs and no profile state. Do not keep duplicate
legacy transition logic in the codebase.

Cover heartbeat-driven ordinal advance, poison and same-generation refresh;
page conflict, oversize disagreement and oversize versus staged or completed
content; a chunk versus oversize; poison that preserves the older completed
cached generation; one-page admission with both budget results; and changed
versus no-op ignored, replay, duplicate and deferred outcomes. Exercise final
page overlay in terminal-last and out-of-order arrivals, digest/encoding
failures and same-generation heartbeat preservation. Add an assertion that a
plan is neither Clone nor Copy and show isolation of receivers with the same
binding. Record the disposable negative compiler probe in validation evidence.
