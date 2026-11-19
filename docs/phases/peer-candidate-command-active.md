# Admit active Command turns on a registered candidate

This design refines [daemon composition](peer-candidate-daemon-composition.md)
for the private Linux AMD64 Command qualification. Source base `c990cf9`
combines the checked native owner at `c033631` with the reviewed
[Command startup adapter](peer-candidate-command-startup.md) at `a992a18`.
See also the outer run admission plan at
`e072a5a:docs/phases/peer-candidate-run-admissions.md`. Use the existing candidate build gate and track composition in
`marmish-dwt.4.12.2`. This phase defines owner-side turns and their required
caller contract. Production mounting remains a separate reviewed composition.

The caller uses an explicit allowlist for Status, authorized ClientInventory
and the normal finite requests CommandPrepare, CommandSubmit, CommandStatus,
CommandCancel, CommandPoll, CommandResponse, CommandCursors,
CommandUnconsumed, CommandInvocations and CommandConsume.
`Request::is_command()` also classifies Stream and all four acceptance-fixture
operations as Command traffic. Those fixture operations include
CommandPrepareCleanup, which can initiate graceful cleanup. Exclude all four,
generic channels, Stream intake, Terminal attach and Peer before candidate
owner work. Stock keeps every existing operation. A profile may
retain generic, Command and Stream rows, sessions and pending transport
obligations from stock operation. Shared maintenance must still guard these
operations. Transport does not become another profile's policy and there is
no new purpose, native format, store migration, or fresh replay ciphertext.

## Authority and execution boundary

- The registered daemon derives one owned Command `Admission` from its
  original registration. Use clones from that exact value for startup,
  the engine and the active owner. Stock takes the existing path. The
  candidate owner retains the checked admission associated with the owner
  as it moves out of startup custody. Do not add a setter callable on a
  stock or already active owner. The eventual caller retains the same
  profile lease, the read-only `Fatal`, and the selected physical custody.
- Full `Admission::admit()` takes the registry mutex, validates the exact
  installed descriptor and calls `sqlite3_vfs_find`. Observing displacement
  trips the existing fatal latch. Use full admission for the independent
  effect starts and result exposure below. Use an atomic latch check when
  a resumed wait cannot initiate native work or hand off new authority,
  such as waiting for SDK acquisition results or an already finished
  forwarder. Every poll of a call that can touch unfenced MDK storage uses
  full admission, including re-entry after suspension. The next wake
  can be the first observer of default displacement. An unset latch
  does not prove default continuity.
- Use one lazy poll wrapper for these boundaries and a small synchronous
  check. Select full polls or latch-only waits at the call sites. Keep
  locks outside downstream bodies. Return a distinct cut before flattening
  an ordinary inner result. Use the fixed owner storage category
  `candidate_dispatcher_lost` when refusing or when a recheck finds loss
  after a result. In particular, apply this before `activate`'s KeyPackage
  and transport mappings, `receive_one`'s delivery-close and ingest mappings,
  `mdk(effects)`, and `tick_inner`'s subscription mapping. Journal or adapter
  refusal may arrive wrapped as storage or transport failure. Preserve
  healthy error classification and never continue to bookkeeping, ACK,
  compensation or reply on a cut. Caller mappings are below.
- One admitted synchronous operation or upstream poll may finish. No lock
  spans an await. Add no watcher, task, sleep, executor or MDK fork. Keep
  stock checks as no-ops and preserve current deadlines and budgets. The
  existing `timeout_at` cancellation precedent informs where to test
  durable reopen. It does not prove that every future can be canceled
  safely or that dropping a future ended raw I/O.

| Effect | Existing protection and additional admission |
| --- | --- |
| Journal and Command engine SQL | Use existing `ConnectionFence` and checked worker mode on their original connections. Keep a full check at the next independent Journal or native effect, without duplicating full verification immediately before already-fenced operations inside an admitted effect. |
| Native account state | `open_encrypted_observed` retains `AccountCustody` but does not fence use of the live native connection. Use full owner admission before each independent native call and each native-bearing poll, including after preceding journal or routing work. An admitted native poll may finish. |
| MDK sends and SDK work | Keep the independent raw fenced connector and physical cut. Check each next SDK publication/subscription entering `PublicationGate` within MDK independently. KeyPackage uses `Publisher`. Check before preparation and independently before sending signed bytes. Bind both once from the same candidate-constructor admission. Preserve recovery pause and ordinary transport behavior. Before-send refusals preserve the obligation, with an uncertain disposition after entering a send. |
| Owner state and adapter ACK | Neither raw transport nor SQL protection covers retry bookkeeping, route projections, replay ACK, ready markers, response queues or status caches. Check each independent boundary using the result, retaining the resources an admitted effect already returned before a subsequent check. |

