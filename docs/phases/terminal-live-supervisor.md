# Live Terminal writer ownership and recovery

This implements the local ownership seam for [Terminal roadmap](../build-plan.md)
using the [reviewed native supervisor](terminal-supervisor.md). The first slice
accepts only an authenticated default writer or explicit new writer. It returns
an exact Busy result without changing the existing writer. Its native tests
drive the interface in process with actual tmux. IPC, enrollment, relay event
spooling, activation and prompt behavior remain with their owning slices.
Observer and takeover use a later reviewed slice after the same writer fence
and ownership rules have passed activation tests.

## Authority and interfaces

`LiveSupervisor::start(profile: Arc<ProfileLease>, material: &IdentityMaterial,
daemon: &Path, tmux: &Path)` opens the existing profile-bound Terminal registry,
loads the restart-only local grants and reconciles tmux before accepting Start.
The shared `Arc` retains the existing flock while the terminal owner has native
resources. No second identity or lease is acquired. The Terminal owner keeps the
derived encrypted registry only. It does not own SDK, Command or Nostr signing
keys. Start borrows `IdentityMaterial` only to verify the profile and derive
the registry key. Shutdown closes admission and retires claims, wakes native
I/O, then joins I/O and the tmux control/reap owners. Owners drop the registry,
endpoints and worker handles before their last lease `Arc`, including on unwind.
Give Terminal shutdown five seconds to report completion, then hand the original
lease back with `Arc::try_unwrap` before invoking the existing 30-second Command
stop observer. The combined normal shutdown observation budget is 35 seconds.
On a timeout or failed unwrap, report pending and retain the Arc and owner
handles in the shutdown keeper. Retry observed reaping in bounded turns and run
Command stop after a successful unwrap. Do not report successful stop or release
the lease to another daemon while a worker is still owned. The existing reap
escalation applies while making progress. A process in an uninterruptible
kernel wait can keep the lease longer than the observation budget, with no
honest finite reaping guarantee. Such a pending result is failure to complete
shutdown, not permission to start another daemon against its profile.

`LiveControl::submit_start(VerifiedStart, oneshot::Sender<StartOutcome>)`
performs a bounded `try_send` into eight Start slots and returns a typed
full/stopped result locally. A separate two-request release queue and out-of-band
stop flag remain available when Start is full. The control owner services
retiring workers and local release before the next Start. Its normal command
work is capped at ten seconds per Start with the existing shorter substep caps.
A local filesystem stall can delay the control thread, but it cannot hold up
native I/O revocation or the shutdown observation deadline.
`VerifiedStart` is a crate-private token delivered by the admitted Terminal
room owner. It carries the verified canonical client account, the two-way
`Default` or `New` selection, initial dimensions, the exact source-generation
token, the Command fence `Arc`, and an operation deadline bounded by the
attachment admission deadline. Only accepted selection in the verified room
may construct this request. The token neither parses nor trusts IPC authority
claims. Keep the account grant check in the existing supervisor and validate
2–250 rows / 2–500 columns at admission and resize. Keep current effective UID
binding and the existing fail-closed root behavior.

`StartOutcome` is `Attached(AdmittedTerminal)`, `Busy { slot, writer }`, or a
closed category. Busy carries the existing exact claim within the local
authority owner for future offer support. The first slice sends only
`terminal_busy` to the client. The enrollment owner reserves one result per
selection transaction. Exact retries cannot enqueue another Start, including a
New. `AdmittedTerminal` is a non-cloneable handle with an opaque `SlotId` and
`Claim`. It exposes `submit`, `try_event` and `close`. Submission takes one
authenticated, ordered operation with its frame sequence and original absolute
deadline. Input carries up to 4 KiB and Resize carries valid bounded dimensions.
Both use the same input head because they share a wire sequence. If the native
I/O owner briefly holds the buffer mutex, `Contended` returns the unchanged
operation. The room owner can retry the same head on a bounded schedule until
the original deadline without issuing credit or a receipt. `Full` means the
previous frame still holds that head. `Closed` reports retirement.
A local I/O event identifies complete native
application of that frame, bounded native output, or retirement. The runtime
can acknowledge input only after complete application, including every
partial write. A pending receipt cannot be lost to output queue pressure.
Byte and generation fields remain memory only. `close` and `Drop` retire the
exact claim independently of channel capacity, wake I/O and never block the
Terminal runtime on tmux or process reaping.

