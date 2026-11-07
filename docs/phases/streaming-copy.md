# Phase 3: resumable regular-file copy

Claude-reviewed design (round 8 APPROVE); implementation is not authorized by
this document alone. Parent:
[streaming plan](streaming-transfer-plan.md), Beads `marmish-8ka.4` (parent `marmish-8ka`).
This phase follows the approved native streaming runtime and rsync adapter.
Design review evidence belongs in [the review record](../reviews/streaming-copy-design.md).

## Behavior and scope

```
marmish put REMOTE LOCAL REMOTE_PATH
marmish get REMOTE REMOTE_PATH LOCAL
```

REMOTE uses the existing npub/alias, relay, SRV and Tor resolution. Local identity
selection remains `-i`. Paths are literal OS bytes, never shell expressions.
Transfer one regular file; no recursive trees, symlinks, devices, sockets, FIFOs,
xattrs, ACLs, ownership changes or sparse-file preservation. Existing destinations
are refused unless `--overwrite` is explicit. `--resume TRANSFER_ID` resumes a
checkpoint across a new stream; it never resumes uncertain stdin delivery.
`--compress` uses existing bounded independent stream compression frames.

A random 256-bit transfer ID differs from the stable stream ID printed for
list/cancel. Print both IDs and progress to stderr. `stream-list` and
`cancel --stream ID` retain their existing meaning. A cancellation retains a
validated durable checkpoint by default. Resume mode requires repeating the same LOCAL and REMOTE_PATH operands; both
ends compare them with the retained manifest and do not adopt a different source.
A lost attachment leaves its helper
running, like every other stream. A helper that never receives Start holds stream
quota but opens no file or checkpoint lock; it stays cancellable administratively. No timeout silently becomes cancellation.

## Grounded integration boundaries

`stream_cli::execute` currently binds fd 0/1/2 and private Peer/pipeline machinery.
Extract a reusable bounded transport session; preserve its stdio adapter and
rsync behavior. Do not redirect process-global stdio to implement copies.

Proposed internal interfaces (names may change without weakening contracts):

```
StreamSession::open(context, target, StreamLaunch, ExecuteOptions) -> StreamSession
StreamSession::exchange(input, eof, stdout_ack, stderr_ack) -> StreamSnapshot
StreamSession::control() -> ControlHandle  // independent status/cancel lane
CopyClient::put(session, source: File, receiver_path: OsString, options)
CopyClient::get(session, source_path: OsString, receiver: PinnedDestination, options)
CopyHelper::run(context_fd: OwnedFd, stdin: File, stdout: File) -> ExitStatus
PinnedDestination::open(path, base_directory_fd) -> PinnedDestination
CopyCheckpoint::recover(destination, TransferBinding) -> RecoveryState
```

The session keeps one exchange in flight, finite control/data buffers, current
ACK/retry rules, keepalive, and independent explicit cancellation. Copy encoding
is a length-prefixed closed schema, capped at 16 KiB control records and existing
8 KiB raw data blocks. stderr is diagnostic only. Unknown versions/fields,
oversized frames, incorrect offsets, trailing data and mismatched hashes fail.
No whole-file buffering or unbounded task/channel queues are permitted.

## Server authority and native helper launch

Add an explicit typed stream launch variant for native copy; it contains direction,
paths/options and transfer ID, never an executable path or helper argv. Keep
ordinary Open/argv semantics intact and explicitly reject unsupported variants
when peers lack copy capability. Native copy capability is negotiated before
launch. Durable Open digests bind the complete launch choice and options.

The server resolves its installed native `marmish` image through `native_layout`,
not PATH or a remotely provided helper path. It validates the image/entrypoint
using existing installed-image checks and constructs a fixed hidden copy-helper
invocation. This is still an ordinary executable authorization decision:
`Engine::authorize`, canonical executable resolution and
`check_execute_request` must all succeed for this exact native image. Defaults
and every matching per-key restriction remain additive. Do not recognize magic
argv on arbitrary executables, substitute the helper after a different executable
was authorized, or run copy operations in the MDK actor.
The policy request is `ResolvedExec { executable: canonical_native_image,
invoked_path: canonical_native_image, argv: server_built_helper_argv }`, with no
remote-supplied argv; its exact descriptor-locator suffix is specified below. The same resolved request
is used at preparation and spawn. Existing policy `executable_names` matches the
executable and alias, not the remaining argv; there is no new copy-only gate.
Operator-facing setup/documentation must explicitly warn before recommending an
allow rule: allowing the native image also allows its other subcommands through
ordinary Exec, including administrative/configuration operations with the server
UID's HOME and credentials. Copy is appropriate only for principals trusted with
that executable's full same-UID authority. Versioned installation image paths
change on upgrade; explicit allow rules must be reviewed for the new canonical
image path rather than silently broadened. Do not market this as copy-only access.

The engine preparation extension is a distinct server-only method:
`prepare_native_copy(ctx, registered_helper_image, CopyLaunchContext)` returns the
same opaque PreparedLiveExec, retaining existing preparation-channel limits and
launch/revocation fences. RegisteredCopyHelper is an opaque native-side value constructed only by
native_layout from its canonical validated installed helper image, version and
expected native entrypoint. Engine preparation revalidates its canonical executable
and normal policy; the registration is not a remote capability or an argv bypass.
Its constructor is not part of IPC/wire. The native application supplies this image
from its own installation; no wire field selects it. Generic live execution
cannot attach this context or request inheritance of arbitrary descriptors.

The hidden helper dispatch runs before layout lookup, profile/keychain opening,
lease acquisition or daemon IPC. Add a no-profile/no-layout early dispatch test.
The helper obtains server-supplied consistency context exclusively from a private inherited read-only FD,
not environment variables, argv identities or the ordinary data protocol. The
server produces a <=4 KiB versioned context containing authenticated client and
server accounts, MLS group/session, stream/transfer IDs, direction, a SHA-256
OpenCopy digest including all typed launch paths/options and codec, and helper
protocol version. This is the same tagged digest stored in the journal, not a
second paths-only digest.
The first bounded copy control record supplies those paths/options; the helper
checks their closed-schema canonical digest against the server-supplied context before
using them. Full path strings need not fit inside the private context pipe. The child rejects a locator <=3, fstats that selected FD as a FIFO, checks its
access mode is read-only, reads at most 4096 bytes plus one overflow-detection
byte to EOF (the server already closed its writer), closes that FD, and validates
the full bounded context record before any profile or transfer filesystem access. It also requires
stdin/stdout to be anonymous pipe/FIFO descriptors, not terminals or files.
This context is an anti-accident/anti-remote-argv gate, not authentication: any
same-UID process or permitted shell can forge a matching pipe. Actual network
identity authority is the server's existing authenticated admission and ordinary
executable authorization, with no per-key path or direction policy. No identity secret, SQLCipher key, profile
lease, journal descriptor or daemon IPC socket is inherited.

Current Linux pre_exec marks every FD >=3 CLOEXEC with close_range; Darwin marks
them individually. Avoid any fixed child FD or dup2 over a potentially std-owned
exec-error descriptor. Construct/prewrite the private context pipe, close its
writer, and reserve its read end with fcntl(F_DUPFD_CLOEXEC, 4). Retain that OwnedFd
through spawn. The dynamically allocated source is strictly above 3, below Darwin
FD_CEILING and cannot be allocated to Rust's exec-error pipe while owned. After
all parent descriptor allocation, existing prepare_spawn validates the descriptor
ceiling. The syscall-only pre_exec first applies the ordinary blanket CLOEXEC
mark, then clears CLOEXEC on exactly this owned integer FD. It performs no dup2
and cannot clobber stdio or the exec-error pipe. Parent CLOEXEC remains set.

The hidden argv is `[canonical_native_image, "__copy-helper-v1", "--context-fd",
ASCII_decimal_fd]`, entirely constructed by engine preparation. The integer is a
locator, not an authority claim; no identities or paths are carried in argv. The
exact ResolvedExec request above includes these final two arguments, and the
same final request reaches spawn. The helper validates the integer >3 and the
pipe before consuming context, then closes it immediately. The existing
zero-argument pre_exec/install_exec_image callback becomes a closure capturing
only that Copy integer, with no fork-time allocation. PreparedLiveExec owns the
optional OwnedCopyContext; ChildSpec borrows only its raw locator for spawn.
Generic live preparation cannot request one. Parent FD ownership lasts until
spawn returns, on success or failure. Test multithreaded spawn, initially closed fd0/1/2, a pipe originally
allocated as FD3 that is relocated above it, no descriptor leaks, and bad exec
image/pre_exec failures still reported as startup failure with the context FD
installed on both platforms. Never replace another process-global descriptor to
reserve FD3.

