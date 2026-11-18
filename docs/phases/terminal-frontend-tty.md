# Local Terminal frontend and TTY safety

This proposed increment implements and tests the local frontend of the
[interactive roadmap](interactive-peers-roadmap.md). Obtain independent Claude
approval before implementation and review the completed code with Claude Code
in auto mode. Keep the module private to `marmish-native`. Its future caller
will be the foreground `marmish` process before ordinary Tokio runtime construction, once the supervisor
and IPC are available. This increment adds no CLI route or flags and
needs no profile, identity, daemon, relay, network or new dependency. Unit
tests model the eventual typed IPC boundary. An opt-in local test exercises
real controlling terminals.

## Boundary and interfaces

The frontend receives ordered bytes from the local daemon only after an
attachment is admitted. An authenticated remote shell, tmux instance or
server can still submit arbitrary bytes. Treat every output byte as hostile
until the display policy accepts it. The review applies equally to writer
and observer output and to a future redraw optimization. Input bytes travel
in the other direction and have a separate local detach recognizer. Terminal
emulator replies, clipboard access, local integrations and shell-visible
trusted prompts are outside the permitted display policy. An adversary can
still draw misleading text on the screen. Use finite local errors, and never
render, log, serialize or interpolate remote bytes into a diagnostic.

Use private `terminal_frontend` and `terminal_output` modules. The first owns
terminal state, signal dispatch, polling and the local escape recognizer. The
second consumes raw output through a single bounded streaming parser. The output
writer accepts only parser-approved units and fixed local normalization bytes.
It never writes directly from an IPC record or arbitrary error string. Hold one
parser and writer per attachment generation and discard both on disconnect,
policy failure or a fresh redraw. An IPC hangup stops rendering uncommitted
units and emits the fixed local terminal-error marker during cleanup.
Authenticated orderly close and exact output commit are separate activation
responsibilities. The frontend consumes typed admission (mode
and dimensions), ordered output, and close events, and emits input, latest
validated resize or detach. Use the proposed [Terminal record](terminal-frames.md) 4 KiB plaintext chunk,
a 32 KiB input queue and a bounded 128 KiB local output queue. Charge parser carry and unwritten
remainders to the corresponding budget. A later attachment owner handles
ordering, authority, acknowledgement, leases and sealed frames. Preserve raw
protocol offsets and delivery bookkeeping independent of the filtered display
bytes. Output removal neither creates a sequence gap nor licenses replay.
[Ephemeral Terminal transport](terminal-ephemeral-route.md)

## Terminal ownership and restoration

- Open `/dev/tty` independently as `O_RDWR | O_NOCTTY | O_CLOEXEC |
  O_NONBLOCK`. Do not mutate stdin/stdout file flags or read from a pipeline.
  Require stdin and stdout to be character terminals with identical
  fstat filesystem device and inode. Do not compare fstat device numbers
  against the /dev/tty alias. Compare tcgetpgrp on all three descriptors to
  the positive process group. Linux additionally compares tcgetsid on all
  three to getsid(0) and resolves the alias with TIOCGDEV to compare the
  underlying device. Darwin uses the TIOCGPGRP check on each descriptor,
  which the kernel rejects for a tty other than the calling process's
  controlling terminal. Check that property with the real two-PTY fixture.
  [Darwin TIOCGPGRP](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c)
  Capture `tcgetattr` and `TIOCGWINSZ` before admission. The supported
  range is 2–500 columns and 2–250 rows, with 24 by 80 used only when an
  explicit fixture asks for it. Refuse a live TTY with invalid initial geometry.
  Preserve the caller's exact termios fields in the restoration guard. Use the
  reviewed fixed xterm UTF-8 console capability and exercise it with tmux.
- Install the signal guard and arm restoration before changing the termios.
  After admission and a fresh foreground check, use `cfmakeraw`, explicitly
  disable `IXON` and `IXOFF`, set `VMIN=1` and `VTIME=0`, and apply with
  `TCSANOW`. Apply the same local display policy to both modes. An observer
  can read local physical keystrokes for detach detection and discards every
  other byte. It emits neither remote input nor resize. A writer forwards
  literal Ctrl-C, Ctrl-Z and other data and sends Resize on the ordered input lane. Keep one coalesced latest-size
  slot while waiting for lane space. Drain neither queued keystrokes nor terminal output with
  an unbounded `tcdrain`. Compare and restore the saved field values in tests
  without comparing struct padding.
