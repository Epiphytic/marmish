# Local PTY and tmux qualification

This ignored operator fixture qualifies the host-side terminal primitives
independently of relay access. It uses the controlling-terminal test
pattern in `native_setup.rs`, libc, tmux and a small child of the same
test executable. It runs only on Linux or macOS. It opens no network
endpoint and starts no Marmish daemon. Running the exact ignored test
requires the operator to set `MARMISH_TEST_TMUX` to a trusted absolute
executable path. An absent executable or version below 3.2 fails the
explicit run with a fixed setup category.

## Process ownership

Resolve the executable to a regular executable file with operator-owned
or system-owned, non-group/world-writable path components. Use
`env_clear` and explicit `PATH`, `TERM`, `HOME` and locale for tmux and
its children. `TMUX` and `TMUX_TMPDIR` stay absent. Run as a nonroot user.
Create a short mode-0700 socket directory and random run-owned session
name. Enforce at most 103 pathname bytes, excluding the terminating NUL.
Use the explicit private socket and `-f /dev/null`. Never connect to the
operator’s tmux server. Use a small fixture executable with separate
arguments as the only pane command.

Start `tmux -D -S PATH -f /dev/null` with no command. This launches a
foreground server tracked as a child with null stdio. All control invocations use null
or captured pipe stdio. Verify that the created session's `#{pid}`
matches the owned foreground server child. Enable `exit-empty` and
disable `destroy-unattached` after the initial session exists. The pane
self-exits on a monotonic deadline even when detached, so the server can
exit on an external fixture timeout. The separate runner's trap covers
the startup window and uses only the held server child or private
verified socket. Verify an owned socket within the owned mode-0700
fixture directory and allow up to 4 seconds for socket acceptance.
Give each control command at most 4 seconds. Publish a private cleanup manifest containing the
socket path, restrict output to fixed categories and remove the manifest
with the directory after cleanup.

Use `openpty`, close both descriptors on exec by default and give only
the intended slave descriptors to the attachment. Close the parent's
last copy of the slave before polling. In minimal async-signal-safe
pre-exec setup use `setsid` and `ioctl(TIOCSCTTY)`. The child and
server should inherit neither the harness master nor unintended slave
descriptors. Check the pane child’s fd identities and current terminal
against the harness descriptors. Use `/proc/PID/fd` on Linux to verify
server isolation. Record platform scope where macOS inspection of the
server is unavailable. Drive the master under poll deadlines and a
64 KiB rolling byte budget per measured stage. Treat Linux EIO after
slave closure and macOS EOF as hangup.

On normal detach send SIGTERM only to the owned attachment child and
close the master. Preserve PID ownership while terminating any remaining
attachment process group members by waiting with `waitid(WNOWAIT)`,
signalling only the group created by this child, and reaping the leader
last. Use the same cleanup on the error path. The pane’s independent
session must stay alive. Final cleanup tries `kill-server` using the
verified private socket and a bounded control command, then TERM and
KILL if necessary against the unreaped foreground child. Never signal an
unverified PID, PID zero or a borrowed group. Assert server and attachment
reaped, the private socket refuses connections, and the fixture
directory is gone. Use an external deadline and exercise one injected
failure after the server is ready.

## Behaviors and measurements

Run with freshly created PTYs. Verify the pane sees its controlling
terminal and foreground group. Wait until the pane has set noncanonical,
no-echo mode with `ISIG` on, installed an async-signal-safe SIGINT
handler and reported readiness. Wait for a rendered fixture marker and
verify the attachment PTY is in raw mode before sending input. For
single-byte round trips use printable keys outside the tmux prefix and
distinct printable responses. Exercise a real resize via `TIOCSWINSZ`
and defer the assertion until the pane reports new dimensions. For
Ctrl-C write the literal byte 0x03 after those barriers. Require an
explicit SIGINT observation from the pane fixture and verify the
attachment remains alive. The test harness is outside both foreground
process groups. An actual harness or attachment SIGINT fails the test.

Capture the pane ID and a nonce out of band and assert they survive
attachment termination and reattach. A fresh attachment must render the
same visible marker and reach the same pane. Measure 30 paced synthetic
single-byte round trips from master write to matching response, using
nearest-rank p50/p95. Report first render and reattach separately as
one-shot samples. Include sample counts, platform and tmux version.
The driver owns the byte budget and discards unrelated redraw bytes
within it. Product frontend termios restoration, fenced writer/observer
modes, TLS or Tor, durable state, remote latency and curses compatibility
remain in the full phase. Keep this fixture out of routine GitHub Actions.
