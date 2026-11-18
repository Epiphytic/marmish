# Native foreground observation and takeover qualification

Extend the approved [Observe and Takeover validation](terminal-observe-takeover.md)
with one ignored test on Linux AMD64. Use native `marmish terminal` processes,
the existing checked private loopback WSS and foreground test infrastructure.
Use the same canonical client and operator-owned fixture daemon. Provide
independent controlling PTYs, exact binary stamps, disposable fixture
identities and one default tmux pane. Keep the production path and core CI
selection unchanged.

1. Connect the writer using the ordinary CLI. Require raw mode, record the
   tmux socket path, device and inode, enter the test shell, and assign a
   variable. A split output marker proves that the command executed.
2. Start an explicit `--observe` CLI with a different PTY size. Confirm its
   raw mode, write a harmless uniquely identifiable candidate command to
   its local PTY, and use local echo flags and a bounded marker witness to
   check that the command did not execute. Have the writer emit a subsequent
   split positive delimiter and read it from both PTYs. Check the writer's
   native geometry using the same bounded split-output probe as the control
   test. The observer's original geometry must differ from that output.
3. Start a third ordinary CLI and require the cooked Busy prompt before
   choosing takeover. At the prompt require ICANON and ISIG, send one
   choice through the PTY and require raw mode on the successor. Reap the
   old writer and observer and assert their exact local termios restoration.
   Waiting for disappearance is bounded and never uses the successor's
   input as a readiness probe. Check preservation of the tmux path,
   device and inode. From the successor, synthesize a fresh marker from
   the original shell variable, prove it reaches the successor PTY, then
   exit the shell and verify successor restoration. Use an observer input
   sentinel distinct from all display probes.
4. Reuse the current Command checks before and after. Capture optional
   single-run wall times for observe Ready, cooked choice to successor
   Ready and first successor marker. Do not identify these samples as
   physical display or p95 measurements. Refer to the existing multi-trial
   backend benchmark for broader measurements.

Give each child one bounded pending admission and own its exact reaping,
stderr drain, PTY and result assertions. Reuse the test-only supervision
helpers where this preserves the existing writer and control behavior.
Concurrent reads may use a bounded accumulator per PTY; carry no output
between independent invocations. On any failure reap all spawned children
and clean up only the known fixture tmux server. Verify the normal native
binary plus independently shell-installed, checksum-verified Linux AMD64
payload with matching stamp. Run the existing writer, controls and quiet
AUTH regressions and strict all-target installed-checks Clippy. Obtain
Claude Code Auto design and code approval and publish sanitized evidence.

The NIP-42 relay and account are confined to the prepared test fixture.
This covers the explicit Observer and the cooked selection route to
Takeover. It leaves externally qualified WSS, macOS and Linux ARM64,
distinct-device identities, and induced parked-takeover failure to their
separate validation.
