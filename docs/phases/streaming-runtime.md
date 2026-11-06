# Phase 2: native streaming runtime

Claude technical approval recorded in [the design review](../reviews/streaming-runtime-design.md).
Implementation authorized by the existing direct-user phased-build instructions.
Parent: [streaming plan](streaming-transfer-plan.md), Beads `marmish-8ka`.
Phase 2a implements the runtime and `stream` CLI. Phase 2b separately adds rsync
invocation parsing and qualification. Each increment requires independent Claude
design approval before coding and code review before publication.

## Authority and execution

Use stream/1 application events only inside accepted dedicated command rooms.
Factor shared native event authentication out of the command decoder: validate
accepted room/current membership/authority, sender leaf/account, native processed
state, application author and exact protocol tag before decoding. Check session,
client/server identities and direction against the sealed native context. Current
trusted-remotes/execution-client grants apply to every operation, cached reply and
publication. Ordinary DMs cannot execute programs.

A bounded engine-owner preparation request carries operation ID, stream ID,
revision, sealed Owner and argv. The owner derives its authenticated engine context
and calls Engine::authorize, resolve_exec and check_execute_request with the
captured daemon PATH. It returns opaque PreparedLiveExec containing the validated
ResolvedExec, account, cwd and sanitized environment. No arbitrary environment,
UID or ChildSpec constructor is public. Filesystem lookup runs off the serialized
network actor. The actor rechecks operation/revision, cancellation latch, launch
fence and current authority before handing preparation to the bounded runner.
This channel does not use the finite EngineCompletion/CommitAck operation ledger.

An explicit opaque engine API owns the actual child:

- `spawn_live(prepared) -> LiveChild`, using launcher::spawn_child, canonical
  executable revalidation, existing account/environment rules, descriptor cleanup
  and a fresh child process group;
- `LiveChild::take_pipes()` transfers each pipe once;
- `LiveChild::identity()` returns diagnostic boot/process-start/PID/PGID identity;
- `LiveChild::try_exit()` observes exit without reaping;
- `LiveChild::signal_term()` and `signal_kill()` require the original owned,
  unreaped child, matching process identity and group;
- `LiveChild::reap_last()` consumes ownership only after final group cleanup.

This is a new ownership wrapper; worker.rs::ChildOwner is private and bound to
its worker identity, and cannot simply be exported as a daemon child. Test that a
child launched by the multithreaded daemon inherits only descriptors 0–2. Remote
PATH lookup uses the captured daemon PATH; child PATH retains the existing
sanitized `/usr/bin:/bin`. Absolute Homebrew executable selection remains possible.

Removing authority refuses further input/control, cached replies and publications
but does not silently cancel an already running stream, matching existing finite
execution policy. The blocked child retains active quota. Local administrative
cancellation remains available; a revoked remote client cannot regain authority
by sending Cancel. Current membership/authority invalidation likewise stops data
flow, records AuthorityRevoked as the attachment reason and preserves the owned
process until explicit administrative cancellation or orderly server stop.
An MDK application-message invalidation conservatively fences every existing
stream projection in that MLS room, invalidating preparation revisions and
withholding further remote data/control publication. It does not implicitly
kill a process. Same-UID administrative cancellation remains available for owned
children. Invalidation preserves Launching, Lost and UnknownLaunch states and
their active credit: neither administrative Cancel nor service Stop may claim
cleanup without an owned child. Explicit reconciliation of possible live state
remains necessary for restart holds.

## Durable launch and restart

SQLCipher owns immutable Open digest, owner/room binding, irreversible cancellation
latch and launch state. Commit Launching before spawn; commit returned child
identity before writing stdin. Duplicate Open attaches to an existing record;
a different digest conflicts. Stale/cancelled preparations never spawn. A crash
between Launching and recording identity is UnknownLaunch, never safe to rerun.

Before processing pending native events on startup, mark every nonterminal local
attachment Lost. Never rebuild lost byte queues, compressor history or pipes, and
never relaunch an old Open. Preserve sealed client bindings so local Lost client
records can still send authenticated Status/Cancel while their grant remains valid.
`streams` lists this local projection and last-known remote state; it is not a
remote List operation.

Persisted identity is diagnostic, not a capability to signal a recycled PID. The
current Linux/macOS process probes do not grant safe ownership of an orphan:
macOS owned-child lookup requires the current parent; probing a recorded start
identity and subsequently signalling a numeric PGID has a reuse race. Therefore
this phase has no `from_recorded_identity` signalling/reaping API. Startup may
report ChildAbsent only after a definitive absent/mismatching identity result;
permission/inspection uncertainty is ChildUnknown. A matching orphan is reported
ChildUnknown/live-unowned, never automatically signalled. Lost records with unknown
live disposition hold quota until explicit local administrative reconciliation;
that acknowledgement records possible surviving execution instead of claiming a
kill. A genuinely absent leader does not prove escaped descendants are absent.

