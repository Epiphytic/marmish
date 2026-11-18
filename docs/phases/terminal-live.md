# Terminal daemon and CLI activation

Connect the reviewed [room owner](terminal-room-enrollment.md),
[framed delivery](terminal-frames.md), [tmux supervisor](terminal-supervisor.md),
and [local frontend](terminal-frontend-tty.md). Both reviewed slices gate
feature completion. The
[interactive roadmap](interactive-peers-roadmap.md) describes the full behavior.

1. Default writer, detach and reattach, explicit new slots and a fixed typed
   `terminal_busy` refusal. Qualify real fresh-room enrollment, default/new
   isolation, persistence, IPC ownership, authority and ordinary Command
   regressions before enabling a writer path.
2. Observer, takeover and interactive busy selection. Qualify exact offers,
   forced retirement, observer no-effects, each explicit mode, prompt expiry,
   concurrent scheduling and restored old frontend before enabling these paths.

## Authority and transport

Use `command_cli::Context::prepare` to prepare a non-fresh ordinary Command
room for the exact profile, canonical peer and resolved relay policy without
executing a command. The sole Command actor forms an
`AuthenticatedCommandContext` from a ready local journal row by checking
current membership, active local authority, accepted role, protected purpose
and exact transport. The IPC caller supplies the resolved destination and
relay policy, never a native digest or authority object.

Maintain one shared, generation-scoped fence per eligible binding. An apply
turn stages an affected Live fence in Suspect before synchronous
`ingest_delivery`. Use the transport group ID to find the binding, stage all
bindings for an unknown envelope or Welcome, and stage all before a runtime
drain. Suspect spans only synchronous mutation and effect application. It
keeps queued bytes and temporarily withholds physical writes and resize.
After effect application, recheck `command_context` and resume the same
generation only if its frozen authority digest still agrees. A mismatch,
unknown result, error, shutdown or panic permanently retires the affected
generation. The physical writer holds the fence guard across one
nonblocking syscall only. Revocation latency includes relay delivery and one
apply turn. The separate Terminal owner receives an immutable binding and
shares only an `Arc<AttachmentFence>` cloned from the Command owner. Filter
unrelated traffic before native placement using existing delivery admission.
Measure incidental pauses during repeated ordinary unknown or unrouted
events and normal Welcome traffic alongside a healthy live terminal.

Register at most 32 binding/relay listener entries. Reconcile on Command
readiness and retirement. Assign in stable canonical peer order round-robin,
newest nonexpired binding first per peer and then additional bindings in later
passes. Report sanitized capacity failures. The dialer uses the first
canonical-order intersection of the profile `RelayPolicy` and verified
binding endpoints. Give each attempt an exact single-URL policy. Connection
failure or timeout before a verified Challenge may advance to the next
eligible origin. Cancel the earlier attempt before switching. A Probe has no
tmux or room effect before its matching Confirm. Ordinary local Ready may
precede peer readiness. Schedule up to four Probe attempts total at
monotonic offsets 0, 6, 12 and 18 seconds. Each send-and-Challenge attempt
has a five-second deadline. Cancel and reap it before the next attempt,
skipping an offset when earlier cleanup still owns the source. On timeout
or send failure try the next locally ranked eligible origin until those
are exhausted, then retry the last origin with the immutable Probe.
The server cannot move ownership to another origin without proof of local
source loss and reaping before Confirm. A valid Challenge ends Probe
retries. The total bootstrap deadline remains 30 seconds after the
separate ordinary 15-second preparation deadline.

Bind all room records to the exact endpoint and REQ generation after
Challenge. Generation loss closes the attachment. Any new connection starts
a new native room with fresh keys. Egress preserves the profile mode. Return
`terminal_transport_unavailable` if the scoped connector for that mode is
unqualified, including Tor or onion under the direct-only first scope.

Install a separate attachment fence after exact ClientHello and before
retiring the 30-second enrollment Authority. It refers to the same Command
binding generation and digest, exact current MLS leaf/epoch, native
subscription generation and server-issued claim. Verify both fences during
handoff. Source loss, permanent authority retirement and owner loss retire
the claim. Clock or fence uncertainty prevents another physical write. Use
suspend-aware clocks, `CLOCK_BOOTTIME` and `mach_continuous_time`, with
checked conversion to the same monotonic abstraction. Each claim has an
independent five-minute keepalive and eleven-minute idle lease. Only
accepted fresh records for that claim extend its lease. Treat a clock
failure as retirement before the next effect. Retire on the confidentiality
plan's 10-second wall-to-monotonic drift bound before the next effect.

## Local IPC and session lifecycle

`marmish terminal DESTINATION` uses the existing exact target resolver.
Allow `--session ID` with a closed default, new, observe and takeover role.
Before starting background runtime work, confirm the local controlling
terminal and establish foreground signal ownership. Remain in cooked mode
through preparation and selection. Print bounded locally formatted
identifiers and fixed categories only.

