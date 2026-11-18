# Local Terminal socket handshake and display commit

Local Linux x86-64 validation used fresh same-UID Unix socket fixtures.
Twelve library checks plus the Terminal frontend display check passed. Four
existing fixed-wire checks passed, as did strict all-target Clippy.

The tests verify ordinary response bytes and half-close, exact readiness and
binary suffix preservation, malformed and duplicate fields, refused or absent
replies, independent pending and attached limits, bounded credit, coalesced
resize ordering, and output acknowledgment after complete display units.
The foreground display test covers partial writes, discard-only records, and
an incomplete sequence at the record boundary.

```sh
cargo test --offline --locked -p marmish-native --lib terminal_socket
cargo test --offline --locked -p marmish-native --test terminal_local_tmux terminal_socket_display_core
cargo test --offline --locked -p marmish-native --lib terminal_ipc_wire_core
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
cargo test --offline --locked -p marmish-native --lib terminal_socket_handshake_bench -- --ignored --nocapture
```

The debug-profile local readiness handshake measured 124.871 µs p50 and
146.961 µs p95 over 100 samples. It measures same-host IPC with an available
owner and excludes tmux, network enrollment, and relay latency. The GitHub
Actions workflow selects only the ordinary byte regression, binary suffix,
and display-commit checks from this set.
