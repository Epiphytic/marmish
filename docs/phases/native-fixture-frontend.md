# Continuous frontend ownership in store fixtures

Bead `marmish-d97.1`; exact implementation base
`ae309a58bee3917bd2a90f643f1ec7da61d51f50`.
This increment changes Rust unit tests only.

The local combined debug suite at `2b951ae` recorded one engine failure:
`denial_precheck_does_not_exhaust_execute_tokens` failed in fixture conversion
with `AlreadyOpen` (143 passed, one failed, one ignored). Relevant Rust
contracts are unchanged between that source and this implementation base.
The fixture dropped its frontend guard after initialization and reacquired
the lock during conversion. A retained alias of the original open file
description can keep its lock held during this gap. The actual historical
holder was not captured; concurrent fork inheritance remains a hypothesis.
Linux uses OFD locks; macOS uses flock. CLOEXEC takes effect at exec.

## Approved design before implementation

Claude Opus 5.5, session `41bfc004-6675-4b7a-a602-80ed4d8564ac`,
approved the lifecycle correction and exact implementation base before code.
Final design SHA-256:
`f20fe58c87a73535ba87928d6b336785615a1a3637278d18598bd24c40974747`;
final design approval response SHA-256:
`5ced048c1e6a0d1c8b05e918f30189a35e8d0c3fd4f42a79b90d1b1818570339`.

Retain `Some(FrontendGuard)` in the test fixture and transfer the same guard
directly into `Engine`. The wrong-native-master reopen test deliberately
takes and drops it immediately after construction, preserving its original
early-release boundary before setup and all rejection/state assertions.
No production lock, retry, timeout, thread-count or platform behavior changes.

Add a fixture ownership regression that rejects competing frontend locks
before and after conversion, retaining the directory throughout. Record RED
with that test selected alone against the old fixture, requiring failure at
its first ownership assertion, then GREEN after correction. Extend the
existing isolated launcher lock test with a real duplicated descriptor:
dropping the original must leave the lock busy until the alias also closes.
Preserve its existing supervisor, presence checks and completion marker.
This deterministic control proves alias retention, not the historical holder.
Do not add an immediate parallel post-drop reacquire assertion.

Run the focused ownership, denial and alias tests, followed by store and
launcher tests under default threads. Obtain independent completed-code
review. Root owns broader native validation and publication. Existing engine
drop/reopen tests keep their current semantics; this increment removes only
the fixture-to-engine handoff gap and makes no global race-free claim.

## Completed increment

The ownership regression demonstrated the expected first-assertion RED and
then GREEN. Affected store and launcher inventories passed, including the
unchanged denial assertions and the descriptor-alias control. Claude approved
completed code and independently ran the full engine library suite: 145
passed, zero failed, one existing ignored helper. See [review and validation
evidence](../reviews/native-fixture-frontend.md). Broader source-bound native
qualification remains root-owned and separate.