Calling the hidden helper through an ordinary remote Exec has no private context
and fails before filesystem access. Authorizing the native marmish image also permits its other ordinary argv;
executable-path policy alone cannot express copy-only access. A permitted executable
can itself execute programs and a same-UID local process can forge descriptors: this is not a
per-principal filesystem sandbox. Checkpoint identity binding protects protocol
consistency and accidental reuse; it does not promise isolation from other code
already running as the same UID.

## File protocol and identity binding

Both sides first validate a fixed manifest: protocol version, transfer ID,
authenticated client/server, direction, source/destination byte paths, source
size, full SHA-256, requested metadata and overwrite policy. Authenticated account
claims must equal server-provided context and the client's actual stream binding.
A transfer can move between fresh rooms/streams for these same accounts; stream
and MLS session IDs are diagnostic attempt fields, not permanent resume identity.
Direction, both paths, source size/hash, metadata, Create/Resume-independent immutable manifest and overwrite policy cannot
change under an existing transfer ID. Paths are bounded to 4096 bytes, with no NUL.

The sender opens the source no-follow/nonblocking, fstats it as a regular file,
with O_NOCTTY; first fstatat(AT_SYMLINK_NOFOLLOW) must identify a regular
file, then open and compare the opened dev/inode/type to that observation.
This prevents known special-file opens and detects substitution, although a
privileged adversary able to replace an entry with a device between these calls
is outside the same-UID security boundary. O_NONBLOCK has no effect on regular
file I/O. Hash through that pinned FD in bounded blocks. Record dev/inode, size,
mtime and ctime before/after hashing and transfer; changes fail SourceChanged (including harmless ctime-only changes). Across attempts, source
inode/mtime/ctime are not portable identity: the immutable full size/hash binds
resume content. The final bytes must match the original full hash. Concurrent writes are not a
snapshot guarantee; advise stable input files. Put reads the complete source at
least twice (initial hash and sending); final receiver verification adds another
full read, and resume adds bounded-memory prefix reads on both ends. Byte paths and offset lengths use
checked arithmetic, file size/offset <= i64::MAX, and no remote value allocates a
file-sized buffer. Hashing and filesystem sync run off the async actor.

Receiver offers only its durable checkpoint offset and prefix SHA-256. Sender
rehashes the same prefix, compares it, then continues at exactly that offset.
The receiver independently rehashes its saved prefix after reopen, validates its
inode/size and rejects mismatches; it never trusts stored hash state alone.
A resume therefore costs O(prefix length) reads with O(block size) memory.

## Descriptor-relative filesystem operations

Pin an existing parent directory with openat component walking from `/` or a
pinned base directory for relative paths. Every walked component uses
O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC. Open `/` directly; pin the client's current
working directory using open(".", O_DIRECTORY|O_CLOEXEC). The server opens its
trusted configured account.cwd once as O_DIRECTORY|O_CLOEXEC, allowing symlinks
in that trusted base only; untrusted appended components remain no-follow. The remote base is account.cwd from
the authorized child specification; the client base is its initial cwd. Neither
side expands `~` or consults a shell. Reject `..`, empty final names, `.`,
NUL, parent symlinks and non-directory components. Never canonicalize an untrusted
whole path then reopen it. Parent creation is out of scope. Directory descriptors
remain pinned throughout the attempt; operations refer to that directory object
if its pathname is renamed concurrently. Do not imply pathname confinement. Linux symlinked layouts such as `/home` to
`/var/home`, usrmerge components or user symlink paths fail UnsafePath; use the
real path or a relative operand below a trusted pinned base. A symlinked source
operand also fails. CLI help documents that preserve-mode requires owner
read/write and therefore rejects read-only source modes when requested.

For macOS only, normalize the exact leading public aliases `/tmp`, `/var` and
`/etc` to `/private/tmp`, `/private/var` and `/private/etc` before walking; the
replacement targets are then walked no-follow. The endpoint owning each filesystem performs this exact normalization. Bind
original raw byte paths in the cross-host manifest; checkpoint both raw paths
and the local normalized execution path plus parent identity. Resume requires
the same raw bytes even when two spellings name one inode. Display alias mappings
in diagnostics. Do not follow arbitrary
symlinks or use canonicalize to accommodate them. All other platforms reject
symlink components. This explicitly handles normal macOS paths without weakening
arbitrary-component no-follow behavior.

Create a private `.marmish-copy-v1-NAMESPACE` directory, where NAMESPACE is
lowercase hex SHA-256 of ASCII `marmish-copy-state/1\0` followed by actual client,
server, transfer IDs and u8 direction. Accounts come from authenticated server
admission / actual client binding, never an unverified frame. This namespaces
state by principals as well as transfer ID: a different authorized account using
the same public ID cannot select or learn Busy details for another namespace.
Create that directory at the pinned receiver
parent with mkdirat(0700). Create mode fails on EEXIST; Resume requires an
existing valid checkpoint and fails NotFound without one, never creating state.
The public random transfer ID is a lookup key, not a secret or capability.
Inside it create `data` and checkpoint staging files with O_CREAT|O_EXCL|
O_NOFOLLOW|O_CLOEXEC, mode 0600. Validate the private directory's owner/mode and
regular files' owner, single link, identity and permissions. For destination inspection use fstatat(AT_SYMLINK_NOFOLLOW), not open of an
unvalidated entry; only a known regular destination may be opened with
O_NONBLOCK|O_NOCTTY|O_NOFOLLOW and then fstat identity-checked. Reject special types
without waiting on FIFOs. Existing hard-linked regular destination files may be
replaced as directory entries with overwrite; their other links are untouched.

Persisted dev/inode comparisons are strict: all recorded parent, private-directory
and data identities must match on recovery. st_dev is not assumed stable across
remount/reboot; a change returns IdentityChanged and never adopts or truncates
the tree, even if a hash appears equal. This conservative policy can strand a
partial after a harmless remount. Such uncertain trees are retained for explicit
same-UID administrative inspection/cleanup, not automatic retention deletion.
The user can start a fresh transfer; there is no automatic identity repair.

A checkpoint records receiver parent dev/inode, temporary directory/data
identities and destination basename in addition to the manifest. Resume through
a path now naming a different directory fails; it never adopts unrelated files.
Keep a per-transfer flock for the helper's entire lifetime. A held lock returns
Busy with no prior stream ID; helper lock metadata may be stale. Known prior
bindings are reported only by the authenticated daemon attempt index in
CopyRefused. Do not steal based on PID or elapsed time.
For the same destination under a different transfer ID, commit policy decides
which attempt succeeds; no multi-transfer serialization guarantee is implied.

## Durable checkpoints, commit and recovery

Default checkpoint every 1 MiB or 5 seconds of newly written data. Sync data
before publishing a checkpoint; write bounded metadata to a new private file,
sync it, rename over the prior checkpoint and sync its directory. Only then send
a durable checkpoint acknowledgement. The durable offset is monotonic, independent
of transport acceptance/delivery ACKs. After crash, fail HashMismatch if the saved offset exceeds actual file length;
never extend it with zeros. Truncate any uncheckpointed suffix only after
validating the recorded data inode; do not advertise it.

After exactly the declared length, rehash the complete temporary file, compare
size/hash, apply final safe metadata and sync it. Mode defaults to 0600; an
explicit preserve-mode option copies only 0777 permission bits and strips setuid,
setgid and sticky bits; preserved modes must include owner read/write for portable
checkpoint recovery. Preserve mtime only on explicit request, using bounded
seconds/nanoseconds and futimens on the open FD. No chown or path-based metadata
writes. Record/sync a CommitPrepared checkpoint before modifying the final entry.

Without overwrite, use descriptor-relative renameat2(RENAME_NOREPLACE) on Linux
and renameatx_np(RENAME_EXCL) on macOS; these primitives already have engine
platform precedents, but the private-directory-to-parent two-directory rename
wrappers and their error mapping are new code that require dedicated tests. EEXIST is Conflict and leaves the old entry intact. Required
primitive unsupported by a filesystem is Unsupported, not a check-then-rename
fallback. No linkat fallback is introduced in this phase; data keeps nlink=1 and
successful commit atomically removes its private name. Sync the parent and
private directory after moving the entry.

With overwrite, reject an observed non-regular final entry and use renameat to
replace the destination directory entry atomically. A symlink installed between
check and rename is replaced as an entry; its referent is never followed. The
explicit overwrite contract replaces the entry current at the commit operation,
not an inode conditionally selected earlier. There is no portable compare-and-swap
rename guarantee. Directories produce a safe failure. Sync both parent and private directory after
rename. Overwrite replaces the old file's inode, mode, ownership and ACLs with
the staged regular file; it does not preserve old destination metadata.

