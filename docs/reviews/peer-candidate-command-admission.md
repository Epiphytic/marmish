# Registered Command admission and Peer route revocation reviews

Claude Code Sonnet 5.5 reviewed the registered Command source in Auto mode.
Session `67c5290d-d488-437a-ac7f-bf7bc0d44709` asked for deterministic coverage
of the stored production custody callback while the registry is held and when
the original notification is delayed. Source `e2b59a9` routes both test cases
through the actual captured callback from
`Admission::from_registered`. This is the sole production Command admission
constructor and consumes the registration's one-use fence.

Session `41a27caa-b611-4c2b-b017-988b54986fc1` **approved** exact composed
source `c51e92b52779517c328c9a97bc4d0cde76a79803`. It verified the prior
finding against the actual registry and ran the immutable native executable.
The reviewer passed one close-poison, three dispatcher and eleven fatal
Command tests. The separate author runs and the remaining cross-crate
test limit are in the
[validation record](../validation/peer-candidate-command-admission.md).

Session `25f78423-3cb3-474a-8dc2-77b5162f4a70` **approved** exact revocation
source `fa5872282b9c9b9b3d9b79f665e9b402ad9eb540` against
[approved Manual actor design](../phases/peer-candidate-manual-actor-mount.md).
It verified that the attached owner and its quota borrow remain live, the
exact route cancellation is idempotent, and raw reap uses that same
cancellation and awaits permit return after port destruction. The reviewer
inspected source. The author ran the eight private WSS cases and strict
Clippy. Remote disconnect timing and keeping a port alive across reaping
were noted as outside these scoped tests.
