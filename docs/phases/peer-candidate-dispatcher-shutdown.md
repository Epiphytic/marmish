# Physical daemon shutdown after candidate dispatcher loss

Scope is Linux AMD64 under `marmish_peer_receiver_candidate`, only on the
registered candidate branch. Base `a036b5a`; the separately reviewed Manual
actor design is `b96e4ce`. This increment closes the fatal lifecycle required
by that design. It does not mount the actor, change stock behavior, start
another process or listener, or change finite-command retention. Bead
`marmish-dwt.4.12.2` tracks the work.

## Existing shutdown cannot handle loss of the default dispatcher

- `command_engine::CommandEngine::shutdown` queues `Control::Stop` after
  CopyGc, Cleanup and other controls. `Worker::run` reads the control FIFO
  first and may next run one live preparation and one finite operation. A
  front-end stopping flag does not stop code already queued on that thread.
  The engine may also still be opening its database on its dedicated thread.
- `command_lifecycle::stop_owned` calls `MdkOwner::stream_begin_stop` before
  handing it to `Frontend::stop`. Stream stop reads/writes the journal,
  progresses late engine results, publishes buffered replies and deactivates
  the MDK account. These are graceful operations.
- `daemon::run_inner` currently waits for Terminal/Peer and the last Arc
  before calling ordinary Command stop. Terminal's `Runtime::join` starts
  its stop only when polled. A live Terminal lane's ordinary close calls
  MDK deactivation before reaping its connector. The established WebSocket
  sink checks connector cancellation on new message conversion; that alone
  leaves already buffered flushes and SDK shutdown frames to consider.
- Dropping `MdkOwner` cancels its connector and aborts its forwarder, but no
  explicit join is observed there. Dropping a thread or task handle also
  does not establish that it stopped. The ordinary keeper retains the
  profile through the worker join, frontend drop, and stream barriers.

## One fatal cut

Create one process-local sticky signal for this exact registered candidate
daemon lifetime. Retain the registration's read-only default verifier
before attach consumes the registration witness. Its failed recheck
(default displaced, descriptor unverifiable, or registry poisoned) latches
the signal before returning the typed indeterminate outcome. Loss that
provably affects only Peer stays Peer-local. Only the exact registered observer can set this signal. It sets the latch
under the same registry guard before exposing `State::Indeterminate`.
The poisoned-lock path uses the guard returned in the poison error to
latch before releasing it, without recovering the registry as usable.
Installing or observing an already indeterminate registry cannot return
ordinary authority. The verifier never resets or repairs the default.

Provide a cheap synchronous admission check and an async wake from that
same sticky state. The source of truth is the fatal edge, not a queue item,
a pending graceful stop future, or the listener's receipt of a message.
Latch is one-way, wins over simultaneous ordinary stop before another
effect is admitted, and terminates read authority. Candidate consumers
receive only the checked default observer or the fatal signal as needed;
there is no path to construct a healthy registration from a flag.

The boundary is observation and admission. An already-admitted
synchronous callback can finish, including its internal SQLite operations.
The race between checked admission and its first machine instruction
remains. Keep such ownership until the call returns and discard any late
outcome without acknowledging or redispatching it. A write admitted to
the kernel before the latch may arrive later. Do not hold registry locks
over effect bodies, SQL, I/O or joins.

### Check exact admission sites