Save/sync Committed and return success only after final directory durability.
A crash/lost ACK after commit must not restart writing or replace a later file:
CommitPrepared recovery checks the final entry's recorded data identity, size
and full hash. If the private data name is absent and the final identity/hash match, this
confirms the completed commit: sync both directories and record Committed.
If private data still has its recorded identity/nlink=1, the rename did not
commit: resume may continue the originally authorized commit, rechecking final
entry type and using the original overwrite/no-overwrite primitive. A no-overwrite EEXIST
leaves CommitPrepared and the staged data intact. Resume cannot change overwrite:
remove the conflict explicitly or start a fresh transfer with new options. A missing
private name with absent/different final entry is CommitUncertain; never write
or overwrite again in that case. A Committed receipt returns prior success
only after verifying that same final identity and hash still exist. Changed final
contents fail rather than restoring the prior file. In-progress checkpoint
states use the original private data identity; overwrite recovery permits that
name to be absent only in CommitPrepared with a matching installed final inode.

Keep committed receipts and inactive partials for 7 days by default plus replay clock skew (currently 7 days + 60 seconds); expiry is
configurable with this inclusive lower bound on retained receipts.
Cleanup touches only validated owned private transfer trees with the lock held;
a live locked helper is never expired. Uncertain/mismatched trees are retained
for explicit local administration. No directory-wide recursive deletion by
untrusted basename. Bound stored metadata to 16 KiB (encode the manifest once; do not duplicate path arrays) and cap retained private
transfer directories at 1024 per parent before new admission, serialized by a
no-follow, same-UID 0600 parent admission lock named `.marmish-copy-admission.lock`
held only during bounded count/create. Open/create with O_CREAT|O_EXCL|O_NOFOLLOW|
O_CLOEXEC, falling back on EEXIST to an O_NOFOLLOW|O_NONBLOCK|O_NOCTTY open and regular-file,
owner/mode/nlink validation. Never truncate, unlink or replace an existing lock.
A user-created collision that fails validation is UnsafePath, not adopted state.
Every Create/Resume obtains this nonblocking admission lock before inspecting or
creating transfer state; contention is Busy, not NotFound. Create holds it until
its private lock and initial Created checkpoint are durable. Therefore another
attempt cannot interpret the mkdir-to-lock creation interval as missing state;
a crashed incomplete owned tree returns Busy with no prior stream
for explicit administrative inspection, never automatic adoption or deletion. Limit directory scans to 100000
entries and fail Quota rather than walk an unbounded directory. Disk space failures
leave the last durable checkpoint truthful. Filesystems that cannot provide the
required sync/atomic primitives fail explicitly, without claiming durable resume.

## Concrete stream extension and session ownership

Do not add optional fields to existing closed Hello/Open schemas. Add closed
RequestBody::CopyHello { versions: Vec<u32>, codecs: Vec<Codec> } and matching
ResponseBody::CopyHello { version: u32, codecs, frame_bytes, window_bytes }.
The versions vector has <=4 distinct entries and must include copy version 1;
the codecs vector reuses existing codecs() validation (1..2 distinct entries). New servers return the
existing Unsupported error for an offered set they cannot support. Old peers
cannot decode the new operation and may drop it; copy clients use a dedicated
copy capability probe deadline (default 15 seconds, configurable up to the
transport idle timeout) and report CopyUnsupportedOrUnavailable, without falling back to a
remote argv trick. These new operations remain in stream/1 format 1 because
old closed decoders reject them safely; there is no compatibility fallback.
Ordinary stream/rsync Hello and Open are unchanged.

Add RequestBody::OpenCopy { launch: CopyLaunch, codec }. CopyLaunch has copy
version, Create|Resume attempt mode, transfer ID, client/server accounts,
direction Put|Get, raw source/destination byte paths, overwrite, preserve-mode,
preserve-mtime. Add these closed variants to wire validation/encode/decode and
runtime dispatch; the existing Opened response can carry its new digest. The
server computes—not trusts—the digest from the authenticated validated OpenCopy.
JSON uses existing lowercase fixed hex encodings for IDs/accounts/digests and
canonical padded base64 for byte paths; require 32-byte transfer/account IDs,
16-byte Binding IDs, decoded paths <=4096 each, <=4 versions and <=2 codecs, and total
canonical wire bytes <=MAX_CONTENT. No JSON raw-byte integer arrays are accepted.

Canonical launch bytes are the fixed ASCII domain `marmish-copy-launch/1\0`,
followed in declared field order by u32 big-endian version, u8 mode, 32-byte
transfer/client/server IDs, u8 direction, u32-length-prefixed raw paths, and three
u8 boolean flags. Mode is part of each attempt's launch digest but excluded from
immutable resume-manifest comparison. Digest is SHA-256 of these bytes plus
u8 codec. No JSON object ordering or client-supplied digest determines authority.
Exactly `SHA256(ASCII "marmish-copy-launch/1\0" || binary Launch || u8 codec)`
is the context and tagged journal OpenCopy digest. Binary Launch includes transfer
ID and client/server account IDs, but excludes Binding.stream, Binding.session,
sequence and the local inherited FD locator. Binding.stream/session are separate
16-byte attempt identifiers; Binding.client/server are 32-byte accounts. The
ordinary request SHA-256 still covers the entire canonical wire envelope,
including Binding and sequence, and every response must match it. The context
carries those separate attempt fields in addition to the launch digest.
Journal StreamRecord stores a tagged execution launch, its complete digest and
capability selection; stream_store recovery preserves that variant. Native
LivePrepareRequest likewise has a closed Exec|NativeCopy choice; only NativeCopy
can produce the private context handoff. No generic argv conversion can enter it.

StreamSession owns Peer, next exchange sequence, raw channel offsets, codec,
keepalive/backoff, per-call retry deadline, pending request bytes and final
output-ACK handshake. `exchange` never accepts a caller-selected sequence. It
returns bounded raw output blocks plus accepted/delivered cursors, credit,
EOF and terminal; the adapter acknowledges only bytes its consumer accepted.
At most one data call runs; a separately cloned ControlHandle can call Cancel
or Status on sequence 0 concurrently. Session retries identical bytes/digest,
never guesses whether stdin was delivered, and propagates finite wire error
codes instead of collapsing every failure to stream_remote_error. At idle it
uses current adaptive scheduling. The stdio adapter keeps existing binary output,
nonblocking I/O, Ctrl-C and terminal handshake behavior on this API. The copy
adapter consumes the same snapshots into its bounded frame parser. No task may
buffer more than one input block plus one bounded output/control frame per side.

### Session state invariants

`StreamSession` and its independently cloned `ControlHandle` own an
`Arc<SessionContext>` containing the nonsecret local IPC address, binding, relays
and timing settings, not a borrowed `&Context`. Profile/runtime ownership follows
the existing local service; an Arc does not reopen or recreate a lost stream.
Dropping the data session is detachment and never sends Cancel. A surviving
ControlHandle can cancel/status that exact binding until explicitly dropped;
its calls cannot start new work or select another identity.

State is Opening -> Ready -> Pending -> Ready/Draining -> TerminalAck -> Finished.
Before awaiting I/O, exchange stores one immutable canonical request (input,
EOF, ACKs and sequence). A dropped future leaves Pending intact. Calling exchange
with different arguments during Pending returns local InputPending without any
network mutation; `resume_pending()` retries the owned exact bytes. An identical
exchange is another retry. Only a matching authenticated response clears Pending
and advances sequence once. No caller can advance offsets on timeout or from
accepted bytes that were not actually reported.

ACKs are monotonic and no greater than received-and-consumed output offsets;
invalid values fail locally without replacing Pending. Keep stdout/stderr raw
cursors separately. An input-closed response stops new source reads and new input;
accepted and delivered cursors retain their distinct meanings. A terminal snapshot
can contain final buffered output, but EOF for both channels must be present.
After all output is consumed, send one no-input Exchange acknowledging both exact
EOF offsets and await its matching terminal response. Finished/exit 0 is available
only after that final acknowledgment handshake; no premature terminal truncates
output. Independent Cancel/Status uses sequence 0 and its own immutable retry
request, never overwriting a pending data exchange. Both control and data calls
validate binding, request digest and finite wire codes.

### Copy driver scheduling and hash timing

