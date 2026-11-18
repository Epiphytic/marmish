# Candidate physical recipient route handoff

This increment implements the sealed route handoff in the
[approved Manual actor design](peer-candidate-manual-actor.md). Its base is
`8bcc2b0`. The retained forwarding verifier is a separate source dependency.
The production actor and daemon integration remain in the enclosing phase.

## Interface and ownership

The attached registration retains the original profile lease and checked
directories, Ready scope, quota and global verifier. State before the
handoff owns one `CheckedPeerRoute`. Use `Option::take` for that one
consuming step. No API rebinds the route or resets its qualification clock.
The registration's earlier recheck continues to compare the qualification
issuer and captured config with the exact material. The attached route
handoff performs these checks and brackets its local checks with the
global verifier. An outer `Indeterminate` takes precedence over local
drift. After the handoff, the local part continues checking lease, material,
Ready scope, directories and quota topology. The checked port owns the
original live route and performs the route, qualification and TLS checks
at its existing use boundaries. A caller of the attached local check alone
cannot infer the live route is current.

Expose a candidate-gated `LocalReceiverRegistration::take_route` consuming
the original route from the same attached/cursor-owning registration, with
lease and material checks. The registration alone constructs an opaque
`RegisteredRoute` carrying that route and the exact signer. It is consumed
by `ReceiverRouteAdmission::registered` in the physical route module. The
token cannot come from a fixture or arbitrary origin, signer or chain of
trust. The port remains the existing typed `PeerRecipientPort::open` and
uses the relay origin and TLS trust from the checked qualification. It
reuses the same account guard, TLS bounds, NIP-42 exchange and live EOSE
qualification. The actor will check readiness and remaining startup budget
before this call and recheck local/global ownership and the live port after
it returns, before yielding a route-dependent effect. This increment offers
no worker or user surface.

A rejected local scope, a repeated take or a failed route check cannot
produce an additional port. Preserve cancellation on scope or route loss.
Keep the shared profile cancellation token and existing physical drop
semantics. Shutdown after global displacement is owned by the separately
reviewed daemon fatal-cut increment. Keep the route handoff and checked
recipient observation leaves shared only by tests and explicit Linux AMD64
candidate. Test fixture minting, founder-only operations and effects remain
test-only.

## Test evidence and exit

- An isolated native child with the actual process default registrar attaches
  once, consumes the registered route and uses it with the real private TLS
  relay and disposable keys. Exercise the exact empty live subscription,
  real authentication and a bounded page. Compare account and origin with
  the original configured identity and trust. Establish ordinary SQL
  forwarding and that no control publishes occur.
- Within the isolated proof, assert second take refusal without resetting the
  original route, wrong lease or material refusal before network use, and
  config replacement after handoff invalidating the existing route. Check
  that an independent global verifier remains healthy after the local
  refusal. Exercise default displacement and priority in the forwarding
  verifier's isolated registration regression.
- Strict locked default and explicit-candidate compiler checks and focused
  existing physical route and registration tests. Candidate code has the
  sealed construction path and stock code retains the prior config and
  fixture gates. The production daemon leaves both unmounted here.
