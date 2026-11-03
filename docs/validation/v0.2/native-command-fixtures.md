# Native command fixture portability checks

Beads `marmish-h75`; based on source `540e39cf0` and the reviewed test-only corrections. No production executable resolution, Darwin identity/FD policy, test skip, or acceptance rule changed.

## Trigger and scope

[Candidate 41626814627](https://github.com/Epiphytic/marmish/actions/runs/41626814627) passed 108 macOS engine unit tests and the concurrent/encrypted storage integration binaries, then the engine integration binary reported 13 passing and one failing test. `partial_job_directories_are_reaped_before_maintenance_and_submission` requested `/bin/true` and observed `NotStarted` instead of exit 0. The successful-execution fixture now uses `/usr/bin/true`, already successfully executed by other tests in the same native binary. The failure is not evidence that production canonical validation should resolve aliases.

The related native canonical-refusal regression now points its alias to the existing private `worker` file created by its own fixture, verifies that target is a file, and retains noncanonical refusal, zero-job, and known-Missing assertions. The target is not executed via the alias.

Only the initial three-package `Verify native components` Cargo command in `v02-release.yml` gains `--no-fail-fast`. A failed binary still makes Cargo exit nonzero and the shell stop; later test binaries and doctests can reveal additional failures in the same candidate. Normal native CI and the later installed-checks command are unchanged. An initial edit to normal CI was identified and reverted before publication; the corrected candidate workflow received a separate preimplementation design approval.

## Local Linux results

Run as ordinary UID 1001 on Linux:

| Check | Result |
| --- | --- |
| `cargo test --offline --locked -p marmish-engine --test engine` | 14 passed; 0 failed, ignored or filtered; 2.61 s |
| `cargo test --offline --locked -p marmish-native --lib real_engine_canonical_refusal_creates_no_job_and_known_missing_never_dispatches` | 1 passed; 311 filtered; 0.18 s |
| `cargo clippy --offline --locked -p marmish-engine -p marmish-native --all-targets -- -D warnings` | passed; 4.23 s |
| Exact-file rustfmt checks, engine edition 2021/native edition 2024 | passed |
| Candidate workflow actionlint and `git diff --check` | passed |
| Engine ripwire quality delta against HEAD | no regressions, gating 0 |
| Disposable Cargo package with failing first integration binary and succeeding later sentinel binary, `--no-fail-fast` | both executed; Cargo exit 101 retained |

The disposable Cargo probe created no repository files or dependencies and removed its temporary package. These results are Linux-only. Fresh full macOS qualification, including later native/vault tests, remains required; the failed candidate is not claimed as a release pass.

## Literal fixture audit

An exhaustive `/bin/true` literal search (which also matches `/usr/bin/true`) over `crates`, `tests`, `tools`, `scripts` and `.github`, excluding lockfiles, found 62 occurrences before correction. The two v0.2 executable-dependent assumptions above were corrected. The other bare `/bin/true` Rust occurrences are parser input (`cli.rs`), wire/probe payload (`command_adapter.rs`), ledger-only payload (`command_store.rs`), authenticated request/refusal payload without actual execution (`mdk_adapter_tests.rs`), or a deliberately conflicting legacy invocation (`marmish-prototype/tests/bridge.rs`). They do not require that executable to exist. Canonical `/usr/bin/true` execution/FD fixtures and malformed path rejection vectors remain intact. Legacy Python fixture paths remain isolated from v0.2 runtime. Other actual `/bin/sh`, `/bin/cat`, `/bin/echo` commands in the same macOS engine integration suite passed; no broad path replacement or platform test serialization was introduced.

## Frozen source hashes

* `crates/marmish-engine/tests/engine.rs`: `46b60b3e3e54ac1b07e463c5f14d78d8369c72332dce8f52e1f924c23db5a657`
* `crates/marmish-native/src/command_engine.rs`: `2d13f34170b74bf433c12927ef4b964de1cd427dd31725ec18526af4fad1d720`
* `.github/workflows/v02-release.yml`: `86f82578c5ec2beb84083f9b43029d4428fee2f9600f7c7eb2bc4329daa00f44`

Actual preimplementation design approvals are [the initial review](../../reviews/v0.2-native-command-fixtures-design-r1.md) and [the corrected-workflow addendum](../../reviews/v0.2-native-command-fixtures-design-addendum.md). [Actual completed-code review](../../reviews/v0.2-native-command-fixtures-code-r1.md) APPROVED, session `b0585aed-d86f-4cfa-b689-a82b95a5c7fd`; the reviewer independently passed the same 14+1 focused tests.
