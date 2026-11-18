# Terminal local record and authority fence validation

Linux x86-64, Rust 1.97.1. The two components in the [phase design](../phases/terminal-ipc-fence.md) compile in test builds. The tests exercise fragmented and adjacent records, role and direction restrictions, invalid lengths, truncation and poison, and one-effect ordering during an authority change, including panic and abandoned updates.

The 88 targeted tests selected by the [core workflow](../../.github/workflows/interactive-core.yml) passed locally. The worker integration includes failed exec and inherited descriptor checks. The corrected fixture creates its mock executable under the canonical home directory so it can exercise the worker's existing ancestry checks even when Cargo binaries are under `/tmp`. Strict Clippy passed for the native library and tests. [Independent review and results](../reviews/terminal-ipc-fence.md)

| Local debug measurement | Median | p95 |
| --- | ---: | ---: |
| Decode a three-byte input payload | 150 ns | 301 ns |
| Decode a 4 KiB input payload | 190 ns | 201 ns |
| Enter fence, check generation and invoke one effect | 60 ns | 70 ns |

The measurements use 20,000 samples each and include timing and assertion overhead. They cover in-process operations. Remote Terminal latency and end-to-end display delivery remain to be measured.

```sh
cargo test --locked -p marmish-native --lib terminal_ipc_wire_core
cargo test --locked -p marmish-native --lib terminal_fence_core
cargo test --locked -p marmish-native --test terminal_local_tmux core_tests_terminal_backend
cargo test --locked -p marmish-native --lib terminal_ipc_wire_bench -- --ignored --nocapture
cargo test --locked -p marmish-native --lib terminal_fence_bench -- --ignored --nocapture
cargo clippy --locked -p marmish-native --lib --tests -- -D warnings
```