Use the current private profile socket and `ipc::verify_peer`'s Linux
SO_PEERCRED or macOS getpeereid check in both directions. Parse one initial
frame within `IO_TIMEOUT` and a short-lived classification permit. Route
TerminalStart directly in `ipc::serve` before enqueueing any ordinary actor
request. Make `Request::is_command` and `uses_short_exchange` exhaustive.
Drop the short classification permit when the independent TerminalStart
pending permit has been acquired. Transfer the accepted stream to its
Terminal runtime with `into_std`, CLOEXEC and reregistration. That runtime
owns its EOF watcher, all expiry/error drops and orderly shutdown. Neither
the PTY master nor the accepted IPC socket is inherited by tmux/worker
children. Keep the pending socket limit at four; hand the stream to a separately
bounded admitted attachment owner and release its pending permit, or close
it and release the pending permit when Start ends otherwise. Cap cold total
client attach and server IPC Start at 60 s, with shorter operation-specific
caps intact.
After admission one socket owns one attachment and uses bounded
length-prefixed typed records, including an
input-credit record and exact output commit, on that socket only. Older
daemons may return the existing invalid-request reply. Once the Start frame
has been written, its result is final for that call. EOF fences the matching
writer immediately and schedules best-effort authenticated Close. Command
text, framing, placement and retention continue through existing paths.

Use bounded selection records in the reserved Terminal receipt kind.
Bind each to transcript, authenticated role, current MLS leaf, freshness
and its transaction ID. Reserve one selection/confirmation transaction per
room. Cache the immutable result until retirement, so replay cannot
duplicate new tmux creation or takeover. A busy offer identifies the
eligible own slot and exact current writer claim. It carries an opaque
server-random 128-bit nonce valid for at most 90 seconds from issuance.
Reuse the immutable cached outcome for exact transaction retries in one
room. A nonce can authorize at most one choice after that initial room
closes. Scope it to the canonical server and client, frozen Command
binding, own slot, old writer claim ID and expiry. Echo all those values
inside the later authenticated selection and compare them with the new
verified binding. Offer observe, takeover, new or cancel in cooked mode.
The Offer is the final result of the first Start. Bound user input to the
first 25 seconds of the offer and issue a separate Start with its own
60-second cap. Validate expiry and the current exact claim at the second
room's selection immediately before effect. A changed, stopped or missing
writer returns `terminal_offer_stale`, and an expired offer requires a
fresh prompt. After an
acknowledgement failure, report uncertain role change and discard possibly
accepted input without reissuing it.

| Choice | Fresh enrollment windows | Admission |
| --- | ---: | --- |
| Default, slot free | One writer | Initial room continues into the attachment. |
| Explicit new, observe, or takeover | One requested role | Writer scope for new and takeover; observer scope for observe. Busy takeover or observe uses an exact server-verified claim within the transaction. |
| Explicit observe or takeover with no current writer | One requested role | Return `terminal_writer_missing`. |
| Default Busy then observe, takeover, or new | Writer, then a new room in the requested role | Initial room ends after the Offer. The second selection consumes its exact option-bound nonce and repeats the live claim check. New allocates only a separate slot. |
| Default Busy then cancel or timeout | One writer | Initial room retires; no new effects. |

The offer room counts toward the cap until reaped. The second leg can
return the typed `terminal_room_capacity` result. The saved nonce can be
offered again only within its original window after a known no-effect
refusal, or the operator starts a fresh default selection.

Create one default persistent tmux slot per server profile, canonical
client and execution user. Reattach creates a fresh native room for that
slot. A new slot leaves the default intact. Give each read-only observer a
fresh room and worker tied to the same exact active writer claim. Its
closed role admits only output, acknowledgement and lifecycle control.
A takeover fences the old writer and its observers before the successor
can write. The old CLI restores its TTY and reports `terminal_replaced`.
Writer lease expiry, Close and shell exit all retire that writer and every
observer tied to it. This supersedes the observer-survival sentence in the
[confidentiality plan](terminal-attachment-confidentiality.md). Reattach
always acquires a fresh claim. Regular disconnect, detach, or daemon shutdown leaves a proved
owned tmux pane running. Orphan workers are reaped through owned handles
on orderly shutdown and checked against exact persisted tmux ownership
proof on startup.

