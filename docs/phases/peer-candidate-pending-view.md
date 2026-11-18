# Candidate Pending read view

This implements the in-memory projection from the reviewed
[candidate mount](peer-candidate-daemon-mount.md#candidate-local-projection).
The [capped bridge](peer-installed-capped-pending-bridge.md) supplies the
native authority. Candidate client and IPC keep the same wire types.
Scope is Linux AMD64 private qualification. The mounted actor phase must
expose the exact test-qualified bridge in candidate compilation, with
the existing sealed turn and without fixture authority.

This phase revises the parent mount's read scheduling. Each verified
same-UID connection uses a shared, memory-only read handle and acquires
a nonwaiting permit from the existing four-slot local read budget.
There is no actor-side read message or queue. Keep the existing IPC
exchange deadline and per-connection bounds, and hold the permit through
the reply attempt. Saturation returns Unavailable. The actor stays the
sole producer and can be blocked on native work without delaying an
admitted read. Candidate session framing and stock dispatch stay as
designed in the parent mount.

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
Pending deadline and verified wall from the exact current tuple. Since
`Change::apply` returns no tuple, perform one exact durable readback after
CAS success within the same fenced turn as the native proof. Compare
signed ancestry, selection, native coordinates, deadlines and generation.
Use a fresh verified wall sample for the underlying CAS. A refused or
ambiguous CAS, a mismatch or a lost fence publishes nothing. Exact checked
read-only recovery can later prove the row.

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
It takes no native, database, file or network locks. Use Live, Invalid
and terminal Stopped states. Treat a poisoned lock as permanently
unavailable. Any invalidation advances the sequence and clears the value.
A publication compares the ticket epoch, listener generation and sequence,
requires an active local owner and elapsed time strictly under 1 s, then
consumes the ticket and advances the sequence. It cannot recover Stopped. A late observation seeding an older ticket cannot
restore visibility. The actor invalidates before transitions that could
change a native image or durable row. The runtime and listener can
invalidate or permanently stop on cancellation, owner loss or listener
exit without waiting for the actor to finish a native turn.

## Maintenance and reply

Schedule maintenance independently of IPC on the single installed actor
with a 500 ms refresh target. Recheck exact current scope, exact row and no-follow
original stage/image witness. The effective durable floor is the maximum
of the authenticated applicable cursor-origin and Consent-version wall
floors. Verify it within the same fence as the exact record. Run a new capped
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
stale. This lifetime uses Rust's monotonic `Instant`, whose suspend
behavior is platform dependent. If wall reaches a verified Pending
deadline, latch expiry for the exact full Selection and actor epoch.
The candidate has one durable slot per actor, so it needs one latch.
Invalidation clears the view and retains this latch for the actor's
lifetime. If wall is below the retained floor, permanently invalidate
that view sequence. A request observing expiry returns an empty List only
when the current proof still qualifies that exact expired row. Show
returns Unavailable. A freshly sampled older wall cannot revive the
same expired row in this actor. After restart, the authenticated persisted
floor still applies. If an expiry was only observed in memory and wall
rolls backward to a time at or above the persisted floor, a new actor
may requalify that row. Reads do not advance a durable floor.

For healthy state return the singleton List or the exactly matching
Show. Check listener generation, actor liveness, sequence and freshness
again at the admission boundary for the final local write. The
one-second freshness guarantee applies to write admission. Delivery to a
slow reader is bounded by the IPC exchange deadline and can take longer.
Release the view lock before the write. There is no SQL, native, route,
timer advancement or filesystem I/O in a request.

## Checks

Exercise Pending and qualified absence, unavailable initial and ambiguous
states, mismatch each selector field and each generation, 1 s monotonic
and wall-deadline boundaries, permanent rollback/expiry handling with
subsequent clock movement, the documented restart floor boundary, poison,
four admitted requests and immediate saturation, a slow reader, blocked
attempt to publish a stale ticket,
invalidate before transition or stage replacement, and stop/drop racing
a finished maintenance turn. On the installed candidate, test timeout,
dropped reply, invalidation before the final write and zero attributed
SQL or native polls for local requests. Compare request-scoped counters
and independent maintenance counters. Installed cold/restart, same-daemon
Command and Terminal and service latency belong to the mounted
qualification. Add no wire fields or CI native build legs.
