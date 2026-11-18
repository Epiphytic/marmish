# Candidate Pending read view

This implements the in-memory projection from the reviewed
[candidate mount](peer-candidate-daemon-mount.md#candidate-local-projection).
The [capped bridge](peer-installed-capped-pending-bridge.md) supplies the
native authority. Candidate client/IPC transport continues to use the
existing wire types. Scope is Linux AMD64 private qualification.

## Producer

Use the exact recipient owner, checkpoint and current candidate generation.
After the bridge's successful installed Pending commit or its exact
read-only Pending replay, extend its local return with an opaque
observation. At the fresh commit/replay fence, take the selection from the
checked custody row, the Held and Pending deadline from the verified
durable row, the durable wall floor used for that check, and a real
monotonic time. Take the deadline for newly committed Pending from the
existing calculation in the CAS transaction. First verify the row is
application-inert Pending, with nonzero generation and
`held_at <= wall_floor < pending_deadline`. For replay, verify the existing
row's exact Pending identity and same checked joined readback. Constructor
authority remains with the branch that already checks both. Keep the
original sealed native proof, paths, relay and account binding with the
bridge; the projection contains only fields required for an observation.
A stale token from an old actor cannot refresh a new view.

One actor owns one view. Since this candidate installs only a single
Unknown slot, start with one optional entry: the existing list bound is
16 and the wire format can represent an empty list. After a successful
initial inventory/scope qualification, absence can yield an empty list.
A failed or uncertain turn marks the whole view unavailable. Never derive a
positive entry from history, the receipt request, stage path or a config
value. Scope loss, a route or owner change, cancellation, ambiguous native
or durable state, or an actor transition that can change the row/image
immediately invalidates the view before any write. Each validity interval
is measured from the fresh positive observation, not the end of arbitrary
actor work.

## Maintenance and reply

Schedule maintenance independently of IPC. On the sole actor, recheck
exact current scope and durable floor, exact Pending row, and the existing
no-follow original stage/image witness. Use fresh bridge readback whenever
its owner cannot confirm the same immutable image. Reuse the existing
bounded native turn. If maintenance runs late or cannot prove this
binding, leave the observation unavailable until recovery completes.
Aim to refresh after 500 ms and expire observations at 1 s. Cancelled or
stale actors cannot refresh. A stopped or unavailable owner returns the
same fixed short error as absent Peer. A change in the real wall below the
retained durable floor invalidates the projection until a fresh
authoritative maintenance turn.

At reply, use only the in-memory singleton, current generation, request
liveness and independently sampled clocks. On expiry at the wall deadline,
omit the row from List and return Unavailable for Show. Never return a
stale proof. For exact healthy state, List sends at most one entry and Show
returns that entry only on complete `Selection` equality. There is no
scope/file, SQLite, native or network access attributable to a request.
The listener owns UID, ready generation, deadlines and queue limits from
the candidate mount plan. It must check the corresponding response
liveness and generation again before sending. Ordinary handlers do not
acquire a Peer path or state.

## Checks

Exercise a fresh Pending projection and healthy empty list; mismatch each
selector field, one-second and wall-deadline boundary, wall rollback or
unavailable clock, duplicate or stale generation, replacement before
refresh, stop/drop and revoked state. Test listener timeout, dropped
reply and no IPC-triggered durable/native turns in the installed
composition. The full positive, forced cold and scope fault cases belong
to the mounted candidate qualification, along with same installed daemon
Command and terminal regression checks. Record any earlier isolated
unit result separately. Do not introduce new wire fields or core CI
native builds for this projection.
