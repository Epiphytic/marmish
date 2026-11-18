# Peer Pending custody checkpoint review

Claude Code Auto reviewed the exact standalone test-gated schema and
checkpoint component against the approved
[Pending join custody phase design](../phases/peer-pending-join-custody.md).
Design session `ae2a5faa-bace-4c04-bf0b-dd44970964cb` approved that scope.
The review composed the SQLCipher component with its cursor, signed wire
and native JoinedReadback dependencies. The integrated owner has a
separate review and native/WSS acceptance gate.

- R1 `5443d870-3ff1-4215-b6b0-20c2881ac3f1` requested changes to
  `30f3eb5ec20a6857ba7b18f343571ea9db57b59f`. The reviewer reproduced
  an existing Prepared Consent rollback fixture failing when a moved
  inventory check raced the fixture's injected wall-write failure.
- R2 `4bec4eea-d6d4-443b-91a5-87a29f144643` requested changes to
  `89e1052a2d1561ceb9ebc2b3eadf93c6609bf620`. The reviewer inspected
  the exact schema, CAS transitions and same-transaction legacy guard.
  The standalone SQL tests bypass real Held and native Pending or
  Activating proof, so the reviewer required those executable checks
  or their explicit assignment to the integrated owner acceptance gate.
- R3 restored the epoch verification between the outbox insert and the
  injected callback to preserve the existing rollback error. The phase
  design now explicitly gates closure on actual Held and native
  Pending and Activating transitions, including authority and deadline
  failures in the integrated owner. The exact revision is
  `497afba5d5ce41750152fcf852e607e228142921`.
  Claude Code Auto `claude-sonnet-5-5` approved it in session
  `b5962221-b89d-4c83-be8d-5af06249c8b2`. The reviewer independently
  rebuilt and passed the repaired library regression in 22.74 s.
  The reviewer inspected the extra package-lifetime and backwards-clock
  checks and noted they lacked direct component tests. The Prepared
  schema requires Consent expiry no later than the package expiry, and
  the shared cursor wall floor rejects backwards time before the custody
  transition. The new checks remain defensive at their own boundaries.