- Run the frontend's synchronous poll loop on the dedicated early foreground
  process thread, before any executor, generic command signal handler or
  concurrent terminal owner is installed. Route the exact Terminal CLI
  command before the generic CLI constructs Tokio. Use a synchronous Unix
  IPC connection with poll-driven partial I/O on the owner thread. Reuse the
  reviewed typed codec during integration. Use the existing CLI signal exit
  outcome convention without installing its process-lifetime handlers.
  Guard this frontend instance against duplicate installation and publish the
  wake fd before installing handlers. Block the managed external signals during
  handler install and teardown on the owning thread. Roll back partially
  installed dispositions and masks on every error. Keep TTIN/TTOU safeguards
  enabled during actual job-control TTY syscalls.
- Use `sigaction` with `SA_RESTART` clear and a nonblocking CLOEXEC self-pipe.
  Linux uses `pipe2`, Darwin uses `pipe` and `fcntl` before concurrent work.
  Register `SIGWINCH`, `SIGTSTP`, `SIGCONT`, `SIGTERM`, `SIGHUP`, `SIGINT`,
  `SIGQUIT`, `SIGTTIN` and `SIGTTOU`. The handler sets one atomic pending bit
  and attempts one `write` of a fixed byte, preserving errno. `EAGAIN` means
  the pipe is already readable. The owner drains the pipe, atomically takes
  the bits and checks them again before the next read, write or poll. The
  self-pipe and published fd live until process exit, including after partial
  installation failure. Block managed signals, restore dispositions, drain
  wake bytes and clear pending bits on teardown. Auxiliary threads inherit
  the managed signals blocked and must keep them blocked. Promptness relies
  on pipe readiness. No I/O, locking, allocation or termios calls
  from a handler except the wake write. SIGTTIN and SIGTTOU use the handler as
  foreground-loss notifications and are neither ignored nor masked at an
  actual TTY call. Keep a last-moment foreground check before every TTY
  mutation and I/O batch. Foreground can change between user and kernel
  checks, so a background `EIO`, `EINTR` or job-control signal fences further
  I/O promptly. Verify that interrupted `tcsetattr` cannot silently change a
  newer foreground owner's terminal on the qualification platforms.
- Local externally delivered termination signals stop new I/O, run explicit
  cleanup, and return `128 + signal` without re-raising. The first signal
  fixes the outcome and later signals cannot re-enter cleanup. Typed 0x03
  and 0x1a stay user input. For foreground SIGTSTP, normalize the display, write the suspend
  marker and restore termios. Close local IPC, then send SIGSTOP to the
  frontend process only. A background SIGTSTP closes local IPC and stops the frontend without
  TTY mutation. The supervisor ceases keepalive as soon as IPC closes. Try
  bounded detach signaling but never wait for its response before stopping.
  Ignore stale SIGCONT while no local suspension is pending. On actual
  continuation, check for termination and foreground ownership. If still
  foreground, open a fresh attachment and require a redraw before accepting
  input. If background, exit without TTY mutation. Use SIGWINCH as a hint to
  read up-to-date geometry and coalesce one latest changed size. Share the
  2–250 row and 2–500 column limits between backend, frontend and frame
  validation in the integration and enforce them at every boundary.
- Use one `poll(2)` loop over the TTY, wake descriptor and the test or local
  IPC readiness seam. Bound each tick and each batch, and preserve exact
  cursors for partial writes. A full data queue pauses ordinary TTY input or
  daemon reads while progress is possible. The control lane still handles
  disconnect, signal, expiry and an already recognized detach. A detach tap
  still queued by the kernel is read when capacity returns. Externally
  delivered termination stays available throughout backpressure. An older
  authorized input batch may
  drain in order on a normal detach under the existing age bound. A fence or
  failed drain discards it and closes the connection without retrying input.
  No foreground writer may outlive cleanup. Prefer a single synchronous
  owner to a second terminal writer or an unjoinable blocking thread.
- Restore on handled exit, signal, local detach, refusal after admission,
  panic unwind and partial startup failure. Use an explicit `finish` path
  with finite results and a best-effort guard fallback. Stop new output,
  discard unstarted display units and finish at most the current partially
  written unit. Try reviewed local display normalization for up to 50 ms
  total and apply saved termios even if that output fails. Verify saved fields
  with tcgetattr to detect partial application. A kernel partial
  write may leave a prefix on the wire. If it cannot complete while foreground,
  mark display restoration incomplete and never append peer data. If the TTY
  still accepts writes, begin fixed local recovery with CAN before complete
  normalization units. An externally revoked foreground and a closed or
  unusable TTY can preclude restoration. In that case stop I/O and report a
  finite outcome without writing to another foreground job. `SIGKILL`, forced
  SIGSTOP before cleanup, and process or machine failure offer no restoration
  guarantee. Never claim to restore arbitrary preexisting emulator modes.

