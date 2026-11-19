# Serve the private Manual actor on its checked connection

This is the scheduler and mount supplement to the [approved actor design](peer-candidate-manual-actor-mount.md). It combines the [idle handoff](peer-candidate-manual-handoff.md), guarded ingress, [measured expiry retirement](peer-candidate-expired-delivery.md), and [sealed Pending observation](../validation/peer-candidate-pending-observation.md). The reviewed integrated crate tree is `5b423ba1121fba8ab5c3636fcfe032249c0c204a`.
Its published source-identical ancestor `e6e2661` is in this design's history.
Implement from the commit containing this design. The
[guarded service design](peer-candidate-manual-service.md) supplies effect fences.
This supplement supersedes its Offer order, the selection order, and the
expired-Delivery disposition. The two reviews of guarded ingress and expiry
already cover the reconciled Delivery wake and test seams. The installed controller in [the acceptance plan](peer-candidate-manual-installed-proof.md) remains the externally observed test. Stock and Passive retain their current paths. Only the explicit private Manual example makes the new actor reachable. The first service can publish Pending. Healthy Empty, approval and Auto need their own approved mounts.

## Exact owner and shutdown

Keep open and initial readiness under the existing 15-second startup budget from the second readiness edge. After the authenticated port is open and that budget is checked, move its existing actor-local borrowed owner and the sole Producer into the Manual branch of the caught future. No extra thread, relay, cursor, key, slot or lease opens from the branch. Passive stays on its current idle loop. Only startup calls the readiness budget or fence deadline. Update the private test constructors to supply the optional stop cell. Share the existing cancellation token for asynchronous waits and an actor-issued stop cell for synchronous guard callbacks. Only the Manual worker carries an actor-issued `Arc<AtomicBool>`; Passive needs only the existing token. Both Actor stop and Drop set the cell with Release before cancelling. Give the worker an RAII stop guard that sets the same cell on any exit or unwind. When the loop observes Fatal or Ready stopped, set the cell before returning. A synchronous check uses Acquire and checks the readiness watch and Fatal independently; ready loss itself does not write the cell; the actor exit guard writes after classification. The cell and watch handed to every native turn come from that actor, not from an adapter-created substitute. Each iteration independently checks forwarding even after a local error. A suspected default dispatcher displacement rechecks and trips the registered shared Fatal through the existing path.

Keep every in-flight port operation owned until its existing deadline, except when permanent stop or Fatal discards it and the exact port is never reused. No detached tasks own exchanges, synchronous file work, or transaction leases. A view-drop guard closes the read side before raw exact route revocation and reap. Keep the actor and ordinary lease alive until actual join. Every route, cursor, history or live ingress error permanently stops the Peer generation and closes the view. No managed, retry, or projection work runs after ingress fails. Add a Manual exit classifier that checks forwarding and gives registered Fatal
priority. With no Fatal, a set synchronous stop cell or cancelled token is
`Stopped`, followed by a stopped readiness watch as `Stopped`. Other local
failures are `LocalUnavailable`. Recheck Fatal while handing the result to
`final_exit`. Test all four branches including a set cell before token
cancellation and Fatal with all three stop signals set. The ordinary owner may continue after a local error with verified forwarding. There is no reconnect in this candidate. Uncertain signed effects honor durable ownership and recovery rather than initiating a second attempt.

## Work classes and selection

On this authenticated port call `Ingress::recover` once, with the exact owner, stop, readiness and forwarding authority. Then rotate among history, live feed, saved Delivery, managed work and view maintenance. Admit at most one invocation of each work class per pass of the wheel. An incoming backlog may not restart at the first class. A fresh recovery uses the guarded `ScanStart::Recovery` transition with a real initial cursor. Assert that in the fresh owner fixture. No view is published during recovery, including while the native work inside a page is running. One history invocation drains up to four buffered live records, commits one authenticated page of up to four records, and can incur a separate five-second measured read per relevant record plus a five-second exchange and bounded SQL work. The admitted budget for those portions is 45 seconds. Synchronous file and SQL operations can return later, so the actor holds the existing resources and refuses late effects. Stop choosing history after its exact final-page commit. A new recovery is fatal to the old view; this candidate has no second in-process recovery.

The live arm calls `live_turn` for at most one record. When work is pending, pass a 20 ms probe deadline from the current monotonic
clock. When idle, pass the next view tick, or the next 500 ms rotor check if
history is incomplete. Ingress further clamps either deadline to its 500 ms
live limit and the current physical horizon. The idle read is admitted for at
most 20 ms per work-bearing pass, exclusive of a measured record. The actor's history turns use the existing five-second page limit computed
inside `history_turn` after the buffered drain. Make the outer deadline
optional so the actor can pass `None` and the existing timeout fixtures can
still pass `Some(deadline)`. Both paths honor the current route horizon.
The actor never clamps a page by a rotor or maintenance tick. A page timeout stops the
generation. A same-founder cursor `Backlog` also stops it permanently. The scheduler never drops an ingress future to satisfy a maintenance tick. Only permanent stop or Fatal may discard it with the entire port and generation. A measured live record can outlast the idle deadline. Only the port can classify timeout as an idle read. Page failure cannot become completion.