| Site | Admission after registration | Already admitted when fatal |
| --- | --- | --- |
| `daemon::open_inner` / `MdkOwner::open`, explicit MDK encrypted smoke open and reopen, native Journal open, Terminal `Registry::open_at` | Exact default recheck before each SQLite connection/opening transaction. Gate profile and Terminal initialization file creation at the owning start boundary. | Take ownership of any newly returned storage before another turn. |
| `CommandEngine::start_internal` / Engine storage and copy helper registration; `marmish-engine::cipher` keyed open callers | Exact recheck before opening or registering. Recheck again at any independently invoked connection open inside a daemon-owned turn. | Close no newly returned store by an implicit owner drop. A separately exec'd finite worker initializes its own SQLite default and is outside this process's fence. |
| `Worker::run`: live prepare, finite execute, CopyGc, Cleanup and queued acknowledgements | Exact default recheck at effect dispatch; memory-only queue bookkeeping checks the latch. Where an effect opens another connection independently, run the exact recheck at that opener. | A synchronous body can return; no later preparation, effect or completion admission. Finish any already admitted finite handoff before discarding. |
| `serve_owner` / `MdkOwner`: status, idle/count exit, activation, Command dispatch, journal tick/recovery/maintenance, room intents and normal shutdown | Exact recheck on each stateful turn and its separately resumed SQL/effect steps; the latch alone is suitable for read-only in-memory bookkeeping/reply revocation. | Bound synchronous operations may return. Pending async progress is canceled; phase owners retain any acquired handle. |
| Terminal native registry/mint or bind, enrollment/lane open or close, tmp/attachment setup and supervisor requests | Exact recheck at independent SQLite or durable-file/process creation/mutation turns, and at corresponding connection opens. Existing reply/attachment and relay fences consume the latch. | Close attachment/runners via owned control only. Do not delete a tmux session or use an MDK deactivate effect. |
| Peer actor's cursor, stage/native, SQLCipher quota recovery and durable-file/SQL turns | Keep the reviewed exact managed rechecks; every independent new SQLite connection rechecks its default first. Apply the same latch to borrowed turns and outward wakes, without releasing quota authority as reusable. | Keep protected storage evidence and cancel or drop the raw port physically. |
| All candidate sockets below TLS; IPC status/replies; asynchronous completions | Recheck the latch at poll/reply admission. The transport layer has no SQL or default-repair authority. | The test distinguishes an already admitted socket syscall from a canceled pending write. |

This inventory covers opens, file effects, transaction/maintenance turns
and transport with the same narrow capability. Sign off the exact open and
close inventory in code review, including helper or error branches before
a complete owner is assembled. A Tokio select solely around a long owner
future does not guard separate effects during an individual poll. Reuse
the immediate per-turn checks where they already exist. The previous
admitted synchronous call's rollback/error return is part of that call,
not permission for a new shutdown transaction.

### SQLite and shutdown custody

A connection's ordinary destructor is a separate turn. Closing can
checkpoint WAL or delete sidecars even when no new SQL is submitted.
Only resources with individually proven trusted close callbacks and a
reviewed exact set of permissible close-side effects may close in Rust
after fatal. This increment uses the smaller common rule: park retained
SQLite-bearing roots from the daemon, engine, Terminal registry or lane,
and Peer on fatal, regardless of reason. Ordinary executable code
cannot recover these roots. After actual streams, tasks, pumps, socket
halves and thread/process handles are reaped, suppress SQLite
destructors on the parked roots. Retain the original profile lease with
them until the candidate process exits with a failure. No startup reset,
same-process re-init or reassignment of those resources. The process
boundary then closes idle OS descriptors without running SQLite
checkpoint/delete logic. Normal closure and runtime cleanup remain in
the trusted path.

Take sockets, producers and live child owners out before parking any
opaque SQLite roots and stop their active tasks. If they cannot be
separated using the owning boundary, report incomplete rather than
treat a task-bearing root as quiescent. A held preparation record or
completion may be discarded only after proving it carries no live
`ChildOwner`; `ChildOwner::drop` can kill an unreaped child. Split
stream cancellation from successful finite exec into its independent
worker at the already established handoff. Do not use bare `_exit` as
the fence: current runner children do not use `PDEATHSIG`.

## Call sites and physical order

1. **Before constructing owners.** Obtain the witness from the exact
   registered forwarding entry. Retain the signal across the existing
   open path, including before native MdkOwner/Journal open and engine
   thread start, so an early indeterminate result drops only resources
   already constructed. For a worker that checks after thread spawn and
   before database open, a fatal signal exits without opening. Re-check
   independent of a Peer turn at ordinary startup and ordinary owner
   admission. Before registering, require the effective ordinary relay
   policy to have `!RelayPolicy::needs_tor()`, including the resolved
   config/CLI route overrides used by the actual connector. `Auto` with
   an onion remains a stock route when the original candidate profile
   was untouched. Do not register and then fall back. Arti owns
   independent SQL-backed caches and has no reviewed physical fence
   here. This does not change Tor or ordinary relay defaults in stock.
   Never turn indeterminate into the untouched-stock path.

2. **Revoke input and views.** In the existing `serve_owner` and
   `ipc::serve_with_listener` use the existing drain/join shape for the
   candidate, with fatal wake biased before dispatch, tick, room intents,
   Terminal binding and status SQL. Select it while activation, dispatch,
   tick, or room creation is pending; do not repoll a canceled future.
   Check the fatal state immediately before a synchronous branch and
   before publishing a response. Stop the read producer and cancel the
   existing listener's request tasks. Close/drop its Command receive queue
   and Terminal binding intake, discarding entries. Track one listener join
   and report a panic as incomplete. Any accepted request waiting for an
   effect either sees the fatal/uncertain disposition or loses its socket;
   never emit a successful stale reply.