Safe automatic orphan cleanup would require a surviving supervisor retaining the
original Child/wait ownership and an authenticated control socket. That is a
material follow-up design, not a promise of this daemon-tied runtime. An abrupt
daemon crash can leave a blocked orphan; document this limit and its operator
cleanup requirement. Ordinary client disconnect is distinct and preserves the
live runner and its recoverable administrative cancellation path.

Keep launch/cancel tombstones until replay-floor/room retirement proves old Opens
inadmissible. A tombstone is never evicted just to make admission space.

## Lifecycle and resource bounds

Finite execution remains unchanged. Stream and finite admissions share the native
SQLCipher active/retained budget transaction: defaults are four active processes
per principal, 32 globally, 1024 retained invocations per principal and 10000
globally; configured lower limits apply to the combined totals. Accepted,
Preparing, Launching, Running, Cancelling and uncertain live/Lost streams retain
active credit. Internal finite-engine quotas remain defense in depth. At most four
preparations run globally, using the existing engine_starting bound. No stream
creates an unbounded task or channel population.

Each stream has 256 KiB raw windows for stdin, stdout and stderr, at most 256
receipts per channel, one outstanding Exchange and one pending preparation.
A maximum 32 streams therefore bounds raw window capacity to 24 MiB globally,
plus bounded codec, child-pipe and publication metadata. Pipe reads pause when
output credit fills; stdin writes happen in runner tasks, never on the actor.
Independent stdout/stderr draining is fair. EOF follows every accepted input byte.
Exchange snapshots carry mandatory `stdin_closed` separately from actual
`stdin_accepted` and `stdin_delivered` byte cursors. EPIPE, child exit or
cancellation closes input; new bytes after closure are rejected without advancing
these cursors. The same Exchange still applies valid output acknowledgements,
advances its logical sequence and caches its snapshot. Clients stop reading and
sending stdin when closure is reported, discard only their unaccepted pending
bytes and continue draining output and natural exit status. This covers early
consumers such as `true` and `head`; it never fabricates accepted input.
Normal completion includes final output offsets; clients drain through both before
reporting exit. Leader exit with descendants holding pipes uses the engine's
bounded drain_grace, then TERM/cancel_grace/KILL while the leader remains unreaped.
Descendants escaping with setsid are outside process-group cleanup guarantees.

Client disconnect, local SIGPIPE/write failure, frontend timeout and peer-idle
expiry do not cancel the process or synthesize stdin EOF. Stop new input, leave
unconsumed output unacknowledged and retain bounded queues/active quota. A child
may block indefinitely on its full output window until explicit cancellation or
administrative server stop. This is deliberate asynchronous command lifetime,
not an automatic ClientAbandoned cancellation policy. The frontend defaults to an
11-minute no-response/progress timeout and five-minute keepalive when otherwise
inactive; both are configurable. Idle polling of a responsive server refreshes
transport liveness; empty responses do not count as payload progress in metrics.

Normal exit propagates remote status; signal N maps to min(255,128+N). A locally
confirmed Ctrl-C cancellation returns 130; an unconfirmed cancellation or protocol/
transport/local-output failure returns 255 and prints the stream ID plus whether
execution may continue. Local failures never send best-effort Cancel implicitly.
A second Ctrl-C ends the attachment without claiming remote termination.

## Cancellation and control lane

Hello, Open, Status and Cancel use envelope sequence 0, identified by exact request
body digest and stream binding. They do not consume or depend on data Exchange
sequence. Data exchanges begin at 1 and advance monotonically with exact-digest
retry. Open digest fixes argv/codec. Status/Cancel responses are current monotonic
snapshots bound to the request digest, rather than an immutable first-state cache;
late responses cannot regress terminal/cancellation/offset facts.

Cancel is an irreversible durable latch for the owning authenticated principal:

- Before Open: write a bounded cancelled tombstone. A later Open cannot launch.
  New tombstone admission obeys normal ID freshness, owner and retained quotas.
- During Preparing: invalidate the preparation revision and finish
  Cancelled-never-started without spawning.
- During Launching: set the latch immediately. Recheck after spawn returns;
  record child identity, then terminate any child that started.
- While Running: persist Cancelling before TERM. Wait cancel_grace, then KILL.
- After terminal completion: return actual terminal status, never rewrite success
  into cancellation or signal an already reaped process.

Confirmed cancellation means final group KILL, if needed, is sent while the owned
leader is unreaped; then reap the leader and commit terminal outcome. Never signal
`-pgid` after releasing leader ownership. If safe ownership cannot be confirmed,
report cancellation uncertain and retain credit; do not claim the process exited.

