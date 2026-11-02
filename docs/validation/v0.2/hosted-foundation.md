# Hosted native foundation: first observed run

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