## Local detach control

Use `~.` at the beginning of the local input stream or immediately after a
local CR or LF. A writer forwards those line bytes in order and an observer
uses them only to update local line state. Keep at most one pending prefix
across reads. `~~` at
that position emits a single `~`. Any other byte after the pending `~` emits
both bytes unchanged. A solitary `~` at an orderly input EOF is forwarded
once for a writer if safe and otherwise discarded. The detach tap consumes
its two bytes, stops new input immediately and discards any remainder of the
current read. Release a pending prefix before the absolute 32 ms oldest-byte
budget, after which a later `.` is ordinary input. A monotonic timer is armed
from the prefix, independent of incoming output or other activity.

Recognize the exact ASCII bracketed paste delimiters `ESC [ 200 ~` and
`ESC [ 201 ~` from local input while the enabled display mode is active. Keep
at most six candidate bytes and forward the delimiters themselves in order.
Within a paste, `~.` remains data and detection resumes only after the exact
end marker or the local paste limit. A malformed delimiter falls back to ordinary input once, preserving
bytes and line state. Flush partial prefixes on the same local deadline.
A disabled paste mode resets the recognizer. An open paste expires after
five seconds of active read time or 64 KiB of local paste data. Pause the
timer while backpressure or suspension prevents reads. Reset local line
state on expiry to permit fresh `~.` without discarding prior data.
Externally delivered signals and supervisor revocation still close the frontend.
Use an injected monotonic clock in the recognizer tests.

## Streaming output policy

Apply this policy for every byte stream starting in the ground state. The
output contains canonical UTF-8 scalars, reviewed single-byte controls or
complete allowlisted control units. Do not infer a complete unit from a frame
or read boundary, and do not convert arbitrary bytes using lossy string
replacement. Hold at most one UTF-8 scalar (four bytes) or one short control
(96 bytes), plus a few counters. Preserve valid UTF-8 byte for byte except
encoded control scalars U+0080–U+009F, which are discarded. Accept only shortest
UTF-8 encodings of valid Unicode scalar values. Invalid leads and unpaired
continuations are consumed without being emitted. Drop bytes already buffered
in an invalid scalar and examine its first offending byte afresh. Recognize
raw C1 introducers only when outside a candidate valid multibyte sequence.
Never copy raw C1 or DEL to the local terminal.

| Input | Initial display policy |
| --- | --- |
| Printable ASCII or complete valid Unicode scalar | Pass exactly, preserving order and width handling by the client emulator. |
| C0 | Permit only BS, TAB, LF and CR, plus SO and SI for the fixed DEC line-drawing capability. Drop all other bytes, including ground-state BEL, NUL, CAN and SUB. |
| `ESC` sequences outside strings | Allow only standalone `7 8 D E M = >` and the exact two-byte suffixes `(B`, `(0`, `)B`, `)0`. Select one complete unit regardless of how its bytes are split. Drop operating-system, UTF-8 mode changes, terminal reset and other escapes. |
| Plain 7-bit CSI rendering | Allow no intermediates and only ASCII decimal parameters with empty/default slots where defined. Permit cursor movement and positioning `A B C D E F G H I Z d f` with at most one parameter except for `H` and `f` (two). Permit edit/scroll `@ L M P S T X b` with at most one parameter. `T` with multiple fields has other meanings and is refused. Allow `J` and `K` only with one optional value in 0–2, `r` with up to two row fields, and `s` or `u` only with no fields. Values are in 0–4096, with row margins no greater than 250. |
| Styling CSI | Permit plain `m` with at most 16 fields. Match ordinary SGR fields explicitly (0–7, 9, 21–29, 30–37, 39–47, 49, 90–97 and 100–107). Recognize `38;5;n`, `48;5;n`, `38;2;r;g;b` and `48;2;r;g;b` as complete groups using 0–255 components. Refuse unknown fields and colon forms for the initial subset. Permit `CSI Ps SP q` with a single 0–6 cursor-style value and that exact space intermediate. |
| CSI mode changes | Permit plain `4h/l` and private `?1`, `?6`, `?7`, `?25`, `?1049` and `?2004` followed by `h` or `l`, one exact value per unit. Reject mouse, focus, synchronized-update, title, report, keyboard protocol and other mode families. Reject all eight-bit CSI and any extra private introducer or intermediate. |
| Control strings | Discard all OSC, DCS, APC, PM, SOS and legacy `ESC k` strings. Consume every payload byte without buffering or recursion. This includes OSC 52 clipboard use, OSC 8 hyperlinks, OSC 7 working-directory hints, iTerm/download commands, sixel/Kitty graphics, and tmux passthrough. An 8-bit introducer enters the same discard state. |

