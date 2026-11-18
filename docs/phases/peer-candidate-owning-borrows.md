# Borrow the registered route beside the exclusive native cursor

The [Manual actor design](peer-candidate-manual-actor.md) opens and
verifies one cursor before taking the qualified physical route. The cursor
borrows the exact attached quota for its full lifetime. On
`0498e267`, `LocalReceiverRegistration::quota` borrows the registration
and its `take_route` needs an exclusive borrow of the same registration.
That interface prevents a live cursor and a route take from coexisting.
Fix the borrow boundary before actor mounting.

## Ownership

Hold two fields in `LocalReceiverRegistration<'lease>`. Keep the
original `ReceiverQuota<'lease>` in one field. Move the existing
`RegisteredScope`, optional original `CheckedPeerRoute` and original
`&'lease IdentityMaterial` into a private `AttachedRouteScope<'lease>`
in the other field. The actual registration still owns both, borrows
the original lease and material, and cancels the shared profile token in
its destructor. It is impossible to move either field out of an
attached registration.

Provide the owner a private candidate-only borrow split through
`LocalReceiverRegistration::actor_parts(&mut self)`. It returns an
immutable borrow of that exact quota and a
`ReceiverRouteControl<'owner, 'lease>`. The controller has a disjoint
exclusive borrow of the route scope and an immutable borrow of that
same quota. The controller has no constructor outside the owning
module, no independent lifetime, and no API for replacing route,
quota, identity or forwarding witness. It exposes only the registered
local recheck and one-use route take. Preserve an origin accessor if
added by the metered cursor increment. That accessor checks the exact
local/global scope with the original route still present, and returns
only an owned origin from that route.

Move the existing local/global brackets into the controller and
delegate the narrow pre-mount wrappers through the same split. Keep
both identity pointer and lease identity checks. A cursor owner borrows
the immutable quota in parallel, so its read or synchronous native
turn can finish before the next controller call without reopening,
reacquiring, or issuing another quota. The controller and cursor
remain in the actor thread. Neither may be handed to the IPC listener.

Preserve the existing ordering for both global forwarding rechecks,
the one-use route and its drop semantics. A controller drop releases
the borrow only. Dropping the registration after all attached borrows
end still invalidates a live route. It is legal for the owner to stop
and drop the port before returning its cursor and controller borrows.
On fatal loss the same process-wide physical shutdown rules apply.
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
  observation and the second cut's retired moved route. Rerun the
  WSS wrong-scope, repeated-take, and owner-drop cases. For the owner
  drop case, finish the borrowed controller and retained quota use
  before dropping the registration while the port stays live.
- The candidate compiles the controller while stock builds omit it.
  An isolated negative compile probe outside the module attempts to
  construct it. Another attempts to release the registration and use
  its borrowed controller afterward. Expect private-field and
  borrow-checker errors respectively. The probes do not enter the
  repository.
- Run strict stock and candidate all-target Clippy and focused physical
  and candidate-registered regressions. This changes only the
  ownership interface. Positive live observation, database transition
  and failure-boundary qualification remain in the mounted actor phase.
