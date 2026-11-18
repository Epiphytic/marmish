# Native PTY and tmux attachment

This phase follows the [interactive roadmap](interactive-peers-roadmap.md) and
requires the approved [Terminal attachment confidentiality proof](terminal-attachment-confidentiality.md).
The proof must complete with pinned MDK, SQLCipher and a bounded relay before
the PTY path carries real terminal traffic. This design covers the native
attachment, the first lossless writer and same-daemon disconnect recovery.
Concurrent observe, explicit takeover and `--new` selection build on
these interfaces in the next increment. Screen diffs are a later,
negotiated mode.

## Admission and entry points

- `marmish terminal <npub-or-alias>[@relay]` uses the existing destination,
  identity and relay policy resolution. It requires a foreground controlling
  terminal and rejects redirected standard streams. Existing command, rsync,
  copy and stream invocation paths stay available without tmux.
- Extend the private profile config with `terminal.clients`, a bounded list of
  canonical npubs that explicitly grant an interactive shell. The default is
  empty. Each entry must also belong to `execution_clients`, and the profile
  must have the server role. This also admits that identity to command rooms
  under the ordinary command policy. An operator granting terminal access
  can explicitly deny noninteractive commands with that policy.
  Wildcards, default executable restrictions and `allow_shell_command`
  grant no interactive access. Show this grant separately
  in future permission views. An interactive shell runs with the full OS
  permissions of the daemon's nonroot execution user. Reject root and set-ID
  execution for terminal service. Check the server identity against the
  client's existing trust and destination policy.
- Authenticate the exact two-member Terminal room, current branch and epoch,
  both roles, key confirmation and a fresh attachment before allocating a PTY
  or creating tmux state. The first authorized idle attach owns input and
  resize. An ordinary attach to an occupied session returns `terminal_busy`.
  Admission distinguishes disabled service, denied grant, missing or unsupported
  tmux, capacity, missing session and unavailable route. Keep process paths and
  raw backend errors local.

## Recovery and configuration

On startup and explicit daemon `SIGHUP`, read the config through an owned
private directory descriptor and open `config.json` relative to it with
`O_NOFOLLOW | O_CLOEXEC`. Reuse the private-file checks on the descriptor,
read and bound one file snapshot, and apply a normalized result. Setup
replaces the file atomically. A concurrent reload reads either the complete
old file or the complete new file. A later reload reads the current file.
Install signal handling in the daemon service shared by foreground and
background startup before announcing readiness. In both modes, `SIGHUP`
requests a reload. A foreground daemon therefore survives a controlling
terminal hangup. Use `SIGINT` or `SIGTERM` to stop it. Hand a policy
snapshot to the terminal supervisor independently of a network turn.

For reload use the intersection of explicit terminal grants, the startup
`execution_clients` set and the freshly read `execution_clients` set. A
removed execution grant fences terminal access immediately on reload.
New execution members require a restart to become eligible. Other changes
to normalized nonterminal settings report `restart_required` and retain the
startup values for ordinary command policy and running services. Terminal
runtime settings other than the grant list also require restart. An invalid
reload closes terminal admission, fences and detaches active attachments,
and invalidates queued work and attachment keys. Keep their tmux sessions
for the normal detached retention interval. A valid reload reopens
admission and ends owned sessions whose keys it confirms were removed.
Session exit and detached retention still apply during a failed reload.
Current command operations keep their startup config. An older binary
rejects the new field because its config schema denies unknown fields.
Document removal of the `terminal` field before rollback.

The client daemon reserves one local writer frontend per destination while
connecting, attached or recovering. A second local request returns
`terminal_busy` while that reservation is live. After relay loss, the same
reserved frontend can request recovery. Local frontend EOF fences that local
attachment even if remote detach cannot be sent. While the daemon survives,
the next local frontend may then recover using the prior attachment handle
kept only in daemon memory. A clean detach acknowledged by the server permits
an ordinary attach.

Recovery uses a fresh handshake with a distinct request mode and the exact
previous session and attachment IDs, bound into the confirmation transcript.
The server prepares only a bounded pending exchange. After confirming the
fresh exchange, the server independently checks the prior handle, unchanged
frozen room authority and exact client leaf, current grant and writer
generation. It fences and reaps that prior writer before granting the next
generation and starts a fresh PTY. Keep the existing writer if the request
fails confirmation. Only an exact replay of the same pending exchange may
resume its handshake. Concurrent claims, stale IDs and other MLS leaves
cannot modify the live writer. There is no input replay across generations.
If the client daemon restarts and loses its volatile handle, an ordinary
attach can wait for the existing 11-minute lease to expire. The server's
authenticated `terminal_busy` reply includes a retry-after hint rounded to
seconds and bounded by that lease, when it holds a live writer lease.
If the server has expired or removed the prior attachment, resolve the
default session and use the ordinary free-slot handshake. For a retired room, the proof's
room rotation must fence the old attachment and preserve the session
mapping before the new room can admit a writer. Review the recovery transcript
extension with the confidentiality proof before implementing it.

