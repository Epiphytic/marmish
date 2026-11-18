# Native Terminal CLI resize, interrupt and detach qualification

Extend the existing ignored Linux AMD64 controlling-PTY fixture through
the native CLI and its real attached tmux on the private WSS test relay.
Use the same fixture-provided trust, exact installed binary stamp, Command
overlap control, socket ownership and helper cleanup. Keep this increment
test-only and preserve the existing GitHub Actions core selection.

- Observe a raw attached writer and a distinct synthesized prompt. With
  one live socket, set a new positive geometry on the owned PTY using
  TIOCSWINSZ. As the child is the controlling foreground process, the
  kernel should send SIGWINCH. Verify the child's remote PTY reaches the
  requested dimensions by collecting output synthesized from stty size,
  within a single fixed retry budget. The test command must not contain
  the expected value. Terminal restoration should preserve the most
  recent externally supplied dimensions.
- Run an interactive foreground sleep with a marker emitted immediately
  before it starts. Wait for the marker, send a literal Ctrl-C through
  the local PTY, check the remote shell prompt returns and confirm a
  follow-up synthesized marker. Make the uninterruptible sleep longer
  than the observation deadline, so normal exit cannot satisfy the test.
  The foreground CLI must stay alive and its local terminal must stay raw.
- Set a distinct shell variable and observe a synthesized completion
  marker. Send the supported line-start tilde-dot escape in the same PTY
  write, wait for clean CLI exit, and assert exact termios restoration and
  an unchanged tmux socket. Relaunch the same client as the default
  writer in a fresh controlling PTY, allow its bounded Ready transition,
  check the same socket and user shell state, then exit the remote shell
  and restore the second local PTY. A fresh tmux instance cannot pass the
  shell-state check. Markers must avoid matching echoed input or stale
  redraw. Do not use shell command or relay retries outside existing
  limits. If daemon detach acknowledgment and qualified default resume
  race, preserve the implementation contract and record the detected
  behavior for a reviewed fix.
- Reuse the proof's concurrent bounded stderr drain, master read during
  reap and owned kill on assertion failure for both CLI processes.
  Never emit raw terminal content. Report the named checkpoint and
  wall time of the complete test. Keep attach and key echo as distinct
  timings, since post-attachment controls do not measure device latency.

Validate with ordinary offline binaries and exact checksum-verified
shell-installed Linux AMD64 payload, including the previous quiet AUTH
refusal. Use all-target installed-checks Clippy. Document remaining
validation for CLI bracketed paste and takeover, platform-specific
geometry behavior, and a standalone daemon on an external qualified
relay. Keep the independently exercised backend observe, takeover and
restart cases as separate evidence. Obtain Claude Code Auto design and
completed integration review before commit and dual-mirror publication.
