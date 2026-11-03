# Hosted native foundation: first observed run

Latest observed run: [41565379100](https://github.com/Epiphytic/marmish/actions/runs/41565379100),
attempt 1, source `22019f9de06327cbab7c7df373d5faf260625cff`, failed its owner
readiness check. All four jobs started within a minute; all three client
platforms passed offline tests, release builds and direct encrypted-message
checks. Cancellation was requested after those direct checks because the failed
owner could no longer publish the onion endpoint. No onion client acceptance
follows from this run.

The new [finite owner report](hosted-41565379100-owner-cleanup.json) locates the
failure at `native_check`: an initial relay reached Connected, then another
Arti bootstrap completed and three connection attempts timed out before obtaining
an onion stream. The connector's own 15-second timer covers stream acquisition
and TLS/WebSocket together; the SDK startup turn separately uses 15 seconds.
This identifies a local deadline that can interrupt rendezvous construction. It
does not prove that a longer deadline will succeed or that all shared Arti work
restarts on cancellation. Relay children were reaped and private state removed;
native readiness cleanup remains unproven. The
[route-aware timing amendment](../../phases/v0.2-phase1-onion-timeouts.md)
must be reviewed and validated before another full matrix.

The final workflow conclusion is cancelled after owner failure. Preserved direct
reports verify all cleanup flags and four 66-byte exchanges per platform:

| Runner | Total | Cold activation | First A→B / B→A | Warm activation |
| --- | ---: | ---: | ---: | ---: |
| [Linux AMD64](hosted-41565379100-linux-amd64-direct.json) | 15.475 s | 349 ms | 357 / 470 ms | 375 ms |
| [Linux ARM64](hosted-41565379100-linux-arm64-direct.json) | 9.673 s | 369 ms | 352 / 349 ms | 468 ms |
| [macOS ARM64](hosted-41565379100-macos-direct.json) | 7.916 s | 941 ms | 466 / 459 ms | 991 ms |

These are tiny encrypted-message/receipt observations, not command latency,
bulk throughput or percentile estimates. No onion client flow ran.

Previous observed run: [41562508066](https://github.com/Epiphytic/marmish/actions/runs/41562508066),
attempt 1, source `bc75231530eb1fc4ef8db60d93c3528d5e9e71bd`, cannot pass: its
owner failed before publishing onion readiness, about 80 s after fixture launch.
All three platform direct checks passed. Once those results were available,
the remaining clients were cancelled because the failed owner could no longer
publish the endpoint. The run is failed/cancelled, not Tor acceptance.
The [owner cleanup report](hosted-41562508066-owner-cleanup.json)
proves relay children reaped and private state removed, but reports native
readiness cleanup unproven. The fixture deleted its private check report/logs,
so neither the failure stage nor the native cause is recoverable from published
evidence. No claim that the startup amendment fixed prior failures follows.
The [reviewed diagnostic amendment](../../phases/v0.2-phase1-owner-diagnostics.md)
preserves only finite stages and allowlisted events in future owner cleanup
reports. Earlier failed runs below remain unchanged.

All three jobs passed their native, vault and helper release tests and release
builds before their direct checks. Preserved direct reports:

| Runner | Total | Cold activation | First A→B / B→A | Warm activation |
| --- | ---: | ---: | ---: | ---: |
| [Linux AMD64](hosted-41562508066-linux-amd64-direct.json) | 14.762 s | 812 ms | 406 / 407 ms | 797 ms |
| [Linux ARM64](hosted-41562508066-linux-arm64-direct.json) | 11.433 s | 1,123 ms | 286 / 296 ms | 994 ms |
| [macOS ARM64](hosted-41562508066-macos-direct.json) | 7.869 s | 918 ms | 428 / 426 ms | 1,046 ms |

Each report records encrypted exchanges, receipts and restart with cleanup true.
These fixed 66-byte receipt measurements are neither bulk throughput nor command
execution, and the cancelled clients did not attempt the unavailable onion flow.

[Run 41516397519](https://github.com/Epiphytic/marmish/actions/runs/41516397519),
attempt 1, tested source `f99d95aa8af09f2133aac99b384ed19feaf249ab`.
These are native encrypted-message checks, **not remote command, installer or
nine-pair interoperability acceptance**. They use disposable file-backed
identities, an explicitly selected direct relay and a separately owned native
onion relay. The GitHub connector supplied job/artifact metadata; `gh run download`
retrieved the binary artifact archives because the connector lacks that operation.

Both Linux architectures passed offline release tests/build and actual direct
and embedded-Arti flows. Each report records four authenticated 66-byte binary
exchanges with durable receipts, encrypted restart, duplicate/conflicting-ID
checks and all native cleanup flags true. Preserved reports:

| Runner | Route | Total | Cold activation | First A→B / B→A exchange | Warm activation |
| --- | --- | ---: | ---: | ---: | ---: |
| Linux AMD64 | Direct | 16.793 s | 846 ms | 463 / 467 ms | 1,104 ms |
| Linux AMD64 | Arti onion | 80.055 s | 36,684 ms | 1,437 / 1,633 ms | 21,886 ms |
| Linux ARM64 | Direct | 10.419 s | 637 ms | 375 / 373 ms | 607 ms |
| Linux ARM64 | Arti onion | 87.570 s | 53,883 ms | 1,682 / 2,004 ms | 16,041 ms |

Reports: [AMD64 direct](hosted-41516397519-linux-amd64-direct.json),
[AMD64 onion](hosted-41516397519-linux-amd64-onion.json),
[ARM64 direct](hosted-41516397519-linux-arm64-direct.json),
[ARM64 onion](hosted-41516397519-linux-arm64-onion.json).
These single observations include durable application receipt processing.
Tiny-message bytes/second fields are not bulk throughput or percentile estimates.

macOS compiled successfully, then failed 23 journal tests in their common
fixture while 67 other native tests passed. All failures were
`Storage("journal")` at the initial journal open. The pinned SQLCipher
`SQLITE_OPEN_NOFOLLOW` implementation rejects symlinks in any ancestor;
the fixture supplied an uncanonicalized temporary path. Production profiles
already canonicalize their base. The precise hosted temporary-path spelling was
not logged, so that spelling is an inference from the source mechanism.

The [scoped independent design review](../../reviews/v0.2-native-macos-fixture-design.md)
approved canonicalizing only the fixture root and adding an owned symlink
regression. The local release journal suite passed **24/24**, including the new
ancestor and immediate-parent refusal checks and canonical open/reopen.
Production database path protections remain intact. A full hosted macOS rerun
is required; this first run is **failed**, not platform acceptance.

The [completed-code review](../../reviews/v0.2-native-macos-fixture-code.md)
approved this increment and independently reproduced **23/23 failures** with the
old fixture under a symlinked `TMPDIR`, then **24/24 passes** with the fix under
the same conditions and under `/tmp`. Root's all-target native Clippy with
`-D warnings`, formatting and diff checks passed. The static quality delta
returned exit 2: it conflated the changed journal `tests` module with an unchanged
engine `tests` module for verbosity and called the new `#[test]` dead code.
The exact source diff is confined to the journal test module; no baseline or
suppression was added, and the static check is not reported as passing.

The owner successfully proved native encrypted onion readiness, observed both
Linux terminal successes and the macOS terminal failure, and deliberately failed
its rollup. Its cleanup report records relay children reaped, private state
removed and native readiness cleanup true. The cleanup step explicitly does not
claim to reap its own still-running supervisor; that process exits separately.
No v0.2 release was published by this workflow.

## Rerun after the macOS path fixture correction

[Run 41519670193](https://github.com/Epiphytic/marmish/actions/runs/41519670193),
attempt 1, tested `112bb3a330f9ef909599fbe1abbb435ed5cc06aa`. All runners began
within a minute. Both Linux jobs passed completely. macOS passed **91 native
tests, 8 vault tests, 2 helper tests**, the release build and the direct flow.
Its journal fixture failure is resolved. The selected-keychain provider was
ignored in this matrix and [qualified separately](macos-v2-provider.md).

| Runner | Route | Result | Total | Cold activation | First A→B / B→A | Warm activation |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| Linux AMD64 | Direct | Pass | 14.424 s | 152 ms | 343 / 348 ms | 395 ms |
| Linux AMD64 | Arti onion | Pass | 71.377 s | 23,024 ms | 1,789 / 2,137 ms | 20,232 ms |
| Linux ARM64 | Direct | Pass | 10.092 s | 666 ms | 348 / 351 ms | 478 ms |
| Linux ARM64 | Arti onion | Pass | 56.207 s | 22,933 ms | 1,477 / 1,780 ms | 16,879 ms |
| macOS ARM64 | Direct | Pass | 6.099 s | 536 ms | 390 / 356 ms | 602 ms |
| macOS ARM64 | Arti onion | **Fail** | 105.258 s | 28,485 ms | 1,857 / 1,543 ms | 36,730 ms |

Reports: [AMD64 direct](hosted-41519670193-linux-amd64-direct.json),
[AMD64 onion](hosted-41519670193-linux-amd64-onion.json),
[ARM64 direct](hosted-41519670193-linux-arm64-direct.json),
[ARM64 onion](hosted-41519670193-linux-arm64-onion.json),
[macOS direct](hosted-41519670193-macos-direct.json),
[macOS onion failure](hosted-41519670193-macos-onion.json).

The macOS onion report proves initial encrypted exchanges and warm restart
activation succeeded, then records a `transport` error during
`restart_alice_to_bob`. It does not identify the underlying transport label.
The 105.258-second total is below the overall deadline; the exchange also had
its own 120-second budget. Source inspection found that the check propagates
the first owner tick transport error, whereas the daemon retries recoverable
transport errors and per-turn timeouts. That mismatch warrants a bounded driver
correction and another full matrix, but does not prove which transport failure
occurred in this run. No macOS Tor acceptance is claimed.

All native reports recorded cleanup true. The owner recorded relay children
reaped, private state removed and readiness cleanup true, observed the macOS
terminal failure, and correctly failed the rollup. The overall run is failed.
These remain tiny-message measurements, not bulk throughput or percentiles,
and do not test remote commands, installation or the nine command pairs.

## Bounded recovery-check correction

The [reviewed amendment](../../phases/v0.2-phase1-recovery-check.md) changes only
the foundation-check driver and its test dependency. Join and exchange waits
give each peer a turn bounded by 15 seconds and the original fixed deadline.
Recoverable tick errors preserve progress; initial submission remains one-shot,
and success still requires exact received bytes and a durable sender receipt.
Finite additive diagnostics distinguish the active operation, last recoverable
role/label and terminal transport label without exposing raw errors or state.
This does not establish the cause of the earlier macOS failure.

The focused offline suite passed **13/13**, including nine new scripted
recovery/schema tests. Native/vault all-target Clippy with warnings denied,
formatting and diff checks passed. Cargo.lock is unchanged. The static quality
delta returned exit 2: `execute` grew from 154 to 173 lines as call sites gained
explicit message/progress structs and diagnostics; it also flagged new private
types/tests as unreachable and similarity between two scripted tests. Those
types and tests are compiled and exercised, and the test cases cover distinct
join and message paths. This tradeoff is recorded without a suppression or
baseline change; the static check is not reported as passing.

Claude's first completed-code review approved the implementation and independently
passed all **99** then-current native library tests. Its useful suggestion to
clear stale recovery diagnostics between stages was incorporated with an
additional scripted regression. The [follow-up code review](../../reviews/v0.2-native-recovery-code-r2.md)
approved that correction and independently passed all 13 focused tests and
strict Clippy/format/diff checks. A fresh full hosted run remains required. The failed
run above is preserved; no platform acceptance follows from offline tests.

## Recovery-driver run: startup failures remain open

[Run 41525523478](https://github.com/Epiphytic/marmish/actions/runs/41525523478),
attempt 1, tested `8a3c0a6e571917a4ee90c4022df410baf2b4330a`. All four jobs
started within a minute. All three client platforms passed **100 native tests**,
8 vault tests, their platform helper tests and the release build. Linux AMD64
passed direct and Arti flows. Linux ARM64 passed direct flow, then failed during
cold Arti activation. macOS failed during cold direct activation, so its onion
step did not run. The overall run and owner rollup correctly failed.

| Runner | Route | Result | Total | Cold activation | First A→B / B→A | Warm activation |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| Linux AMD64 | Direct | Pass | 17.507 s | 1,447 ms | 476 / 478 ms | 1,160 ms |
| Linux AMD64 | Arti onion | Pass | 73.429 s | 27,725 ms | 2,556 / 1,911 ms | 18,439 ms |
| Linux ARM64 | Direct | Pass | 11.018 s | 751 ms | 413 / 415 ms | 893 ms |
| Linux ARM64 | Arti onion | **Fail** | 55.821 s | — | — | — |
| macOS ARM64 | Direct | **Fail** | 48.871 s | — | — | — |

Reports: [AMD64 direct](hosted-41525523478-linux-amd64-direct.json),
[AMD64 onion](hosted-41525523478-linux-amd64-onion.json),
[ARM64 direct](hosted-41525523478-linux-arm64-direct.json),
[ARM64 onion failure](hosted-41525523478-linux-arm64-onion.json),
[macOS direct failure](hosted-41525523478-macos-direct.json).
Each failure records `activation_cold`, operation `activation`, category
`transport`, terminal label `other`, and no completed samples. These occurred
in the one-shot activation operation, before the amended tick driver. The
precise underlying transport cause is not known from these reports. Neither
the recovery amendment's effectiveness against the previous post-restart failure
nor platform acceptance is established by this run.

All five native reports recorded runtime/state/combined cleanup true. The owner
recorded relay children reaped, private state removed and readiness cleanup true;
its separate supervisor exit is not claimed as reaped by that cleanup step.
The owner observed one successful and two failed client outcomes. These are
tiny-message latency observations, not bulk-throughput or command acceptance.