Use proven exact reads for the next due selectors. The existing test-only `daemon_due` is founder-specific and includes Auto, so it cannot choose candidate work. Reuse its verified query predicates and coordinate decoder where the schema matches. Derive an installed-owned due selector from the single Unknown charge for active stage, outbox, Held, JoinStarted or Pending. Check source and verified wall, exact nonterminal custody and current stage generation, and honor retry and signed deadlines. The fresh received-Offer choice orders currently eligible unreserved invitations by exact canonical server bytes, Offer ID and Offer revision. Existing rows record no local arrival sequence. Neither signed time, nullable decision time nor SQLite rowid supplies one. Use the existing bounded lifetime, quotas and cooldowns without promising first-arrival ordering. If there is a charged slot, an ambiguous image or a failed proof, fresh provisioning is ineligible. Revalidate selection in each effect's existing aperture. A selection read lends coordinates only. It cannot create a Reservation, a signed package, or a joined witness. The saved-Delivery visit uses the exact two-step work described below. Apply the following closed retirement table after the Manual exit check.

| Retirement | Rotor action and connection |
| --- | --- |
| `Retired` | Preserve the package charge, proceed with the current port and grant no Pending witness. |
| `Reobserve` | Drop the stale coordinates and keep the current port. Observe again at the next rotor visit. |
| `Transient` | Keep the exact row and port. Defer this class until both the next rotor visit and the next 500 ms period boundary. |
| `Stop` | Close the view and stop the Peer generation through the Manual exit classifier. |

Authoritative corruption, refusal of forwarding, or a lost current route stops the Peer generation. The next cycle serves other classes even if the same transient row remains the due head. Never delete a charged original to advance the scheduler.

Read selectors under `Checkpoint::read_installed_consent` on the current
physical recipient aperture. Compare the full single class charge and stage
journal. SQL selection leaves both floors unchanged for Manual, Pending and saved-Delivery probes. Verify the durable schema
and both retained floors, and preserve `Fenced` and `JournalCandidate` as
ineligible for fresh work. Re-read the exact signed Offer through
`installed_live_offer` before creating its native window. Revalidate its
cutoff and the reservation in the corresponding effect aperture. An expired,
ACKed, deferred, or otherwise ineligible occupied charge cannot expose a
fresh Offer. Preserve the existing exact one-Pending check in the Pending
selector and revalidate during sealed observation.

Use the separate stage apertures and timers in `RecoveredOwner` for Bound, NativeStarted and Prepared. Advance at most one applicable work branch per managed visit. Keep no token across rotor iterations. Bound to newly signed then attested is one owned work invocation, with one schema interval of up to 15 s and two native windows of up to 5 s for signing and attestation. Clamp every interval by the signed cutoff and current physical route. Fresh reservation and queueing have separately checked short SQL turns. Rules for the turn's interfaces follow.

| Turn | Scoped input and output |
| --- | --- |
| Pick managed work | `RecoveredOwner::due_manual_work(&mut self, &mut PeerRecipientPort) -> Result<Option<ManualDue>>`. Its closed output is `Provision(Selector)`, `Resume(Selector)`, `Publish(Selector)`, `Held(Selector,u64)`, or `JoinStarted(Selector,u64)`. Select exact open outbox, reservation, or custody with the same origin, nonterminal generation, physical lease and due deadline; the SQL aperture accepts the checked wall and source only. |
| Saved Delivery | Use a read-only `probe_due_managed_delivery` returning exact coordinates for the same bounded oldest-due row, ordered by `retry_at,id`. It returns no frame or native authority and leaves both wall floors unchanged. Use the existing installed source, scope and cursor checks. The coordinate includes origin, ID, revision and record digest. Once the rotor selects this visit, invalidate the view before acquisition. The guarded `due_managed_delivery` must compare the precise observed coordinates while re-querying that due head, then acquire the exact snapshot. A gone, stale or replaced head cannot advance either floor or admit a second row on this visit. It follows the stale `Reobserve` disposition. |
| Fresh provisioning | Derive `Reservation` from `checkpoint.reserve_provisioning(source, selector, wall)` within the checked recipient aperture. Check exact eligible Unknown slot and current invitation again. `start_package(quota, &reservation, &window)` yields a single `StartedPackage` only for Bound. |
| Recovery of Bound | `checkpoint.recover_provisioning(source, selector, wall)` under the same checked aperture. Use the existing allocation and exact file checks only, with no reissue from NativeStarted. |
| Schema and first issuance | Consume `StartedPackage` at `prestage_package(quota, &reservation, started)` to get `PrestagedPackage` with its signed-clamped issue window. `issue_package(quota, &reservation, prestaged)` consumes it once to get `Artifact`. An interruption after binding remains NativeStarted. |
| Attestation and queue | `attest_package(quota, &reservation, live_artifact, &window)` obtains a sealed `PackageProof` from a measured copy and compares the optional live artifact. NativeStarted recovery uses `None` and requires its already durably signed package; missing or unsigned state remains charged. Prepared recovery requires the recorded artifact to agree. Queue through `queue_managed_consent(quota, &reservation, &proof, &window)` under the existing measured aperture. |
| Held ingest and JoinStarted readback | `PendingTurn::borrow(owner, lease)` borrows the same actor lease for its existing lifetime and rechecks the quota. Consume `ArmedWelcome` within the Held visit, then drop it. Qualify from readback within a separate current visit after the durable effect. |
| Pending selector | `RecoveredOwner::current_manual_pending(&mut self, &mut PeerRecipientPort) -> Result<Option<(Selector,u64)>>` reads only exact same-source Pending coordinates under the port's checked wall and a read-only transaction. It lends no native witness or commit authority. |

