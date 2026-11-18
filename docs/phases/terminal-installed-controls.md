# Native Terminal CLI resize, interrupt and detach qualification

Add the ignored `terminal_installed_runtime_native_controls` selector
with a dedicated Controls workload. It exercises the real foreground
CLI and its attached tmux on the existing Linux AMD64 private WSS test
relay. Reuse fixture TLS trust, exact installed binary stamp, ordinary
Command before and after, and bounded fixture ownership. Keep all new
code under the test fixture. Leave the core GitHub Actions selection
intact.

- Write the exclusive client config once for the entire fixture. Launch
  the first CLI with a fresh controlling PTY. Capture original termios
  and all four geometry fields before spawn. Require raw mode with
  ECHO, ICANON and ISIG cleared, one live tmux socket, and the
  synthesized shell prompt. Raw Ctrl-C now reaches the CLI as input.
- Set geometry to 31 rows by 91 columns, with unchanged pixel fields,
  using TIOCSWINSZ on the owned PTY. This is in range and differs from the
  fixture's original geometry. The foreground child should receive the
  kernel's SIGWINCH. It holds keyboard input while a resize is pending.
  Run `stty size | tr ' ' :` as a synthesized-output probe, with at
  most three attempts within one 15 s budget. Each probe ends in a
  distinct split completion marker, so the test can distinguish an old
  geometry from a slow command and can consume each attempt completely.
  The expected value `31:91` is absent from the transmitted input.
- Start a 60 s foreground sleeper with a marker emitted once the test
  process has an interrupt handler. Send the byte 0x03 to the owned
  local PTY only after the marker appears. Require a synthesized
  interrupt marker and a new follow-up command marker within 15 s.
  The follow-up marker proves the shell survived. A natural process exit
  falls outside the check deadline. Verify the CLI is still alive and
  the local PTY is still raw.
- Set a shell variable, printing only an unrelated split completion
  marker and ending the command with a carriage return. After consuming
  its marker, send the distinct two-byte `~.` in a single PTY write.
  Both bytes must be available within the existing 32 ms input prefix
  budget. Wait for exit 0, assess stderr, and compare all original
  termios fields and the newly set geometry including pixel fields.
  Record the socket identity by path, device and inode before detach
  and ensure it remains the same.
- Start the same client as the default writer in a second fresh
  controlling PTY with its own original size and termios capture.
  Observe the initial PTY output concurrently with the attach check.
  If the cooked Busy prompt appears before raw mode, fail with the
  named `resume_busy` checkpoint and close via the owned cleanup.
  Do not retry attach or select another mode. Check the unchanged
  device/inode and socket path after Ready. Make the resumed command
  synthesize a fresh marker from the pre-detach variable. The original
  phase printed no such marker. Fully exit the remote shell and assert
  that the second local PTY's own original size and modes are restored.
- Each launch has its own 35 s attach deadline and concurrent bounded
  stderr drain. Output and I/O checks use 15 s budgets. On failure or
  after the checkpoints, use the existing bounded wait, fallback kill,
  PTY drain and stderr join for the exact child. Retain bounded socket
  cleanup. Print named checkpoints and successful wall times only.
  Keep the existing attach and key echo samples separate.

Validate with normal offline binaries, the matching checksum-verified
shell-installed Linux AMD64 payload and the existing quiet AUTH
refusal. Run strict all-target installed-checks Clippy. Document
CLI bracketed paste and takeover, external qualified relay and
platform-specific geometry as separate validation. The existing
backend cases cover observation, takeover and restart. Obtain Claude
Code Auto design and integrated code review before dual publication.
