# Local Terminal supervisor and persistent tmux

This increment qualifies a server-side owner for persistent tmux session
selection, observer attachments and writer fencing. Build it in the native test
binary, using real `CommandConfig`, SQLCipher and the existing native attachment
worker. A test may supply an already authenticated account to its private
admission seam. A later activation increment must bind that seam to the proved
Command enrollment, exact Terminal room and generation, current MLS leaves and
ordered roles. It also owns IPC, named-profile daemon integration, relay
scheduling, decode-to-effect wiring, user prompts and suspend-aware clocks. No
ordinary daemon opens the supervisor database or schedules a Terminal worker.

## Authority and owner turns

Start under the existing single nonroot UID and an exclusive profile lease. Load
`CommandConfig` once and use `terminal_allowed` for every admission and physical
PTY effect, including every retry of a partial write. That method requires both
the execution and explicit shell grants and a server-capable profile. The grant
is a full same-UID shell. Files, processes, other sockets and local interactivity
remain available to that shell under ordinary OS permissions. Tmux read-only
flags limit what an observer can do with its attachment. Neither the supervisor
nor the flags impose an operating system security boundary between authorized
shells running under the same UID. No remote field selects a UID, executable,
working directory, socket path, tmux options or shell arguments.

One synchronous owner holds the registry and all `Attachment` values. Its
exclusive `&mut` turns serialize authority changes and actual nonblocking
write/resize syscalls. Check the exact live claim and role inside the same turn
immediately before calling the backend. The owner never lends out an attachment
or raw descriptor. Poll receives an immediate deadline in an effect turn.
Management commands and initial launch have separate bounded turns. A future
async integration must keep this owner off the Command actor and arrange a
wakeable, deadline-bounded scheduler. There is no shared attachment mutex and
no lock held while waiting for a client's input.

An attachment claim is a random 128-bit ID plus a nonzero, server-wide generation.
A detached/replaced claim never matches again, even after restarting the owner.
An exact Close or Takeover is processed in owner order. If Close wins, Takeover
with the old claim fails stale. If Takeover wins, the old Close fails stale.
Validate authorization and exact occupancy before reservation or fencing. Fence
logical claims before physical cleanup. Discard the unused suffix of any queued
or partial input outside this owner and recheck before any retry. Writes already
accepted by the kernel can take effect after fencing. Delivery progress and its
11-minute lease remain the Terminal Frames attachment owner's responsibility.
Activation will call this owner to detach for authenticated Close, lease expiry
or policy retirement. The Unix suspend-aware clock belongs at that activation
boundary, where Linux `CLOCK_BOOTTIME` and macOS continuous time require native
qualification.

Grants are restart-only, using the existing configuration lifecycle. An active
owner never hot reloads a policy. At startup, before offering any authorized
slot, reconcile the registry against the fresh policy and terminate every
verifiably owned session for a removed grant. A refused or ambiguous cleanup
leaves the account denied and the affected row quarantined. A later regrant
cannot silently revive a session already marked for retirement.

## Session selection and attachment lifecycle

Each authenticated account may choose its default slot or an explicit opaque
slot ID previously returned for it. A first default selection creates its slot.
Explicit New always allocates a separate persistent slot and never changes the
account's existing default. Unknown and wrong-account IDs get the same missing
outcome. Keep the private default map in SQLCipher. Storage and tmux use random
128-bit names, with no account key, alias or externally supplied label in their
path or process arguments. The manager may return bounded metadata for the
requester's own slots and the occupied writer claim only. No session can have
more than one writer, three observers or four tracked attachment workers total.
Cap the registry at 16 slots per profile and the running/reaping attachments at
64. Pending, quarantined and retiring slots count. The cap covers resources
started by the supervisor. Shell descendants and user-managed tmux changes are
not bounded by it.

An occupied writer gives a typed busy result with its exact claim. Observe and
Takeover are explicit. An observer is tied to a live writer. It has its own
fresh PTY and reads through a bounded slice supplied by the caller. Check the
role for both input and resize. Never invoke native write/resize for an
observer. On intentional writer detach or replacement, fence all observers and
the writer, close and reap their workers, and require no old tmux clients on
that slot before attaching a successor. Writer detach or takeover invalidates
this observer output source, so no observer in this owner keeps its frames
binding across the change. A worker with pending bounded cleanup
continues consuming its slot budget. A failed cleanup never permits another
writer. Unexpected writer exit is detected by local polling and fences its
observers. While the writer is absent no observer claim is admitted. Fresh
attachments rely on tmux to redraw the screen, without saved PTY data.

Ordinary shutdown and detach preserve the selected tmux shell. A reopen adopts a
verified slot and issues a fresh claim. A real shell exit, uncertain tmux
operation or foreign process never turns a stale claim into an implicit New.
Use closed typed outcomes for authorization, missing, busy, stale, wrong role,
capacity, unavailable tmux, launch failure and pending/ambiguous recovery. Do
not log process output, keys, markers, socket paths or command text.

