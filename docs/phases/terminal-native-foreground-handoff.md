# Native foreground observation and takeover qualification

Extend the approved [Observe and Takeover validation](terminal-observe-takeover.md)
with one ignored Linux AMD64 test. Use the actual `marmish terminal` binary
in three independent controlling PTYs, the existing Terminal and Command
owners, and tmux against the checked private loopback WSS fixture. The
clients share the same canonical identity. Stamp-check the executables,
use disposable credentials, and leave production code and core CI alone.

## One pane through three foreground clients

1. Start the ordinary writer in a 24×80 PTY and record its original
   termios and window size before launching it. At raw Ready, record the
   sole owned tmux socket's path, device and inode. Reuse the predictable
   shell, set a variable and wait for output of a split marker command.
2. Start explicit Observe by appending `--observe` to the test launcher
   arguments after the destination. Give the observer a 40×120 PTY and
   record its own termios and size. Wait for raw Ready while draining
   writer output. Submit a uniquely identifiable harmless shell command
   to the observer PTY. Its typed candidate and evaluated marker must
   differ and neither can occur in the shell's existing state. Wait under
   the I/O deadline for two zero pending-input readings via `FIONREAD` on
   the raw observer slave, at least 25 ms apart after the test wrote the
   candidate. This lets the line discipline receive the submitted bytes.
   Drive the writer to a subsequent unique split delimiter under the same
   bounded observation, framing the delimiter command so an intact echo
   marks its own line. Require the echo and delimiter in each PTY, and
   absence from both capture windows of the candidate text and evaluated
   marker. Also require those two values absent from the successor redraw
   window. This checks local input consumption and read-only behavior at
   the explicit CLI entry point. The separate backend
   `reject_observer_effect` test covers native observer rejection directly.
   From the writer, use another split probe to prove the remote geometry
   stays `24:80` on both PTYs, with no `40:120` output.
3. Start the successor as an ordinary invocation. For the expected Busy
   branch, drain the old clients continuously and recognize the full
   cooked prompt within the initial 35 s attach budget. Require ICANON
   and ISIG and immediately submit `t\r` in one write. Prompt input is
   flushed before the question is printed and the CLI allows 25 s to
   choose. Send nothing else before raw Ready. Use a separate successor
   raw deadline of at most 60 s after the choice and 70 s after launch.
   Continue draining both old PTYs during that interval and use a bounded
   reap to verify both exits. For both old clients require code 0 with
   empty stderr, or code 255 with exactly the single stderr line
   `marmish: invalid input: terminal_replaced` or
   `marmish: invalid input: terminal_transport_closed`, with its final
   newline. These are the ordinary close outcomes covered by the
   existing native backend proof. Reject all other errors and any
   `terminal_display_restore` report. Verify their exact termios and
   original window sizes after exit.
4. Require raw Ready and the same tmux socket path, device and inode.
   Check the successor's bounded initial redraw through the distinct
   state marker it produces from the original writer's variable. Every
   positive marker uses split print arguments and a distinct label absent
   from earlier screen content so an old redraw or an echo cannot satisfy
   a later stage. Issue `exit 0` from the successor, require code 0 with
   empty stderr, and assert its exact original termios and window size.

Run the existing independent ordinary Command turn before, during and
after handoff. The fixture handles both Command owners on one task. Start the during
turn after the successor's choice and the exact subsequent binding lookup,
so the synthetic full Command round trip cannot starve that lookup's one-second
deadline. Require the Command turn to start before raw Ready while all three
PTY readers continue draining. Record its start and end offsets from the
choice and the Ready observation. The fixture uses the existing bounded
multi-room relay limit of 256 events and reports the number of observed
publication attempts. This workload uses the Command
room, writer, observer and two successor admission legs. Keep the
benchmark's higher cap scoped to its existing workloads. Exceeding the
existing cap fails this proof and calls for revising the fixture workload.

## Lifetime and evidence

Extend existing test-only helpers where their behavior can be preserved.
Give each child one bounded pending admission and one reader task per PTY, with a
capture capped by the existing scan limit. Use offsets in that capture
for independent observation windows. Raw mode signals attachment after
the initial writer, while exact tmux socket identity checks continuity. Give each stderr drain a 4 KiB capture
cap and consume further bytes only to avoid backpressure, with overflow a
test failure. Keep draining the old clients while the successor waits, in
addition to the earlier explicit capture windows. If the workload panics
or misses a deadline, signal all live children first, then concurrently
reap, drain and join stderr tasks under one overall 15 s cleanup deadline.
Kill-on-drop remains a fallback. Preserve the original panic and leave
owned tmux cleanup to the fixture's existing drop owner.

Report separate single-run wall durations as `observe_raw_us` for
observer launch to observed raw Ready, `choice_to_raw_us` for cooked-choice
write to observed raw Ready, and `successor_marker_us` for successor
state-command write to observed output. The Ready observations
include local polling and the sample does not measure physical rendering.
Report the independent Command turn durations. Use the existing
multi-trial backend benchmark for percentile measurements.

Run with matching normal native executables and independently with the
checksum-verified Linux AMD64 package installed by shell. Run the
existing foreground writer, controls and quiet AUTH selectors with
matching executables, plus strict all-target `installed-checks` Clippy.
Obtain Claude Code Auto review on both design and integrated source.
Record sanitized results and scope. The clients and NIP-42 relay run
locally, and the daemon and worker are the existing prepared fixture.
External relay trust, other platforms, distinct-device identities and
induced parked-takeover failure stay in their separate validation scopes.