For short ESC/CSI units enforce grammar and per-field limits while parsing.
A fresh ESC abandons a pending short unit and starts a new one, recognized C1
introducers may start discard, and CAN/SUB cancels a short unit. Any other
C0 aborts a pending ESC or CSI and is reconsidered in ground state. A raw
C1 CSI (0x9b) denies the entire unit through final byte, cancellation or fresh
introducer. A malformed or
oversize short unit discards through its final byte, cancellation or fresh
introducer. A CSI unit with a parameter immediately followed by an unknown
private or intermediate byte is denied as a whole. Never forward a partial
prefix or syntactically valid unit with an unapproved final byte. Do not
reassemble a control out of separately permitted fragments. Re-emit allowed
CSI in seven-bit canonical form, stripping leading zeros from numeric fields
and preserving meaningful omissions.

A discarded string ends on a complete `ESC 0x5c` or a standalone raw C1 ST
(`0x9c`). OSC additionally accepts BEL. CAN/SUB and nested introducers within
an unsupported string remain discarded payload. Account for paired `ESC ESC`
across chunks as escaped data, so its second ESC cannot combine with the next
backslash to terminate a tmux passthrough wrapper. Recognize the ASCII `tmux;`
prefix at the beginning of a DCS with a bounded prefix index. For that wrapper
only an undoubled 7-bit ST ends discard, even if a raw C1 ST or BEL occurs in
the payload. Do not unwrap or parse the inner stream. Do not let a C1 ST byte
from a candidate UTF-8 continuation terminate a string. If an incomplete
unit lasts eight seconds of consumable input time or a discarded string exceeds
64 KiB, end the attachment with a finite policy error and reset on a fresh
redraw. Poll the deadline even while no bytes arrive. Pause elapsed time
when I/O is suspended or cannot accept input. All bytes from discarded
strings stay outside the writer. Nested and doubled escapes only guide
where text rendering can resume. At EOF discard any
incomplete scalar, control or string. [XTerm control sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html) ·
[tmux 3.4 input parser](https://github.com/tmux/tmux/blob/3.4/input.c)

The output writer only accepts vetted complete units and uses the same type
for fixed trusted local controls. It may coalesce complete units into bounded
writes with a cursor retaining the current unit boundary. It renders them in
stream order. Normalization while foreground uses fixed SGR, cursor style and
visibility, scroll region, origin, wrapping, insert mode, ASCII G0/G1 and SI,
normal cursor keys and keypad, and bracketed paste resets. Exit alternate
screen only if this frontend's output enabled it. Record mode transitions at
completed writes, account for a partially sent transition during cleanup, and
never use `RIS` or clear the user's scrollback. Report an incomplete display
cleanup after local recovery, separately from the termios result. End
normalization, after any required alternate-screen exit, with fixed CR LF,
one fixed local marker (`[marmish: detached]`, `[marmish: suspended]`,
`[marmish: interrupted]` or `[marmish: terminal error]`), and another CR LF
while OPOST is clear. Use this order for suspend, detach and errors.
Restore saved termios after this trailer or after the display deadline.
A peer can use allowed output to draw fake prompts, hide text using matching colours
or fill scrollback. Local cleanup cannot erase that history.

The current backend sets `TERM=xterm-256color` and `-u` in the attach worker.
Tmux 3.4 sends device and color queries while opening that terminal and has a
feature fallback timer. Drop queries without synthetic replies, and verify
fixed capability rendering against actual tmux with the filter applied. Both
ordinary and alternate-screen updates must still render, resize and recover
when those queries remain unanswered. Any extra mode or sequence required by
qualified tmux must be reviewed by exact grammar and terminal effect before
allowlisting it. Preserve input for arrow/function keys and bracketed paste
without accepting output that requests generated emulator replies. Disable
server-side tmux passthrough separately when the supervisor owns tmux
configuration. [Native backend](terminal-native-backend.md) ·
[tmux 3.4 TTY initialization and requests](https://github.com/tmux/tmux/blob/3.4/tty.c#L293-L360)

## Verification and phase exit

1. Add one focused `core_tests_terminal_frontend` selector to the existing
   trusted-branch core workflow. Keep it offline, without tmux, and bounded
   well under one second of test execution. Cover text and all allowed
   sequence families at every byte split and with deterministic rechunking.
   Cover multibyte scalars with C1 continuation bytes, malformed and overlong
   scalars, raw C1 forms, unknown syntax, split BEL/ST, doubled ESC inside
   nested tmux envelopes, timeout, oversize and EOF. Assert exact sanitized
   output on a ground-state token oracle and assert no prefixes emitted from
   blocked controls. Exercise targeted OSC 52 reads and writes, download,
   hyperlink, title and window changes, device reports, focus and keyboard
   queries, and combined private mode lists. Split CSI permits and denials
   around the prefix, field, delimiter, intermediate and final transitions.
   Exercise all 256 byte values from every parser state, validate emitted
   controls against an independent grammar oracle, and check idempotence
   and chunk invariance under identical byte and clock budgets.
2. With a fake clock and deterministic nonblocking sinks, check partial output,
   EINTR, EAGAIN, queue backpressure, token-boundary cleanup, and restoration
   ordering even if the display sink cannot drain. Test local detach's split,
   doubling, failed match, deadline and EOF cases, plus split paste markers,
   paste containing CR/LF and `~.`, paste expiry by both time and bytes,
   and observer gating. Check coalesced
   resizes, valid limits and an unchanged previously accepted size after an
   invalid request. Feed deterministic high-volume hostile strings and verify
   bounded scratch memory and linear work.
3. Add an opt-in POSIX process fixture that launches the exact frontend unit
   in a dedicated executable with an early main and one private controlling
   PTY. Reuse the local tmux proof's finite watchdog and held-child ownership
   conventions, under an isolated private directory. The child creates its
   session and `TIOCSCTTY` on its own slave before entering the frontend.
   Use separate readiness/control descriptors for the fixture, never inherited
   by a shell. Check terminal flags before admission, during raw mode, after
   failed admission, after handled exit and after injected unwinding. Check
   typed Ctrl-C/Ctrl-Z, window ioctl/SIGWINCH through the real PTY, signal
   coalescing and wake under a full data queue, partial nonblocking writes, and
   read-only detach. Observe SIGTSTP stop via `waitid(WSTOPPED | WNOWAIT)`,
   confirm restoration before continue, exercise foreground-gated resume and
   fresh redraw, and send SIGHUP/SIGTERM to the held child only. Cover
   duplicate install, every injected partial sigaction failure, descriptor
   number reuse, a SIGWINCH storm during writes, termination during suspend
   cleanup, background SIGTSTP, stale SIGCONT, and SIGTTOU at tcsetattr. Verify denial
   with no controlling TTY and with a different foreground process group.
   On Darwin check foreground from within the controlling session, as done by
   the existing tmux qualification. Assert stderr and errors expose no raw data.
4. Extend the opt-in native tmux proof or share its fixture. Run the real tmux
   attachment backend and frontend PTYs with an in-memory bridge that supports
   fixed chunk boundaries and caller-triggered detach. Check an actual
   first render, ANSI color, cursor movement, alternate-screen redraw,
   UTF-8/wide text, bracketed paste and Ctrl-C while all queries are discarded.
   Inject hostile synthetic strings into the same frontend output lane and
   compare the actual receiving PTY bytes with reviewed expected bytes.
   Confirm a blocked string cannot leak raw control bytes to the PTY, and a
   clean detach restores the original TTY while the tmux pane stays alive.
   Check native Linux x64, Linux arm64 and macOS arm64 before claiming native
   qualification. Cross-target checks establish source compatibility only.
5. Outside routine Actions, benchmark at least 1,000 short output chunks and
   256 KiB or more each of text, color-heavy redraw, and hostile strings, at
   one-byte, 128-byte and 4 KiB boundaries. Report filter p50/p95/p99 latency,
   throughput, queue and parser high-water marks, and rejected bytes. For the
   offline real PTY bridge, report at least 30 paced input-to-visible-response
   samples, first visible render, exit restoration latency and write syscall
   counts, with warmup, platform, tmux version and load noted. Run with and
   without local filter cost in the trusted synthetic timing harness only.
   Report failures and resource use separately. These are local measurements.
   Sanitized evidence belongs under `docs/validation/`.

Implementation exit requires the focused suite and a native Linux local PTY
and tmux proof, plus independent code review. Record any runner gaps and keep
them as frontend activation gates on the affected platforms. The supervisor
phase owns actual IPC wiring, lossless ordering, lease renewal, session
admission and resume. Test terminal filtering through that entry point before
exposing it to remote output. [Local tmux qualification](../validation/terminal-local-tmux-proof.md)
