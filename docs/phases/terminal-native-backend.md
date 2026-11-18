# Native terminal backend increment

Implement the private host backend from the
[interactive roadmap](interactive-peers-roadmap.md), informed by the
[local PTY and tmux fixture](terminal-local-tmux-proof.md). This increment owns
one local attachment and exercises it through a native test harness. The
authenticated supervisor will become its normal caller in a later increment.
There is no remote command, shell grant, bootstrap or relay traffic here.

## Launch boundary

- Add one reserved worker token to the executable-only early dispatch in
  `cli::main`. Require `daemon_binary` and `validate_executable_layout` before
  entering it. Reserve the token in command configuration and reject it from
  normal frontend parsing. The branch precedes profile, Identity, Engine and
  async runtime setup and gives its parser only the expected fixed arguments.
  Both binaries continue through normal parsing for ordinary invocations.
- Use an explicit absolute, canonical layout-validated daemon executable in
  the caller API. The daemon caller will supply its own path and the
  integration fixture uses `CARGO_BIN_EXE_marmishd`. The test reads
  `MARMISH_TEST_TMUX`. Product code takes an explicit validated path or uses
  fixed default locations and reads no fixture variables. Keep the caller API
  `#[doc(hidden)]` in this increment so integration can run it and document its
  active intended caller. Expose only checked path and attachment types.
- Independently verify nonzero account identity and real/effective UID and
  GID before and inside the worker. On Linux also verify saved IDs. On
  Darwin require the same real/effective IDs and reject `issetugid()`.
  Existing engine account helpers are private and their command-execution
  trust rules differ. Use bounded passwd lookup for the future login shell
  and allowed environment, with no new engine export in this increment.
  Reuse native layout checks for the daemon plus a terminal-specific tmux
  path validator. Check the link locations and resolved executable chain,
  regular executable, no set-ID bit, trusted directory owners and no group
  or world write bits. Refuse excessive link depth and untrusted paths.
- Open the master and slave with atomically close-on-exec PTY descriptors.
  Linux uses `TIOCGPTPEER`. Darwin obtains the slave pathname from
  `TIOCPTYGNAME`, checks its `lstat` as a character device owned by the
  caller, and opens with `O_NOFOLLOW`. Require matching owner, mode,
  filesystem device, inode and device number from `fstat`. Set the master nonblocking. The worker receives
  only the validated description, slave on fd 0, status pipe on fd 1 and
  /dev/null on fd 2. Verify the endpoints and installation again in the
  worker. A Darwin anonymous status pipe may briefly have an unflagged
  end before setting close-on-exec. The only possible inherited payload is a
  fixed launch stage. An inherited end can delay EOF and cause bounded false
  launch failure. It cannot authorize an attachment. No secret or PTY is on
  that pipe. Design a shared spawn lock only if observed compatibility
  evidence requires one.
- Use no multi-threaded `pre_exec` and no parent `process_group(0)`. In the
  single-threaded early worker, disable panic output before touching the
  descriptors. Use only fixed status records and `_exit` for errors. Reset
  the signal mask and inherited dispositions, including hangup, interrupt
  and job-control signals. Duplicate the status writer with CLOEXEC and
  close unrelated inherited descriptors. On Linux call `SYS_close_range`
  with flags zero for the ranges from fd 3 to one below the duplicate and
  from one above it to the maximum unsigned descriptor number. When the
  syscall is unavailable, fall back to an exact `/proc/self/fd` inventory
  to keep the Linux 4.13 floor. On Darwin use an exact `/dev/fd` inventory
  in the single-threaded worker. Exclude the enumeration directory's own
  descriptor, close every other fd above 2 except the duplicate, and
  close the inventory descriptor afterward. Require inventory to show
  the known status duplicate and its own directory descriptor. Fail
  closed if enumeration cannot be established. Only afterward map the
  slave over fds 1 and 2, replacing the old descriptors via `dup2`.
  Treat unexpected descriptor errors as refusal.
