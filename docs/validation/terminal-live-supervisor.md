# Native Terminal writer qualification

The native writer supervisor is compiled into the test harness. These
qualifications exercise the local admission, PTY and recovery interfaces.
Transport, CLI activation, observer and takeover have separate phases.

## Reproduce

Build `marmishd` and run the tests as an ordinary login user on Linux or macOS.
Use a trusted build path and a trusted tmux 3.3 or later. Set
`MARMISH_TEST_TMUX` if the installed tmux is outside `/usr/bin/tmux`.

```sh
cargo build --offline --locked -p marmish-native --bin marmishd
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
cargo test --offline --locked -p marmish-native --lib --no-default-features core_tests_terminal_live_supervisor
cargo test --offline --locked -p marmish-native --lib --no-default-features core_tests_terminal_frames_timed_sink
cargo test --offline --locked -p marmish-native --lib --no-default-features core_tests_terminal_supervisor
MARMISH_TEST_DAEMON="$PWD/target/debug/marmishd" \
  cargo test --offline --locked -p marmish-native --lib --no-default-features \
  terminal_live_supervisor_ -- --ignored --test-threads=1 --nocapture
```

To qualify intermittent admission behavior, repeat the native readiness and
resume fixture 20 times with the same built test binary. Cargo prints its path
when running the suite above. Set `test_binary` to that path.

```sh
export MARMISH_TEST_DAEMON="$PWD/target/debug/marmishd"
for trial in $(seq 20); do
  "$test_binary" terminal_live_supervisor_default_new_busy_restart \
    --ignored --test-threads=1 --nocapture || exit
done
```

Use the path under the configured Cargo target directory when
`CARGO_TARGET_DIR` is set. The native fixtures create a disposable
file-backed identity and private SQLCipher state. Their configured loopback
relay receives no traffic during these tests.

## Results

Linux x86-64 with Rust 1.97.1 and tmux 3.4 exercised the frozen phase source.
Strict Clippy passed for all `marmish-native` targets.

- Thirteen fast writer and recovery tests passed. They cover exact source,
  physical-effect revocation, contention, clock drift, suspend and absolute
  expiry, independent Start and release capacity, cancelled installation,
  stopped queue replies, a contended mailbox after a coalesced wake, the
  unchanged operation under held input-lock contention,
  partial input progress with one receipt, expiry after a partial write,
  HUP and a late Resize, and transactional schema 1 migration and privacy.
- The timed frame sink test and both earlier supervisor core tests passed,
  for 16 deterministic tests in total. The timed sink preserves both the
  signed-age and the frame gap deadline.
- Five opt-in tests passed in 41.87 s. They use a real worker and tmux,
  isolate default and new sessions, verify Busy and shell state across
  restart, expire a blocked input under Command suspicion, complete local
  control while another attachment holds a native output slot, and verify
  exact startup grant refusal.
- All four earlier native supervisor regressions also passed against the same
  binary. They cover quarantine limits, ambiguous restart, writer and observer
  behavior, and the local resume path.
- Recovery tests retire an unlaunched Prepared slot before creating a
  replacement, preserve an unexpected or inaccessible parent, repin a
  verified live no-proof row and release an exact dead slot at the
  16-row cap. They cover retained lease ownership through reaping and
  unwind, an owned zombie, same-boot ambiguity, post-reboot row-only
  retirement preserving a listener, the 64-path cap and recovery at each
  checked cleanup cut.

| Measurement | Samples | Result |
| --- | ---: | ---: |
| Local writer input to shell output marker, p50 | 100 | 8.257 ms |
| Local writer input to shell output marker, p95 | 100 | 8.709 ms |
| Cold create and attach | 1 | 276 ms |
| Restart and warm attach | 1 | 165 ms |

The same fixture passed all 20 consecutive runs, with 100 exchanges per
run and 2,000 measured exchanges total. The runs took 148.84 s in aggregate
and produced zero false capacity errors, contention failures or timeouts.

| Measurement per run | Median across 20 runs | Range |
| --- | ---: | ---: |
| Local echo p50 | 8.302 ms | 8.211–9.182 ms |
| Local echo p95 | 8.492 ms | 8.314–14.847 ms |
| Cold create and attach | 247.5 ms | 191–389 ms |
| Restart and warm attach | 167.5 ms | 130–217 ms |

The echo fixture measures local shell execution and native output batching
under ordinary host load. It calculates nearest-rank percentiles within each
run. Relay timings and production daemon shutdown composition have dedicated
integration gates, along with native macOS, Linux ARM64 and actual reboot
qualification.

An advisory Ripwire quality-delta run returned a nonzero result. Its 54 gated
heuristics included 33 clone matches, 18 reuse matches and three dead-code
matches across the terminal and wider code. The phase relied on exact strict
Clippy, independent review and the native tests for its acceptance gates.