## Local tmux management

Use one private tmux server and one initial detached session per slot. Separate
sockets prevent normal tmux session navigation from switching to another
Marmish slot behind the supervisor. The profile lease and same-UID validation
precede every operation. Canonicalize and verify a private `t` directory under
the validated local profile root. Use `t/<32 hex slot chars>/s` beneath it,
with owner-only directories, and require the physical socket path to fit the
backend's portable 103-byte bound before recording a slot. Refuse unsupported
long profile paths with a typed result. A trusted shorter placement scheme may
be qualified for activation. Do not follow preexisting links or remove stale,
unverified sockets. Create each random slot directory with exclusive creation.
On stopped or ambiguous sockets, attach/read/cleanup use tmux `-N` and never
start an implicit server. The only command allowed to start one is the first
journaled create at a new verified path.

Resolve and validate an absolute local tmux binary and the same account's
passwd shell with the native backend path checks. There is no shell override
in this increment. Pin minimum tmux 3.3 for `allow-passthrough`, and use a strict
bounded version parse. Start via argv, with `-u -S <path> -f /dev/null`, an empty
inherited environment, known PATH and locale, validated passwd HOME, USER and
SHELL, and physical home as initial directory. Tmux `-f` replaces the default
configuration list. In one command request start the server, set `status off`,
`window-size latest`, `set-clipboard off`, `allow-passthrough off`, and 2,000
lines of history. Set `default-shell` to the validated passwd shell, leave the
spawn command empty, and inherit the checked physical home as the tmux client's
working directory to avoid interpolating a path into a tmux format. The
initial pane starts the default shell as a login shell. Set a per-slot random
startup proof in a dedicated session user option in the same request. Tmux
executes that request sequentially. The journal must still handle a crash
between session creation and option set.

The Linux x86_64 tmux 3.4 spike ran this exact command shape with a private
fixture socket and a disposable HOME. Each item below is an argv token, with
a literal `;` passed directly to tmux between the commands. The shell was
`/bin/sh` and the proof was a fixed 32-digit hexadecimal test value:

```text
/usr/bin/tmux -u -S <socket> -f /dev/null
  start-server ;
  set-option -g status off ;
  set-window-option -g window-size latest ;
  set-option -s set-clipboard off ;
  set-window-option -g allow-passthrough off ;
  set-option -g history-limit 2000 ;
  set-option -g default-shell /bin/sh ;
  new-session -d -s shell -x 80 -y 24 ;
  set-option -t shell @marmish-proof <proof>
```

A follow-up `-N list-sessions -F` returned exactly `shell`, tmux ID `$0` and
the proof. All six settings matched readback, the fixture's user config option
was absent, and the shell printed an argv zero of `-sh`. A command with `-N`
against a missing socket failed without creating it. The 3.3 minimum is a
version gate in this increment. The native test host supplied 3.4.

Management stdout is bounded to a small closed format (at most 4096 bytes).
Drain a bounded pipe while waiting on the owned command for at most two seconds.
On timeout or violation close its pipe and kill/reap only that direct child.
Treat its effect as ambiguous, since the persistent tmux process may already
have applied it. Normal start suppresses stderr and does not log command output.
Inventory must return exactly one `shell` session plus its exact server socket
owner and startup option. Strictly parse and record the real tmux session ID.
On reopen compare that ID and marker before adoption. Target a verified ID for
mutation, and use the native exact `=shell` attach with the fresh managed socket
check. Never launch an attachment after a failed inventory check. If a managed
slot's inventory times out, is malformed, or has unexpected clients, stop and
return unavailable/cleanup pending. Keep an explicit registry of held children
rather than discovering them by a UID-wide process scan. After closing old
workers, wait up to one second for their clients to depart. After launching a
worker, wait up to one second for its client to register. Poll at 10 ms
intervals with each query bounded by the same deadline. Exact client counts
are required. An undershoot during departure or an overshoot during registration
fails the operation immediately.