A start response is reported after the I/O owner has installed its sole native
`Attachment`. A final enrollment or transport check failure on the recipient
closes the handle and abandons the possibly-created slot without retrying the
effect. A cancelled Start after `Prepared` follows existing adoption rules.
The local pending budget still owns its operation until the handoff or cleanup
finishes. Recheck cancellation, continuous deadline, exact source generation
and Command fence before registry preparation, every native launch and send
of the handoff, and again in the I/O owner before installing its handle. An
expired or retired start never progresses. A Suspect view before an effect
withholds that phase while the deadline remains, while Suspect during handoff
or installation retires the late candidate. Any late result after those
checks retires without returning an admitted channel. Do not hold a fence
across database or tmux calls. The bound concerns entry to a native step and
publication of an attached handle, since an already-running native call can
finish after cancellation. Never reissue uncertain native effects.

## Runtime ownership

The tmux control worker alone owns the SQLCipher connection, configuration,
tmux management subprocesses, slot reservations and each worker's claim
metadata. Preserve the existing exact marker, native ID, checked socket inode,
argv, timeout and client-count checks. Factor its existing launch helper to
return the owned `Attachment`. The live path does not register it in the
synchronous fixture's active I/O container. Leave the archived fixture
available to catch role and cleanup regressions until all live modes share the
new I/O owner. A slot reservation includes Pending, Admitted or Retiring.
Pending and Retiring count toward the global eight attachments until the
matching result or owned reap. A new claim on a retiring slot waits or gets a
bounded cleanup refusal. A scan of another slot cannot retire its workers.

One independent native I/O worker owns the masters of admitted attachments.
It multiplexes native readiness and its local wake descriptor, rotating the
first serviced attachment. Per turn it handles retirement, then the eligible
head Resize or Input and one bounded output read for each ready attachment. It
can check a claim's child status without waiting. Never run a tmux command,
join a process, poll a single inactive master to a distant deadline, or call
the SDK on this worker. The trusted allocation limit is eight: one installation
slot, at most one 4 KiB Input or one pending Resize per attachment, and
at most one published 4 KiB output slot plus one 4 KiB staging buffer per
attachment. A Resize cannot overtake an earlier Input. Record exactly one
completion and one immutable terminal reason in dedicated per-attachment
slots, not in the output queue. Notify the Terminal runtime with a coalesced
wakeup and recheck readiness after arming to avoid a missed transition.
Before dispatch, the listener passes any size and frame limits which are also
checked at the native boundary. Output uses the 4 KiB or 8 ms coalescing rule.
The native I/O scheduler applies expiry in each rotation, with one poll using
the earliest claim, ordered-head, staged-output or 10 ms health deadline.
Never block on one PTY or on a lock outside one bounded native call. The
owner checks time again before dispatch, so runnable scheduling delay cannot
authorize an expired effect. Its scheduled health wake is 10 ms or less.
A full input slot
withholds credit. Preserve the frame receiver's earlier absolute head or
signed-age deadline across the native handoff. Expiring a blocked head closes
the exact attachment. Add a bounded `Lane::begin_sink` accessor that returns
`min(pending.deadline, unresolved_since + DELIVERY_WAIT)` with its existing
offer. This preserves the current wire and queue behavior. Only confirmed
full write applies `Effect::Wrote(exact_offered_len)` or `Effect::Resized` to
that frame. A lost or late completion closes without replay. No queue offers
unbounded task count or bytes.

