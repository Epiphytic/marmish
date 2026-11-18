# Observe and Takeover over native Marmot

Linux AMD64 qualification used native Marmish binaries, a private loopback TLS
relay, disposable Nostr accounts, native SQLCipher profiles and tmux. The
[Observe and Takeover design](../phases/terminal-observe-takeover.md) and its
[supervisor review](../reviews/terminal-observe-takeover-supervisor.md)
define the ownership and fencing contracts.

| Selector | Result | Runtime |
| --- | --- | ---: |
| `terminal_installed_runtime_observer_read_only` | Passed | 71.21 s |
| `terminal_installed_runtime_takeover_named_resume` | Passed | 126.19 s |
| `terminal_runtime_core` | 10 passed across native and PTY targets | < 1 s |
| `terminal_local_tmux` default tests | 38 passed, 27 opt-in or child fixtures | 0.10 s |
| Full opt-in controlling tmux/PTY run | 62 passed | Included native binaries |
| `terminal_socket_frontend_native_pty` opt-in | Passed | 0.01 s |

Build and reproduce from the reviewed source:

```sh
cargo build --offline --locked -p marmish-native --bins
cargo test --offline --locked -p marmish-native --lib \
  terminal_installed_runtime_observer_read_only -- --ignored --nocapture --test-threads=1
cargo test --offline --locked -p marmish-native --lib \
  terminal_installed_runtime_takeover_named_resume -- --ignored --nocapture --test-threads=1
cargo test --offline --locked -p marmish-native --test terminal_local_tmux \
  terminal_socket_frontend_native_pty -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
```

Observe verifies the same tmux pane and display data and rejects raw local
Input and Resize on a reader socket. Takeover checks stale and consumed
offers, retirement of both old attachments, rejection of old input,
preservation of shell state through a named reattach, and ordinary Command
traffic during and after the Terminal workload. The separate local foreground
check opens an actual controlling PTY and verifies display acknowledgment and
terminal restoration.

These elapsed times are test runtimes. The relay fixture mounts the native
runtime in its test process and uses real framed client IPC and a native tmux
worker. Packaged daemon and installer coverage, macOS, and Linux ARM64 require
separate qualification. The unsampled private relay workload supports no
latency distribution claim.
