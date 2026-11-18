# Terminal local records and Command authority fence

This increment supplies two local, independently testable pieces of
[Terminal activation](interactive-peers-roadmap.md). Compile them with tests before live
wiring, then requalify both through real daemon and foreground entry points.
Ordinary Command IPC bytes and exits keep their current format.

## Record stream

Keep the existing bounded versioned JSON classifier for one initial request
and reply. A TerminalStart reply must say whether the remainder of the
socket is a Terminal stream with the optional field `terminal_stream`.
Serialize it only for a Terminal reply. Ordinary IPC replies still end the
write side and keep byte-exact serialized fixtures.
A Terminal stream uses an incremental binary parser with a two-byte
network-order body size checked before allocation, one version byte (1),
one opcode byte and the following exact payloads. Multibyte integers use
network byte order. Body size counts version, opcode and payload, and must
be at least two. The initial authenticated reply fixes the role; records
carry no locally supplied identity or session override.

| Direction | Opcode | Body bytes | Payload |
| --- | ---: | ---: | --- |
| CLI to daemon, writer | 1 | 3–4098 | 1–4096 input bytes |
| CLI to daemon, either role | 2 | 10 | Nonzero u64 cumulative output ID committed to display |
| CLI to daemon, writer | 3 | 6 | u16 rows 2–250 followed by u16 columns 2–500 |
| CLI to daemon, either role | 4 | 2 | Detach, empty payload |
| Daemon to CLI, either role | 5 | 11–4106 | Nonzero u64 output ID, then 1–4096 output bytes |
| Daemon to CLI, writer | 6 | 10 | Monotonic cumulative u64 input-byte credit (positive) |
| Daemon to CLI, either role | 7 | 3 | One fixed close-category discriminant |
| Daemon to CLI, writer | 8 | 2 | Resize applied at the accepted input boundary |

Close discriminants are 1 ended, 2 idle, 3 replaced, 4 authority, 5
transport and 6 budget. Clock failure and the five-second local output stall
map to transport. The local timer is clamped to the native head deadline. A stale offer is a
typed startup refusal before a stream exists. Close ends the daemon-to-CLI
direction. Bytes after Close in that direction are a protocol error. Once
the daemon sends Close, it still accepts acknowledgments for output IDs sent
and Detach. An acknowledgment may repeat the previous ID and may not
exceed the highest fully sent ID or go backwards. Detach ends the CLI-to-daemon
direction and subsequent bytes are a protocol error. On Detach abandon
unacknowledged output and send no more fresh Output. Serialize Close after
any partly written Output; if the frame cannot complete within its deadline,
retire the socket without a typed Close. After sending Close, the daemon rejects further Input
or Resize, including while awaiting the final acknowledgments. EOF at a
record boundary differs from EOF during a record. Each incomplete direction
retires on EOF and drops unfinished work.
Use a single head partial buffer with an exact consumed count so callers
can preserve a read suffix containing another frame. Preserve partial-write
ownership in the caller; it retains the remaining bytes until written or
interrupted. The codec checks direction, role, closed opcodes, integer
bounds and maximum frame size before exposing a record. Input credit starts at zero until the first credit record. Both owners
track accepted or sent cumulative input bytes and allow at most 16 KiB of
unspent credit. The writer waits for enough credit for its whole Input
record. The daemon rejects the whole Input record, poisons the stream and ends it
with transport if its length exceeds credit remaining. It counts only accepted bytes. The stream owners reject
repeating or decreasing credit and overflow in arithmetic. They also
enforce geometry effect ordering and monotonically increasing Output IDs. A failed parse permanently poisons
that decoder and cannot be recovered by scanning for a later header.

Limit the largest serialized record to 4,108 bytes. The stream owner never
has more than one unfinished record per socket. In product use, independently
apply the existing bounded queues, absolute progress deadline and EOF fences.
Socket identity, startup permits and transfer to the separate owner follow
the activation design and are integrated only when the owner exists.

## Command binding fence

A Command actor creates an `Arc<AttachmentFence>` after checking the
authenticated binding. The fence stores the frozen 32-byte local authority
digest and an internal short mutex protecting Live, Suspect or Retired.
The single Command owner assigns a new monotonic nonzero u64 generation
from its process-wide counter for each binding or re-authentication.
Allocation refuses overflow and retired generations are never reused. Handoffs and diagnostic records use the number
and never compare addresses. Only the Command owner creates and changes this
fence. The I/O owner gets read-style `state()` and
`with_effect(|| one_nonblocking_syscall()) -> Effect<T>`. An unsuccessful try-lock is Contended unless an update is requested.
The I/O owner leaves the head unchanged for Suspect or Contended and
rechecks within the 8 ms attachment tick while honoring the progress deadline. A poisoned lock is interpreted
as permanently Retired. Never hold a guard across an async operation or
a tmux subprocess. The attachment owner checks its local claim, exact origin and clock lease,
then takes the attachment guard before the innermost Command guard.

The Command owner starts a synchronous update with an owned RAII token.
It sets an update-requested atomic flag with Release before taking the short
mutex, changes Live to Suspect and drops the lock before returning.
`with_effect` checks the flag with Acquire both before trying the lock and
with the guard held. New effects cannot starve the update. The flag clears
with Release under the mutex only after resolving the pending state.
Explicit retirement sets the flag before taking the same mutex. Only the Live-to-Suspect transition creates an inspecting token. When
ingest and synchronous effect application complete, the token consumes a
verified exact digest and resumes only an equal still-Suspect generation.
An absent/mismatched result or dropped update token permanently retires
the fence. A dropped token retires even when the lock is poisoned, without
panicking during unwinding. An already retired fence cannot resume. Owner shutdown explicitly
retires its set; an owner guard will cover abnormal termination at wiring.
The stop-to-return bound is one in-progress nonblocking effect before the
state mutation. No retired effect is issued by the short guard.

## Validation

Test exact codec bytes, all split points and concatenated framed reads,
minimum/maximum fields, strict direction and role matrix, overlength headers,
truncation versus boundary EOF, post-Close and poison behavior. Keep only
deterministic cases in GitHub Actions. Test the fence's equal and unequal
digests, early-drop, poison, retiring while Suspect and requested-update priority under
continuous effect attempts. Test cross-thread effect
ordering using explicit synchronization. Measure the codec and guard cost
separately from end-to-end IPC/relay timing. Verify no unrelated production
behavior or dependency change. Compile these private modules only in tests
until their production owners exist. Before activation, add real socket routing,
registration/admission and PTY side-effect tests under the next integrated
slice. Have Claude Code independently review this design and the completed
increment, with tests allowed in code review.