Cancel and Status retries are permitted despite publication ambiguity: their
logical operations are idempotent and cannot launch execution. Reuse exact logical
request bytes/digest; never invoke MDK send again for an already claimed ambiguous
publication. A new control publication attempt may contain the identical logical
operation even when the previous publication remains ambiguous, after inspecting
its native state and allocating a separately owned bounded attempt. Its app-event
publication identity is distinct (a later creation timestamp); the logical request
bytes/digest stay identical. Never invoke send a second time for the same claimed
publication. Retain each earlier obligation until its own ACK cleanup completes. Retrying Cancel cannot undo its
latch. `marmish cancel --stream ID DEST` works from another terminal through the
local record, including a Lost client record, until authenticated terminal status
is known. Administrative cancellation of a locally owned child is separately
available even when the remote grant was removed.

## Exchange scheduling and retention

Stop-and-wait initially provides roughly 8 KiB/raw round-trip per output channel;
measure this constraint honestly. Server replies immediately with currently
available bounded data/status; there is no held network request. After an exchange
that transfers bytes or advances EOF/terminal state, the client schedules the next
exchange immediately, subject to rate limits. Consecutive empty responses back off
250 ms, 500 ms, 1 s, then 2 s maximum. Local stdin readiness, EOF or Ctrl-C wakes
the scheduler immediately; waking cannot exceed server quotas. Five-minute
keepalive is a maximum silence interval, not a five-minute output poll interval.

Server new-Exchange token buckets allow eight/second per stream (burst eight),
32/second per principal (burst 32), 128/second globally (burst 128). Separate
control buckets allow two/second per stream (burst two), eight/second per principal
and 32/second globally. Existing logical retries are recognized before sequence
mutation, but still bounded by these request-rate budgets. Rate refusal does not
consume an Exchange sequence, launch a process or allocate an unbounded error
outbox. Reserve one queued Cancel control per stream independently of data credit.
At idle cap a stream emits at most one request and one response every two seconds
in steady state; delayed relay RTT reduces that rate further. Stream IPC returns
current state/empty data within 100 ms, well below its existing 15-second budget.

Persist the current/previous Exchange request digest and exact reply for bounded
retries; at most four ordinary slots per stream, plus one reserved Cancel slot.
Ordinary slots stop at 124 per principal and 992 globally; Cancel can use the
reserved capacity up to hard totals of 128 per principal and 1024 globally.
Each serialized envelope is <=32 KiB; expiry of reply
payload defaults to one day, configurable with payload retention. Within an active
stream prune only after validated next-sequence progress makes the previous slot
unnecessary; older retries fail explicitly. Never prune a pending native MDK
publication, queued intent, source or ACK obligation merely by age. These have
separate hard caps of 128 per principal and 1024 globally; exhaustion pauses new
exchanges/admissions, with reserved cancellation credit. Keep fixed-size terminal
launch fences after payload pruning. Existing finite limits are not enlarged.

Terminal slots and argv are reclaimed when the client has acknowledged final
output or configured payload retention expires. Final offsets and the last
Exchange digest survive in the compact record. Expired undrained output returns
Closed rather than fabricated bytes. Active and uncertain/Lost records never age
out. A client durably fingerprints Open before its first publication, so its
nonterminal Open intent or running attachment survives beyond seven days even
though only server records reserve active execution credit. Missing replies do
not prove that the remote did not execute it. Every other safely inactive fence,
including an abandoned Hello without an Open intent, can expire only
after its stream ID is older
than the seven-day admission window plus 60 seconds clock allowance and all
publication obligations have definitively drained. Older Opens then fail the
normal freshness check. Publication reconciliation prioritizes unresolved rows;
acknowledged or definitively failed history is scrubbed and bounded so it cannot
hide new work behind an oldest-row limit.

Authenticated admission refusal returns Capacity or Closed only when an existing
charged stream record can own the response. A fresh Hello refused at retained
capacity or while stopping is acknowledged and dropped without a response; its
client may time out. This deliberate limit prevents uncharged retry-ledger and
outbox rows from bypassing per-principal retention accounting. No unknown-stream
refusal allocates durable publication or retry state. Retention maintenance runs
at most once per 30 seconds; hot preparation and owned-runner progression inspect
only the bounded active/owned set. A sweep still processes the retained ledger
synchronously and can briefly delay the actor.
Retention sweeps run at most once every 30 seconds per owner; live runner and
publication progression continue on each actor tick.

Repeated local polls do not create publications. Each stream/frame digest has
one pending attempt, with retries gated by the previous attempt's ACK or proven
failure and 250/500/1000/2000 ms backoff measured from retirement. Attempts use
monotonically increasing publication timestamps within the normal future-clock
bound. Ambiguous ownership is never retired just to enable retry or make space.