The copy I/O driver owns adaptive sleep and keepalive scheduling, not the file
worker. While Ready it waits with a biased select on explicit control, file-worker
input/credit/progress notifications and an idle poll timer. Reset backoff on real
input/output/worker progress; otherwise grow from 250 ms to at most 2 seconds,
with the next poll no later than configured keepalive. Every wake rechecks
bounded source/output credit; while hashing or syncing it still sends no-input
Exchanges and consumes authenticated snapshots. File work never owns this timer.

Pending owns the exact in-flight request and a fixed per-call deadline of
now+ExecuteOptions.idle; notifications cannot change its bytes or reset that
deadline. Match current Peer behavior: poll local Pending replies at <=100 ms,
bounded by the remaining deadline. Timeout preserves Pending/uncertain input
state and exits without cancellation; an explicit retry resumes those exact
bytes. Keepalive/idle scheduling never invents a new sequence while Pending.
Copy execution must run this driver concurrently with blocking file work;
a foreground call to exchange alone is not the reusable scheduling primitive.

Put performs Open, sends Start, then hashes its pinned source in the worker;
Get helper hashes after validating Start. Neither hashes before Open. Progress
and empty exchanges continue during initial hash before Manifest, both prefix
rehashes, and final verification. Start/Manifest waits have deliberately no
hidden deadline: accepted helper processes remain alive on disconnect under the
user's asynchronous execution contract. Before Start the helper opens no file;
Put's receiver creates/locks state only after Manifest validation. Indefinite
waits consume bounded combined quota and can be explicitly cancelled by the
owning preserved profile or server-local administrator. A connected authorized
principal can hold its quota just as with an ordinary long-running command.

### Admission response and attempt index

Add closed `ResponseBody::CopyRefused { code: CopyAdmissionError,
prior_binding: Option<Binding> }`. Codes are Unsupported, Denied, Busy, Conflict,
Capacity and RecoveryRequired. Busy and RecoveryRequired include the prior stream
binding when available from the server journal; it is diagnostic and is not
sufficient to fabricate a missing local MLS membership. The client maps these to
finite copy diagnostics, prints transfer/stream IDs, exits 1, and does not launch
a helper, guess a remote argv or retry automatically. Existing exact OpenCopy
retries return their persisted Opened/CopyRefused response. A reused stream ID
with a different launch digest remains Conflict. Wire denial is authorization;
file permission errors originate only after helper execution.

Add `copy_attempts` to the existing encrypted journal database, with key
(client[32], server[32], transfer[32], direction), immutable raw paths/options,
current stream/session binding, canonical tagged launch digest, attempt revision,
state, physical-cleanup-confirmed boolean, created/updated/completed timestamps
and retention deadline. States mirror Preparing, Launching, Running, Stopping,
TerminalConfirmed, Lost, UnknownLaunch and CleanupUncertain. The raw manifest's
size/hash remains receiver checkpoint authority, not actor metadata.
Synchronous admission checks only closed-schema bounds, negotiated capability,
authenticated account/room authority, current grant membership, retained attempt
conflicts and combined quota. These can persist CopyRefused with Unsupported,
Denied, Busy, Conflict, Capacity or RecoveryRequired before any Opened. For an
accepted request, `admit_copy_open` inserts/updates its index attempt, new
StreamRecord and Opened(Created) response in one SQLite transaction. A refused
request stores only a bounded replay response keyed by its stream ID/digest;
it does not replace the prior active index owner or consume active process quota.
Bound refusal receipts to 1024 per authenticated principal and 16384 globally;
refusal expiry is the replay retention fence. Never evict unexpired launch fences.
If capacity prevents caching a refusal, before responding atomically advance a
single copy-only unknown-Open timestamp floor to that refused stream timestamp.
Resolve known journal entries first; any unknown OpenCopy at/below the floor is
permanently Expired, never admitted after capacity frees. This bounded conservative
fence may force other clients to use a fresh stream ID after that timestamp; it
does not affect already known attempts or ordinary Exec/rsync. A future-skewed
Open is subject to existing age/skew checks before advancing this floor.
Failure rolls everything back before engine preparation. A replayed Busy applies
only to that identical request/stream ID; each user Create/Resume attempt uses a
fresh stream ID, so a new attempt can observe a subsequently released lock. Preparation completion verifies
both record/index revision and current authorization. Canonical resolution,
executable command restrictions and worker preparation run asynchronously after
Opened; failure produces the existing stream terminal Failed with finite code
(Denied, Unsupported, Capacity, Invalid or Uncertain), closed stdin and EOFs, with
no helper frame required. The client maps Denied to authorization_denied,
Unsupported to copy_unsupported, Capacity to capacity, Invalid to preparation_failed
(all explicit exit 1), and Uncertain to uncertain exit 255. It does not relabel a
missing helper Error frame as Protocol when this terminal proves no execution.
There is no late CopyRefused replacing a persisted Opened; the durable launch fence
updates both in one transaction before spawning. There is never an index-only
permission to spawn. TerminalConfirmed requires actual runner ownership cleanup;
Lost/Unknown/CleanupUncertain require the existing explicit local administrative
reconciliation acknowledgment and cannot become confirmed by a missing handle.

Recovery cross-checks the index and stream journal before admission. Missing or
inconsistent counterparts become RecoveryRequired without launching. Update
cancellation, revocation, shutdown and completion state atomically with the stream
record. Use the existing bounded journal persistence path; file/hash transfer work
remains off the MDK actor. Eligible index rows and receiver receipts expire
only after the retained replay fence; cleanup runs opportunistically at admission
with bounded work. Active/uncertain rows never expire automatically. Receipts and
index expiry do not authorize reuse of an old replay-expired stream ID.

## Copy frame grammar and state machine

The copy byte protocol is above stream chunking: a control frame can span several
8 KiB stream blocks. Reassemble at most one bounded frame, not whole stdout.
Each frame is u8 tag + u32 big-endian payload length + payload. Control payloads
are closed canonical records with the field order below, capped at 16 KiB;
byte strings use u32 length prefixes, IDs/hashes have fixed lengths, integers
are u64 big-endian, flags/enums are u8 and no duplicate/unknown/trailing fields
are accepted. Data is tag + length + u64 offset + <=8192 raw bytes. An empty Data
is invalid; zero-length files use Done without Data.

| Frame | Producer and order | Required payload |
| --- | --- | --- |
| Start | Client first, both directions | Exact CopyLaunch and codec matching server-supplied context digest |
| Manifest | Sender: client for Put, helper for Get | Immutable accounts/ID/direction/raw paths/options, source size/SHA-256, safe requested mode/mtime |
| Resume | Receiver after Manifest validation | Durable offset and rehashed prefix SHA-256 (offset 0/empty hash for Create) |
| Prefix | Sender after comparing Resume | Accepted offset and same prefix SHA-256; mismatch is Error |
| Data | Sender only after Prefix | Contiguous offset and bounded raw bytes |
| Checkpoint | Receiver after durable checkpoint | Monotonic durable offset and prefix SHA-256 |
| Done | Sender after exact source length | Full size/SHA-256; no further Data permitted |
| Commit | Receiver after durable commit | Transfer ID, size/full hash, Committed disposition |
| Finish | Sender after validating Commit | Transfer ID and matching full hash; then sender closes its data direction |
| Progress | Either endpoint during long file work | Closed stage enum, bytes examined and declared total; no durability credit |
| Error | Either endpoint at any valid boundary | Finite code; Busy carries no prior stream ID, no arbitrary error string |

The receiving endpoint validates fixed fields against Start/context; for Get it
learns source size/hash only from the helper's Manifest delivered over the authenticated stream. The client
also validates accounts against its actual local preparation binding. Both retain
the agreed manifest before acknowledging Resume. A sender may have <=2 MiB raw
bytes ahead of the last durable Checkpoint; when reached it services control/output
and waits instead of reading more source. The helper uses poll/timed reads so the
5-second checkpoint rule fires even when the next Data frame never arrives.
Checkpoint traffic cannot be blocked behind unbounded queued Data. Done triggers
final checkpoint/verification even for a sub-checkpoint-length file.
The copy durable window is fixed at 2 MiB with 1 MiB checkpoint interval in this
phase; future configuration must enforce window >= 2 * interval with checked
arithmetic. This is logical file progress, not a 2 MiB buffer: the transport still
holds <=256 KiB per channel and frames are consumed/written promptly. The receiver
checkpoints after each interval (or the timer), independently of sender Done,
and keeps draining bounded frames/control while a sync worker runs. The sender
reads reverse Checkpoint/Progress traffic even when durable credit is exhausted;
both ends keep draining reverse control before sending more Data. These rules
avoid a circular wait when either bounded transport window fills. Outstanding
file writes beyond the durable cursor never exceed the durable window.