Keep `&mut owner` and `&mut port` borrows local to one selected invocation. The current-thread actor awaits at most one turn. `wait_live` and Consent can hold exclusive port access across their bounded awaits. Drop borrowed measurements and retained aperture scopes before beginning another turn. The parent actor carries the long-lived lease and quota borrows. A recovered NativeStarted may attest the exact signed image. It cannot mint a new issue token. Revalidate Prepared from measured readback before queueing. Call `PeerRecipientPort::publish_managed_consent` on the exact due outbox, carrying the paired stop cell and readiness. The port settles the ACK and wakes matching saved Delivery internally. The
actor receives only its outcome. Apply the exit check and then the table.

| Consent publication | Rotor action and connection |
| --- | --- |
| `Unavailable` | Keep the current port. Re-query at the next rotor visit after the next 500 ms period boundary. |
| `NotSent(Local)` | End the Peer generation conservatively and preserve durable work for later recovery. |
| `Rejected` | With a still-current port retain durable retry timing and continue other work. It may only be attempted again after its existing retry deadline. |
| `PossiblyExposed(Local)` | End the Peer generation and preserve the attempted bytes and charge. No repeat publish or settlement in this generation. |
| `Settled(Wake::Completed(n))` | Continue with the current port if still usable. The wake count is evidence of processing only. The exact custody selector and readback decide Pending. |
| `Settled(Wake::Failed)` | End the Peer generation and preserve its settled ACK and custody state for later recovery. |

An unsigned image at NativeStarted stays charged and blocks the single slot.
The existing attestation uses the same invalid result for absent signature
and corrupt signed evidence. End this generation on that refusal. Keep the
original charge and never mint a replacement token from either image.

Only a current Held row may invoke `PendingTurn::arm`, then consume the returned single-use token for one guarded ingest. Use the existing bounded native window. An ambiguous ingest or cold JoinStarted may only use `PendingTurn::qualify` through the existing readback. A successful current Pending CAS merely makes the row eligible for an independently checked view refresh. The projection cannot run any package, Welcome or durable effects.

## Saved Delivery effects

Acquire and observe using the same registered port and owner, no retained
borrows across visits. `use_saved_delivery` supplies the exact checked snapshot
to `observe_managed_saved_delivery`. The current actor owns a native-window
guard that checks the paired stop cell, readiness, forwarding and the current
port's precise Fatal route provenance. In addition to the existing data
checks add final hooks to the writing `due_with_fence` and
`retry_again_with_fence` paths. Run each hook after the precise head or
snapshot check and after speculative UPDATE and wall-floor writes, just
before commit. A refusal drops the transaction and rolls back all writes.
The unguarded entry points used by the old fixtures keep their semantics.
The new actor never calls them. Apply the effect fences already named in the
guarded service design to normal measured retry, provisioning, Consent and
Held effects as they are mounted.

| Observed snapshot | One rotor disposition |
| --- | --- |
| `ManagedSaved::Waiting` | The checked no-ACK backoff already updates the same row under its guarded transaction. Keep the port, grant no native witness and honor that due time. |
| `ManagedSaved::Live` | Use `use_delivery_native(prepared.challenge(), ...)` to seal the exact `PreparedSaved` challenge. Pass the same owner predicate to `measure_delivery_guarded` and the guarded `finish_measured_saved_delivery`. Re-enter the same port through `use_saved_delivery` for finish, rechecking signed lifetime and source, before the existing Held or invalid commit. Reobserve next visit if a current snapshot has aged past its cutoff, cancelling any tentative live commit. No other errors can convert live data to expired retirement. |
| `ManagedSaved::Expired` | Use `use_expired_delivery_native` to obtain the historical sealed measurement, then `retire_expired_delivery` in the current stop and readiness window with the owner predicate. Map only the exact retirement outcome through its table. Historical authority grants deletion only. |

