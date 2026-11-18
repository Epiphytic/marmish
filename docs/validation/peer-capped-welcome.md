# Capped recipient Welcome and joined readback validation

The Linux x86_64 run used exact source `3b2a00f29895e2d28882289acd985c7aada2c579`,
upstream MDK `bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`, the locked offline Cargo
build and unoptimized workspace profiles with debug information disabled. The
opt-in fixture spawns a fresh process for each case because SQLite's VFS
registration is process-wide. It uses disposable keys, SQLCipher test state,
and real founder and recipient MDK runtimes with an adapter that refuses all
transport I/O. The host, relay and terminal roles of installed binaries are
outside this proof. The joined stage uses the existing v1 fixture, so a separate
phase must qualify the full v2 recipient owner and capped Pending CAS.

| Case | Result |
| --- | --- |
| Join | A verified Welcome from the real founder passed preflight on a metered encrypted copy. One managed native ingest established exact recipient, founder, group, package, receipt, epoch and leaf authority. Separate capped readbacks recovered authority without changing the original. The fixture committed local Pending through its existing checkpoint and replay failed. |
| Capacity | A copy overshooting the aggregate allowance by one byte was refused before creation. The exact copy boundary passed. The original opened once under its remaining exact allowance and SQLite then refused one rollback journal write. Source bytes stayed identical, and recovery could not report a join. |
| File identity and deadline | The adapter refused a different authenticated outer carrier, an occupied snapshot, a replaced stage inode and a modified stage after preflight. Exact expiry and stop blocked readback. The consumed nonce remained unavailable without a native result. |
| Cancellation | A stop after the durable JoinStarted cut prevented the native open. An inert JoinStarted row and intact stage remained, and neither retry nor readback could establish a join. |
| One-poll teardown | A forced Pending future borrowing a real managed SQLite handle closed it synchronously inside the guard. No managed callback or handle remained outside the turn. A second owner failed fast while the file turn was held. |

Single-run timings around the local native work, using an unoptimized build:

| Measurement | Time |
| --- | ---: |
| Successful fixture preparation before metered operations | 11,808 ms |
| Successful metered preflight | 230 ms |
| Successful metered native join | 193 ms |
| Successful metered cold readback | 286 ms |
| Capacity refusal after native open | 87 ms |

The capacity case recorded a 2,416,640-byte peak for the selected managed
main and its journal, 0 main-write refusals, 1 journal-write refusal, and 1
original-main open. The aggregate budget included every other reserved main,
sidecar and fixture charge within 128 MiB. The successful native path requires
no egress and returns a joined proof only after resources and the disposable
copy close. Relay latency, installed enrollment throughput, and macOS behavior
remain separate qualification work.

| Focused checks | Result | Wall time |
| --- | --- | ---: |
| Capped Welcome proof | All 4 disposable child cases passed | 51.93 s excluding compile, 99.20 s total |
| Quota VFS and dispatcher | 7 passed | 16.61 s |
| Terminal leave, including actual capped native leave | 4 passed | 58.30 s |
| Actual native PostJoin following the extracted Held helper | 1 passed | 15.12 s |
| Offline locked normal binary `cargo check` | Passed | 11.39 s |
| Offline locked all-target Clippy with `installed-checks` and `-D warnings` | Passed | 40.31 s |

Reproduce on the same Linux setup with `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0`:

```sh
cargo test --offline --locked -j2 -p marmish-native --lib peer_capped_welcome_native_proof -- --ignored --nocapture
cargo test --offline --locked -j2 -p marmish-native --lib peer_quota_vfs -- --include-ignored --test-threads=1
cargo test --offline --locked -j2 -p marmish-native --lib peer_terminal_leave -- --include-ignored --test-threads=1
cargo test --offline --locked -j2 -p marmish-native --lib exact_postjoin_commit_is_recovered_and_delivered_once -- --ignored
cargo check --offline --locked -j2 -p marmish-native --bins
cargo clippy --offline --locked -j2 -p marmish-native --all-targets --features installed-checks -- -D warnings
```