Add a Progress frame emitted by either endpoint during Hashing, PrefixHash,
Verifying or Syncing, carrying only stage, bytes examined and total bytes. It does
not acknowledge durability. Hashing/sync is off the I/O driver; use one coalesced
progress slot, emit at most once/second and service it within two seconds whenever
transport credit exists. No hard silence guarantee is possible under output
backpressure or a hung filesystem. The I/O driver continues stream exchanges and
keepalives while file work runs; configured idle timeout measures failure to get
an authenticated transport response, not absence of new file bytes or Progress.
There is no default file-work wall-clock deadline. A stalled disk remains visible
as a live but stalled transfer until explicit cancel or operator action.

CopyError is Unsupported, Protocol, PermissionDenied, NotFound, UnsafePath, Busy, Conflict,
SourceChanged, HashMismatch, DiskFull, Quota, CommitUncertain, LocalIo or
IdentityChanged. PermissionDenied is displayed as file_permission_denied; it is never authorization denial.
For Put the client sends Finish after receiving Commit; the helper validates
Finish, then closes stdout and exits 0. For Get the helper sends Finish after
receiving the client's Commit, then closes stdout and exits 0. The client closes
stdin only after sending its final frame. A receiver retains its Committed receipt
if Finish is lost. Both directions therefore verify the matching Commit and
Finish (sent or received as appropriate), followed by clean stream terminal.
An Error frame terminates copy parsing; process exit is only a second corroborating
signal. Exit 0 requires a validated matching Commit plus clean stream terminal
Exited(0); exit alone cannot claim transfer success. CLI exit 130 is confirmed
explicit cancellation, 255 is uncertain cancellation/transport/commit, and 1 is
an explicit copy failure with its finite category on stderr. Helper exit 1 means
it sent an Error, exit 0 means completed copy handshake; malformed/truncated error
frames produce Protocol, not a guessed category. Wire-level Denied/Unsupported
before helper launch remain distinct from copy-file permission or path failures.

### Unified refusal/error mapping

There are no new WireErrorCode variants. CopyRefused is its own closed response;
helper Error frames use the numeric CopyError table. The complete mapping is:

| Condition | Representation | Client disposition |
| --- | --- | --- |
| Unsupported capability/layout | CopyRefused(Unsupported,None), or async Failed(Unsupported) | exit 1 copy_unsupported; fix installation/capability before new attempt |
| Account/room/grant denial | CopyRefused(Denied,None) | exit 1 authorization_denied; no retry without grant change |
| Executable restriction/preparation denial | Opened then Failed(Denied) | exit 1 authorization_denied; no helper/file access |
| Quota/preparation capacity | CopyRefused(Capacity,None), or async Failed(Capacity) | exit 1 capacity; explicit fresh-ID retry after capacity frees |
| Active journaled prior attempt | CopyRefused(Busy,Some(prior_binding)) | exit 1 busy; print owning-profile cancel command, no automatic retry |
| Parent admission contention, checkpoint flock contention, incomplete created tree | Error(Busy,None) after Start | exit 1 busy/unknown attempt; retry only explicitly after known cleanup/administration |
| Immutable ID/path/options conflict | CopyRefused(Conflict,None) or Error(Conflict,None) | exit 1 conflict; fix operands or use new transfer ID |
| Uncertain/missing index counterpart | CopyRefused(RecoveryRequired,optional prior_binding) | exit 1 recovery_required; server-local reconciliation required |
| Existing wire Invalid/Gap/Expired/NotFound/Closed errors | unchanged ResponseBody::Error(code) | finite named stream error, exit 1 unless execution state is uncertain; do not collapse |
| Unknown execution/cancel/commit | Failed(Uncertain) or transport timeout, or Error(CommitUncertain,None) | exit 255 uncertain; inspect receipt/ownership before retry |
| Helper CopyError other than Busy/Conflict/CommitUncertain | Error(code,None) | exit 1 named file/protocol category; deliberate retry after correcting condition |

`Busy(None)` is the sole wire representation for NotReady/UnknownAttempt wording.
No zero or fake stream ID is persisted. Created checkpoints always know the
attempt stream ID from context; absence exists only in an optional Busy response.
An unauthenticated frame is dropped by the existing transport authority check,
never granted a reflection response with another principal's IDs.

### Shared native APIs and engine spawn types

Own `stream_session` and `stream_driver` in marmish-native, used by stdio/rsync
and copy adapters. Own the pure copy launch/binary codec in marmish-stream's
`copy_protocol` module; it performs no filesystem or identity-provider access.
Proposed Rust method contracts, not public user APIs:

```
StreamSession::open(Arc<SessionContext>, PreparedTarget, StreamLaunch,
                    ExecuteOptions) -> async Result<StreamSession, SessionError>
StreamSession::exchange(&mut self, input: Option<&[u8]>, eof: bool,
                        consumed: OutputAcks) -> async Result<StreamSnapshot, SessionError>
StreamSession::resume_pending(&mut self) -> async Result<StreamSnapshot, SessionError>
StreamSession::control(&self) -> ControlHandle
ControlHandle::status(&self) -> async Result<StreamSnapshot, SessionError>
ControlHandle::cancel(&self) -> async Result<ConfirmedOrUncertainTerminal, SessionError>
StreamDriver::run(&mut StreamSession, &mut impl StreamIo) -> async Result<StreamTerminal, SessionError>
```

Session assigns raw input offsets; callers supply at most MAX_BLOCK bytes.
SessionError includes Wire(WireErrorCode), CopyRefused(code,optional binding),
InputPending, Protocol, LocalIo and Uncertain, retaining finite categories.
`StreamIo: Send` has `poll_input(&mut self,cx)->Poll<Result<InputEvent>>` and
`poll_output(&mut self,cx,channel,offset,bytes)->Poll<Result<usize>>`; InputEvent is
bounded Data(Vec<u8>), Eof or WorkerProgress. Output consumes only a prefix and
registers the driver waker when worker credit is full. Driver scheduling above
uses these notifications, its timer and independent control. Callbacks enqueue
bounded worker jobs and never perform blocking file work on the async driver.

Share `copy_fs::CopyReceiver` in marmish-native between Put helper and Get CLI:
`open_or_recover(pinned_base, destination, manifest, attempt)`;
`receive_data(offset,bytes)`; `checkpoint(now)->DurablePrefix`;
`prepare_commit()->CommitPrepared`; `commit()->Receipt`; and
`receipt()->VerifiedReceipt`. All are synchronous worker-only filesystem methods
with the same path/lock/state implementation in both directions. `copy_client`
and `copy_helper` are adapters, not separate receiver implementations.

Engine-side `PreparedLiveExec` gains a private optional `OwnedCopyContext`;
ChildSpec may borrow its raw locator during spawn, never serialize/clone an FD.
`OwnedCopyContext { read: OwnedFd, launch_digest, registered_image_identity }`
is constructible only inside `prepare_native_copy`, which accepts the authorized
context, native-selected canonical helper path/identity and typed CopyLaunchContext.
Generic prepare_live_exec always has None. The native adapter unwraps its private
RegisteredCopyHelper into this engine request; no native type is imported by the
engine crate, and no public wire/IPC operation supplies that path.

Change install_exec_image's setup argument from `unsafe fn()->io::Result<()>` to
a generic `F: FnMut()->io::Result<()> + Send + Sync + 'static`. Its closure captures
only integer locator and ordinary platform setup, calls unsafe syscalls explicitly,
and allocates nothing after fork. Allocate/reserve the FD first, then run macOS
prepare_spawn_below ceiling/rlimit validation including this FD before installing
the closure. Ownership remains in PreparedLiveExec across fork/spawn; the child
inherits its copy, and the parent drops its OwnedFd immediately after spawn
returns on both success and failure. A definite pre-fork failure drops it too;
an uncertain startup never leaves a parent-held writer or read-FD leak.

### Concrete private helper context

The shared pure `copy_protocol::HelperContext` codec is one closed record:
fixed ASCII domain `marmish-copy-context/1\0`; context_version:u32(1);
helper_protocol:u32(1); client:[u8;32]; server:[u8;32]; group:B(1..1024);
session:[u8;16]; stream:[u8;16]; transfer:[u8;32]; direction:u8;
launch_digest:[u8;32]; retention_seconds:u64; expected_version:B(1..64 ASCII atom);
expected_target:B(1..64 ASCII atom); expected_commit:[u8;20];
entrypoint:B(exact ASCII `__copy-helper-v1`). ASCII atoms contain only letters,
digits, period, hyphen and underscore. The entire record must be <=4096 bytes,
with exact field lengths and no trailing fields. Commit is the decoded 40-digit
lowercase hex from native_manifest::compiled_release_metadata().commit; the
native build script enforces that format, using zero40 only as its existing
non-git fallback. Version and target use that same compiled metadata, never the
client's build. The helper checks these against its own compiled metadata before
transfer filesystem access; this remains compatibility detection, not image pinning.

