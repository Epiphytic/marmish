# Candidate same-row Pending observation

This is the positive Pending leaf in section 4 of
[the approved Manual actor mount](peer-candidate-manual-actor-mount.md).
It uses the candidate's already bound Linux AMD64 recipient owner. The
implementation adds no caller to the daemon or listener. A later actor turn
owns the refresh cadence and stopped view lifecycle.

## Proof and handoff

1. The read-only associated entry in `PendingTurn` acquires an opaque,
   single-use producer ticket before borrowing `RecoveredOwner`. Even that
   borrow reads registered file and cursor evidence. The ticket pins the actor,
   listener and sequence, records both `Instant` and the existing
   suspend-aware continuous clock, and invalidates the former view before a
   fresh attempt. Clock failure or refusal leaves the view unavailable.
2. Borrow the existing attached quota and verify the existing cursor. Read the
   exact selected Manual v2 custody through the current recipient port.
   Require an unexpired Pending state before the native read. Reuse the
   bounded Joined readback under the existing native fence. The disposable
   charged copy and its cleanup may touch temporary files. The durable cursor
   and original native image are read only. The upstream rollback path is
   available only for JoinStarted, and this entry rejects that state before
   the copy. No Welcome poll, package operation, write CAS or relay send is
   needed for this projection.
3. Under the same owner, recheck the cursor and checked port; read the exact
   row and both persisted wall floors within one deferred read transaction.
   Require current Manual, known-false v2 policy, exact current signed
   Offer/Consent/Delivery ancestry, a nonterminal Pending row, same owner,
   stage, generation and saved image as before native readback, and exact
   native group, epoch, epoch identity and local leaf from the same charged
   Joined proof. Check the wall, Held time, Pending deadline, and native fence
   again. After the SQL callback, explicitly recheck the current port account,
   origin and wall before minting the projection. The existing callback checks
   on entry. A final sample below the proof wall or at/after the Pending
   deadline refuses. Share row and Joined comparisons with the existing replay
   branch where this retains its behavior.
4. Only the owner module can pair that result with its original ticket in a
   private non-Clone Pending observation. The view can consume the opaque
   pair and extract the redacted positive entry, verified wall and effective
   persisted floor. Only the existing pure tests construct arbitrary
   observations, including synthetic empty input. The production leaf cannot
   publish a healthy Empty. A current previously verified row can still be
   listed as expired by the existing exact-expiry mask while its proof is
   fresh.

## Time and local reads

At publication and at selection and final reply, both elapsed clocks must be
strictly below one second from the ticket's first sample. Missing, regressed or
stale samples fail closed and invalidate current production read evidence
where applicable. A wall before the verified observation or its effective
floor is unavailable. Keep test clock injection private. The mutex guards
only samples and memory. File, SQLite, native and relay checks run outside it.
The existing four-slot reader and socket deadline remain in effect.

## Validation

Exercise both independent clocks at the expiry boundary, including a
suspend-aware jump while Instant is fresh, backward sampling, and a late
selection/final reply. Check invalidation, generation mismatch, expiry and
the existing same-UID framing regressions. In the existing private
installed-Pending fixture, run positive exact observation and negatives for
a wrong selection/generation, wrong native coordinates and pre-Pending state,
with no new native Welcome polls, closed successes or committed custody
transitions. Check the exact redacted List/Show while the proof is fresh and
unavailability after a failed re-observation. The known single observation
can have a one-second lifetime even if a qualification budget lasts longer.
Healthy Empty still depends on a completed, current authenticated inventory
scan and is a separate actor mount obligation.
