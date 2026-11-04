# Native command lookup and diagnostic validation

This source increment implements [the approved design](../phases/native-command-path-errors.md). It follows the background daemon increment at `13b460a6b4c29260e13eecb6a1d9a08125178700`. Published v0.2.2 binaries predate these changes. Both peers require coordinated upgrades for the new closed wire values; no new release is claimed here.

## Independent reviews

Actual Claude Code ran in `--permission-mode auto`, model `claude-sonnet-5-5`. Design round 1 rejected incomplete launch/alias/error contracts; round 2 approved the amended design before implementation. Code round 1 caught the implicit libc shell fallback on invalid executable format. A separately approved direct-execve amendment fixed that behavior. [Code round 2](../reviews/native-command-path-errors-code-r2.md) approved the integrated source. [Code round 3](../reviews/native-command-path-errors-code-r3.md) approved the subsequent definitive-refusal journal transition fix. Reviews were static; executable validation is root-owned and recorded separately below.

The final refusal fix is substantive: after a real Dispatch refuses before engine admission, the row returns to Accepted, retains the immutable refusal and clears obsolete uncertainty. This releases the active slot and permits cleanup after restart. Engine-known and genuinely uncertain-launch paths retain their previous behavior.

## Local checks

- `cargo test -p marmish-engine -p marmish-prototype --locked`: passed. Engine: 115 library, 2 concurrent-storage, 3 encrypted-storage, 14 engine integration and 5 native-storage tests (139 total). Prototype: 2 library and 9 bridge tests (11 total). Binary/doc targets also succeeded. Nested subprocess test summaries are not counted again.
- Engine fault injection: 8 passed in 34.53 seconds using explicit, separately built production and fault-worker artifacts through `MARMISH_TEST_PRODUCTION_WORKER` and `MARMISH_TEST_FAULT_WORKER`. The initial attempt without those required fixture variables failed configuration; it was not a product failure or a successful test run.
- Native all-feature library suite: **481 passed, zero failed**, 366.73 seconds, with no concurrent executable rebuild. This includes the real-engine refusal/cleanup fix. The subsequent fixture-only credential timeout amendment passed all 22 identity tests with 8 threads, followed by **20 further parallel identity-suite repetitions** (440 passing executions); no failure was reproduced. These passes do not establish the original CI error cause.
- Strict Clippy on native, engine and prototype, all targets and all features: passed. One lazy Option closure was replaced by equivalent Option::and as suggested.
- Rust formatting and `git diff --check`: passed.

Coverage includes PATH order/unset/empty/relative/non-UTF8 inputs and bounds; canonical and alias deny policy; literal argv and original argv0; non-executable files, directories and symlink loops; explicit environment image; invalid executable format with no implicit shell or side effect; missing interpreter; private pre-exec error separation; persistence, replay and known-invocation recovery. Native tests exercise 140 sequential policy refusals followed by an allowed dispatch without execute-token exhaustion.

An earlier native run passed 480 tests and failed the real-engine cleanup test, exposing the stale Dispatching/uncertainty bug. The fix has a real-Dispatch, reopen/replay/cleanup regression. Earlier engine integration expectations were corrected to distinguish InvalidRequest from actual policy denial.

The first live trial then caught a stale wire guard: validate_submit still rejected bare argv0 before the server resolver could run. The guard now rejects empty argv0 and NUL while preserving all size limits; the existing design already requires bare and relative names. Exact wire encode/decode/re-encode tests cover bare, relative, non-UTF8, metacharacter and empty-tail bytes, and negative empty/NUL inputs. [The final wire review](../reviews/native-command-path-errors-wire-final.md) approved this correction. The subsequent affected native suite passed **315 tests, zero failed**, 168 MDK tests filtered, in 40.82 seconds. The earlier 481-test full run predates only the later fixture and wire-guard corrections; it is not described as a full final-source rerun. Both daemons from the failed live trial emitted joined cleanup receipts and their owned private fixture was removed.

## Static quality disposition

`ripwire --pr-context` identifies the engine/native wire, persistence and launch boundaries; whole-crate tests and live commands are selected for those boundaries. `ripwire --quality-delta`, measured at `13b460a6b+dirty`, exits 2: 129 findings, 56 gating. It is not reported as a green gate. Most gating rows are historical short-horizon churn. Remaining rows include the test-only policy helper missed by the static call graph, deliberate small errno fixtures, similar bounded daemon-handshake tests, and added lines in finite wire/CLI/launch handling and regression fixtures. Real admission/launch/committed-diagnostic hub growth was reduced through focused helpers. Explicit exhaustive mappings and direct lifecycle handling remain; no baseline or suppression was added to hide findings. All new unsafe execution-image ownership and callback behavior received the actual Claude reviews above. Nonblocking replacement/cwd-race diagnostic and defensive engine-caller refinements are tracked in Beads `marmish-wyl`.

## Platform follow-up

The preceding daemon candidate [41887894604](https://github.com/Epiphytic/marmish/actions/runs/41887894604), source `13b460a`, failed macOS native verification in two startup-channel fixtures and Linux AMD64 in an existing interrupted-credential-helper fixture. Linux ARM64 passed native verification and packaging, then its installed-feature build/test step failed the timeout-helper fixture (470 passed, 1 failed). The actual error variant was not logged; a root cause is not established. Installed qualification did not run. This increment includes the reviewed Darwin poll/recv startup correction and the Linux test-only helper request-read synchronization. These are not evidence of a successful macOS run. Candidate results for the new source must be recorded separately.

## Final live relay result

The [sanitized machine-readable record](native-command-path-errors-linux.json) passed all **15 command cases** plus background readiness, private diagnostic checks and joined cleanup. It used final rebuilt local Rust debug binaries on Linux AMD64, fresh file-backed identities, Tor off and the explicitly selected `wss://relay.us.whitenoise.chat/`. This is actual authenticated Marmot command execution, not a relay-publication check or installed-release qualification.

The server inherited a private custom PATH prefix; the client found that server-only command by its bare name and received exactly 50 bytes containing dollar signs, semicolons, backticks, quotes, backslashes, slash, newline, tab and Unicode. Standard bare lookup, `/bin` alias, cwd-relative lookup and original argv0 also passed. The other cases verified missing bare/absolute paths, missing execute bits, a directory, a symlink loop, additive default/per-key and canonical symlink policy refusals, invalid executable format and a missing interpreter. Every refusal returned its expected category/reason and empty stdout. The logs contained finite reasons and excluded the private argv sentinel and PATH. Both 180-second-bounded daemons emitted joined engine/MDK/runtime cleanup receipts with no panic; private state and keys were removed.

| Measured operation | Elapsed |
| --- | ---: |
| Background server readiness | 738 ms |
| First command, including cold local client startup | 4,297 ms |
| Five successful command calls, median | 2,915 ms |
| Ten expected error calls, median | 1,942 ms |
| Full run, including the bounded daemon lifetime and cleanup | 186,507 ms |

These are end-to-end CLI timings for tiny outputs on one public relay, including protocol, scheduling and finish-delivery overhead. They are not bulk-throughput measurements or macOS performance results. SHA-256 and byte counts are retained without raw output or identities. Final strict Clippy and native production-bin rebuild passed after the wire correction; formatting and staged whitespace checks passed.