Authenticated admission constructs accounts, actual MLS group bytes and session/
stream identifiers. The group is trusted context only: Start has no group claim,
and neither group nor attempt identifiers are added to the immutable Launch
digest. `validate_start` checks client/server/transfer/direction and the full
Launch-plus-codec digest. This concrete codec/API refinement requires scoped
verification in the next independent implementation review; design approval does
not imply the uncompiled implementation has been validated.

Retention is trusted lifecycle policy, not a peer claim or immutable file
manifest field. Add `limits.copy_receipt_retention`, an integer count of seconds
with default 604860. Configuration loading rejects zero and values above 365 days;
accepted values below the replay floor normalize upward to 604860. The 365-day
ceiling reuses the project's retention ceiling without reusing its other policy.
This additive defaulted key keeps CONFIG_FORMAT 1; older strict binaries reject
configs explicitly setting the new key instead of silently ignoring it.
`limits.payload_retention` and `core::Retention` (including `None`) never supply
this value: their chat/command payload policies remain distinct.

The context carries the server's normalized copy receipt retention. Put's server
receiver uses it. In Get the helper is a sender: it validates this mandatory field
for uniform codec/schema handling but otherwise ignores it. Get's client receiver
uses its own local `limits.copy_receipt_retention`, normalized the same way.
Receiver `now` is local Unix wall-clock seconds, converted with checked `i64`
conversion; retention also uses `i64::try_from`, and timestamp addition is checked.
Changing lifecycle retention between attempts does not change Launch digest or
manifest binding; existing unexpired receipts/fences never have their deadline
shortened. Wall-clock jumps can change effective lifetime; age alone is never a
sufficient cleanup authorization.

The receiver namespace intentionally excludes group: the same authenticated
account pair, direction, transfer ID and matching immutable manifest may resume
from another authorized MLS group. Each new attempt still passes that group's
current admission and normal executable authorization. Group membership alone
never permits resuming another account's transfer.

Before transfer filesystem access, a compiled metadata mismatch produces a fixed
`helper_server_build_mismatch_restart_daemon` stderr diagnostic, finite
`Unsupported` Error frame when stdout is a validated usable pipe, and exit 1.
Malformed/truncated context or Start/digest mismatch instead maps to finite
`Protocol` with fixed `helper_context_invalid` or `helper_context_launch_invalid`
diagnostic and exit 1; never include context bytes or paths. Unusable context/stdout
permits only the fixed diagnostic and exit 1. These are compatibility checks only:
dirty builds, reused identifiers and the zero40 fallback can remain indistinguishable.
The bounded atom grammar deliberately rejects version metadata containing `+`;
such builds need a compatible identifier before helper admission.

Pure filesystem operations initially retain private trees conservatively. Actor-
authorized GC integration is a Phase 3 acceptance requirement, not a promise made
by the initial pure module. It receives an explicit eligible-inactive set from the
daemon attempt index, then locks and validates each tree before cleanup. A free
flock or elapsed retention alone never resolves Lost/UnknownLaunch ownership.
Before GC integration, reaching the 1024-private-tree parent cap returns `Quota`
with fixed `copy_private_state_quota` diagnostic; creation performs no implicit
cleanup. The same-UID administrative inspection/reconciliation path described
above is the only manual recovery. Trees without an authoritative attempt-index
row (including journal restore/downgrade) remain ineligible: report
`copy_state_unindexed`, retain them and require explicit same-UID administrative
inspection before deletion. Neither an unindexed tree nor a free flock proves
that its helper/descendants have stopped.

### Journal version and helper-image binding

Current JOURNAL_FORMAT is 6. Phase 3 introduces format 7 in one exclusive startup
migration after acquiring the normal profile lease: create copy tables/floor,
convert existing live_streams argv fields to tagged Exec variants without changing
their old Open digests, preserve all state/identity/replay fields, and set
PRAGMA user_version=7 only in the transaction that finishes migration. Accept
old supported schemas 0/3/4/5/6 through existing migrations first. Invalid rows
abort without partial conversion. New copy tables start empty with no admission
floor. Old binaries reject user_version=7 via their existing supported-format
check; downgrade requires an explicit offline backup/restore, not silent reopen.
The floor lives only in `copy_replay_state(singleton,floor)` and is consulted
only in the OpenCopy dispatch branch after existing known-request lookup. Tests
must prove setting it cannot reject ordinary Exec Open or rsync streams.

RegisteredCopyHelper accepts all three existing native_layout forms: receipt/
manifest-validated installed versions; canonical executable cargo sibling layouts;
and the exact version/target-named downloaded release image. No arbitrary external
path override or remote helper choice is introduced. Installed checks retain
receipt/manifest version/target/hash/mode validation. Noninstalled forms retain
existing same-UID/canonical executable checks and an explicit development/unmanaged
layout diagnostic; they make no stronger provenance claim than normal Exec.
Record canonical path, dev/inode, size, mode and full image SHA-256 when registering.
At engine preparation/authorization and immediately before parent-side spawn,
compare that identity and full digest. A detected change is a clean preparation
failure. This is a best-effort check, not an executable FD pin: current ExecImage
owns a CString pathname and calls execve on both platforms. Phase 3 retains this
ordinary Exec trust boundary and adds no fexecve/execveat implementation. Atomic
replacement after the final check can execute a different image. Hostile same-UID
replacement or in-place modification is outside this boundary, just as for
ordinary permitted executables; the private context cannot constrain a malicious
replacement executable.

For accidental upgrades of trusted Marmish images, the context includes expected
native build/version and context schema. The helper checks its compiled build/
version and supported context schema before any transfer filesystem access; a
mismatch is an explicit helper compatibility failure. This detects trusted newer
images with different build identifiers, not arbitrary image replacement. A
rebuild reusing the same identifier is not guaranteed to be detected after the
parent's last digest check. No /proc/self/exe or macOS path lookup is claimed as
a portable running-image pin. Qualification must demonstrate detected pre-spawn
change and trusted-build mismatch refusal while documenting the remaining
check/exec race honestly. Installed version directories should stay immutable
during execution and upgrades should create separate version directories.

## Exact copy encoding and re-entry

The following numeric assignments are fixed for copy version 1. Integer encoding
is unsigned big-endian unless signed is stated. `B` is u32 length followed by
that many bytes (paths <=4096); `O(T)` is one byte 0 or 1 followed by T only for 1.
Booleans are exactly 0 or 1. IDs/hashes are raw fixed bytes, not hex/base64.
Frame length excludes its u8 tag and u32 length; it must equal the parsed payload.
Every field is mandatory, ordered as below, with no implicit/default fields.

`Launch` is: version:u32(1), mode:u8(Create=1,Resume=2), transfer:[u8;32],
client:[u8;32], server:[u8;32], direction:u8(Put=1,Get=2), source:B,
destination:B, overwrite:bool, preserve_mode:bool, preserve_mtime:bool.
Codec is u8(None=0,Zlib=1). These same enum values apply to the canonical
OpenCopy digest defined above; JSON wire enum spellings remain closed strings.

`Manifest` is: version:u32(1), transfer:[u8;32], client:[u8;32], server:[u8;32],
direction:u8, source:B, destination:B, overwrite:bool, preserve_mode:bool,
preserve_mtime:bool, size:u64, sha256:[u8;32], mode:O(u32),
mtime:O(i64 seconds + u32 nanoseconds). Mode is absent iff preserve_mode is false;
otherwise only 0777 bits are allowed and owner read/write must be present.
Mtime is absent iff preserve_mtime is false; nanoseconds <1000000000 and seconds
must be representable by both endpoints' safe futimens conversion (otherwise
Unsupported). There is no attempt mode, source inode or current stream in the
immutable Manifest. Size and every offset are <=i64::MAX.