## Daemon and local IPC

The **client daemon** binds the local Terminal socket and owns its frontend
reservations, authenticated exchanges, and bounded in-memory admission, input
and display queues. The frontend process performs local TTY I/O. The client
daemon has no attachment PTY or tmux child for the client role. The **server
daemon** has no local Terminal frontend for the server role. Its terminal
supervisor owns per-session state, grants and worker controls. Each writer
worker owns the PTY master, cursor, bounded queues, attachment child and writer
fence. A profile configured for both roles enables both components with
separate role limits. Both use the existing single profile lease and
identity-owning MDK owner.

`daemon.rs::serve_owner` serializes mutable MDK turns. Add terminal-specific
owner entries to submit proof-validated sealed records with the current
epoch pin and to poll Terminal-ready effects into a bounded mailbox.
Accept typed public handshake records on the same purpose-specific lane.
Use the proof's publication permit, receive acknowledgement and deletion
interfaces and never enter Foundation `Send` or `Received`, the command
adapter, or its application journal for these records. Complete the socket
and PTY operations outside the owner. No worker waits while holding a grant
or IPC queue lock across MDK, storage or relay work.

Initially keep the existing 100 ms MDK tick and a bounded Terminal drain
per turn. Serve up to one outgoing record and up to two eligible incoming
deliveries in a turn, with round-robin choice among ready rooms and one
unresolved native application send per room. Reuse native retained-send and
ingress progression without waiting inline for an end-to-end terminal ACK.
Keep command ticks, room intents and the Terminal mailbox independently
bounded. Existing tick calls can occupy the owner for up to 15 seconds and
legacy Foundation turns for up to 120 seconds. The 8–32 ms batch settings
bound queue preparation only. Each relay direction can incur a 100 ms turn
wait when healthy, and an occupied owner can delay it further. The worker's
local fence, overflow and expiry controls operate while the owner is busy.
On route or owner delay that exceeds the queue age bound, detach and require
a fresh redraw. Validate mixed traffic before changing the scheduling model.

The terminal submission cap is independent of Foundation's Base64 and page
limits. Count the largest actual Terminal JSON record, full signed inner
event, sealed bytes, native envelope Base64 and signed kind-445 event
including routing and expiry overhead in the proof. Measure the worst
full-width allowed counters and maximum group identifier, and exercise the
configured relay's accepted wire size with bytestream fixtures. A raw 8 KiB
chunk plus the 16-byte AEAD tag takes 10,944 Base64 bytes before either
envelope. Set the final raw cap to the largest proved fit up to 8 KiB and
refuse larger native envelopes before publication. Record results and enable
the lane only after this check.

Use a separate duplex Unix socket with a short basename under the profile
lease. Reuse the owned private directory, 0600 socket, path-length and
bidirectional same-UID
credential checks from `ipc.rs`. The existing JSON exchange half-closes after
one request or response and retains its current limits and semantics. Use a
versioned, length-prefixed bidirectional Terminal IPC with a bounded handshake
and typed records for attach, admitted, input, output, resize, detach and error.
Reject invalid lengths, directions, pre-admission data and EOF mid-frame. Bind
the connection to its server-assigned attachment ID, which the local client
cannot change after admission. Allow only one concurrent IPC connection per
attachment. Use separate finite permits for connecting and attached frontends,
and release both on shutdown, close, timeout or invalid data. Stop and join the
terminal listener and tasks before releasing the identity lease.
UID is the local security boundary, as for command IPC. Another process running
under the same UID can request an attachment via the client daemon using that
profile identity. The foreground-TTY checks are performed by the frontend for
safe interaction and cannot authenticate a process to the daemon.

