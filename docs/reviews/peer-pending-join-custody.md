# Peer pending custody design review

Claude Code Sonnet 5.5 used Auto permission mode for read-only design
review of the [pending custody plan](../phases/peer-pending-join-custody.md)
against the actual M1 source at `4bb566e`. The exact approved plan has
SHA-256 `ce0d0f36b6598aeeccd916901206abdc8fe3104ce5ee7c487ec3825cfc151bef`.
Session `ae2a5faa-bace-4c04-bf0b-dd44970964cb` approved it without blockers.
The [standalone preflight review](peer-m2-native-preflight.md) separately
verified the public MDK join and snapshot behavior.

Round 2 session `2c3f40bd-5bae-4de2-8f27-717537f8522d` requested exact
trust and phase bindings. Round 3 session
`68a6de2c-0077-40d6-9858-c85d21c4dea8` identified that replaying through
unjoined native package attestation would wedge after join, and asked
for a closed typed cursor disposition and retry table. Round 4 verified
that the revised design supplies the required exact paths.

Carry these points into the implementation review:

- Keep the signed outer stamp, authenticated inner flight creation time
  and immutable verified Consent transcript revision distinct. The
  re-opened store can verify the saved Consent outer and bindings. It
  cannot rederive its inner rumor ID once the package is consumed.
- Retain the exact custody row through the fixture lifetime. After the
  authenticated replay horizon, return Invalid and never rejoin.
- Use the exact deferred record and clear it when a replay is already
  applied. A fresh durable Held outcome must be distinguishable from
  legacy cursor effects. Legacy sweep must preserve the held exact
  authority and its charge.
- Use the ordinary invalid cursor commit for malformed carriers.
  Keep post-join image verification separate from unjoined package
  inspection. Tampering must fence local progress even when a replay
  receives the same index disposition without reading the stage.

This design approval authorizes the opt-in milestone with native egress
closed. Acceptance, ordinary daemon/CLI wiring and production
qualification need separate evidence.