| Tag | Exact payload |
| --- | --- |
| 1 Start | Launch, codec:u8 |
| 2 Manifest | Manifest record above |
| 3 Resume | phase:u8, durable_offset:u64, prefix_sha256:[u8;32] |
| 4 Prefix | offset:u64, prefix_sha256:[u8;32] |
| 5 Data | offset:u64, remaining payload 1..8192 raw bytes |
| 6 Checkpoint | durable_offset:u64, prefix_sha256:[u8;32] |
| 7 Done | size:u64, sha256:[u8;32] |
| 8 Commit | transfer:[u8;32], size:u64, sha256:[u8;32], disposition:u8(Committed=1) |
| 9 Finish | transfer:[u8;32], sha256:[u8;32] |
| 10 Progress | stage:u8, examined:u64, total:u64 (examined<=total) |
| 11 Error | code:u8, prior_stream:O([u8;16]) |

Stage is Hashing=1, PrefixHash=2, Verifying=3, Syncing=4. Phase is Created=1,
Receiving=2, Verifying=3, CommitPrepared=4, Committed=5. Error is Unsupported=1,
Protocol=2, PermissionDenied=3, NotFound=4, UnsafePath=5, Busy=6, Conflict=7,
SourceChanged=8, HashMismatch=9, DiskFull=10, Quota=11, CommitUncertain=12,
LocalIo=13, IdentityChanged=14. The optional prior_stream field is always absent
in helper Error frames; known prior bindings appear only in CopyRefused. Wire admission
RecoveryRequired maps to a local recovery diagnostic, not a forged helper frame.
Progress is allowed only after Start and before Finish/Error, interleaved at frame
boundaries. It never changes protocol phase or accepted/durable offsets. Error
is allowed after Start at any otherwise valid boundary and ends parsing.

| Retained receiver phase | Resume frame and subsequent frames |
| --- | --- |
| New Create | Create and sync initial checkpoint after Manifest; Resume(Created,0,empty-hash); Prefix(0), Data*, Done, Commit, Finish |
| Receiving/Created | Reopen/validate/truncate saved suffix; Resume(saved phase,offset,prefix); Prefix(offset), Data from offset, Done, Commit, Finish |
| Verifying | Durable full-length checkpoint is required before recording this phase; Resume(Verifying,size,full-hash), Prefix(size), no Data, Done, reverify, Commit, Finish |
| CommitPrepared | Recover rename evidence first; Resume(CommitPrepared,size,full-hash), Prefix(size), no Data, Done; complete original authorized commit or confirm prior rename, then Commit, Finish |
| Committed | Verify installed final identity/hash, Resume(Committed,size,full-hash), Prefix(size), no Data, Done, resend matching Commit, Finish; no rename or mutation |

The sender always compares Resume to its rebuilt immutable manifest and prefix.
For all size-offset re-entry it sends Prefix then Done without reading/sending
Data; new file content cannot replace a retained manifest. A receiver in a final
phase rejects any Data. In Put the receiver sends Commit and the sender sends
Finish; in Get these directions reverse, as specified above. If loss occurs at
any of these boundaries the receipt/checkpoint, not an optimistic stream exit,
determines whether a future attempt may finish or must report uncertainty.

### Normative final-phase ordering

For a receiving partial, after accepting Done(size,hash): stop Data; finish any
pending contiguous writes; sync data; publish the full-length Receiving checkpoint
and its directory; emit Checkpoint(size,full-hash); persist/sync Verifying; rehash
the complete data FD; validate hash and unchanged identity; apply final metadata
and sync data; persist/sync CommitPrepared; perform one permitted atomic rename;
sync both directories; persist/sync Committed; emit Commit; complete Finish.
Verifying is never durable without a prior full-length durable checkpoint.
An interval checkpoint that already reached size can be re-emitted identically;
Checkpoint duplicates at the same offset/hash are valid, regressions/conflicts
are not. Sender accepts Checkpoint after Done while waiting for Commit.

Progress may interleave at complete-frame boundaries throughout these steps;
it changes neither phase nor durability and can never replace Checkpoint/Commit.
On final-phase resume, Prefix(size), Done are still mandatory. Verifying repeats
verification; CommitPrepared first resolves existing rename evidence then finishes
only the original authorized commit; Committed only revalidates its final receipt.
They may re-emit the full-length Checkpoint before Commit, but never accept Data.
A Committed receipt does not reapply mode/mtime, rename, or modify the file.

## Checkpoint schema and lifecycle

Checkpoint v1 fields are: immutable agreed manifest; transfer ID; receiver parent
and private-directory dev/inode; raw and local normalized paths; data dev/inode;
phase; durable offset/prefix hash; full size/hash; requested final metadata;
last-attempt stream ID; creation/update times; and Committed final identity when
known. Encode a closed bounded canonical record with a domain-separated SHA-256
checksum over all preceding fields. This detects torn/corrupt state, not malicious
same-UID forgery. Metadata/checksum are never execution authority.

| Phase | Permitted transition/recovery |
| --- | --- |
| Created | Directory/data/initial manifest synced; offset zero; then Receiving |
| Receiving | Append contiguous Data; publish durable checkpoints; then Verifying |
| Verifying | Done validated; verify full data FD; then CommitPrepared |
| CommitPrepared | Data identity/size/hash and final metadata fixed, then one atomic rename; recover as described above |
| Committed | Idempotent verified receipt; no writes/replacement; expire only after retention fence |

After mkdir sync the receiver parent; after creating data and initial checkpoint,
sync data/checkpoint and private directory before advertising Created/Resume.
After a recovered uncommitted suffix is truncated, sync the data before advertising
the saved durable offset. Phase 3 rejects preserve-mode values lacking owner
read/write before starting, so staged data remains safely reopenable through
CommitPrepared recovery; it does not promise portable mode-000 recovery. Reapply
the recorded final mode through the validated open FD before commit.

Checkpoint v1 has one canonical binary layout: version:u32, Manifest:B,
phase:u8, local_normalized_path:B, parent:(u64 dev,u64 ino),
private_directory:(u64 dev,u64 ino), data:(u64 dev,u64 ino), durable_offset:u64,
prefix_sha256:[u8;32], last_stream:[u8;16], created:i64, updated:i64,
expires:i64, committed_final:O(u64 dev + u64 ino), checksum:[u8;32].
Manifest is encoded once, exactly as above; transfer/raw paths/size/hash/metadata
are not duplicated elsewhere. Destination basename is derived from the validated
normalized path, not stored again. Domain-separated checksum excludes its own
32 bytes. Signed timestamps use i64 two's-complement big-endian seconds.

Worst-case Manifest size is 8362 bytes: version 4 + accounts/transfer 96 +
direction 1 + two (4+4096) paths + flags 3 + size 8 + SHA 32 + optional mode 5 +
optional mtime 13. Exact macOS normalization adds at most 8 bytes, so normalized
path including its length is <=4108. Relative paths stay relative; no cwd
string is prepended into this stored path. Total checkpoint maximum is
4 + (4+8362) + 1 + 4108 + 48 + 8 + 32 + 16 + 24 + 17 + 32 = 12656 bytes,
below 16384. Reject over-cap input/serialized metadata before creating state;
never truncate, silently omit fields or split one checkpoint into unbounded parts.
Bounds use checked arithmetic and are tested with maximum paths and metadata.

Sync means fsync on Linux; on macOS regular files require F_FULLFSYNC as in the
existing engine platform helper, while directories use fsync. State the macOS
weaker directory power-loss guarantee: orderly process/daemon crashes are covered,
not an unconditional guarantee against filesystem/hardware loss of recently
synced directory entries. Never describe fsync as a backup guarantee.

Flock is held on `lock` inside the private directory, opened no-follow 0600,
owner/nlink/type validated. Use nonblocking exclusive locking. After acquiring,
save/sync the current known attempt stream ID. Helper lock contention always
returns Error(Busy, prior_stream=None); it never trusts potentially stale owner
metadata while the holder is publishing it. UnknownAttempt is diagnostic wording
for that absent optional field, not a stored ID or separate wire enum. Lock files are never
unlinked/replaced while an attempt could hold them. A disconnected helper may live
indefinitely and holds the existing combined stream quota; server-side local
administrators can list/cancel its stable stream ID through existing same-UID IPC.
No remote holder is reclaimed by PID or age.

Helper SIGTERM sets only a flag in its signal handler; normal code checks it at
frame/block and sync boundaries. Before CommitPrepared, stop new Data/commit and
retain the last durable checkpoint. After CommitPrepared, perform no new rename
once cancellation is observed; if rename already happened finish durability and
receipt best-effort, otherwise leave recovery evidence. The existing bounded runner TERM-then-KILL grace can expire while hashing or
fsync is in progress; this does not extend ownership deadlines or imply the
checkpoint completed. SIGKILL can interrupt any operation and uses the same
crash-recovery rules; kernel-blocked physical cleanup remains uncertain. Authority withdrawal follows
phase 2 policy: no new remote input/control/replies, preserve the running helper
and accepted work/quota; it is not automatic cancellation. Already accepted work
may finish a commit. Explicit local administrative cancellation remains available.