- Take the controlling terminal with `setsid` and `TIOCSCTTY`. Emit an
  explicit session-owned stage after the session transition and a fixed
  exec-ready stage just before exec. Do not use stdout, stderr, logging
  or Rust unwind once the worker starts. Duplicate the slave onto 0–2 and
  exec the exact tmux executable with fixed `-u -S ... -f /dev/null
  attach-session -E -t =...` argv. Clear the environment and provide only
  a documented safe PATH, home, user, TERM and UTF-8 locale. No
  `TMUX`, `TMUX_TMPDIR` or profile environment. If exec returns, report a
  fixed exec-failed stage through the still-open CLOEXEC writer. Parent
  launch success requires both ordered stage records, writer EOF and
  survival through a 50 ms EOF grace period. Observe with nonblocking
  wait during the period and at its end without reaping a PID needed for
  ownership. Exit during the period is a launch failure. Later exit is
  the attachment's ChildExited result. This classification bounds an
  observation, and actual render qualifies interactive readiness.
- Attach only to the harness-owned tmux session in this increment. Require
  an exact, length-bounded pathname under the checked owner-only socket
  directory and an owned Unix socket. The fixture verifies its foreground
  tmux child and session association. A missing server, inaccessible socket
  or wrong path fails before spawn with no new server where detected. A
  stale socket that passes path checks but causes tmux to exit inside the
  grace period is a launch failure. A later exit remains ChildExited.
  Product ownership recovery and
  lifecycle have separate approval. The test watchdog handles its fixture
  server; the attachment backend owns only the attach child.

## I/O and ownership

Use a synchronous nonblocking API driven by bounded `poll(2)` with a wake
fd for later cancellation. It owns the PTY master and unreaped child.
Distinguish would-block, partial writes and completion. Normalize Linux EIO
and Darwin EOF after closure, and watch child exit independently of remaining
bytes. Require the exact local generation under the same mutable borrow
immediately before each write or resize. A superseded generation can neither
write nor resize. A deterministic test fences between two partial writes.
The later supervisor supplies current grant and monotonic lease checks at the
same syscall boundary.

Before the session stage, target only the held child PID. After that stage,
verify its session and process group against its unreaped PID. Send group
signals only with that verified ownership and reserve the leader PID with
`waitid(WNOWAIT)` before final reaping where necessary. Use one bounded
`shutdown(deadline)` operation that returns either Reaped or Pending with
the retained cleanup handle. The caller keeps retry ownership for Pending.
Drop cannot block. On an unexpected drop, close the master and hand any
still-owned child to a bounded reaper already reserved for the attachment.
If that reservation cannot be made, refuse before spawn. Do not reuse a
foreign PID or group. Test the Pending and timeout logic and an unexpected
drop handled by the pre-reserved reaper. Keep all fixture processes under
test ownership until their exit is observed.

## Validation

Run fast path and worker dispatch tests without tmux. Cover reserved-token
refusal by `marmish`, unsupported descriptors and IDs, ordinary daemon
help, malformed or incomplete status, helper exec failure and an already
waitable test child at the grace check, inaccessible socket, stale generations and descriptor cleanup. Use private
fixtures and deterministic sources where a real low-privilege invocation
cannot set different IDs. Verify the no-profile entry ordering structurally
through the dispatch and controlled fixture behavior. Add only one focused
core label to trusted-branch GitHub Actions.

Run the real daemon worker binary via the named ignored tmux integration
test for resize, Ctrl-C, exact reattach, UTF-8 output, hangup, retained fixture
state and Linux old-slave descriptor checks. Measure spawn-to-first-render,
paced simulated keystroke latency, detach and reattach separately. Keep the
real benchmark outside routine Actions. Native verification requires the
existing self-hosted Linux X64, Linux ARM64 and macOS qualification runners.
The `macos-native-regression.yml` runner is available for Darwin validation
once that workflow is safely authorized for the review ref. Report any
unavailable runner and distinguish source checks from native execution.
