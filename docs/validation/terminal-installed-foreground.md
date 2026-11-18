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

## Private WSS foreground with native and packaged binaries

On Linux AMD64 the ignored
`terminal_installed_runtime_native_foreground` test launched the native
`marmish terminal` frontend in a controlling PTY and connected through
same-user IPC, verified loopback WSS, native MDK, a real worker and tmux.
The test required local raw mode, one remote tmux socket, a shell
prompt, single-key echo, and synthesized output containing shell
punctuation and UTF-8. After a remote `exit 0`, it required CLI exit
status 0, empty stderr and exact restoration of termios and window size.
The fixture also executed ordinary Command before and after the
attachment and retained its checks that relay records and raw durable files omit the generated
marker.

| Linux AMD64 run | Result | Raw attach | Key echo | Ordinary Command before / after |
| --- | --- | ---: | ---: | ---: |
| Normal Cargo binaries, source `464300c` | Passed in 22.04 s | 7.890 s | 141.667 ms | 1.696 / 1.856 s |
| Optimized shell-installed payload, source `d091cc8` | Passed in 20.69 s | 6.974 s | 175.470 ms | 1.679 / 1.910 s |

Each timing is one wall-clock sample on private loopback WSS. Raw attach
ends when the frontend sets raw mode and the tmux socket exists. The
shell prompt is checked after that measurement. These timings include
setup and transport scheduling and do not measure internet latency.

The shell installer installed the three executables in 0.866 s from
the [optimized package build](mdk-master-3548.md#optimized-linux-installer-and-private-terminal). Each installed
executable matched its package input byte for byte. The test harness
used the same MDK lock, source `d091cc8` plus the two
[reviewed test-only changes](../reviews/terminal-installed-foreground.md),
and the exact package build stamp `d091cc81125036a1bfcb04723d44e05c47ea9594`.
Two preliminary package runs exercised the layout check with a
mismatched harness stamp. The supervisor stayed unavailable before
attachment. Building only the harness with the matching
`MARMISH_BUILD_COMMIT` resolved those failures. The installed
executables and layout validation stayed intact.

The quiet AUTH startup regression passed in 17.58 s. It required the
exact refusal category, zero tmux sockets, and unchanged publication
counts. Strict all-target Clippy with `installed-checks` passed in
43.65 s on `464300c`, and the normal binary build took 25.84 s.

The private relay uses a fixture-provided TLS root and prepared local
owners. This run covers the installed frontend and worker payloads.
A standalone production daemon against an external qualified relay,
macOS, Linux ARM64, and CLI-level resize, interrupt, detach, paste and
takeover remain separate validation cases. The private framed-client
fixtures cover backend takeover, resume and observation independently.
