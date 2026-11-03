# SQLCipher test-path validation

Tracking: `marmish-amf`. Baseline source: `12c1ac391908f77d456e226f7033649c5a48d7b7`.

Actual hosted candidate 41624375873 macOS job 125877905688 passed 108 engine unit tests and fresh-process concurrent storage initialization, then failed the independent retained-storage inspection with extended SQLite error 1550 for a `/var/folders/.../state/engine.sqlite3` path. Pinned SQLCipher maps symlink path resolution with `SQLITE_OPEN_NOFOLLOW` to 1550 (`SQLITE_CANTOPEN_SYMLINK`). Production already resolves the trusted existing parent on Darwin; the test fixture did not.

The fix affects only shared engine integration-test temporary fixture creation and encrypted-storage regression coverage, plus the previously missing fault-feature `initialize_storage` import. No production storage flags, encryption, dependency versions, or scheduling changed.

Local Linux validation ran as UID 1001, so the actual retained-command test executed rather than taking its root-only return:

- `cargo test --offline --locked -p marmish-engine --test encrypted_storage -- --nocapture`: 3 passed, 0 failed, 0.40 seconds. Includes real Engine/worker retained command/input/output and independently keyed inspection; owned ancestor alias and final database symlink both refused with exact extended code 1550; canonical keyed access succeeds and wrong-key/unkeyed schema reads fail.
- Built default engine worker and copied it to a private temporary artifact; separately built fault-injection worker and copied it to a distinct artifact. With `MARMISH_TEST_PRODUCTION_WORKER` and `MARMISH_TEST_FAULT_WORKER` pointing to those respective artifacts: `cargo test --offline --locked -p marmish-engine --features fault-injection --test faults every_worker_crash_boundary_preserves_no_rerun -- --exact --nocapture`: 1 passed, 7 filtered, 27.50 seconds. This executes twelve boundaries and the raw encrypted spool reader.
- `cargo clippy --offline --locked -p marmish-engine --all-targets -- -D warnings`: passed.
- `cargo clippy --offline --locked -p marmish-engine --all-targets --all-features -- -D warnings`: passed. An initial run exposed the missing fault-feature import; the final run includes its correction.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Shared-helper integration checks: `cargo test --offline --locked -p marmish-engine --test engine --test native_storage`: engine integration 14 passed (2.54 seconds); native storage 5 passed (5.74 seconds).

Local raw logs remain outside the repository. No Darwin runner was available locally; these results do not replace the required whole immutable-source cross-platform candidate. Wrong-key SQLCipher diagnostics in local logs are expected negative-test results.
