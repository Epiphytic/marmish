# Candidate physical recipient route handoff

This increment implements the sealed route handoff in the
[approved Manual actor design](peer-candidate-manual-actor.md). Its base is
`8bcc2b0` with the exact forwarding verifier source
`e8bf71f3ccbd8c23facbb8865d1585b17339c42b` composed first. The actor and
daemon integration remain in the enclosing phase.

## Interface and ownership

Keep the preflight and registered route concrete before attach. At attach,
split the attached `ReadyReceiverScope`, `CheckedDirectories`,
`ForwardingVerifier` and `ReceiverQuota` from the checked route. The
attached registration holds that route as `Option<CheckedPeerRoute>` and
borrows the original `IdentityMaterial` for the same ownership interval
as the lease. No API rebinds the route or resets its qualification clock.
The retained `ForwardingVerifier::recheck_default` yields
`RegistrationFailure::Indeterminate` on sticky registry refusal.
`ReceiverQuota::recheck_registered_lease` checks the exact attached lease,
profile/member descriptors and reserved-name topology. These facilities
are from the composed forwarding verifier source.

`LocalReceiverRegistration::take_route(&mut self, lease: &Arc<ProfileLease>,
material: &IdentityMaterial) -> Result<RegisteredRoute, RegistrationFailure>`
has this order.

1. Verify global forwarding before inspecting local state.
2. Refuse a second take as `LocalUnavailable` before any local check or
   cancellation. A global failure from step 1 still has priority.
3. Check the exact lease and original material witness, the Ready manifest
   and captured config, the original route's account and device slot,
   the qualification issuer and config witness, and the directory and
   quota topology. `check_prepared_peer_identity` pins the manifest's
   account and device slot. The original borrow and issuer verification
   also differentiate the SQLCipher root. An ordinary local refusal
   cancels the shared profile token.
4. On local success, take the original route out of the option. Perform
   the global check even when the local result refused. Dropping a taken
   route on global refusal cancels the token. Indeterminate has priority
   over local refusal.
5. Only after the second global check succeeds, mint the one-use token
   with the taken route and signer from the checked material.

The post-attach `recheck` keeps the same local and global checks for
the retained authority. After a take, the local recheck covers exact
lease, original material, Ready scope, directories and quota topology.
The live port owns the route, qualification clock and TLS window. A
caller of the attached local check alone cannot infer the route is current.
Dropping the attached registration cancels its shared token, invalidating
the port even if retained by a mistaken caller.

Define `RegisteredRoute` in `candidate_registration` with private fields.
Its `into_parts` is visible only within `crate::peer_relay_trust`. The
test and explicit Linux AMD64 candidate path consumes it through
`ReceiverRouteAdmission::registered` inside `peer_physical_route`, which
checks the signer/account binding. No fixture or caller-supplied origin,
signer or trust chain creates this token. Compile a temporary privacy probe
outside that module to verify the private construction and verify the
candidate excludes the test-only admission.

Keep the existing typed `PeerRecipientPort::open`, using the qualified
relay origin and TLS trust, NIP-42 exchange, and live EOSE. Share the
existing `use_current_recipient` observation with the explicit Linux AMD64
candidate so an actor checks live scope after returning from open. The
existing stock port gates stay intact. The global registry lock is never
held across the asynchronous open. The actor owns the post-open
local/global and live checks as well as the readiness and remaining startup
budget before and after open. The actor will pass the shared fatal read
handle through the transport construction once the fatal admission fence is
composed. This increment distinguishes typed Indeterminate and leaves
process-fatal closure to that composition.

Dropping an unused admission, failing open, losing the port or dropping the
attached registration invalidates the common route/profile token. On normal
shutdown first stop or drop the port, then release attached cursor/quota
ownership, then release the original lease after join. A failed local
postcheck after a port drop does not authorize journal repair, cleanup, a
second route or evidence retirement.

## Test evidence and exit

Use a fresh child per isolated case through the existing authenticated
route test module under `peer_invitation`. Create the private WSS TLS
relay and disposable identities in the child. Use a Manual variant of
`qualified_control_fixture` with `from_ready_configured`, actual
qualification with its 90-second record window, and subsequent
`acquire_ready`. This setup issues the qualification files. The route
comes from actual receiver preflight and registration.

- Success and repeated take run in one child. Use the actual process
  default registrar, attach, consume the registered route and open the
  existing typed port. Verify authenticated account and origin, one empty
  live relay request, a bounded empty page and zero control publishes.
  Refuse a second take while preserving the original route and verify
  that the port can page after the refusal. Replace the config by inode
  and verify the next live use fails while the independent global
  verifier and ordinary stock SQL still work.
- Separate subprocess cases run before any recipient requests or control
  publishes. Exercise a foreign lease from a different disposable
  profile, wrong material with a different SQLCipher root, and a dropped
  unused admission. Each refuses future local scope. Assert cancellation
  through the existing typed local recheck. Verify exact ordinary SQL
  forwarding while local failure leaves the global verifier current.
- The displacement case combines real global default replacement with
  local config drift before attempting take. Require typed Indeterminate,
  no admission, no recipient requests and no relay publish attempt.
- Run default and explicit-candidate compiler/Clippy checks and the
  focused registered handoff, existing physical route and registration
  selectors. Candidate code has the sealed construction path and keeps
  fixture minting test-only. The production daemon leaves the handoff
  unmounted in this increment.
