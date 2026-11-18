# Candidate Pending read view

This implements the in-memory projection from the reviewed
[candidate mount](peer-candidate-daemon-mount.md#candidate-local-projection).
The [capped bridge](peer-installed-capped-pending-bridge.md) supplies the
native authority. Candidate client and IPC keep the same wire types.
Scope is Linux AMD64 private qualification. The mounted actor phase must
expose the exact test-qualified bridge in candidate compilation, with
the existing sealed turn and without fixture authority.

## Producer and generation

Use three separate identities. A non-reused actor epoch owns the view.
The actual listener runtime generation gates IPC admission and reply.
The nonzero durable row generation is a field of the wire `Selection`
and participates in exact Show equality. A row generation alone confers
no actor or listener authority. Only the current actor has the non-Clone
producer. Bind it to the listener and actor at existing runtime readiness.

After the bridge's successful installed Pending commit or its exact
read-only Pending replay, return a private, non-Clone opaque observation.
Before the earliest row or native evidence read for that observation, the
producer takes the monotonic origin and an internal publication ticket.
That ticket pins the current actor epoch and view sequence. The checked
bridge consumes the ticket. It takes the entry coordinates, Held time,
Pending deadline and verified wall from the exact current tuple. Use either
the complete tuple returned by a successful CAS or a fresh exact durable
readback with matching signed ancestry, selection, native coordinates and
current generation. If the CAS is ambiguous or the fence is lost, publish
nothing. Exact checked read-only recovery can later prove the row.

Construct the observation only inside the branch that checks both the
application-inert durable Pending row and the sealed native witness.
Require `held_at <= verified_wall < pending_deadline`, a nonzero row
generation and the current actor ticket. Keep paths, native authority, route
and group binding with the bridge. No positive projection can be created
from history, a request, stage filename or config.

One actor can publish one Unknown row in this candidate. The existing
wire bound remains 16. Healthy-empty is a distinct positive observation
with the same epoch, sequence, monotonic origin, wall-floor and freshness
rules. It takes a successful exact scope and inventory pass that proves
absence. Initial, invalidated, ambiguous, Reserved, Held and JoinStarted
state cannot create healthy-empty. Only independent maintenance can
publish a new qualified value.

Serialize the view and publication sequence in one short critical section.
It takes no native, database, file or network locks. Any invalidation
advances the sequence and clears the view. A publication compares the
ticket epoch and sequence, requires the same active generation, a live
lease and elapsed time strictly under 1 s, then consumes the ticket and
advances the sequence. A late observation seeding an older ticket cannot
restore visibility. The actor invalidates before transitions that could
change a native image or durable row. The runtime and listener can
invalidate or permanently stop on cancellation, owner loss or listener
exit without waiting for the actor to finish a native turn.

## Maintenance and reply

Schedule maintenance independently of IPC on the single installed actor
with a 500 ms refresh target. Recheck exact current scope and durable floor,
exact row and no-follow original stage/image witness. Run a new capped
readback when the existing bridge cannot confirm the same immutable
image. Take the monotonic origin anew before its earliest evidence read.
A late or inconclusive turn never extends validity. The one-second proof
lifetime includes the native turn. During slow or refused maintenance
clients may see Unavailable. The startup proof measures availability
over the candidate workload in addition to pass/fail outcomes.

At reply, use only the view, listener generation and deadline, actor epoch
and independently sampled real and monotonic clocks. Sample the real wall
as integer seconds since the Unix epoch, rounded down by `as_secs()`,
and reject a missing clock. Exactly 1 s after the origin the proof is
stale. If wall is below the retained floor, permanently invalidate that
view sequence. If the verified Pending deadline is reached, latch expiry
for the exact row generation so a backwards wall movement cannot expose
it again. A request observing expiry returns an empty List when the
still-qualified view contains only that expired row, or Unavailable
for Show. A later fresh observation must obey the existing durable floor
and the latched exact-row deadline. For healthy state return the
singleton List or the exactly matching Show. The listener rechecks
generation, liveness and a corresponding in-memory observation sequence
at the admission boundary for the final local write. It never holds the
view lock across the write. There is no SQL, native, route, timer
advancement or filesystem I/O in a request.

## Checks

Exercise Pending and qualified absence, unavailable initial and ambiguous
states, mismatch each selector field and each generation, 1 s monotonic
and wall-deadline boundaries, permanent rollback/expiry handling with
subsequent clock movement, blocked attempt to publish a stale ticket,
invalidate before transition or stage replacement, and stop/drop racing
a finished maintenance turn. On the installed candidate, test timeout,
dropped reply, invalidation before the final write and zero attributed
SQL or native polls for local requests. Compare request-scoped counters
and independent maintenance counters. Installed cold/restart, same-daemon
Command and Terminal and service latency belong to the mounted
qualification. Add no wire fields or CI native build legs.
