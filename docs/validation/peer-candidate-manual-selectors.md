# Private Manual work selection validation

The Manual scheduler selector and package effect leaf was checked on Linux
AMD64 against the approved scheduler design. It borrows the existing
qualified recipient port and charged receiver owner. The scheduler and
public CLI are outside this leaf's runtime scope.

| Source and configuration | Observed result |
| --- | --- |
| `20bb4e58e2c7915c0c5579277424bd22319a2aaf`, stock standalone cursor | The keyed SQLCipher selector passed in 0.24 s. It covers canonical Offer order, an occupied or stale charge, mismatched account and route, wall rollback, exclusive expiry, unaffected read floors and rollback after refused Bound, NativeStarted and Prepared precommit checks. |
| Same source, stock native library | The exact charged `JoinStarted` journal selection and wrong-coordinate exclusions passed in 0.22 s. |
| Same source, stock opt-in native library | Real registered quota and SQLCipher/MDK package issuance, managed Consent queue and attempt, and restart after the queue commit cut passed in 24.55 s across two separate three-child scenarios. |
| Same source, stock compiler | Strict native all-target Clippy passed in 28.99 s. Scoped Rust formatting and `git diff --check` passed. |
| Earlier `4906677c697b0bbe8c3b7f321fd87d959dc302c8`, Linux candidate | Standalone cursor suite passed 79 cases in 81.46 s with four opt-in cases skipped. Strict native all-target Clippy passed in 34.37 s. These counts precede the JoinStarted and stage-fence correction. |

For the two stock fixture profiles, the actual stage image was 2,416,640
bytes. Native issue measured 265 and 277 ms after schema preparation.
The managed attempt authority checks measured 390 and 394 ms. These are
private fixture timings from two runs. Relay latency and view cadence are
measured separately in the combined scheduler.

With the stock Linux target, the focused tests are selected by
`manual_due_uses_canonical_offer_coordinates_without_updating_floors` on
`cargo test --locked -p marmish-native --test peer_cursor_store` and
`cold_join_started_journal_only_selects_exact_readback` on
`cargo test --locked -p marmish-native --lib`. The same library's
`managed_consent_queue_and_attempt_preserve_exact_authority` is opt-in
with `-- --ignored`.
