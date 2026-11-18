# Integrated Peer join custody review

Claude Code `claude-sonnet-5-5` reviewed the integrated test-only milestone
in Auto mode. The [design](../phases/peer-pending-join-custody.md) was
approved in session `ae2a5faa-bace-4c04-bf0b-dd44970964cb`, with SHA-256
`ce0d0f36b6598aeeccd916901206abdc8fe3104ce5ee7c487ec3825cfc151bef`.
The checkpoint and native subcomponents also had independent approvals.
See the [checkpoint review](peer-m2-custody-checkpoint.md) and
[native review](peer-m2-native-join-readback.md).

Integrated session `a6479939-984b-4de8-aaac-30e233c28533` approved the
source. It inspected signed ingress, byte-exact replay and conflicts, ACK
authority, custody transitions and older owner guards, repeatable native
readback, error precedence and deny-all physical egress. Two test-only
assertions changed from the initial requested `2145259` during this review
to assert the existing no-work outcome and recheck state under a permitted
policy. No implementation changed. The independent binary ran both M2
WSS cases in 64.69 s, the native proof in 38.37 s and the M1 WSS regression
in 31.35 s. Its two custody and 11 cursor tests also passed.

The reviewer requested explicit checks at the durable deadline and a
direct activation CAS check with verified native readback. The existing
suite had tested raw replay expiry and the independent system-clock
tolerance. The follow-up uses a signed short-lived Delivery to place the
custody deadline inside the qualified wall tolerance. It adds refusal of
expired JoinStarted, Pending and Activating, including a direct native
proof at the CAS. The expanded debug fixture uses the dedicated-stack
pattern from the native Terminal integration tests.

The resumed review tested an intermediate artifact. It reproduced a stack
overflow on the default debug test thread. The standalone rerun with a
larger stack and all three tests on a subsequent artifact passed. A fresh
Auto review, `a33e6d76-31e2-461b-80e7-35d5cbc6994a`, approved the exact
source at `b8cf197710196ce3538c84adf8bb92d8b4c507e0`. It independently
ran both custody cases in 86.40 s with a default test environment. The
author rebuilt that exact commit and ran all three private WSS cases. The
review noted that the held and pending deadlines coincide in the fixture,
so it proves refusal at their shared boundary.

The reviewed fixture requires a realistic same-UID Unix stream and
checks wrong generation, current block and manual-versus-automatic
authority. A privileged cross-UID Unix integration test and injected
wrong-stage proof at the CAS were outside this run. The independent
native proof covers wrong exact Delivery, origin and stage evidence.
The source checks exact proof equivalence before the CAS. The upcoming
daemon phase binds manual decisions to the registered socket listener.

The [validation report](../validation/peer-pending-join-custody.md)
records the author runs and limits.