## Concrete checkpoints

| Owner path | Admission boundary |
| --- | --- |
| Native and open-time | Keep `native_preflight::check`, `MdkOwner::open_candidate` and its checked connection and startup custody sequence from the native phase. Keep the Ready binding check and exact lease. Admit `reconcile_client_inventory` before its SQL turn and apply `set_retention` as ordinary in-memory setup before first maintenance. Independently admit the startup adapter's history, configure, binding, recovery and worker entry. No second preflight, identity, connector, registry or journal. |
| `activate` | Guard bootstrap, notification forwarder creation (retain handle immediately), each configured relay's connect turn, KeyPackage republish, `activate_transport`, first native drain, application effect projection, `replay_pending`, `submit_unsubmitted` and final Active. Route connect waits and native calls recheck after suspension. No successful status, parent readiness, or external listener authority is committed merely because the transport connected. |
| Status/exit/readiness | Guard `command_count_exit_ready` before its synchronous Journal/native read set, `status` before SQL, and independently check before exposing the result, honoring count exit or declaring Active. Keep the original `ServiceLifetime` and signal/limit ordering. Listener status cache and queued replies independently recheck at the actual IPC write poll even when `serve_owner` is scheduled out. Preserve explanatory activating/degraded state on healthy paths, with no stale success after fatal. See reporting below. |
| `dispatch` | Guard `client_inventory`, admitted `command_local` and Busy-to-`command_enqueue_preparation`. Cover submit/consume/poll/cancel. Only stock retains `command_acceptance` operations in this slice. Atomic, synchronous nested reads/writes in one admitted operation may finish. Check again before a separately selected mutation or submission and before returning a response. Keep short exchange expiry, ownership validation and exact retry behavior. |
| `tick_inner` | Guard each existing Command-completion/Stream-progress pass, source reconciliation, retention, route rebuild and independent recovery. Split recovery at forwarder replacement, each SDK reconnect initiation, reactivation, catch-up receive and `PublicationGate::resume`. Keep retries and the existing retained forwarder on an unfinished dial. Guard subscription synchronization, native drain, effect projection, receive, retained work and each publication pass. Recheck before the second completion/maintenance pass. Never start any of them because an earlier health result was good. |
| `receive_one`, `apply_effects` | Separate wait/read, native ingest, projection, adapter replay ACK and transport-group sync. After ingest or projection returns, begin neither ACK nor sync without a fresh check. Durable projection stays ahead of its corresponding ACK and preserves storage backpressure/error typing. Between independently processed records or batches, recheck before the next projection, pending deletion or invalidation write, including any command/stream/ordinary branch. |
| `command_open_room_turn`, `create_command_room` | Separate orphan reconciliation and selection, each deliberate acquisition/discovery drive, original token reservation and native `create_group`. After discovery cannot create based on an earlier admission. Post-native `accept_command_group`, any conditional forget/confirm, projection and subscription sync each need their own admission. If cut after preparation, ID, token and native interpretation remain subject to existing restart reconciliation. No fresh room, Welcome, or retry just because a result was interrupted. |
| Retained replay and retention | In `replay_pending` guard subsequent row disposition, in `drive_retained` each orphan or original Welcome redelivery and targeted `advance_application_convergence`. Separate existing Journal claim from native send, later bind/project, and each later row in `submit_unsubmitted`, `submit_command_publications` and `submit_stream_publications`. Distinguish native read, journal claim and nondurable retry bookkeeping from new sends. Protect fanout/queued intents/native invalidation witnesses and retain their normal order. In `maintain_journal` recheck before invalidation writes, Command maintenance and general maintain, and in `progress_streams` before preparation, completion/runner launch, observation and reclamation. Do not compensate or ACK on refused admission. |

