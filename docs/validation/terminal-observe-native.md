# Native Terminal observation groundwork

The [Observe and Takeover design](../phases/terminal-observe-takeover.md)
defines the installed behavior. This increment covers the pure slot
decision table and the native observer's link to its writer. The installed
Observe and Takeover routes, offers and confirmed successor handoff need
their integrated qualification.

All 19 deterministic native checks passed on Linux AMD64. Run them with
`cargo test --offline --locked -p marmish-native --lib core_tests_terminal_live_`.
The tests cover classification precedence and every slot decision for
ordinary attach, Observe, both takeover legs and New. They also cover a
stale offer for each ticketed operation.

The geometry checks use real Unix PTYs. The writer resizes after the
observer PTY has been opened using its earlier dimensions. Installation
applies the writer's current dimensions, and a later writer resize also
updates the observer. The fixture verifies that observer input and resize
are denied by both the I/O port and the native attachment. Substituting a
different port for the same writer claim prevents observer installation.
Forcing an observer ioctl failure leaves the writer current and able to
resize. Brief contention on either Command authority fence pauses observer
delivery while the geometry mirror retries under one 250 ms cutoff. The
checks cover successful catch-up, withholding already queued output, and
expiration without extending the cutoff for a newer size. Retiring the
exact writer fences the observer and closes its PTY master before the
writer is returned for reap.

The Linux AMD64 local tmux regression suite passed 55 checks. Run it with
`MARMISH_TEST_TMUX=/usr/bin/tmux cargo test --offline --locked -p marmish-native
--test terminal_local_tmux -- --include-ignored --skip terminal_local_tmux_cleanup
--skip terminal_local_tmux_pane --skip frontend_local::terminal_frontend_child`.
Those three entries run as child fixtures of the suite. The tests use
disposable profile directories, Unix sockets and native tmux processes.
The local suite checks the native writer and foreground regressions. The
integrated phase must separately prove read-only display and takeover over
private WSS, writer death while a real tmux observer persists, and native
Linux ARM64 and macOS ARM64 behavior.