On normal detach, owner loss or shutdown, I/O stops the exact claim and
closes its master. It transfers the same `Attachment` with master removed to
the reaping path. The owner never creates a second `Child` handle. The reaper
uses the existing checked process-group ownership and child grace; a timed-out
child continues counting and is never dropped as proof of absence. A closed
channel, I/O panic or control worker panic stops effects through the shared
retirement state and owned resource teardown. `LiveControl` callers cannot
move raw fd, PID, PTY handle or worker child. Wake descriptors and the PTY
master use verified close-on-exec and preserve the reviewed child allowlist.

## Physical effect fence and time

The room owner must transfer an immutable exact source token for its verified
MLS leaf, epoch and subscription generation, plus its shared Command binding
fence, with Start. It retires the source immediately upon transport, generation,
room-owner or IPC ownership loss. Native I/O extends a claim only after
ordered sink commit, advancing authenticated application Ack or fresh pulse
for that exact live room and claim. Every renewal carries the exact source
proof and the accepted continuous-clock time. Reject a future accepted time
or one older than the previous accepted renewal, and never revive an expired
claim. It rejects duplicates, uncommitted admission,
unconfirmed gaps and additions for a different or retired generation. Inbound
records cannot keep a local queue alive indefinitely; native budget and input
progress checks remain independent of freshness.

The local guard records the claim, source generation, initial checked
continuous time, corresponding wall time, an 11-minute idle lease, and limits
from the enclosing phase. Lock order is local claim/source, then Command
`AttachmentFence::with_effect`, then at most one daemon-owned nonblocking
PTY syscall. Use try-lock on each native effect path. The root-owned
`with_effect` returns `Effect::Applied`, `Effect::Suspect`, `Effect::Contended`
or `Effect::Retired`. Suspect and Contended each withhold the effect.
The Command owner releases its guard
after publishing Suspect and before applying Command mutations. Command and
local retirement each acquire only their own guard and can outwait at most
one already-admitted syscall. Recheck the deadline and time inside the final
effect closure and skip the syscall if expired. On `Suspect` retain at most
the existing unresolved bounded operation. On
`Retired`, local mismatch, expiry or observing an uncertain clock, retire and
discard its pending input. A raw syscall already in progress may complete
before a retirement transition obtains its guard. No next syscall may begin
once that transition returns. Poll and wait never hold either lock. Keep
partial suffix and frame completion state with that one claim only. Check
source and lease before admitting output, and never use a newly admitted
attachment to finish old input or output.

Reuse the checked Linux `CLOCK_BOOTTIME` and Darwin `mach_continuous_time`
from `command_limits::continuous_time` via an internal visibility change, with
one continuous-clock domain for admission, receipt stamping and native expiry.
Use injectable clocks in core tests
only. Compare continuous and wall elapsed time under the approved 10-second
drift rule and close on backwards, overflow or platform clock failure.
Evaluate time during idle scheduling and again inside the physical-effect
turn. A suspend spanning the 11-minute lease expires before any effect.
Keepalive and wire freshness remain the room-owner responsibility and only
its accepted events can renew the lease. An eight-hour attachment expiry is
absolute. The room owner owns frame, wire-byte and native room budgeting.

## Recovery and local release

Persist `Prepared` before native creation. Extend the Ready ownership proof to
include the fixed tmux native session ID, tmux server PID in `1..=i32::MAX`,
boot UUID, Linux PID namespace device/inode, and the private directory and
socket device/inode. Reuse the engine's boot UUID reads and validation through
a small checked helper. Linux reads `/proc/sys/kernel/random/boot_id`, and
Darwin uses `kern.bootsessionuuid`. Bind the same-boot Linux PID probe to the
namespace identity returned by `fstat` on `/proc/self/ns/pid`. Missing or failed
boot or namespace observation confers no absence proof. Read the PID from the
same single bounded
tmux `list-sessions -F` record as the exact marker, native session ID and
name, using the same pre/post socket identity check. PID alone grants no
authority to terminate or unlink. Never deliver a signal to a PID taken from
disk. A changed verified boot UUID proves the recorded tmux process has gone,
even if unrelated processes now use that PID. On the same boot and within the
same PID namespace, `kill(pid, 0)` observes presence. Only `ESRCH` proves
absence. A live, reused, zombie, EPERM or unchecked PID remains ambiguous.

