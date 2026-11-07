# Native copy local validation (source only)

Linux AMD64 development worktree based on rsync increment
`33753e271595da1651780a5d80fb0d067b6293f4`; reviewed Phase 3 source was
published as `64ee9fb409e6edde4f86a826a8b3d4401087d87c`.
These are local native/unit/IPC tests, separate from real relay and installed
cross-platform qualification. The published v0.2.3 package has no native copy.

| Check | Result | Snapshot limit |
| --- | --- | --- |
| All `copy_` native tests | 90 passed, one expected ignored helper subprocess entry, 172.80 s | Before final 8179-byte packing/replay fixes |
| Focused `copy_` cohort excluding long helper integration | 90 passed, 32.40 s | Includes packing, remaining-credit and worker lock-barrier regression; before final cached Claim/replay cap changes |
| Local receiver GC and command-room route | 20 passed, 19.20 s | Includes release guards, replay/timeout tests, actual client OpenCopy→Activate and fresh/cached activation revocation guards |
| Final helper integration | 6 passed, one expected ignored helper entry, 102.67 s | Includes production residual credit, exact metadata, corrupt-prefix refusal and real checkpoint/Commit-reply disconnect recovery |
| Session IPC matrix | 6 passed, 0.10 s | Exact retry/deadline, ACK regression, EOF/terminal and denial |
| Abort IPC matrix | 4 passed, 0.13 s | Includes lost transport leaving cleanup unconfirmed |
| Worker bound matrix | 5 passed, 0.08 s | Packed read size and physical worker ownership |
| Large-file filesystem cohort | 28 passed, 21.46 s | Adds 66 MiB + 17-byte zero and pseudo-random files, repeated checkpoints, actual uncheckpointed-suffix truncation, literal paths, committed receipt resume and unsupported-mode refusal; no helper/codec/relay coverage |
| CLI regression suite | 56 passed, 7.72 s | Before warning-only cleanup and DTO boxing |
| Strict native Clippy, all targets, warnings denied | Pass, 12.323 s command time | Final product guards and strengthened actor test; all targets, warnings denied |

The actual hidden-helper integration covers >2 MiB raw/zlib transfers in both
directions, literal unusual names, metadata, empty files, committed resume,
conflicting destinations, durable partial resume, changed-source refusal and
missing-source categories. It uses actual owned subprocesses and native IPC,
not a relay. An earlier packing run passed five tests in 139.14 seconds; the strengthened
final helper cohort passed six in 102.67 seconds (one expected ignored helper
subprocess entry in each). The additional case drops actual IPC replies after
a durable checkpoint and after Commit, then starts a fresh helper to verify
partial-prefix reuse or zero-Data committed-receipt recovery.

Four additional actual IPC cleanup regressions exercise malformed output,
invalid durable credit, and delayed typed peer errors after stdin closure. They
check full transport ACK accounting, Error/EOF drain and physical terminal
confirmation, without implicitly sending Cancel. Worker tests include a held
physical lock that remains owned after a shutdown timeout; uncertain cleanup
does not release receiver ownership.

The [relay results](copy-runtime-live.md) retain an early timeout and a later
verified 2 MiB upload followed by a Get admission failure. After fixing the
authenticated room lookup, a 12-case small relay smoke passed. Independent
integrated source review is approved; default large-file and installer/platform
qualification remain required before phase completion.

## Static quality limits

The quality delta against the rsync base is **not clean**: 428 reported rows,
125 major preexisting-symbol gates. Some basename-only symbol identities, test
clones and short-horizon churn need individual interpretation. A separate
read-only review confirmed real complexity growth in six production dispatch
functions: engine `Worker::run`, actor `prepare_streams`,
`project_stream_request`, `reclaim_stream_state`, `stream_request_local`, and CLI
`run_stream_command`. Their added branches reflect typed copy admission and GC
sequencing; they increase the cost of auditing authentication order and claim
completion. They are not all false positives. No correctness defect was proved
by the static metrics. Bounded extraction followups are tracked in Beads;
current source is frozen for functional review and qualification rather than
silently suppressing this result.

## Reproduction commands

Working directory: repository root. These commands used
`CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target` on the qualification
machine; a fresh checkout can use its own target directory.

```sh
cargo test -p marmish-native --lib copy_integration_tests -- --nocapture
cargo test -p marmish-native --lib stream_session::tests -- --nocapture
cargo test -p marmish-native --lib copy_io_abort_tests -- --nocapture
cargo test -p marmish-native --lib copy_worker::tests -- --nocapture
cargo test -p marmish-native --lib copy_local -- --nocapture
cargo clippy -p marmish-native --all-targets -- -D warnings
cargo build -p marmish-native --bins
```

Test logs are development evidence, not shipped runtime output. The final receiver cohort passed 20 tests in 19.20 seconds; strict Clippy passed
(11.98 seconds reported by Cargo, then 12.323 seconds command time after a
test-only strengthening); native binaries built in 17.472 seconds command time.
The authenticated replay/revocation seam test passed in 6.73 seconds after the
final sentinel assertions. All helper integration qualification used debug builds.

## Large-file filesystem followup

The reviewed source followup adds two tests to `copy_files.rs`; no product code
changes. Each 66 MiB plus 17-byte payload is generated and consumed in bounded
8192-byte blocks. The test independently computes its expected hash, checks
Source length/hash, performs 66 durable checkpoints, writes an uncheckpointed
suffix before dropping the receiver halfway, and verifies Resume truncates to
the exact retained offset/hash and physical length. It checks final content,
manifest mode 0640, exact directory entry count and a recovered committed receipt.
Invalid UTF-8 bytes are tested only on Linux; legal literal metacharacters are
used on other platforms. Actual execution here is Linux AMD64 only.

Modes without owner read/write and special permission bits are deliberately
unsupported: 000, 0400, 0200 and 07777 return Protocol before creating any state.
The first draft's assumption that 0400 was preservable was corrected against the
actual protocol contract. This is refusal coverage, not a new permission feature.

Reproduction: `cargo test -p marmish-native --lib copy_files::tests -- --nocapture`.
All 28 tests passed in 21.46 seconds; strict native all-target Clippy passed in
12.47 seconds after the final assertions. This does not qualify 66 MiB through
a helper, compressed transport or a relay. Source metadata-to-manifest mapping
is covered separately by helper tests, not inferred from this receiver test.

The incremental quality delta against `64ee9fb` reports five rows and one gate:
a short assertion-pattern clone between the mode-refusal and existing quota test,
two Rust test-entry dead-code reports, and complexity 20 / 94-line length for the
large fixture test. The tests exercise different refusal contracts; no generic
abstraction was added solely to suppress the normalized-token clone. The earlier
production dispatch complexity findings remain tracked independently. No clean
static quality result is claimed.
