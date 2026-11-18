# Local Observe and Takeover foreground validation

The [Observe and Takeover design](../phases/terminal-observe-takeover.md)
defines end-to-end behavior. These tests qualify the CLI and the local
same-UID Unix socket. A controlled local server fixture supplies the exact
Busy, Ready and failure replies. The fixtures use disposable profiles,
sockets and PTYs.

Author checks used Linux AMD64, Rust 1.93.1 and tmux 3.4. The socket
selector passed 31 tests with two optional benchmarks skipped. The CLI
selector passed all three checks and the foreground selector passed both
checks. The full local tmux suite passed all 61 checks after excluding
three entries launched by the suite as child fixtures.

```sh
cargo test --offline --locked -p marmish-native --lib terminal_socket_
cargo test --offline --locked -p marmish-native --lib terminal_cli_
cargo test --offline --locked -p marmish-native --lib terminal_foreground_
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --offline --locked -p marmish-native \
  --test terminal_local_tmux -- --include-ignored \
  --skip terminal_local_tmux_cleanup --skip terminal_local_tmux_pane \
  --skip frontend_local::terminal_frontend_child
```

The scenarios cover both local role shapes, canonical named slots, strict
offers, a Busy socket closing and releasing admission even at capacity,
the previous local daemon upgrade response, and the absolute second-leg
deadline. On controlling PTYs, a prompted observer cannot send Input or
Resize, and New and explicit Takeover flush cooked typeahead before
writer raw mode. New prints its slot. Takeover distinguishes a definite
stale offer from a lost reply and does not replay either request. Prompt
SIGINT, cooked EOF and job-control suspend all discard the offer and open
no second socket. A suspended offered New reports that it cannot reattach
when the slot is still unknown. A suspended observer restores termios
and exits after continuation. A suspended confirmed writer resumes by
requesting its exact prior slot.

In a separate controlling-PTY check, the built CLI received a Busy reply
and no input. It exited 25.003 seconds after Busy with status 0, displayed
the fixed prompt without displaying the offer, and opened no second Start.
This full-duration check runs outside the Cargo suite.

## Local admission timing

Each workload takes 100 samples in one process against a temporary
same-UID Unix listener. Wall time covers a fresh local connection, Start
write and parsing the exact Ready or Busy reply. These are socket timings.
The private relay, MLS enrollment, tmux startup and physical display are
outside this measurement.

| Workload | Samples | p50 | p95 |
| --- | ---: | ---: | ---: |
| Writer Ready | 100 | 135.911 µs | 171.337 µs |
| Busy offer | 100 | 154.335 µs | 180.293 µs |

```sh
cargo test --offline --locked -p marmish-native --lib terminal_socket_handshake_bench \
  -- --include-ignored --nocapture
```

Private WSS qualification, platform coverage and physical-display
benchmarks remain part of the integrated phase.