Automatic retirement additionally requires absence of the socket or a
same-device/inode socket that refuses a nonblocking connection. Revalidate
the private `profile/t` and exact slot directory with directory descriptors,
no symlink following and fresh identity checks. Treat only `ENOENT` for `s`
under the exact still-verified slot directory as ordinary path absence.
`ENOTDIR`, `EACCES`, changed/moved parents or a different live listener remain
ambiguous. A verified boot change proves the old tmux server and its live
socket listener are gone. Finish the normal checked socket and directory
cleanup if path proof still agrees. A changed filesystem device number
confers no unlink authority even with a matching inode. When path ownership
is unproved after a verified boot change, both automatic recovery and local
release can retire the exact encrypted row and preserve all filesystem entries.
An unrelated socket at that old random path is neither connected to nor
signalled by row-only retirement. A new slot uses a freshly generated ID and
exclusive directory creation. Emit a fixed diagnostic that an old private
directory was preserved. No more than 64 entries beneath the verified `t`
root are allowed before new slot creation refuses with a storage-capacity
result. Count with a bounded directory scan before persisting `Prepared`.
Retain same-UID release and attachment to verified slots at that cap.
Document local inspection of
preserved orphan directories before removal when their inactivity can be
verified independently. An absent socket under the exact verified directory
can retire without unlinking a socket. On exact writer exit give the recorded tmux
server up to one second to exit using the same independent control deadline.
A proved ongoing server and pane leaves the slot ready for reattach.

Cleanup first commits `Retiring` with the proof intact. Using the checked
directory descriptors, confirm the exact socket and then unlink only within
that directory. Sync the slot directory. Remove the same now-empty directory
from the verified `t` root and sync `t`, then delete the Retiring row in its
own transaction. If a crash interrupts after unlink, ENOENT for `s` in the
same slot directory allows cleanup to continue. If interruption is after
directory removal, a missing exact child in verified `t` under a Retiring
row permits repeating absence proof, syncing `t` and deleting the row.
Ready rows without their slot directory are ambiguous. Preserve marker,
native ID, PID and socket checks before an authorized tmux kill, and run the
same cleanup protocol afterward. An unexpected verified object or a missing
proof quarantines. A transient failure leaves cleanup pending. Recheck at
startup, each relevant Start, and local release, without making an interrupted
check a permanent decision. A row-only reboot retirement is one transaction
against the exact stored slot and proof, with no filesystem transaction to
recover. A new default can replace a row only after its registry retirement.

Validate new proof fields in the profile-bound SQLCipher registry. A Ready
row with complete proof cannot be repinned if a changed PID or socket still
answers with the old session marker. Rebuild the schema 1 table transactionally
under schema 2 with strict all-or-none proof fields and read/write bounds.
The existing binary refuses schema 2 on downgrade. An old Ready row
may fill proof only by re-verifying the exact stored marker/native ID on a
live tmux server. Legacy or interrupted rows without that proof remain
quarantined. The old schema was test-gated, so these no-proof rows were never
produced by a released daemon. Rollback recovers the last committed row,
never a partial proof.
Startup reconciles revoked grants and live Ready slots before listening. It
adopts exactly verified Prepared sessions, quarantines ambiguous Prepare or
Retire, and retries native cleanup only with current exact authority. During
operation an exact writer exit schedules an independent control-side absence
check after its master closes and owned child is reaped. An exiting tmux
client alone leaves the pane running. The final PTY handling distinguishes
readable bytes reported alongside HUP, which can enter an available local
output slot before retirement, from interrupted stream output. An interrupted
stream abandons uncommitted output per the activation plan; successful relay
commit is never implied by native read.

