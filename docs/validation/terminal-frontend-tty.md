# Foreground Terminal validation

Linux x86-64 with tmux 3.4. The performance table uses the optimized Rust
release profile. The tests use
disposable private PTYs, a Unix socket pair and the built-in native worker.
Fourteen core tests passed. They check the incremental byte
policy, 256 input byte values across 16 representative parser states,
split-invariance, output and detach carry, inactivity limits, rejected
terminal controls, writer/observer input and partial display writes.
The focused tree passed strict Clippy.

Isolated fixtures
checked all nine partial signal-install failure points, restored visible
signal dispositions and masks, refused mismatched descriptors and absence
of a controlling TTY, restored after panic unwind and SIGHUP, and
observed SIGTSTP stop and continuation. A separate fixture exercised the
actual frontend poll loop for typed resize, suspend, SIGHUP, SIGTERM with
a full input path and local IPC loss. Both writer and observer saw exact
sanitized output on real PTYs. The writer delivered literal Ctrl-C and
Ctrl-Z, bracketed paste containing the local detach sequence, including after a
leading tilde, and the
ordinary bytes before a local detach. The observer delivered no input.
The native tmux fixture checked actual pane output and Unicode, synthetic
color and alternate-screen controls through the same output path, Ctrl-C
at the remote worker and preservation of the pane after frontend detach.

## Local latency and throughput

In the real tmux worker and two-PTY fixture, 30 paced one-byte input-to-pane
responses had 227 µs median and 527 µs p95. First pane key response took
54.574 ms from the start of worker attachment. Local detach, child exit and
termios verification took 5.089 ms. The Unix bridge wrote tmux output 34 times.
Process scheduling and the test's 5 ms exit polling affect these results.
The samples contain no Marmot relay delay and make no claim about a
terminal emulator's rendering speed.

The filter benchmark feeds the same generated seed for each boundary, using
262,145–262,160 bytes at 1 and 128 bytes and 4,096,008–4,096,018 bytes at
4,096 bytes. For the 4 KiB boundary there were 1,001 samples each.

| Workload | Chunk B | MiB/s | p50 µs | p95 µs | p99 µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Plain UTF-8 text | 1 | 18.37 | 0.030 | 0.040 | 0.040 |
| Plain UTF-8 text | 128 | 86.61 | 1.382 | 1.403 | 1.413 |
| Plain UTF-8 text | 4096 | 86.18 | 43.991 | 50.153 | 56.194 |
| Color and cursor redraw | 1 | 16.31 | 0.030 | 0.060 | 0.080 |
| Color and cursor redraw | 128 | 59.79 | 2.004 | 2.044 | 2.074 |
| Color and cursor redraw | 4096 | 61.68 | 63.037 | 65.261 | 68.797 |
| Discarded control strings | 1 | 18.50 | 0.030 | 0.040 | 0.041 |
| Discarded control strings | 128 | 64.94 | 1.833 | 2.053 | 2.254 |
| Discarded control strings | 4096 | 71.37 | 53.940 | 59.439 | 64.509 |

The filter instance occupied 120 bytes. All text and redraw fixture bytes
were emitted byte-for-byte and all discarded control string bytes were
suppressed. A separate Linux strace run counted 34 reads and 43 writes
on the frontend's `/dev/tty` fd. It had 42 bridge writes. The traced run
is for syscall counting, with latency measured separately above. The
21 selected tests and benchmarks passed in 0.62 seconds optimized and
1.47 seconds in a separate debug run. The 30 debug timing samples yielded
184 µs median and 313 µs p95, so compare these small runs with their
scheduling variability in mind. There is no
measured filter-free baseline or queue high-water profile for this run.
Foreground process-group switch races, authenticated orderly drain and
output commit, daemon IPC, relay throughput and native macOS and ARM64
remain activation checks.

## Reproduction

```sh
cargo test --locked -p marmish-native --test terminal_local_tmux core_tests_terminal_frontend
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --locked -p marmish-native --test terminal_local_tmux 'frontend_local::terminal_frontend_' -- --ignored --nocapture --test-threads=1
MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --release --locked -p marmish-native --test terminal_local_tmux frontend_local:: -- --include-ignored --nocapture --test-threads=1
cargo clippy --locked -p marmish-native --test terminal_local_tmux -- -D warnings
```