The session worker owns the PTY master, its outbound queue and the writer fence.
The control lane can revoke or detach even when data queues are full. Write one
nonblocking slice at a time and check the current grant, attachment ID, writer
generation and lease at the actual syscall. A revocation or writer change is
serialized with that syscall, with no asynchronous wait between the fence check
and the write. Recheck on subsequent partial writes, retries, readiness changes
and resize. Following a fence, the worker accepts no further bytes or resizes
under that generation. Bytes accepted by the kernel before the fence can
still reach tmux or the shell. For replacement, close the previous PTY, send
SIGTERM to the owned attachment child if needed and reap it. Allow one second
for graceful exit and then one more after SIGKILL. If reaping remains
unconfirmed, retain the fenced slot for cleanup and refuse a replacement.
A background cleanup task remains responsible for the exact owned child.
Never retry ambiguous input by translating it into a new frame. Relay retries
reuse the identical sealed sequence and duplicate frames cannot reach the PTY
twice. When ordering cannot be recovered within the bounded receive window,
discard the attachment and require a clean redraw on reattach.

## Native process and terminal lifecycle

- Open the PTY master at `/dev/ptmx` with `O_NOCTTY | O_CLOEXEC` and use
  `grantpt` and `unlockpt` on that descriptor. Linux opens the slave
  directly with `TIOCGPTPEER` and close-on-exec flags. Linux kernel 4.13 or
  later is required for terminal service. If the peer ioctl is unavailable,
  refuse terminal admission with a local compatibility diagnostic. macOS uses
  the kernel's `TIOCPTYGNAME` to name its slave and opens that path with
  `O_NOCTTY | O_NOFOLLOW | O_CLOEXEC`, then checks the result is a
  character device. Allocate all daemon-side descriptors atomically
  close-on-exec, including duplicates. Set the initial window size
  and keep the nonblocking master in the worker.
  After authorization launch a single-threaded private Rust entry point of
  the daemon binary with ordinary `Command`, mapping the slave to stdin
  (fd 0), the status pipe to stdout (fd 1), and stderr (fd 2) to
  `/dev/null`. Use no `pre_exec` hook. This entry point runs
  before Tokio, identity or database setup. It duplicates the status pipe
  with `F_DUPFD_CLOEXEC`, establishes a new session and controlling
  terminal with `setsid` and `TIOCSCTTY`, installs the slave on fds 0–2
  and replaces itself with tmux using `exec`. The status pipe carries only
  a fixed launch stage or failure. The daemon closes its slave copy,
  closes partial descriptors on error and reaps the exact child. Retry
  EINTR, wait on EAGAIN, and handle Linux PTY master EIO after slave close
  alongside EOF on macOS. Successful writes alone advance the byte cursor.
  Observe child exit independently of remaining readable bytes.
  [Linux PTY driver](https://github.com/torvalds/linux/blob/master/drivers/tty/pty.c),
  [Apple PTY driver](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty_ptmx.c).
- Require tmux 3.2 or later. Resolve an operator-configured absolute
  executable before admission, or use a fixed candidate in `/usr/bin`,
  `/usr/local/bin` or `/opt/homebrew/bin`. Resolve links such as Homebrew
  Cellar links once and check the canonical executable and every directory
  in its chain. Require a regular executable, no set-ID bits and root or
  service-UID ownership without group or world write permission throughout
  the chain. Use the checked absolute path with a cleared environment.
  Verify version and support for required flags in a bounded probe. Use
  fixed argv, exact session matching and a private
  `-S` socket per tmux server and session. Per-session servers keep ordinary
  tmux session switching scoped to a single Marmish session. Use a reviewed
  generated config with bounded scrollback, clipboard integration disabled and
  a trusted passwd-derived absolute login shell. Start with a cleared
  environment and explicitly set the execution account's home, user, safe
  PATH, locale and fixed supported TERM. Disable environment updates on attach
  (`-E`). Validate the working directory and prevent descriptor, identity key,
  database credential and launcher environment inheritance. Only the local
  operator specifies these paths. The public command policy never constrains
  commands typed into the authorized shell.
  [tmux 3.2 attach implementation](https://github.com/tmux/tmux/blob/3.2/cmd-attach-session.c).
- Anchor tmux socket and state creation in a verified 0700 directory with an
  owner-only creation mask. Reject symlinks and unexpected object types. Check
  the full path against the shorter supported Unix socket limit. Support an
  explicit trusted short runtime directory when a profile path is too long.
  Keep tmux sockets separate from `ipc::bind` stale-listener reclamation. The
  daemon records a content-free session mapping under the profile lease before
  creating tmux. It binds daemon identity, peer, effective UID, opaque session
  ID, MLS group and expected private socket. On restart, reconcile creation
  reservations and known sockets using exact identities, recheck grants, and
  create a fresh attachment handshake. Only signal or clean a session the
  profile can prove it owns. Fail closed on mismatch and report cleanup state.
- Client detach, authenticated lease expiry and orderly daemon stop close
  the attachment PTY and reap the tmux client while the shell remains in
  tmux. An unobserved relay loss can leave the old slot occupied until recovery
  or lease expiry. A fresh PTY draws the current screen on reconnect.
  Shell exit, server revocation or detached retention expiry ends the owned tmux session and clears
  the mapping only once termination is proved. Deliberately detached OS processes
  may survive. Disable raw logs and scrollback export and keep attachment keys,
  plaintext, queued input and partial writes out of durable state. Only the
  content-free mapping and the inner ciphertext identified by the proof can
  persist in Marmish. Tmux itself keeps bounded in-memory scrollback while alive.
- The next increment uses a new PTY per observer with tmux `attach-session -E -r`
  and filters input before the backend. Read-only attaches must never send input,
  resize, terminal query replies or process signals. A slow observer loses its
  attachment and may reconnect for a clean redraw without stalling the writer.
  Takeover revokes the former writer's generation and waits for the former
  child to be reaped before creating a new writer. Tests for multi-attachment
  policy live with that increment. The single-writer phase proves fencing on
  detach and revoke.

## Frontend, output safety and limits

Open `/dev/tty` independently and verify it is the foreground terminal. Capture
termios and window size before requesting attachment. Enter raw mode only after
acceptance and install cleanup before the first terminal mutation. Restore mode
and reviewed terminal display state on disconnect, refusal after admission, error,
SIGTERM, SIGHUP and local detach. Restore mode before local SIGTSTP suspension and
reapply it on SIGCONT only while foreground with a valid attachment. Raw Ctrl-C
and Ctrl-Z are remote foreground job-control bytes. Use a distinct local detach
escape at the beginning of a line with a doubling rule for a literal escape, and
test byte boundaries. Deliberately uncatchable process death can require the
operator to reset their local terminal. Send `SIGWINCH` updates as bounded resize
records, applying `TIOCSWINSZ` only for the active writer. If cells are outside
the supported range, fail the resize without changing the active geometry.
Use a bounded native `poll(2)` I/O worker with a cancel/wake pipe for the
frontend TTY on Linux and macOS. This avoids reliance on macOS kqueue
registration for `/dev/tty`. Use the same readiness strategy for the
server PTY if the native macOS fixture rules out `AsyncFd`. Signal handling
wakes the frontend worker, stops TTY I/O before restoration, and joins it
before the frontend exits.

Place a streaming terminal control policy before writing to the local TTY. Carry
parser state across frames and preserve UTF-8 and supported VT sequences when
split. Pass ordinary text and reviewed rendering and mode controls such as
cursor movement, colors, alternate screen and bracketed paste. Reject clipboard
OSC 52, unapproved OSC or DCS strings, window manipulation, file transfers and
terminal report requests in the baseline. Bound control sequence length and
discard oversized strings safely through their terminator or terminate the
attachment. No peer bytes reach the local terminal from debug or error paths.
Verify the fixed tmux terminal capability works with filtered queries. Preserve
the permitted rendered stream in order, including split escape and UTF-8 input,
with no last-frame-wins behavior.

| Limit | Initial value |
| --- | --- |
| Local and sealed terminal data chunk | At most 8 KiB plaintext, or less if the confidentiality proof establishes a smaller encoded MDK limit |
| Batch timer | 8 ms idle, up to 24 ms when busy, absolute 32 ms since oldest queued byte |
| Dimensions | 2–500 columns and 2–250 rows |
| Pending writer input and per-attachment output | 32 KiB and 128 KiB respectively |
| Client daemon | Up to 4 in-progress local handshakes and 16 active frontend slots per profile |
| Server daemon | Up to 4 in-progress remote handshakes, 16 total tmux sessions and at most 1 live attachment worker per session in this phase |
| Data publication rate | At most 8 input or output records/s sustained per direction per attachment with burst 2, within a profile-wide allowance of 8 data records/s sustained with burst 4 shared across both directions and local roles |
| Keepalive and lease | 5 min default, 11 min since last fresh authenticated activity, measured on a suspend-aware monotonic clock |
| Detached idle retention | 24 h default, independently configurable from encrypted relay message retention |

Flush detach and resize promptly without reordering earlier authorized input.
Arm the batch timer on the oldest byte and test it under continuous traffic.
Count only fresh authenticated inbound activity from that endpoint toward its
lease, excluding output, duplicates and other attachments' activity. Use
`CLOCK_BOOTTIME` on Linux and `mach_continuous_time` on macOS for lease and
queue age, and check expiry before any post-resume write or resize. Timers
can run late while the host sleeps. After resume, expired traffic must be
fenced before it can run. Handshake, keepalive and fence records use bounded
control capacity outside both data token buckets. Give handshake and fence
records reserved mailbox space. The data token buckets count published
records independently of the batch timer. Retain the byte order of ready
batches. Input saturation pauses local reading while recovery is
possible, with the proof's 30-second age bound. If output would
lose bytes or the backlog exceeds its deadline, detach and use a new PTY to
redraw. Enforce all limits before growing queues, spawning work or reserving resources.
[Linux clock reference](https://www.man7.org/linux/man-pages/man2/clock_gettime.2.html),
[Apple continuous clock reference](https://developer.apple.com/documentation/kernel/1646199-mach_continuous_time).

## Validation and exit criteria

- Deterministic unit tests cover closed framing, bounded allocation, private
  socket rejection, distinct grant/default behavior, resize ranges, partial
  writes and fencing, 5/11-minute virtual-clock behavior including a suspend
  jump, continuous batching and record pacing, split escape sequences and raw
  gaps, queue saturation and isolated cleanup. Prove a new writer waits for
  the old child to be reaped and that invalid recovery cannot fence it.
- A native PTY fixture runs the real frontend in a controlling TTY and a real
  tmux shell under a private short socket with disposable identities. Test echo
  disabled, application-generated output, Ctrl-C reaching a foreground process
  while the shell remains alive, `stty size` and SIGWINCH, literal controls and
  Unicode, bracketed paste, entering and exiting full screen, blocked clipboard
  controls, termios restoration on failure and handled signals, detach and fresh
  redraw after changing shell state, recovery before lease expiry after a
  relay loses the detach, rejection of a second live local frontend,
  restart with live tmux and fresh attachment keys, and grant revocation.
  Exercise SIGHUP for foreground and launched background daemons, config
  rename and symlink cases, malformed reload preserving the detached shell,
  removal of a terminal or execution key on a valid reload, and
  restart-required configuration edits.
- The installed two-daemon acceptance uses an explicit bounded relay, existing
  identity ownership and actual sealed Marmot traffic. Check durable snapshots
  against the confidentiality proof, old room and binary behavior, duplicate
  and reordered traffic, local authorization failure, exact tmux process cleanup,
  and standard command, stream, rsync and copy behavior. Run Linux AMD64/ARM64
  and macOS ARM64 with the native runner or local equivalent and retain the
  actual targets exercised. Capture `tmux -V` and verify the installed path
  on the self-hosted macOS slot before scheduling its fixture. Availability
  on that runner has not been established. Missing tmux is a failure in the
  explicitly invoked native fixture and a normal product refusal. The
  default core suite does not require tmux or a relay.
- Measure key-to-application-response and first render on a monotonic client
  clock. The raw pane fixture replies with a nonce only after reading the input,
  and echo is disabled. Measure PTY/local IPC baseline and relay-backed cold
  attach and warm traffic separately. Also measure scrolling goodput on a fixed
  fixture, batching delay, frame count, bytes, CPU time and observed queue peaks.
  Use warmup followed by at least 1,000 draws to report p50/p95/p99 for a
  warm latency workload. Report at most p50/p95 for 100–999 successes.
  Keep cold sample counts and failures separate. The initial healthy
  loopback objectives are warm p95 below 750 ms, p99 below 1.5 s, cold attach
  p95 below 10 s and one-attachment scrolling goodput of at least
  16 KiB/s for a 256 KiB fixture. The owner, relay and SQLCipher measurements
  include queue age, command response under terminal load, relay events/s,
  relay ACK and limiter outcomes, MDK transactions and observed fsync or
  checkpoint cost under the configured terminal rate. Compare command-only
  and mixed traffic on the same machine. Record the enforced byte and slot
  bounds, peak queue use, and child/thread counts. Record revision, platform,
  tmux version, relay type, active settings and fixture sizes in sanitized
  evidence under `docs/validation/`. These objectives guide the phase
  assessment. They are measurement targets outside routine CI, and any
  miss needs a reviewed explanation or tuning before closing the phase.
- Add only the deterministic core smoke to the routine self-hosted CI subset.
  Keep fixture, installed and benchmark runs explicitly selectable. Aim for a
  sub-minute warm smoke excluding compilation, avoid hard performance gates on
  noisy runners, and run platform qualification serially on the shared
  ARM64/macOS slot. Use bounded per-test and suite deadlines and scoped owned
  cleanup so a failure cannot leave a tmux server running.

Review the design with Claude before coding and record the disposition. Review
the integrated patch with Claude Code in auto mode after validation. The phase
closes after reviewed evidence, updated specs and verified source publication.