`LiveControl::release(slot)` is a separate bounded local supervisor request.
Only the authenticated same-UID private IPC route can create it.
It requires the exact profile row and positive absence of its worker and
tmux server. Any deletion of a socket or directory requires the same path
proof above. Release after a verified boot mismatch can retire the row alone.
It has no
room or worker dependency and can operate when all 16 rows are quarantined.
It may refuse a no-proof legacy row, even if the operator believes it is
stale. Detach and orderly shutdown never terminate a verified surviving pane.
Revoked local grants terminate exact verified owned sessions on restart and
leave quarantined rows inaccessible. No terminal data, timestamps of
keystrokes or decrypted transcripts enter persistent state or logs. Ownership
identifiers are confined to the encrypted registry and excluded from logs.

## Acceptance

- Fast offline core tests inject slot counts, exact claim/source generation,
  lease handoff/cancellation, deadline boundaries, clock regression and
  suspend, partial/WouldBlock results, Busy and full independent queues.
  Assert one physical application after recovery from Suspect and no new
  write or resize after acknowledged retirement, including contention between
  the source and shared Command fence. Expire a blocked head held in Suspect.
  Cancel Start at preparation, launch, handoff and installation by source or
  Command fence loss. Exercise a failed lease unwrap under a pending reaper.
  Assert a stopped input worker or
  activity on another claim cannot delay expiry or admission bookkeeping.
- Native opt-in tests use disposable file-backed identity, no relay, a
  trusted tmux and real worker execution. Cover default/new isolation, Busy
  no-effect, exact detach and preserved shell state on restart, external shell
  exit and default recreation, shutdown with input/resize pending, saturated
  output while control still retires, native HUP, stale or absent socket proof,
  all 16 quarantined rows and exact safe release. Inject reboot with a reused
  PID, same-boot EPERM and zombie states, a changed PID namespace, a mismatched
  socket with ESRCH, and parent ENOTDIR and EACCES. Inject a verified reboot
  with a changed filesystem device, assert automatic retirement and local
  release each preserve the exact stale directory and socket, and prove a
  new default uses a different path. Check the filesystem cap and continued
  release at capacity. Cut and reopen after each real encrypted
  retirement transition and filesystem cleanup step. Migrate an exact live
  schema 1 fixture and a dead fixture with no legacy proof. Check worker
  descriptors, ordinary store digests and durable state privacy. Native macOS and Linux ARM64
  validation remain separately reported, including actual platform clocks.
- Report the single-host scheduler delay distribution under artificial slow
  publication plus concurrent local control and native attach. Measure ready
  output to I/O-owner publication, local wrap-to-render echo, attach and warm
  recovery separately. The complete 12 ms ready-to-send target, ordinary
  Command workload and physical terminal-to-relay echo are measured by the
  integrated transport qualification. Keep native tests and metrics opt-in;
  the root-owned CI selector adds only the fast core authority/recovery tests.

Obtain independent Claude design and completed-code review before activation.
The completed-code review includes the integration seam, exact tests and a
recorded scope with files and test evidence.

## Test-gated qualification and composition gates

This writer owner is exercised in process. The production daemon must still
exercise the five-second plain lease handback, retained shutdown keeper,
and ordering with ordinary Command stop before Terminal is enabled. The local
test exercises failed lease unwrap while a reaper holds the owned attachment,
then verifies lease recovery when that reaper completes. The 12 ms readiness
target and ready-output-to-relay publication are integration measurements.
The local echo measurement includes coalescing.

The release qualification retains native macOS and Linux ARM64, actual
reboot, exact attachment descriptor inspection in the installed process,
and cross-component cancellation at admission, launch, routing handoff and
IPC transfer. Record each result against that integration. Local fixtures
exercise interrupted Prepare, cancellation at queued installation,
native reaping on unwind, pending-head expiry, and per-owner retirement.
