# Borrow the registered route beside the exclusive native cursor

The [Manual actor design](peer-candidate-manual-actor.md) opens and
verifies one cursor before taking the qualified physical route. The cursor
borrows the exact attached quota for its full lifetime. On
`0498e267`, `LocalReceiverRegistration::quota` borrows the registration
and its `take_route` needs an exclusive borrow of the same registration.
That interface prevents a live cursor and a route take from coexisting.
Fix the borrow boundary before actor mounting.

## Ownership

Declare `quota: ReceiverQuota<'lease>` first in
`LocalReceiverRegistration<'lease>`, then
`route_scope: AttachedRouteScope<'lease>`. Inside the private route
scope retain the existing declaration order `scope: RegisteredScope`,
`route: Option<CheckedPeerRoute>`, `material: &'lease IdentityMaterial`.
The actual registration still owns both fields, borrows
the original lease and material, and cancels the shared profile token in
its destructor before those fields drop. It is impossible to move
either field out of an attached registration.

Provide the owner a private candidate-only borrow split through
`LocalReceiverRegistration::actor_parts(&mut self)`. It returns an
immutable borrow of that exact quota and a
`ReceiverRouteControl<'owner, 'lease>`. The controller has a disjoint
exclusive borrow of the route scope and an immutable borrow of that
same quota. The controller has no constructor outside the owning
module, no independent lifetime, and no API for replacing route,
quota, identity or forwarding witness. It exposes only the registered
local recheck and one-use route take. Any checked origin accessor
introduced by the separate metered cursor increment requires its own
composition review and exact test.

Move the existing local/global brackets into the controller and
move `take_route_checked` along with its `#[cfg(test)] after_take` hook
between the original move and the final global verifier. Delegate the
narrow pre-mount wrappers through the same `actor_parts` split. Keep
both identity pointer and lease identity checks. A cursor owner borrows
the immutable quota in parallel, so its read or synchronous native
turn can finish before the next controller call without reopening,
reacquiring, or issuing another quota. The controller and cursor
remain in the actor thread. Neither may be handed to the IPC listener.

Preserve the existing ordering for both global forwarding rechecks,
the one-use route and its drop semantics. A controller drop releases
the borrow only. Dropping the registration after all attached borrows
end still invalidates a live route. The owner may stop and drop the
port while the cursor and controller borrows are live. The registration
can drop only after those borrows have ended. On fatal loss the same
process-wide physical shutdown rules apply.
This phase adds no retry, native side effect, process setter, or daemon
startup exception.

## Tests

- Keep the real Manual qualification/registered WSS success case.
  Acquire the disjoint borrows, use the quota reference across route
  take and an authenticated empty page, and prove that the exact
  original quota remains usable by a checked owner. Compare the
  before/after quota identity from the actual registration. The
  combined actor phase also uses the real keyed `RecoveredOwner`
  across the transfer.
- Exercise both real VFS displacement cuts against the same controller
  used by production callers. Preserve the first cut's uncancelled
  observation and the second cut's retired moved route. Point the
  existing assertions into `route_scope` and use the controller's
  callback without dropping any assertion. Rerun the WSS wrong-scope,
  repeated-take, and owner-drop cases. The success case uses the
  controller after the first take to make a repeated request, then
  verifies the authenticated port is still usable. End every borrow
  from `actor_parts` before the owner-drop case drops the registration
  with its port still live.
- The controller compiles in stock tests and explicit candidate
  builds for Linux AMD64. It is absent from stock production. In a
  scratch worktree, add a temporary sibling probe module inside the
  crate outside candidate registration. Require only `E0451` when
  attempting private field construction and only `E0505` when
  attempting to drop the registration and then use its borrowed
  controller. The probes do not enter the repository.
- Run strict stock and candidate all-target Clippy and focused physical
  and candidate-registered regressions. This changes only the
  ownership interface. Positive live observation, database transition
  and failure-boundary qualification remain in the mounted actor phase.