## Native publication and IPC

Own every stream publication in SQLCipher before send; durably claim it before
runtime.send, bind queued intent IDs, and reconcile with MDK durable fanouts.
Ambiguous data send ends the attachment as TransportUncertain; it never relaunches
execution or blindly resends into MDK. A definitive transport failure may retry the
same logical frame under bounded stop-and-wait identity. Missing peer ACK is not
successful delivery merely because relays accepted publication. Journal published
evidence before native acknowledgement, confirm removal using unfiltered fanouts.

Extend publication ownership dispatch to stream events at every published, failed,
queued-refused and invalidation site. A protocol discriminator on shared prepared
publication machinery is preferred where validation remains closed and exact;
a separate store must implement the same complete obligations, not bypass them.
Incoming MDK sources are acknowledged only after bounded stream projection owns
the effect; startup Lost fencing prevents source replay from re-executing it.

Extend bounded same-UID IPC with stream open/exchange/status/cancel/list. Actor
operations inspect/enqueue state; never wait for pipes or executable lookup. The
CLI ingests stdin concurrently with output retrieval. stdout contains command
bytes only; diagnostics, stream IDs and optional JSON statistics use stderr or an
explicit file. Repeated local requests retain logical identity.

## Daemon stop and count drain

The daemon has no default 30-minute expiry: RawLimits::default sets max_uptime=None,
and ServiceLifetime::start(None) has no deadline. MAX_UPTIME=1800 seconds is the
maximum accepted explicitly configured uptime, not a forced lifetime. Preserve
that default and existing launch-origin/suspend-aware accounting. Existing
apply_limit_overrides supplies a 1800-second bound when max_commands is configured
without an uptime; preserve and disclose that count-limited test-service behavior.

Explicit Stop or configured uptime expiry stops Open admission, records
ServerStopping and applies bounded orderly cleanup: up to two seconds for terminal
output/control progress, then TERM/cancel_grace/KILL with owned-leader safety.
Commit ServerStopped only after confirmed cleanup; otherwise record cleanup
uncertainty, retaining the diagnostic distinction from normal command exit.
This is administrative shutdown, not client disconnect. Persist final state and
make a bounded attempt to publish it before stopping; no false delivery claim.

One stream Open reserves/counts one command against configured max_commands;
Hello, retries, controls and exchanges do not. Count-based shutdown waits for
combined finite/stream finished counts, zero reservations/active streams, idle
preparation/control work and all stream/finite publication/native ACK obligations.
Reaching the count stops new Open admission but never terminates an active stream
just to satisfy drain. Uptime expiry and explicit Stop are separate overrides.

## Phase 2a acceptance

Use native cat/sleep/yes or purpose-built helpers; rsync is not required here.
Validate forged sender/room/direction, additive restrictions, missing versus denied
executable, revocation stopping data without implicit execution cancellation,
duplicate/conflicting Open, cancelled-before-Open tombstones, Preparing/Launching
races, lost launch fence and no rerun, startup Lost with absent/unknown diagnostics,
and denial of unowned/reused-PID signals. Exercise FD hygiene in the multithreaded
daemon, TERM-ignoring children, descendant-held pipes and reap-last ordering.

Exercise simultaneous random/NUL bytes beyond 64 MiB under slow consumers, bounded
memory, EOF while output continues, stderr fairness, compression on/off, idle
message-rate limits, immediate stdin/interrupt wake, request/outbox caps, cache
pruning, old retries and native ambiguity/ownership/ACK cleanup. Disconnect and
idle timeout must preserve a demonstrably live child and quota; later explicit
cancel must work, including through a Lost client record after client-daemon
restart. Test configured uptime cleanup, unlimited default lifetime, combined
count drain and revocation/admin-stop behavior. Actual fresh-key relay execution
must return and hash real binary output, exit status and cancellation evidence.

## Phase 2b scope and acceptance

`marmish rsh [options] [-l NPUB] HOST PROGRAM ARGS...` implements rsync's remote
shell invocation with existing destination/relay resolution. Bare NPUB uses the
configured relays. `--compress` negotiates independent zlib blocks; default none.
Require rsync >=3 secluded-args (`-s`) and reject unsupported shell-command remote
paths before Open. Preserve argv literally, never shell-join. Phase 2a exposes
arbitrary streamed exec only through `marmish stream DEST -- PROGRAM ARGS...`.

Qualify actual rsync push/pull/update/delete, interrupted rerun, binary hashes,
spaces/quotes/backslashes/Unicode/newline filenames and restricted-rsync denial.
Record timings, raw/encoded bytes and measured throughput/latency. Three-platform
installed qualification follows in phase 4; local tests do not establish it.