Pinned upstream MDK `bdf9833` illustrates why guarding only the outer daemon
method fails. `AccountDeviceRuntime::republish_key_package` may fall back
to fresh publication, `drain` drives already queued output and retained
fanouts, `create_group` awaits native creation then publication, and
`advance_convergence` awaits exact fanout retry before newer convergence.
Treat the polls as native-bearing and make the next transport check when
control enters a Marmish callback. A finding that a required effect escapes
that ownership boundary blocks mounting until addressed. It does not justify
treating these APIs as synchronous or weakening the promised cut.

## Caller contract for the later mount

The stock `serve_owner` at the source base has no exact fatal branch. The
active owner is tested unmounted. Mounting is conditional on the caller
wiring all of the following:

- Derive one read-only `Fatal` from the exact registration and bias its
  branch first in activation, dispatch, tick, room work, reporting and
  ordinary stop. Retain the original lease, owned native/worker/relay
  resources and `ServiceLifetime` through disposition. Use the explicit
  request allowlist above before registered dispatch. Stock and existing
  fixtures keep their handlers.
- Check fatal or typed cut before classifying owner results, including
  dispatch `unwrap_or_else(failure)`, tick's transport and timeout
  degradation, and the room retry cache's generic storage arm. Check
  dispatch again before it queues a response. Fatal prevents even a
  generic fallback reply. Require a current check before
  `status.send_replace`, successful count exit, ordinary-ready and
  parent reporting. The listener's last write poll must independently
  suppress stale Status and Command replies while the owner is suspended.
  The owner phase leaves framed IPC untouched.
- For the detached reporter use an owned nonblocking candidate success
  path on the same validated channel, with its existing timeout. Take
  the one stream before changing its mode so the retained stock blocking
  `failed()` path can still handle errors before the take. Check before
  the actual R-byte write poll and before accepting ACK. Once the stream
  is taken, an interrupted success writer drops it and has no second
  `failed()` turn. Before an R write the parent sees EOF/startup failure
  if no ordinary failure arrived. After an admitted R the parent can
  already have seen it and sent A. A cut before ACK acceptance prevents
  successful child activation. The existing wire exchange cannot retract
  bytes or a previously returned parent result. Test these separately.
  Cancelling the stock `spawn_blocking` await leaves its writer running.
- A fatal result revokes intake and follows the reviewed physical custody
  path. Separately prove that known tasks, socket work and workers drained
  or park unproved custody with the original lease. Graceful deactivation,
  Journal advances, SQLCipher close and runtime-tree removal remain
  forbidden under recorded loss. The owner phase adds no reaper or listener.

## Owner phase exit

- Use a small test-only effect observer analogous to `StartupMode::probe`.
  Sweep adjacent cut steps around each independent owner decision in
  activation, count/status and dispatch, room work, delivery and each tick
  phase. For resumed native-bearing futures stop while the poll is pending,
  displace the registered default before their next wake and prove refused
  re-entry into the native call. Also exercise already-tripped latch-only
  waits. Compare durable rows/native evidence, outgoing counts and adapter
  ACKs with the actual last admitted operation. Assert that each typed loss
  mapping returns the fixed owner storage category.
- Exercise keyed durable state for original retained Welcome, generic and
  Stream publication, and a claimed Command publication. Verify exact IDs
  and retained bytes across healthy reopen, and that the next operation
  resumes interrupted ownership without fresh ciphertext for that obligation.
  Confirm call boundary coverage and unchanged normal finite Command
  behavior in stock. A timeout alone is insufficient restart evidence.
- Run exact candidate and stock strict checks and get independent Claude
  Code Auto review of the owner source. Mounting and release are outside
  this exit.

## Composed mount exit

Use disposable private WSS, real registration and the same-UID listener.
Run one finite byte-preserving Command and a clean restart. Exercise decisive
default displacement after a KeyPackage ACK and before the next native poll,
before/after native ingest with no subsequent replay ACK, between room
discovery and prepare/create, during a retained publication, after a queued
local reply, before detached R and while waiting for A. Record the first
observer of the cut, unchanged post-cut evidence, actual worker/socket join
or explicit held custody, and bounded latency. Include Status and Command
reply races and the healthy stock comparison. Keep composed tests opt-in.