The pass admits at most 85.020 s of explicitly timed awaits and native
windows. This is the sum of 45 s for history, 20 ms idle live probing and
5 s for one live measurement, 25 s for one managed visit, 5 s for one saved
retry or retirement and 5 s for one view verification. Clamp further by
signed and route deadlines. There is no hard wall-clock limit for a stalled
synchronous call. Stop/late checks retain ownership until return and
prevent new effects. Deterministic tests cover changed and empty save
heads, both precommit refusals and both floors, plus fairness when one
durable class cannot progress.

## View cadence

Set the first refresh anchor at authenticated open and a 500 ms cadence based on scheduled origins. Advance to the first future tick after a late turn. Never burst missed refreshes, reset the schedule to the completed turn, or let more than one maintenance refresh wait behind a long admitted native or transport operation. A prior proof expires at its original one-second clocks even while the actor is blocked. Pause an idle live read no later than the next scheduled tick. Take and carry the existing Producer read ticket before the first selector or SQL read in the refresh, including the Pending coordinate selection. Add `PendingTurn::observe_with_ticket(owner, lease, port, selector, generation, clock, producer, ticket)` or an equivalent scoped argument bundle. The existing fixture wrapper can obtain the ticket for its already pinned selector. Only the sealed native and SQL readback can publish the production ticket. If a scheduled selector is absent or the observation is refused, invalidate its ticket and return unavailable. Preserve the separate expired-proof masking rule already supported by the Reader. Add `Producer::invalidate(&self)` to advance and clear the memory stamp under the existing short lock, with the existing irreversible handling of overflow or stopped state. Invalidate before Held arm and ingest, JoinStarted qualification, and
expired retirement. The guarded ingress live operation gains a bounded
`before_record: impl FnOnce()` callback. After obtaining one attributed
record and rechecking current ingress authority, invoke the callback in
`live_turn` inside the branch for `Some` from either `take_buffered_live`
or `wait_live`, immediately before `self.ingest`. A timeout leaves the
callback unused. Use this to invalidate conservatively on real ingress while keeping normal idle reads available. The history turn's direct buffered drain has no callback. Keep the view unavailable until the final page reports completion and a scheduled sealed refresh succeeds. None or refusal from the Pending selector refuses its ticket only if that ticket still owns the current stamp. The actor's Producer proof alone can publish, and accounting writes do not extend old clocks.

Test the rotor and deadline arithmetic with controlled time, including continuous live work, overdue maintenance, high-priority native work, an active saved Delivery or expired head, and termination before a second physical effect. Test with one independent founder over private WSS and no direct cursor edits by the controller. Include one new live invitation and a later exact same-row recovery after cold restart. Check that stop and route loss invalidate the actual Unix listener before cleanup while ordinary work can continue when the shared Fatal remains clear. Treat a stale or unavailable read during a long bounded exchange as the freshness contract. Measure elapsed scheduled origin to fresh observation, authenticated Offer to Pending and the separate ordinary command and closure measurements. Retain the original 20-refresh near-cap measured-image gate and report p50, p95, max, refused windows and saturated view gaps. Require p95 at most 500 ms for the actual maintenance operations and every published ticket under one second on both clocks. Record missed periods separately from the elapsed time of attempted publications. Run existing core CI commands locally without expanding the hosted-runner matrix.


Re-run the actual Peer read suite, sealed same-row observation, and cold-cut subprocesses. Cover refused clock/ticket, page timeout, permanent failed ingress, readiness loss and simultaneous stop/Fatal, cursor replacement during a callback, and Consent possibly exposed or ACK wake incomplete with custody retained. Force one busy turn beyond the freshness deadline and verify the actual reader becomes unavailable, then verify a single scheduled refresh after recovery. An independent founder must reach Pending and a new same-profile process must observe the same row using only native readback. Include before-selection ticket acquisition and a mid-refresh stamp invalidation in the time-controlled tests.

A green private Pending proof leaves the operator-facing work tracked under `marmish-dwt.4.12` and `marmish-dwt.4.9` for supervised requalification, reconnect and fresh empty reporting. Approval, decline and secure group activation remain under `marmish-dwt.3` and its custody work. Accepted peers, stats and heartbeat remain under `marmish-dwt.4`. This candidate's process and topology cannot establish those behaviors.
