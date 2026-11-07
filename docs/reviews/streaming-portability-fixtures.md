# Streaming portability correction review

Scope: `docs/phases/streaming-portability-fixtures.md`, against baseline
`ad9cf27e184214d826ea1876b3c0964b69448739`; Beads `marmish-8ka.5.2`
and `marmish-8ka.5.3`. This is unreleased source qualification, not a release
or a claim that the installed platform matrix passed.

## Independent design review

Actual Claude Code, Claude Sonnet 5.5, `--permission-mode auto`, session
`e68f4659-76f5-4b93-8b2f-70109f277f62`:

| Round | Verdict | Duration | Turns |
| --- | --- | --- | --- |
| 1 | REQUEST_CHANGES | 102.266 s | 15 |
| 2 | APPROVE | 31.603 s | 4 |
| 3 | APPROVE, correcting one earlier clarification | 15.189 s | 3 |

The first review required positive proof that isolated test subprocesses ran,
a distinct parked-reply cancellation barrier with red/green evidence, and an
explicit production amendment for transient context-writer inheritance. All
three were incorporated before implementation. The second review approved the
amended design. Its suggestion that held-writer expiry should be Protocol was
incorrect; the third review inspected the existing assertions and confirmed
LocalIo for expiry, preserving Protocol for malformed EOF and overflow.

The production reader keeps one absolute one-second deadline, mandatory EOF,
the 4097-byte sentinel allocation, descriptor validation and compiled-build
checks. A private readiness-wait boundary supports deterministic real-pipe
tests; it introduces no environment or global hook. A writer retained beyond
the deadline can still cause safe refusal. Source inspection found no shorter
timer in the immediate helper setup caller; installed operation remains a
separate required check.

The design reviews were read-only and ran no Cargo or network tests. macOS
behavior must be confirmed by execution, not these approvals.

## Regression evidence

The cancellation red run used detached baseline `ad9cf27` plus only the bounded,
stop-aware parked-reply hook and the assertion waiting for its distinct metric.
The unchanged control lane failed with `cancel_ack_timeout` in 5.94 s. Cleanup
and worker-join assertions executed before that expected failure.

SHA-256 identifiers for that red run:

- Integration fixture: `36585122165e31e9c6a6952f3dc6dfb61a036949a74fd23c09250a4058992b74`
- Cancellation fixture: `ba2b479c1ab6daa23d3b7dbaa2dd0b27d739b1ec0315086de993d5efe81dc368`
- Execution log: `6ceb0325cb3a710434ae778004118d8b5665b39a1c0cbb98fdece67aaa5f156b`

The identical parked-reply scenario passed after the dedicated Cancel lane
correction: one test, both directions, 6.38 s. The full cancellation module
passed four tests in 75.49 s, covering eight direction/barrier/buffered cases
and the closed-control regression. Additional Linux AMD64, UID 1001 checks:

| Check | Result | Test duration |
| --- | --- | --- |
| Filesystem cases, including portable and Linux raw-byte names | 34 passed | 20.42 s |
| Helper context reader, including progress deadline regression | 11 passed | 3.00 s |
| Engine copy launch, normal parallelism | 11 passed | 0.19 s |
| Local copy journal | 13 passed | 1.74 s |
| Canonical-parent database symlink refusal | 1 passed | 0.41 s |
| Preparation interrupt and stdio restoration | 1 passed | 0.24 s |
| Descriptor restoration on success/partial failure | 1 passed | <0.01 s |

Commands used `cargo test -p marmish-native --lib` with the corresponding module
or exact test-name filter; engine used `cargo test -p marmish-engine --lib
copy_launch::tests`. Strict native all-target Clippy passed, followed by combined
native/engine all-target Clippy with `marmish-native/installed-checks` enabled
and `-D warnings` (17.09 s). `cargo fmt --all -- --check` passed. Compiler was
rustc 1.97.1, target x86_64-unknown-linux-gnu. No relay was used.

## Independent integrated code review

Actual Claude Code, Claude Sonnet 5.5, `--permission-mode auto`, session
`7355c5fd-9aa0-4370-89af-0e2e249e4ccd`, round 1: **APPROVE**, conditional
on clean regression evidence; 120.565 s, 16 turns. The reviewer inspected all
eight changed Rust files against baseline, README, agent specification, design
and this record. It ran no tests because an author held the shared Cargo lane.
The review found no blocking code defects and requested repeated launch and
cancellation checks. The suggested progress regression was added: three actual
WouldBlock events (two further chunks and separate EOF) must pass the identical
absolute Instant to the production poll. Combined with the existing real expiry
check, it detects renewal without sleep-based timing assumptions. Its final
execution is included in the follow-up review scope.

Review dispositions and limits:

- The parked post-receipt cases prove durable commit with the receipt-bearing
  IPC reply still unobserved, followed by Cancel and fresh Resume. They do not
  separately establish cancellation after the frontend has observed that reply.
- Context reading precedes installation of the helper SIGTERM handler. A signal
  during the bounded context wait can therefore terminate it through the default
  signal action; helper exit 1 is an assertion for the later parked copy cases,
  not a universal cancellation exit guarantee.
- The README signal clarification follows `ExecutionSignals::register` and the
  existing rsync validation; the corrective margin statement follows the
  separately published [selected rerun evidence](../validation/copy-runtime-live-r5-corrective.md).
- Darwin polling, the descriptor flag mask and portable filename behavior remain
  source-reviewed until a new hosted macOS execution passes. No review or Linux
  component test qualifies the installer or full platform matrix.

The quality-delta check at `dcb7aacc1+dirty` exited 2: 30 rows, 14 major
preexisting-worse gates, including test-bridge complexity 80 to 105, nesting 4
to 5, fixture duplication/verbosity and short-horizon churn. New test functions
also appear as dead code in its static model despite executed test evidence.
This is not a clean quality result. The bridge's scoped request future is a
deliberate cancellation boundary; refactoring its state machine in this bug fix
would expand the reviewed change. Production context code adds no reported
complexity/verbosity gate; its reported gate is churn. Existing Beads
`marmish-8zu` tracks the broader complexity debt. The bounded change-context
report found affected test paths for the production helper and launch changes;
its name-based counts overstate precise runtime reach and do not substitute for
the component and installed checks.

All 11 engine copy-launch tests passed another 20 invocations under their normal
parallel execution, including concurrent spawn and the unmatched-filter negative
case. The expanded actual-helper integration module passed 14 tests with two
explicitly ignored large workloads in 102.53 s, including another complete
cancellation-module run. No ignored case is counted as passed.

Round 2, same actual Claude session: **APPROVE**, 137.281 s, nine turns. The
reviewer independently ran the final 11 context tests (3.00 s), the third full
cancellation module (four passed, 75.66 s), and two further post-receipt runs
(one passed each, 6.35 s and 6.37 s). It also ran the combined strict all-target
native/engine Clippy with installed checks, formatting and diff checks; all
passed. It inspected the new progress regression and confirmed that it closes
the deadline-renewal test gap. It made no edits, Git changes or network calls.
The previously conditional source approval is now satisfied for Linux x86_64
component regression scope. [Sanitized execution evidence](../validation/streaming-portability-local.json)
records source and log hashes without retaining private runtime state.

The previous hosted candidate remains failed; these corrections do not
retroactively qualify it. A fresh exact-source hosted candidate and installed
cohorts remain required. No release assets or version pins change here.
