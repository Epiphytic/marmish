# Observe and takeover native supervisor review

Review date is November 18, 2026. Scope includes the native Terminal supervisor, its confirmation and slot planning modules, checked tmux observer verification, the native attachment room lease, the offer ledger, and their regression tests. The selection driver and complete Marmot transport have their own integration review.

Claude Code Sonnet reviewed the first snapshot in `--permission-mode auto` under session `1f6bf192-36ca-4ce6-98fc-b9a5fa842d76`. The reviewer required two changes. Successful confirmation had to remove the exact parked entry in the same owner turn, and an expired or invalid ticket had to return StaleOffer even at native capacity. The next snapshot implements both findings. The local confirmation gate now publishes activation only after the acknowledgement send succeeds. A replacement marks only the retired writer and its linked observers as Replaced, which lets the authenticated stream send its existing Close record.

The frozen native patch SHA-256 is `67ec2e439a020a2c53425a55d708cfe5eb869d7f89a762fc77e6182261dc445f`. For integration the old selection wire and legacy native Start variants are gated to tests.

## Independent disposition

Claude Code Sonnet approved the second snapshot in `--permission-mode auto` under session `ec8989b7-f4eb-4649-b0a1-6dcdb91f1924`. The reviewer verified both blockers were resolved and found no new blocker in reaping, room lease custody, observer geometry and writer linkage, authority checks, confirmation gating or owner-turn bounds. The review ran 29 native Terminal live tests, seven installed native cases, and the full Terminal-filtered suite with 169 passed and 40 ignored. The review reported formatting differences only in pre-existing files outside this scope.

The reviewer noted two coverage improvements. The installed fixture checks both stale and valid offer outcomes at the four-link slot cap, and a ledger unit test checks non-consuming validation. There is no installed retry of the same ticket after capacity changes or ticketed-New check at the global eight-link cap. After capacity and authority checks pass, failure in later native readiness can still spend a ticket. A fresh offer is needed for another ticketed attempt in that case.

## Author validation

- `cargo check -p marmish-native --tests --offline` passed. The review checkout keeps a legacy selection compilation bridge so it can run its standalone native fixtures.
- The focused test binary passed all 24 native live core tests, all three offer ledger tests and all eight replacement accounting tests.
- The Linux private native sweep with real tmux, SQLCipher, and an explicit built `MARMISH_TEST_DAEMON` passed all seven cases in 71.46 seconds. The suite checked writer and observer role gates, a wrong nonce at full observer capacity, exactly three observers, mirror and display behavior, same-turn confirmation with another parked slot, typed retirement of exact old links, offer replay, and named reattach preserving shell state. The supervised unwind fixture emits an intentional panic while checking that the reaper releases its native lease.
- In the same sweep, the existing local coalesced writer echo fixture recorded 8.221 ms p50 and 8.383 ms p95 over 100 samples. The native measurements omit relay, MLS and record transport.
- `git diff --check` passed.