## Disconnect, cancellation and observability

Attachment loss keeps helper/file locks alive; buffers apply backpressure.
Resume of a live locked transfer returns Busy and identifies the previous stream.
For Put the client holds no receiver checkpoint: it rebuilds the source manifest
and compares the remote durable manifest. It prints the transfer ID before Open,
and server stream-list persists/displays it even if the CLI dies; no local secret
resume token is required. For Get the CLI is the receiver: it owns the private
checkpoint lock from recovery/Create through Finish or local teardown. A local
crash drops that lock but leaves truthful checkpoints; losing the client does not
terminate the remote sender. Local Ctrl-C first requests remote Cancel, stops new
file work at the helper's same checkpoint/commit boundaries, and retains evidence
if confirmation is uncertain. Both endpoint implementations use the same receiver
state machine, sync and recovery rules.

The server also keeps a durable transfer-attempt index keyed by authenticated
client/server, transfer ID and direction, binding immutable launch fields. Under
one actor transaction it admits at most one physical helper attempt for that key.
A new stream returns CopyRefused(Busy) with the old stream ID while that attempt is active,
Lost, UnknownLaunch or cleanup-uncertain, regardless of whether the receiver lock
is currently free. Old terminal streams with confirmed physical cleanup permit a
new Resume attempt; old launch digests/attempt IDs remain replay fenced. This
index is required for Get as an ownership/quota invariant, even though an extra read-only
sender cannot corrupt a pinned receiver: a lost local lock must not create
unbounded orphan sender attempts for the same transfer. Its receiver lock lives
on the client. It is not a
filesystem authorization capability or a promise to reclaim uncertain children.
Local administration must resolve uncertain ownership before a new attempt.
A new Create with a previously used retained transfer ID fails Conflict; a retry
of the same stream/Open remains the existing idempotent request. Retain index
bindings and attempt fences for at least the checkpoint receipt retention period.
The owning user can run `marmish cancel REMOTE --stream OLD_ID` from a new CLI
process using the same local identity/profile. Existing cancel_selected loads the
persisted client-side stream binding (session/client/server/stream) from the local
daemon's journal and uses the independent authenticated sequence-0 Cancel lane;
it does not need the crashed frontend or its data sequence. Copy session setup
must durably store this binding before first Open, like ordinary streams. Busy
prints both IDs and that concrete cancel command, returns an explicit failure,
and never auto-cancels or auto-retries a competing attempt. After confirmed
physical cleanup the user explicitly retries `--resume TRANSFER_ID`.

Loss of the entire client profile/journal is different from a CLI crash: current
cancel_selected cannot recover the old MLS binding from an ID alone. Report
RecoveryBindingMissing and require server-local same-UID list/cancel/reconcile;
do not claim remote recovery from merely possessing an npub or transfer ID.
Authority withdrawal may likewise require server-local administration. Never auto-cancel another endpoint or relaunch a lost
process. Ctrl-C uses the existing independent stream Cancel lane; a second
interrupt may leave without confirmation. Cancellation before commit preserves
the last valid checkpoint; cancellation racing commit requires receipt recovery
to distinguish committed from partial. It does not roll back a committed file.

Status lists transfer ID, attempt stream ID, direction, durable byte offset,
source size, phase and finite diagnostic category; paths and file contents are
not written into published validation reports. Stable IDs support cancellation
from another terminal. Protocol error versus authorization denial, source change,
missing path, unsafe path, Busy, hash mismatch, disk full, conflict and uncertain
commit are distinct errors. Logs do not include secrets or payloads.

## Acceptance boundaries

Before implementation, independently review this design with Claude and resolve
blocking findings. Implementation divides pure protocol/checkpoint validation,
filesystem receiver, trusted engine helper handoff, session adapter, and CLI;
assign disjoint file ownership and review the integrated result before publication.

Required tests include real put/get through Marmot in both directions; empty,
non-UTF8, whitespace/newline/metacharacter/leading-dash filenames; incompressible
and compressible data beyond 65 MiB; complete hashes; read-only/missing parent;
symlink swaps at every component; FIFO/device rejection; hardlinks; concurrent
attempts; wrong account/direction/path/source/metadata resume; corrupted prefix;
source mutation; checkpoint crashes before/after every sync/rename; no-clobber
race and overwrite race; lost commit ACK; cancellation while buffers are full,
while syncing and during commit; disconnect/BUSY/cancel/resume; private context FD
forgery/missing/oversize/leak prevention; denied helper executable; authority
withdrawal; quota and retention; fd0/1/2 initially closed; Linux AMD64/ARM64 and
macOS ARM64 installed interoperability. Measure raw goodput and compression ratio;
network tests require explicit relay selection and disposable identities.

Additional required negatives: Create on existing ID, Resume without checkpoint,
context pipe initially allocated exactly FD3 then relocated, bounded frame reassembly and durable-byte window,
macOS alias spelling, remount IdentityChanged refusal and directory durability claims, and preserved mode without
owner read/write bits.

Additional session/authority tests: Get client death while the remote sender is
alive; transfer-attempt admission after local lock loss; concurrent mkdir/Create
and Resume; unsafe admission-lock collisions; slow initial/prefix/final hashing
with transport keepalives; progress coalescing; durable-window/cadence invariants;
finite wire error categories surviving session extraction; and two-directory
no-replace rename unsupported-filesystem behavior.

Runtime qualification workloads must use bounded deadlines derived from measured
relay goodput plus at least 4x margin, never an assumed LAN transfer rate. The
current one-block-per-exchange transport bounds goodput by roughly 8 KiB per RTT;
large-file tests may need hours on slow relays. Publish observed rates and actual
timeouts with results, and use smaller bounded cases for routine CI.

Session tests must cover dropped futures with mismatched retry arguments, ACK
regression/overshoot, independent ControlHandle after data-session detach, both
output EOFs before final terminal acknowledgment, pre-helper Failed(Denied),
principal-separated transfer namespaces, and strict dev/inode change recovery.
Existing-stream status/control uses the journaled binding; maximum new-Open age
validation does not itself mean an active stream is forcibly terminated after
7 days. Qualification must verify this actual distinction and fresh-ID resume.

Supported storage is local filesystems with working nonblocking flock, atomic
rename and required sync semantics. NFS/network/removable filesystems are not
qualified; detected unsupported locking/sync/rename fails Unsupported. The direct
parent scan cap means a destination parent with >100000 entries fails Quota even
if few are copy directories. This is an explicit limitation of phase 3.

Additional regression cases: each accepted format 0/3/4/5/6 to format-7 migration/rollback and old-binary
rejection; copy refusal floor leaves Exec/rsync untouched; typed helper context
FD ownership on every spawn error; image change between registration/authorization and final spawn check, plus
trusted-build mismatch after the final check and documented check/exec race; all registered layout forms; shared receiver tests in
both adapters; and the unified Busy/unknown-attempt mapping.

A copy-only unknown-Open floor refusal must advise the client to use a new stream ID;
a retained original request still resolves before that floor check.

### Reviewed server receipt GC refinement

The server's current confirmed inactive attempt, matching revision and immutable
manifest, durable exclusive GC claim, and physical receiver flock establish GC
authority. The checkpoint's `last_stream` may identify an earlier attempt when a
later Resume stopped before opening the receiver; GC does not require equality
to the latest attempt. New Create/Resume is refused while the durable claim
exists. This changes neither ordinary resume binding nor checkpoint observation
proof, and never authorizes deleting an unindexed tree.

Server GC schedules bounded scans with durable fair turn and retry-after values.
At most 32 blocked rows are deferred in one transaction; failed physical work
retains its index and moves behind unattempted work with a 60-second retry delay.
New admitted revisions clear old delay, and index deletion removes its scheduling
metadata. Interrupted claims remain cleanup_uncertain rather than becoming fresh
retry authority.

A missing namespace is retained, not reported cleaned: a changed cwd or replaced
parent may hide the original receipt. Existing trees must pass checkpoint parent,
private/data descriptor identities, immutable manifest and lock checks. Unknown
private entries, stale checkpoint temporary files, never-created/moved namespaces,
and deletion without confirmed journal completion can retain capacity indefinitely.
This increment has no supported CLI for clearing these filesystem/index uncertainty
cases; stream reconciliation is not filesystem cleanup proof. Capacity remains a
finite distinct refusal. Fair scheduling allows other eligible transfers to proceed.
The scoped design approval and subsequent code-review status are recorded in
[server copy review](../reviews/streaming-copy-server-code.md).
