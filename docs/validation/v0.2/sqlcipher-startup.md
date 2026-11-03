# Concurrent SQLCipher startup correction

Scope: Beads `marmish-i7y`; candidate run `41620775751`, Linux AMD64. This records local validation, not hosted acceptance.

The candidate engine integration binary failed three of fourteen normally parallel tests at `Engine::open` with `StorageFailure` and `sqlcipherCodecAttach: sqlcipher not initialized 1`. The pinned SQLCipher implementation publishes SQLite initialization before running its extra initialization hook. Independent design and code reviews confirmed that another first opener can reach SQLCipher before that hook finishes.

The correction adds one shared process-wide `OnceLock` around `sqlite3_initialize`, caching both success and failure. Every repository opener, the upstream MDK storage boundary, and Arti directory-cache construction cross it before any SQLite API. No compiler configuration, dependency, keying, encryption format, connection ownership or test parallelism changes. Future SQLite consumers/dependency changes require repeating the opener/reverse-dependency audit. Arbitrary third-party callers bypassing this first-use barrier are outside its guarantee.

Design: [r3 approval](../../reviews/v0.2-sqlcipher-startup-design-r3.md). Code: [r1 approval](../../reviews/v0.2-sqlcipher-startup-code-r1.md); [completion-sentinel r2 approval](../../reviews/v0.2-sqlcipher-startup-code-r2.md).

## Actual checks

All commands used `--offline --locked` for Cargo. Local focused compilation reused the existing dependency build: the initial regression compiled in 1.92 s and the fixed focused integration set in 0.98 s. No forced dependency rebuild or C configuration override was introduced. The full hosted rebuild cost is not measured here.

| Command | Result |
|---|---|
| `cargo test --offline --locked -p marmish-engine --test concurrent_storage -- --nocapture` against baseline initializer | 2 passed; three fresh child processes, 16 concurrent encrypted databases each. Original race did **not** reproduce locally. |
| `cargo test --offline --locked -p marmish-engine --test concurrent_storage --test encrypted_storage --test engine -- --nocapture` | Concurrent regression 2 passed plus three child passes; encrypted storage 2 passed; normally parallel engine integration 14 passed, 2.52 s. |
| `cargo test --offline --locked -p marmish-engine --lib` | 100 passed, 21.94 s. |
| `cargo test --offline --locked -p marmish-native --lib native_preflight -- --nocapture` | 2 passed, 0.07 s. |
| `cargo test --offline --locked -p marmish-native --lib journal::tests -- --nocapture` | 24 passed, 3.90 s. |
| `cargo clippy --offline --locked -p marmish-engine -p marmish-native --features installed-checks --all-targets -- -D warnings` | Passed, 5.52 s. |
| `cargo fmt --all -- --check`; `git diff --check` | Passed. |
| `ripwire crates/marmish-engine --quality-delta` | Exit 0, no gating findings. Cargo test entrypoints were reported as dead-code candidates; these are executed test functions. |
| Concurrent regression after exact child-completion sentinel refinement | 2 passed plus three child passes, 0.24 s. |
| `cargo test --offline --locked -p marmish-engine -p marmish-native -p marmish-vault --features marmish-native/installed-checks` | Exit 0: engine 100; concurrent storage 2 plus three child passes; encrypted storage 2; engine integration 14; native storage 5; native 380 (297.08 s); worker integration 1; vault 8; vault helper 3. Native keychain qualification remains a separate hosted gate. |

Each regression child starts before any SQLite call, synchronizes sixteen threads, opens distinct encrypted databases, checks exact data after reopen, rejects plaintext file/header/canary, and rejects wrong keys. The parent enforces a 45 s timeout with kill/reap and requires a per-child completion sentinel, so selecting zero tests cannot pass. Expected wrong-key SQLCipher diagnostics occur in successful negative tests.

The local baseline passing is probabilistic coverage, not a claimed red/green reproduction. The observed hosted failure and inspected pinned source establish the race; a fresh full Linux AMD64, Linux ARM64 and macOS ARM64 candidate remains required. No live transport, network or keychain tests were run for this correction. Raw logs remain outside the repository.
