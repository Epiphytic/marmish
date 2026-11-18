# Installed Terminal foreground validation

The [installed writer design](../phases/terminal-installed-writer.md) uses the installed CLI's normal destination resolver and requires a controlling terminal before Command preparation. The scoped foreground build was checked on Linux x86_64 with Rust's offline locked build and tmux 3.4. These tests cover the client-side boundary with a same-UID local fixture. The integrated daemon and private relay have separate acceptance tests.

| Check | Result |
| --- | --- |
| Local Terminal integration suite with `MARMISH_TEST_TMUX=/usr/bin/tmux` | 55 passed. The three filtered tests are child-process entry points invoked by their parent fixtures. A caught fixture panic exercises termios restoration during unwinding. |
| Foreground unit selector | 3 passed, including 60-second active and continuous ceilings, autostart child signal-mask restoration, and preservation of the ordinary daemon CLI flag. |
| Terminal CLI parser isolation | 1 passed. |
| Scoped staged diff whitespace | Passed. |

The real PTY cases prove that Start preserves the resolved Command target, session and relays. They verify geometry changes before Ready, cooked mode on failure, SIGINT exit 130 while waiting, SIGHUP exit 129 when the master closes, and receipt after the local PTY write. The child-process cases check termios restoration, a foreground job that can be resumed by its shell, and recovery when a POSIX orphaned process group ignores the normal stop signal. New never repeats once its Start may have arrived. The fixture checks an exact typed Close and an old local daemon's upgrade error separately.

```sh
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --offline --locked -p marmish-native \
  --test terminal_local_tmux -- --include-ignored \
  --skip terminal_local_tmux_cleanup --skip terminal_local_tmux_pane \
  --skip frontend_local::terminal_frontend_child
cargo test --offline --locked -p marmish-native --lib foreground
cargo test --offline --locked -p marmish-native --lib terminal_cli_does_not_reuse_the_ordinary_command_flags
```

The existing unoptimized local display-filter benchmark produced the following 4 KiB samples on the same host. This measures parser and filter work only. The real PTY tests above verify physical receipt separately.

| Workload | Samples | Input bytes | Output bytes | MiB/s | p50 (µs) | p95 (µs) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Plain text | 1,001 | 4,096,008 | 4,096,008 | 16.78 | 230 | 250 |
| ANSI redraw | 1,001 | 4,096,011 | 4,096,011 | 10.92 | 355 | 369 |
| Filtered controls | 1,001 | 4,096,018 | 0 | 21.78 | 179 | 185 |

Run `cargo test --offline --locked -p marmish-native --test terminal_local_tmux benchmark -- --include-ignored --nocapture` to repeat these local measurements.

The isolated build enables the local controller and intentionally omits unfinished daemon activation. Strict Clippy hit unused daemon-side code and an unguarded dependency fixture module in that frozen source. The integrated source must supply its own native lint result. Background continuation as a shell job exits with the foreground-ownership diagnostic. Default continuation makes a fresh bounded Start and may encounter Busy if remote retirement is still in flight. Qualification of the controlling-terminal path on macOS ARM64 and execution on Linux ARM64 belong to the native platform runs.