3. **Stop the Command worker out of band.** Pass the optional fatal fence
   into the existing worker constructor before spawn. Check before engine
   open and registration, each control after receive (including CopyGc
   and Cleanup), live preparation after receive and before effect, each
   selected finite operation at execute, and before returning a completion.
   Revalidate the exact forwarding witness at these effect admissions,
   including initial storage open. The queue, pending vector, events and
   completions may be discarded on this path without acknowledgements or
   redispatch. Add a physical join on the existing engine handle that sends
   no `Control::Stop` and does not call `collect` in shutdown. The
   OS join remains on the existing blocking keeper. A worker
   blocked sending an event must be released by disconnecting its receiver
   before join. Never use `Control::Cleanup` or `into_cleanup` here. If
   an already admitted finite startup completed, its separately exec'd
   worker retains normal responsibility for the job. Do not cancel it,
   modify its database rows, revoke its process group, or launch a
   replacement from the fatal path.

4. **Close live children and transports before waiting.** From MdkOwner
   take the engine and capture the existing `stream_runner_barriers`
   (in-memory only). Close Command admission, retire terminal grants, call
   each runner's `suspend_input` and `cancel`, and observe its existing
   physical `finished` proof. Do not call stream progress or a cancellation
   journal write. Signal Terminal stop synchronously via its existing
   transport stop cell, so join need not be polled first. Gate new
   `TerminalStart`, listener opening, binder returns, and room/attachment
   work using the same fatal signal. Let the existing native supervisor
   stop and reap owned attachment resources, preserving the normal tmux
   session retention policy.

   Use the existing MdkOwner connector cancellation, forwarder abort and
   observed join, existing SDK/connector disconnect and Terminal reaper
   barriers. In the fatal-only branch skip MDK account deactivation (also
   in Terminal `Lane::close_inner`, including the alias-detach path).
   Abort producer pumps before awaiting physical disconnect. Candidate
   ordinary and Terminal connectors cancel the token before awaiting
   SDK stop, while keeping their per-route socket semaphores open.
   Apply the existing `reap_terminal_socket` proof to every ordinary
   route, using the existing permit carried by both socket halves, and
   close semaphores only after acquiring all the slots. Do not use
   ordinary `cancel()` first, which closes semaphores and prevents this
   proof. Raw Peer retire/drop owns its actual socket and skips
   subscription CLOSE.

   At the `RelayConnector::connect_stream` direct stream handoff wrap
   below TLS and before WebSocket upgrade. The ordinary connector,
   Terminal child connectors, and raw Peer live connector all use that
   handoff. Gate each `poll_write`, `poll_write_vectored`, `poll_flush`
   and `poll_shutdown` and register the pending poll's wake on the
   latch before forwarding. After fatal, return I/O error without
   forwarding even if TCP never becomes writable. This covers buffered
   SDK output, reads that flush tungstenite Pong/Close, TLS
   close_notify, retry and `Lane::drop`'s async `stop_lane` cleanup.
   Race the cancellable connect/open against fatal before acquiring
   each socket. No generic transport policy is added.

5. **Retain real ownership.** Initiate the cut for all owners before
   awaiting any individual join. Reuse the existing keeper/profile
   retention pattern and actual thread/runner/Terminal reaper barriers:
   the last lease owner outlives the engine join, frontend/connector
   destruction, listener drain, live runners and actual Terminal/Peer
   owner completion. A stalled callback or runner delays release; observer
   timeout is diagnostic and incomplete. Retain private evidence, skip
   `ProfileLease::cleanup_runtime` and do not run ordinary
   `command_lifecycle::Frontend::stop`. Report typed indeterminate/fatal,
   without stock clean report. If ordinary stop started while forwarding
   was trusted and the same candidate becomes fatal before it finishes,
   disallow subsequent journal/network progress and converge to the same
   physical-only path. Stock and a healthy candidate still use ordinary
   shutdown. Existing graceful helpers must share the fence after they
   take ownership; the daemon cannot retrieve MDK or the engine from
   spawned helpers once `stop_owned` transfers it.

