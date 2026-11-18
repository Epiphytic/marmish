# Native Terminal backend verification

[Design](../phases/terminal-native-backend.md) ·
[Independent code review](../reviews/terminal-native-backend.md)

On Linux x86-64 as a nonroot account, the frozen backend passed all five
focused library tests and both binary tests. The descriptor inventory
test uses an actual inherited probe descriptor and real standard/status
descriptors. The binary checks include the private-token refusal, the
trusted worker dispatch, an exec failure and inherited-descriptor cleanup.
Separate tests exercised partial writes across a generation fence, launch
stage validation, cancellation/wake and cleanup ownership.

```sh
cargo test --locked -p marmish-native --lib core_tests_terminal_backend
cargo test --locked -p marmish-native --test terminal_local_tmux core_tests_terminal_backend
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --locked -p marmish-native \
  --test terminal_local_tmux terminal_native_backend_tmux -- --ignored --nocapture
```

The core selections took 0.04 s and 0.02 s to execute after compilation.
The actual worker fixture passed with tmux 3.4. It checked UTF-8 output,
resize, Ctrl-C, detach and reattach to a surviving pane, deliberate child
exit, and cleanup. The independent reviewer passed three repeats at about
0.38 s each and also passed the original local tmux fixture.

| Metric | Author, two runs | Independent reviewer, three runs |
| --- | ---: | ---: |
| Key to echo p50 | 64–114 µs | 59–114 µs |
| Key to echo p95 | 131–775 µs | 105–176 µs |
| First render | 53.6–54.1 ms | 53.9–54.5 ms |
| Detach | about 10.1 ms | about 10.1 ms |
| Reattach | 54.5–54.7 ms | 53.6–54.1 ms |

Each repeat sent 30 paced synthetic keys through the local attachment.
First render and reattach include the required 50 ms launch grace. These
are local PTY and tmux observations; they exclude a remote terminal
frontend, encrypted transport, and relay delay.

Darwin ARM64 and x86-64 backend ABI paths were source-checked in isolated
target checks. Real macOS and Linux ARM64 worker, PTY, termios and tmux
execution still need native hosts. The GitHub workflow contains the fast
offline selections for the trusted branch. This feature branch did not
execute the workflow.
