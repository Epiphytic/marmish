# Candidate same-row Pending observation

This is the positive Pending leaf in section 4 of
[the approved Manual actor mount](peer-candidate-manual-actor-mount.md).
It uses the candidate's already bound Linux AMD64 recipient owner. This
increment adds no caller to the daemon or listener. The actor mount owns
the refresh cadence and stopped view lifecycle.

## Proof and handoff

1. Add an associated `PendingTurn::observe_pending(owner, lease, port,
   selector, generation, clock, producer) -> Result<()>` in
   `peer_receiver_v2_pending.rs`. Pass the already open borrowed
   `RecoveredOwner`, exact borrowed `ProfileLease`, checked
   `PeerRecipientPort`, `PendingClock` and sole view `Producer`.
   First acquire a non-advancing `Producer::read_ticket()`. Even
   `PendingTurn::borrow` reads registered file and cursor evidence, so it
   follows the ticket. The ticket pins actor, listener and sequence and
   captures `Instant` and `command_limits::continuous_time()`. The
   old proof remains readable while the synchronous read-only refresh
   runs. On refusal, `Producer::refuse(ticket)` invalidates only that
   still-current ticket's generation and sequence. Clock failure at
   ticket creation similarly invalidates the current proof. A stopped or
   displaced producer cannot invalidate a newer proof.
2. Borrow the existing attached quota and verify the existing cursor. Read
   exact selected Manual v2 custody through the port. Require unexpired
   Pending before the native read. Run existing `PendingClock::run`
   with the Pending deadline and reuse its bounded Joined readback. The
   disposable charged copy and its cleanup may touch temporary files. The
   durable cursor and original native image are read only. Existing native
   proof compares the original digest before and after processing the copy
   under this same exclusive owner. It does not persist a digest in the
   Pending row. The rollback branch requires JoinStarted, which this entry
   rejects before the copy. No Welcome poll, package operation, write CAS
   or relay send is needed for this projection.
3. Recheck the cursor and checked port. In the new
   `Checkpoint::observe_installed_pending` in
   `peer_installed_pending_checkpoint.rs`, use one deferred transaction
   for the exact row, both persisted wall floors and completeness of this
   single-entry List. The effective floor is the maximum of the
   `peer_consent_version.wall` singleton and the exact
   `peer_cursor_origin.wall`. Prove current Manual known-false v2 policy,
   exact signed Offer/Consent/Delivery ancestry, same checkpoint owner,
   stage, generation and entire saved SQL row, and the exact native group,
   epoch, epoch identity and local leaf from the same charged Joined
   proof. Conservatively refuse if a second Pending row appears. This
   defends the one-entry List even though the current installed actor
   admits only one unknown Manual slot. A terminalized row other than
   the selected row can conservatively count toward this refusal. The
   existing replay can share exact row lookup and native coordinate
   comparison without changing its transitions.
4. After the SQL callback, explicitly recheck the current port account,
   origin and wall. The callback itself checks only on entry. Use the
   latest final checked port wall for `verified_wall`, requiring it to
   be at least the SQL proof wall, at or above the saved Held timestamp
   and both floors, and below the Pending deadline. Recheck the cursor
   witness and sample the existing native fence at the end of
   `PendingClock::run` after SQL and the final port check. File, SQL,
   route and native work all remain outside the view mutex.
5. On success only `peer_receiver_v2_pending.rs` assembles the private,
   non-Clone `PendingObservation` from the original ticket and the
   exact positive result. Expose a narrow type re-export in
   `peer_invitation.rs` and the consuming `into_parts` accessor for
   `Producer::publish_pending(proof)` in `peer_pending_read_view.rs`.
   The projection maps the exact server, Offer ID, Offer revision and
   generation to the selection, saved Held timestamp to `held_at`
   and Pending deadline to `expires_at`. Only the pure test path can
   construct a synthetic empty observation or publish raw inputs. The
   existing expiry mask can omit an expired row while its prior proof
   is still fresh. This loss of a row does not prove Healthy Empty.

## Time and local reads

At publication and at both local selection and final reply, elapsed time
on both clocks must be strictly below one second from the original ticket
sample. Missing, regressed or stale clocks fail closed. A client deadline
alone does not invalidate shared state. The port samples effective route
wall and the native fence also checks its continuous clock. The reader
samples raw system wall and refuses it below the verified wall or effective
floor. A raw wall behind the projected route wall can temporarily make the
view unavailable. The mutex guards only samples and memory. The existing
four-slot reader and socket deadlines remain.

## Validation

Exercise each clock independently at the strict boundary, including a
suspend-aware jump while Instant is fresh, backward and missing samples
and late final reply. Show that a prior proof remains readable during a
refresh and that stale/foreign refusal cannot invalidate a newer proof.
Retain expiry and same-UID regressions. Cover the single-row guard with
two candidate Pending class values in a disposable bounded SQL fixture.
In the existing private installed-Pending relay scenario, run one
positive same-port Pending observation using actual
`PendingClock::Candidate`, borrowing the Ready/stop setup already
used by the managed Consent WSS test. Exercise wrong selection or
generation, wrong native coordinates and pre-Pending state, with no new
native Welcome polls, closed successes or committed custody transitions.
Use an isolated fixture hook at the final-fence cut when necessary to
exercise revoked native readiness or stop during the successful readback
path. Check exact redacted List/Show while fresh and unavailability on
refusal. Candidate strict compilation must exclude all raw publication
and empty-observation fixtures. Healthy Empty continues to depend on a
completed, current authenticated inventory scan.

Claude Code Auto (Opus) approved exact `9c7d7b7` in session
`4b8c28a8-41a2-4d86-a385-cb5a63cdfbad`.
