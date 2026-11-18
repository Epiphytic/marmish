# Framed terminal controller validation

The author tested on unprivileged Linux x86-64 with tmux 3.4. The independent
Claude reviewer reproduced the correctness tests and strict Clippy on the
same staged source. [Review record](../reviews/terminal-socket-controller.md)

- **26 library checks passed.** Real nonblocking Unix peers exercise both
  controller directions, short writes, fixed deadlines, writer-only Input and
  Resize, and receipt-gated native commit. Regression checks distinguish
  in-flight Input and Resize from either record after a fully sent Close, and
  check Detach with idle, unwritten and partially written Output.
- **49 local integration checks passed.** The framed foreground harness uses a
  real controlling PTY and exercises credit-driven input, initial resize,
  physical display before receipt, local detach, typed end of stream and
  terminal restoration. The suite also exercises the raw tmux frontend,
  handled signals and output sanitization. The two excluded selectors are
  child-process entry points invoked by their parent tests.
- **Two opt-in socket benchmarks and strict all-target Clippy passed.**

| Local measurement | Samples | p50 | p95 |
| --- | ---: | ---: | ---: |
| 4 KiB Unix socket display-receipt turn in one process | 1,000 | 2.845 µs | 2.955 µs |
| Same-UID socket admission | 100 | 141.250 µs | 191.123 µs |
| Physical local tmux frontend echo | 30 | 94 µs | 139 µs |

Two framed PTY fixtures measured 1,486 µs and 2,194 µs from output submission
to physical display and receipt. These are same-host debug builds. The
measurements cover the local socket and PTY paths. Relay latency and the
installed daemon's end-to-end interactive path are outside this increment.

```sh
cargo test --offline --locked -p marmish-native --lib terminal_socket
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --offline --locked -p marmish-native \
  --test terminal_local_tmux -- --include-ignored \
  --skip terminal_local_tmux_cleanup --skip terminal_local_tmux_pane --nocapture
cargo test --offline --locked -p marmish-native \
  --lib terminal_socket -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
```
