# Local PTY and tmux validation

The ignored fixture passed on Linux x86-64 with tmux 3.4 and Rust 1.97.1. It used a foreground tmux server and two fresh controlling PTYs. A native resize reached the pane as 32 rows by 100 columns. The first attachment received a printable key response, then the literal Ctrl-C byte reached the pane's SIGINT handler while the attachment stayed alive. After detaching, the old master reported hangup. The new attachment rendered the same readiness marker and the pane kept its ID, process identity and nonce.

The test verified the pane's controlling terminal and foreground process group. While the first attachment was live, it checked Linux `/proc` for the harness master in the server and for either attachment descriptor in the pane. After the attachment was reaped and its master closed, it checked that neither the server nor the pane retained either descriptor. A deliberate missing-marker timeout exercised the independent watchdog, which used the private manifest and verified socket to stop the server. A separate injected unwind exercised the harness's own cleanup guards and verified the socket closed and the directory disappeared. Forcing a failure in `kill-server` to exercise the TERM/KILL escalation remains untested.

Run as a regular user with tmux 3.2 or later. Set the executable to an absolute path whose resolved components are owned by root or the operator and are writable only by their owners. Routine CI skips this test. The fixture opens no network endpoints and starts no Marmish daemon. [Detailed plan](../phases/terminal-local-tmux-proof.md)

```sh
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --locked --offline \
  -p marmish-native --test terminal_local_tmux local_tmux_attachment \
  -- --ignored --exact --nocapture
```

The compiled test binary completed three runs under `timeout --foreground 15s` without reaching the deadline. In a separate abort check, the runner sent SIGKILL only to the test process after observing its owned tmux server and private socket. The watchdog removed the fixture directory, and the observed children stopped running within 11 ms on that run. The watchdog runs in a separate process group and keeps its own deadline.

Each run reported 30 one-byte round trips and one measurement apiece for first render and reattach. Both render timers start just before spawning the attachment, after pane readiness and PTY allocation, and stop on the rendered marker. Key timings exclude the 5 ms pause before each sample. The pane and driver use wakeable polls with 10 ms and 15 ms timeout intervals, respectively. Those polling intervals and OS scheduling affect the timing granularity. Nearest-rank p50 and p95 are the 15th and 29th samples after sorting.

| Run | Key p50 (µs) | Key p95 (µs) | First render (ms) | Reattach (ms) |
| --- | ---: | ---: | ---: | ---: |
| Cargo integration test | 53 | 73 | 2.068 | 2.193 |
| Guarded run 1 | 93 | 130 | 2.110 | 1.909 |
| Guarded run 2 | 66 | 127 | 2.368 | 2.080 |
| Guarded run 3 | 59 | 90 | 2.426 | 2.835 |

These timings measure the local fixture. Network and Marmot latency remain outside its scope. An isolated Clippy check with the repository's Rust toolchain and the pinned `libc` and `tempfile` versions passed with `-D warnings` for Linux x86-64, macOS arm64 and macOS x86-64. The Darwin checks were performed from Linux. Native macOS and Linux arm64 runtime results are pending, and the Linux `/proc` descriptor check has no macOS counterpart in the fixture. The attachment's controlling-terminal check runs in the child because Darwin's `TIOCGPGRP` requires the queried terminal to be the caller's controlling terminal. [Darwin terminal implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c) · [tmux 3.2 manual](https://github.com/tmux/tmux/blob/3.2/tmux.1)