| Ordinary stage already owning resources | Fatal transition |
| --- | --- |
| `stop_owned` before stream checks and `stream_begin_stop` | Transfer only memory-owned barriers and owners; begin no further journal turn. |
| `Frontend::stop` runner grace, `progress_streams`, drain/receive, stream stop publication | Wake/cancel pending work, skip all further progress or publication and run the physical frontend teardown. |
| `MdkOwner::shutdown` including deactivate | Skip or cancel deactivate. Observe connector and actual forwarder/reaper stop and park storage roots, preserving owners still needed to join. |
| `start_keeper` inside FIFO `CommandEngine::shutdown` or awaiting frontend completion | Drop the event receiver to release a blocked send. Switch to the out-of-band engine join, keep runner proofs, and skip `cleanup_runtime`. Prior queued Stop carries no authority to do any work. |
| Final `stop_reporting` and caller, including clean keeper completion before final Arc destruction | Recheck fatal state before the success report or releasing the final lease. Fatal wins even if prior graceful stages happened to finish. |

## Qualification

Exercise disposable child processes so an actual SQLite default
registration replacement cannot leak to the test runner. Use a fixture
VFS of known stock ancestry and return to the original profile only from
fresh processes. Use deterministic pause points outside the admission
checks and counters at the actual SQL/file, worker, journal and socket
boundary as applicable. Test-only SQL counters count actual `xOpen`,
`xWrite`, `xDelete` and `xSync` at the displacing VFS and the verified
forwarding/base callbacks; include writes attributable to connection
close and monitor backing-file evidence. In cuts with no in-flight
effect, all post-latch callback counts are zero. For a gated previously
admitted effect, count its named in-flight body separately and prove no
later callback, untrusted open, checkpoint, sync or delete from
shutdown. Count actual execute/live prepare/CopyGc/cleanup body entry,
process spawn, socket poll admissions and relay-received frames or bytes
independently of the checked fence.

1. Stop a real engine thread with a nonempty FIFO (CopyGc and cleanup),
   live preparation and queued finite operation at a barrier immediately
   before dispatch. Actually displace the default, release and prove zero
   new worker effects and bounded exit. A disposable authorized copy
   reclamation sentinel must remain. Exercise the blocked event sender
   and before-storage-open cut using the same machinery.
2. Pause one admitted operation and a late completion. After fatal stop,
   hold that real worker and require acquisition of the same lease to fail.
   Release, prove exact joins, no completion-driven journal/relay effects,
   no stale IPC response. On the fatal process path parked storage keeps
   that lease unavailable until process exit. Require successful lease
   acquisition from the fresh process after exit.
   Exercise an already authenticated finite job in its separately exec'd
   worker: it completes after daemon fencing, and a fresh safe process
   recovers its exact durable exit/output without duplicate execution.
   Fresh recovery of a fatal-interrupted live stream reports uncertainty
   without a false clean/complete record or a replacement runner.
3. On the existing real same-UID socket, queue Command and Terminal
   work and keep a live stream and an installed Terminal attachment.
   Use one private WSS fixture to stage an SDK write before its final
   transport check and one ordinary publication awaiting a remote reply.
   Trigger the same fatal edge and require listener closure, stopped
   read view, no newly admitted frame including protocol cleanup frames,
   observed physically finished stream and attachment owner/reapers,
   and no effect from late engine results. Retain the lease during a
   deliberately gated runner or reaper. Do not claim tmux session deletion
   as attachment cleanup. Measure actual close/join outcomes; compare
   publication attempts at the transport boundary and delivered bytes
   for frames held before the boundary. Also block a real socket write
   pending behind full buffers, latch, require it to wake without further
   write admission, and observe the Terminal lane's drop/reaper path.
4. Keep a pre-owner indeterminate refusal and a controlled Peer-local
   loss with the exact default still valid. The latter must keep real
   ordinary Command alive. Include fatal while graceful Frontend/keeper
   already owns resources and immediately before the final report; use a
   mixed `Auto` + onion configuration to prove ineligible-before-register
   without opening a Tor connection. Run existing ordinary stop, finite survival
   and Terminal detach/rejoin checks in stock and healthy candidate
   configurations. Record fatal versus ordinary outcomes separately.
   Use existing fixtures and scoped tooling. Full installation/actor
   mounting remains part of the parent integration gate.

Implementation ownership should follow the existing lifecycle: one owner
for the signal, engine and keeper, and one for daemon/IPC and Terminal
transport cancellation using that interface. Do not start implementation
until the exact design is independently approved.