Configuration changes take effect on daemon restart. Reconcile removed
grants before serving. Retire a Ready row proven absent after pane exit,
tmux server exit or reboot, so the next attach can recreate the default.
Adopt a positively verified live slot. Report ambiguous slots as
quarantined. `marmishd terminal release SLOT_ID` is a same-UID private
daemon request under its exclusive profile lease. It verifies the exact
slot ownership/binding and absence of the slot's active worker, tmux server
and live socket listener before retiring the registry row transactionally.
A verified boot change proves the old listener is gone. If stale filesystem
entries cannot be attributed to that slot, row-only retirement preserves those
entries. This control path needs no free room/worker slot and works when all
16 rows are quarantined.
Require positive ownership proof to unlink a path or signal a process.
Grant removal terminates proved owned sessions at the next daemon restart.
Authenticated Command authority checks remain live while it is running.

## Output and scheduling

Separate the tmux control owner from the nonblocking I/O owner and from
the Terminal SDK runtime. A bounded channel moves an admitted PTY to the
I/O owner. Retire its master there and do blocking reap off-thread.
Do not share Terminal SDK task/runtime capacity with Command. Cap admitted
attachments globally at eight, including observers. Use rotating turns,
independent control capacity and a single unresolved publication per
room. Serve Close, resize, Ack/pulse and data in that priority. Apply
independent caps of 16 live or retiring native rooms, eight admitted
attachments, and the supervisor's four tracked workers per slot. Refuse
room exhaustion with `terminal_room_capacity`. Release a retiring room
only after local close, exact generation retirement and owned task/transport
reap. Timeouts still count until reaped. Limit attachments to eight hours,
65,536 frames and 512 MiB per direction. Limit rooms to 24 hours and
65,536 send/ingest attempts. The first exhausted resource closes the
attachment with `terminal_budget`; fresh user attach starts a new exchange.

Release the first idle key immediately. Coalesce server output to a 4 KiB
maximum or an 8 ms first-byte deadline. Measure the 12 ms local
ready-to-send target during saturated ordinary traffic and concurrent
attach separately from physical key-to-echo, which includes relay RTT.
Clamp all queues and close on sustained overflow. Retain adaptive tuning
and further screen-diff evaluation for results-driven follow-up work.

Use complete independently renderable units in every native output record.
The producer holds a bounded incomplete UTF-8 or control unit until it can
emit or discard it and packs completed units into records. On the frontend
the exact record must be fully consumed, and all its display units physically
written or deliberately discarded, before output commits. A five-second
local output head stall is clamped to the native delivery deadline. It can
close the attachment while the TTY is blocked. Exact authenticated orderly
Close can finish existing commits. EOF without Close interrupts and
abandons uncommitted output. Tmux redraws from the surviving pane on a fresh
attachment.

External SIGTSTP restores the TTY and closes the attachment before stopping.
On continue, repeat the foreground and admission checks and get fresh
native state. Raw Ctrl-C and Ctrl-Z are remote input bytes. Operating
system termination restores termios and preserves its reason.

## Tests and measurements

Use two actual CLI/daemon pairs with disposable identities and private
directories, a TLS relay, trusted tmux and an isolated controlling PTY.
Qualify first connection without prior commands, exact input, Unicode,
resize, bracketed paste, control keys, suspend, detach, restore, warm
reattach, new slots, writer end then reattach, and the busy/observer/takeover
paths. Check the old writer and observers lose their old effects, verify
different native room/attachment IDs, and run actual Command and Stream
regressions on the same relay. Check that TerminalStart never reaches the
Command queue and that the IPC socket and PTY master are absent from
tmux/worker child descriptors. Exercise stale/closed IPC, shutdown and
old-daemon behavior, timeout/relay policy, pre-peer Probe, generation loss,
unknown/matching Command mutation, drain and actor exit. Assert private
terminal contents never appear in persisted files and logs.

Cover expired offers, observer-only commit, clock advancement, stale
replay, input-credit and output-commit lifetime, retirement races, stalled
TTY, byte caps, slow output versus fast control, capacity over 16 eligible
rooms, eight admitted attachments, all 16 quarantined slots, local release,
reboot recovery and grant revocation on restart. Check observer retirement
independently for writer idle expiry, authenticated Close and shell exit.
Exercise each exhausted budget using injected boundary counters and clocks.
Test native scheduler
queue limits with concurrent attach/takeover and socket loss. Qualify
Linux x86_64 and report the actual coverage on other self-hosted targets.

Report cold and warm attach, key-to-echo p50/p95/p99, batching delay,
render and wire events/bytes, sustained scrolling throughput, reattach,
takeover, concurrent observers and detach restoration. Include samples,
hardware and relay locality. Attach, reattach, takeover and bandwidth
metrics are report-only apart from operation timeouts and the stated
local scheduler target. Keep network, native PTY, installer and
benchmarks opt-in. Run the fast closed parser, state and authority tests
on trusted self-hosted GitHub Actions. Obtain separate actual Claude auto
review of the integrated slices and record test/review evidence before
source publication.

Use an injected continuous clock to check the Probe attempt cap, failed-origin
cleanup, sequential ownership, and the original enrollment deadline.
