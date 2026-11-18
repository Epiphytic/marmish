# Terminal relay readiness diagnostic validation

The Linux x86_64 fixture uses disposable non-root identities, a private direct
WSS endpoint with test-local TLS trust, SQLCipher, a controlling PTY and built
Marmish binaries. It opens a native Command room and checks literal output
and remote exit status before suppressing AUTH on new sockets. The installed
client obtains ordinary status and the existing Command session through the
real same-user IPC listener. Both Terminal directions subscribe for their
exact bootstrap aliases. The fixture returns each subscription's own EOSE
without sending AUTH. No quiet socket submits AUTH, sends an encrypted Probe
or publishes room events. The client refuses with exit 255 and
`terminal_relay_auth_challenge_timeout`. Each daemon records the fixed category
at its local open stage. The server allocates no tmux socket.

The native WSS regression covers explicit CLOSED before AUTH, a denied AUTH,
a wrong AUTH acknowledgment, ordered readiness, backlog limits and retirement
of disconnected or refused exact sources. The separate positive private WSS
runtime checks interactive bytes, isolation, overlap with ordinary Command
and Default/New/Busy/warm attach. Another private run tests takeover and named
reattachment. Stage evidence uses fixed categories with no relay identities,
selectors or remote data.

## Measurements

Single Linux x86_64 samples from the matched builds on a private loopback
relay. The writer and takeover controls used approved commit `c1f5c22`.
The quiet refusal and ordinary Command control used matched commit
`a7c26e7ae433f5c75ce4a2ada5c01dfa7ecd794e`. Final code
`795afcc74e5a75409ddb0d485834a5b4dc19f3f1` changes the standalone test PTY,
an additional assertion and README text. Runtime code is byte-identical.

| Operation | Wall time |
| --- | --- |
| Installed CLI quiet NIP-42 refusal | 5.032 s |
| Ordinary Command before quiet refusal | 5.163 s |
| Positive private WSS cold writer ready | 7.075 s |
| Positive private WSS warm writer ready | 7.760 s |
| Positive writer single-byte echo | 136.7 ms |
| Ordinary Command before / while / after positive Terminal | 5.208 / 4.501 / 4.954 s |

The quiet time measures from CLI spawn through exit, including frontend and IPC
work. The fixed internal wait is five seconds. The standalone local foreground
PTY fixture used tmux 3.4 and measured p50 146 µs and p95 256 µs across 30
local bridge samples. These PTY bridge timings are independent of the relay.

## Reproduce locally

Build the matched binaries with `cargo build --locked -p marmish-native --bins`.
Run the focused tests with `cargo test --locked -p marmish-native --lib FILTER`.
For fixture rows add `-- --ignored --nocapture`. Set
`MARMISH_TEST_TERMINAL_DEBUG=1` to print both exact open stages for the
quiet fixture. For the standalone foreground PTY fixture, set
`MARMISH_TEST_TMUX` to the absolute path of an installed tmux 3.2 or later
and run `cargo test --locked -p marmish-native --test terminal_local_tmux terminal_frontend_ -- --ignored --nocapture` as a non-root user.

| Filter | Result |
| --- | --- |
| `terminal_runtime_core_` | 22 passed |
| `terminal_socket_core_` | 12 passed |
| `terminal_enrollment_transport::readiness::tests` | 3 passed |
| `terminal_installed_runtime_relay_auth_challenge_timeout` | 1 passed in 30.21 s |
| `refuses_incomplete_auth_and_retires_lost_exact_wss_source` | 1 passed in 10.53 s |
| `terminal_installed_runtime_native_writer` | 1 passed in 69.03 s |
| `terminal_installed_runtime_takeover_named_resume` | 1 passed in 96.42 s |
| `terminal_local_tmux` with `terminal_frontend_` | 6 passed in 0.29 s |

`cargo clippy --offline --locked -j2 -p marmish-native --all-targets -- -D warnings`,
`git diff --check` and rustfmt checks on the changed Rust files passed. The workspace-wide formatting
check found differences already present in the base in
`crates/marmish-engine/src/lib.rs` and
`crates/marmish-native/src/native_setup.rs`.
The existing GitHub core filter selects the three new fast transport tests.
The WSS and PTY checks are opt-in.

The CLI keeps using the selected relay. This diagnostic adds no automatic
relay fallback. The fixture does not determine public relay policy. Both PTY
fixtures set the window size on the slave with `TIOCSWINSZ`. The independent
reviewer type-checked the installed helper against locked libc source for
Linux x86_64, Darwin ARM64 and Darwin x86_64. Full builds and runtime checks
on macOS and Linux ARM64 still require those runners. See the
[code review](../reviews/terminal-relay-readiness-diagnostics-code.md).

## Integrated upstream MDK validation

On Linux AMD64, integrated source `d8c85de` locked upstream MDK to
`b3cd2cb1`. `cargo clippy --locked --offline -p marmish-native --all-targets -- -D warnings`
passed. A local replay of the 35 CI core selectors passed all 159 test
executions in 80.52 s. The quiet private WSS
regression passed in 30.67 s. The installed CLI refused Terminal after
5.035 s, and an ordinary Command completed in 4.411 s on the same fixture.

The NIP-42 writer control passed in 67.73 s with real tmux, a key echoed in
139.988 ms, and cold and warm readiness at 7.334 and 8.093 s. Ordinary
Command passed before, during and after Terminal, in 3.943, 4.371 and
4.109 s. These are individual loopback samples. The quiet and positive
tests each ran one exact selector. These results cover built native
entry points with fixture TLS trust. Packaged Terminal over an external
qualified relay still requires a separate run.