Extend `CheckedLaunch` and reserved worker argv with an enum role and strict
fixed tokens. Preserve the current writer constructor contract for already
qualified callers. Pass `-N` to both roles. Use
`attach-session -E -f read-only,ignore-size -t =shell` for observers and
`attach-session -E -t =shell` for writers. The PTY worker always uses its own
initial geometry. In the Linux x86_64 tmux 3.4 spike, 121×61 then 134×72 observer
geometry left an 80×24 writer unchanged, writer resize changed the pane to
93×31, and the observer could not enter input even when the harness wrote to its
local PTY. After the writer detached, tmux used the sole observer's 134×72
geometry despite `ignore-size`. The owner therefore closes dependent observers
first and never hands one off across writer loss. A crash may briefly redraw
until the unexpected writer exit is noticed. A replacement writer attaches with
its new explicit geometry after old clients are gone. Native macOS remains
unqualified until a real local run. [Tmux configuration and no-start flags](https://github.com/tmux/tmux/blob/3.4/tmux.c#L338-L385).

## Private SQLCipher journal

Use a separate `terminal.sqlite3` connection in the verified profile state,
owned solely by the local supervisor under the same exclusive profile lease.
No writes or schema bump in the ordinary Command journal. Use the public
`marmish_engine::initialize_storage` for process-wide SQLCipher initialization.
The native identity layer already supplies the master via
`IdentityMaterial::sqlcipher_root()`, while `ProfileLease::profile_id()` and
`public_key()` identify the already-locked profile. Mirror the narrow
keying/open discipline from `native_preflight::open` and `Journal::open`:
`identity::open_private`, `rusqlite` no-follow flags, checked cipher availability,
`sqlite3_key` with a zeroizing raw key representation, compatibility 4,
4 KiB pages, memory-only temp storage, secure deletion, `journal_mode=DELETE`
and `synchronous=FULL`. Validate any existing rollback journal as a private
regular file before letting SQLite recover it. The supervisor derives its own
zeroizing key with HKDF-SHA256 and a unique native `terminal-supervisor-v1`
info label including the profile identity. Its inputs and label are separate
from native Command/MDK keys and from engine State and Spool. No engine-private
`StorageKeys` API, exported raw engine connection or engine change is needed.
Bind the registry to that profile identity and server key
before processing any row. Only schema 0 with no tables and schema 1 are
supported. Install format 1 in one transaction and reject an unknown format
before changing it. Store the account, opaque ID, default choice, management
state, random startup proof and verified native session ID. Store a monotonic
nonzero allocation counter separately and represent the full unsigned 64-bit
range as 8-byte big-endian blobs. Commit the next generation before returning
any claim or spawning its worker. Never persist a claim, transcript, MLS key,
resize history, input, screen, output or lease state.

Use `prepared`, `ready` and `retiring` rows. Commit a prepared intent with the
unique slot ID and marker before creating directories or processes. Promote to
ready only after a bounded managed inventory round trip. On crash after intent,
while starting, after the marker or before commit, adopt only when the
ownership fields match and exactly one valid session is present. A missing,
unmarked or mismatched session from an ambiguous operation stays quarantined
and consumes capacity. Never start another server at that path. On revocation
commit `retiring` before issuing verified cleanup. Exclude retiring slots
regardless of later config and retain uncertain records. Delete them only after
proven clean retirement when management status is unambiguous. On tmux 3.4
the socket can remain after a successful `kill-session`. Remove it only when
its device and inode still match the verified socket and a nonblocking connect
reports connection refused. Leave mismatches and ambiguous results in the
registry. A crash after removing the socket and before deleting the row also
leaves a quarantined row. A separate activation design will qualify release of
quarantined slots. For this increment 16 quarantined rows cause a capacity
refusal before any process launch. Tests exercise this with disposable profiles
and only the fixture cleans its own intentionally ambiguous test resources.

## Implementation and acceptance

1. Add the test-gated owner, registry and tmux management modules. Reuse the
   native path checks and attachment primitives. Keep production CLI, daemon
   integration, ordinary stores, network and workflows outside this increment.
2. Extend the reserved worker argv and verify observer/no-start contracts and
   malformed dispatch. Small native core cases cover exact policy and role,
   stale/foreign claims, default/New resolution, bounded allocation, generation
   exhaustion, schema/foreign key refusal and reconciliation decisions. Keep
   real SQLCipher and process tests opt-in if their runtime exceeds the existing
   core runner budget.
3. Run the ignored native integration with the real checked daemon worker and
   tmux. Prove same pane reuse after detach and SQLCipher reopen, New and account
   isolation, concurrent observation without writer size change, observer input
   and resize denial, stale takeover with delayed old input and resize, exact
   takeover, observer cleanup, and new-config grant removal. Exercise prepared
   rows with no socket, unmarked or foreign socket, already marked slot, and
   interrupted final commit. Exercise no-start on stale sockets, path length
   refusal, older tmux version, wrong-key and future schema refusal. Confirm separate
   derivation from native Command/MDK keys and unchanged Command account
   stores under the profile. Reuse the
   existing immutable installed-daemon harness patterns. Use disposable profiles,
   keys and owned children with bounded teardown. Validate no existing Command
   behavior or ordinary migration changes. Run the existing engine encrypted
   storage and fault suites without edits. Use Linux x86_64 for the local gate
   and report native arm64/macOS qualification separately.
4. An opt-in native test reports p50/p95 from at least 30 bounded echo operations,
   plus initial launch, first render, detach, resume and observer attach timings.
   Report no pane content or identity. Run Clippy and obtain integrated Claude
   Code auto review of the frozen source and evidence. Daemon/relay integration,
   concurrent ordinary Command measurements, and wire/PTY-effect retry proof
   are follow-up activation work. Get Claude approval on this design before
   code changes.
