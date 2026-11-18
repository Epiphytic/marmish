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
provably affects only Peer stays Peer-local. The verifier does not
register, reset or repair the process default.

Provide a cheap synchronous admission check and an async wake from that
same sticky state. The source of truth is the fatal edge, not a queue item,
a pending graceful stop future, or the listener's receipt of a message.
Latch is one-way, wins over simultaneous ordinary stop before another
effect is admitted, and terminates read authority. Candidate consumers
receive only the checked default observer or the fatal signal as needed;
there is no path to construct a healthy registration from a flag.

The boundary is observation and admission, not atomic cancellation of
arbitrary external SQLite or kernel instructions. Check immediately before
each independently dispatched effect. Treat synchronous work already
admitted as one in-flight turn, including SQLite normal close of handles
already bound to their original VFS. Keep its ownership until it returns
and discard any late outcome. A socket write already admitted to the
kernel may reach the relay after the latch. Require no new write admission
from the process, and measure against deterministic held-before-write
cuts. Do not hold registry locks over SQL, I/O, socket writes or joins.
No new transaction, maintenance, recovery, publication, or spawn turn may
be started by shutdown. Do not relabel a result as durably committed or
clean based on reverse inference from the fatal exit.

## Call sites and physical order

1. **Before constructing owners.** Obtain the witness from the exact
   registered forwarding entry. Retain the signal across the existing
   open path, including before native MdkOwner/Journal open and engine
   thread start, so an early indeterminate result drops only resources
   already constructed. For a worker that checks after thread spawn and
   before database open, a fatal signal exits without opening. Re-check
   independent of a Peer turn at ordinary startup and ordinary owner
   admission. Never turn indeterminate into the untouched-stock path.

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
   no `Control::Stop` and does not call `collect` in shutdown. A worker
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
   Abort producer pumps before awaiting drop/physical disconnect. Cover
   both ordinary connectors and each Terminal child connector before
   WebSocket upgrade, using the same candidate fatal signal. Enforce the
   signal at actual underlying async write/flush/close admission for
   registered candidates so a buffered SDK frame, AUTH, subscription
   CLOSE, heartbeat or retry cannot add bytes. Socket ownership may be
   reaped without sending a protocol frame. Raw Peer stop similarly
   retires and drops its actual socket without subscription CLOSE when
   fatal. Normal optional Peer/Tor/Terminal policy and normal shutdown
   are unchanged. No new generic transport policy or relay configuration.

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
   physical-only path. Reuse the signal before each graceful stage and
   cancel sleeping async stages; no fatal-mode fallback to graceful
   cleanup. Stock and a healthy candidate still use ordinary shutdown.

## Qualification

Exercise disposable child processes so an actual SQLite default
registration replacement cannot leak to the test runner. Use a fixture
VFS of known stock ancestry and return to the original profile only from
fresh processes. Use deterministic pause points outside the admission
checks and counters at the actual SQL/file, worker, journal and socket
boundary as applicable, not just a mock dispatcher ack.

1. Stop a real engine thread with a nonempty FIFO (CopyGc and cleanup),
   live preparation and queued finite operation at a barrier immediately
   before dispatch. Actually displace the default, release and prove zero
   new worker effects and bounded exit. A disposable authorized copy
   reclamation sentinel must remain. Exercise the blocked event sender
   and before-storage-open cut using the same machinery.
2. Pause one admitted operation and a late completion. After fatal stop,
   hold that real worker and require acquisition of the same lease to fail.
   Release, prove exact joins, no completion-driven journal/relay effects,
   no stale IPC response, and require successful new lease acquisition.
   Exercise an already authenticated finite job in its separately exec'd
   worker: it completes after daemon fencing, and a fresh safe process
   recovers its exact durable exit/output without duplicate execution.
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
   for frames held before the boundary.
4. Keep a pre-owner indeterminate refusal and a controlled Peer-local
   loss with the exact default still valid. The latter must keep real
   ordinary Command alive. Run existing ordinary stop, finite survival
   and Terminal detach/rejoin checks in stock and healthy candidate
   configurations. Record fatal versus ordinary outcomes separately.
   Use existing fixtures and scoped tooling. Full installation/actor
   mounting remains part of the parent integration gate.

Implementation ownership should follow the existing lifecycle: one owner
for the signal, engine and keeper, and one for daemon/IPC and Terminal
transport cancellation using that interface. Do not start implementation
until the exact design is independently approved.
