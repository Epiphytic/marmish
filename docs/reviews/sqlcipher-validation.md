# SQLCipher storage validation

Storage increment Beads `marmish-315.13`, validated 2026-10-22. Plan approval is in `sqlcipher-plan-final.json`; completed-code review is recorded separately. Source/native dependency and worker artifact hashes are in `sqlcipher-build-evidence.json`.

## Compiled stack

Rust 1.97.1; SQLCipher **4.17.0 community** verified by runtime `PRAGMA cipher_version` in both an independent public-API integration test and cipher unit tests. Exact rusqlite 0.40.1, libsqlite3-sys 0.38.1 at git revision `5ae7fdf83085c595fd54f977b3a56ccacabaf16b`, openssl-src 300.6.1+3.6.3, openssl-sys 0.9.117. The old Rust 1.93.1 fails the native build script's `cfg_select!`; repository toolchain and stated requirement now match the tested MDK toolchain. Vendored builds require a C compiler, make and Perl.

## Passing checks

- `cargo fmt --all -- --check`.
- `cargo build --offline --locked --workspace`.
- `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`.
- Same Clippy command with `--features marmish-engine/fault-injection`.
- `cargo test --offline --locked --workspace`: **90 tests**, excluding nested helper subprocess repetitions.
- Separate release production build with `--no-default-features` in `target/phase1-production` and feature worker build in `target/phase1-fault`, following the phase-1 matrix.
- `ulimit -c 0` and explicit production/fault artifact variables; `cargo test --offline --locked --workspace --features marmish-engine/fault-injection`: **98 tests**, including eight cross-process fault tests. This adds six bootstrap crash positions inside one matrix, expiry between database/journal removal, direct keyed chunk inspection before/after spool COMMIT, and the original admission/spawn/stdin/output/exit/disconnect boundaries. Production worker ignores feature-only sentinel.
- Python offline transport regression: **23 tests**, plus `py_compile` for runner/tests. No public traffic was generated for storage validation.

Independent acceptance verifies actual keyed access, no-key/wrong-key rejection, command/stdin/stdout/stderr canaries absent from live database/WAL/spool artifacts, byte-exact keyed recovery and exact invocation replay. Failed wrong/missing-key opens preserve retained logical content and do not run maintenance or orphan cleanup. Cleanly shut-down roots are also checked for byte-identical persisted files; this is not a general byte-preservation promise. Claude independently reproduced that a failed wrong-key read-write open on a hot WAL may checkpoint raw encrypted frames and remove WAL/SHM sidecars while the correct key still recovers the same logical content. Live artifact scans run in another process to avoid releasing the frontend's SQLite POSIX locks. Key/publication tests cover unsafe modes, partial/staged/missing keys, hardlinks, symlinks and FIFOs; surviving-job damaged/empty-schema recovery refuses initialization. Real worker/child interfaces are checked for master/derived key bytes and hex, and private descriptor inheritance. Memory-security encryption also runs with RLIMIT_MEMLOCK zero. Wrong-key negative tests may emit fixed native SQLCipher HMAC failure diagnostics without key/payload content.

## Completed-code review dispositions

Claude Sonnet 5.5 approved the full increment in `sqlcipher-code-initial.json`, with no permission denials. It independently passed the workspace, format, Clippy and separate release-worker fault matrix, verified SQLCipher 4.17.0 and absence of fault labels in the production worker, and reproduced hot-WAL wrong-key behavior with a scratch probe.

The useful restrictive-umask finding is fixed by setting mode 0600 on the exclusively created pending-key descriptor before validation. A subprocess regression changes only the child's umask to 0277, fails before the fix, and now verifies key initialization, exact permissions/length, reuse, and no staging remainder. All **13 launcher tests** pass; the engine library now contains 63 tests (the earlier complete suites contained 62). The hot-WAL documentation precision finding is incorporated above. Optional additional worker-only key-failure coverage and an unverified, preexisting possibility of overlapping output gaps remain follow-up considerations; no fix or coverage is claimed for them. Claude Sonnet 5.5 also approved the final focused delta in `sqlcipher-code-final.json`, with no permission denials, independently passing all 13 launcher tests. Remaining restrictive-umask sensitivity in unrelated file creation fails closed and is not claimed fixed; the new test intentionally isolates pending-key creation. Rebuilt production/fault workers and the six-position bootstrap crash matrix pass after the fix, as does fault-feature Clippy.

## Structural quality check

`ripwire --quality-delta` used the preserved preincrement sidecar baseline, without rebasing or hiding findings. Its final pass still exits **2** with 13 gating rows (62 total reports, including the new regression test). Meaningful changes extracted read-only main-schema validation and bounded authenticated spool range reads, and consolidated the duplicated SQLite error mapper. Remaining reports are consciously retained: key lifetime/error propagation adds four branches and 13 lines in worker startup; encrypted spool open/read add format/bounds/recovery checks; fault assertions add branches; independent integration fixtures duplicate small setup/auth/retry/UID patterns. The tool also misidentifies Rust test modules/functions as dead code despite passing tests, and conflates same-named `tests` modules across files. New key publication and acceptance test length plus explicit borrowed-key parameters are visible tradeoffs. No claim of a clean quality-delta gate is made; further splitting solely for a metric would obscure the reviewed lifecycle and test scenarios.

## Limits

This increment encrypts retained content with an unattended colocated key. Same-account commands can read that key; memory, swap, cores, child-created files, file metadata, credential tokens and a backup containing both key and data are outside the confidentiality claim. Logical expiry is not forensic erasure. Output logical quotas have explicit finite database and rollback-journal overhead. Unreleased plaintext roots are rejected intact and require a separate fresh root after existing workers finish. No automatic migration/rekey or production transport enablement is included. Strict production relay enforcement and payload-safe MDK integration remain separate phase-2 gates.
