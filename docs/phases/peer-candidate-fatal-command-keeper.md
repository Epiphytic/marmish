# Command worker and keeper on candidate dispatcher loss

This is the next unmounted increment of the approved
[physical shutdown design](peer-candidate-dispatcher-shutdown.md), after the
sticky transport primitive at `121dc73` and evidence at `617a40f`. Bead
`marmish-dwt.4.12.2` tracks composition. Scope is the ordinary Command
worker and its blocking keeper. The daemon, IPC, native Terminal, physical
Peer route, and registry remain owned by their existing slices.

## Worker contract

- Keep `CommandEngine::start` and stock callers unchanged. Compile the dormant
  candidate constructor under the `dispatcher_fatal` condition (Linux x86_64
  and either `test` or `marmish_peer_receiver_candidate`). It accepts a typed
  `Admission` that combines the shared `Fatal` and a bounded synchronous check.
  The eventual `ForwardingVerifier` plus `Trip` must perform recheck/latch in
  one registry-guarded observation. That function cannot reenter engine/SQLite
  or perform I/O. Worker admission fails on a refusal or a fatal signal before
  or during the check. The frontend keeps the same latch for cheap memory-only
  gates. No installed entry point can construct this path.
- On the worker thread, gate before the engine open and before each independently
  received control, live preparation, finite execute, and copy helper
  registration. Check the latch before idle wait and additional queue staging;
  keep the current 2 ms idle poll. Guard every event send and late result before
  mutating `known`, `unresolved` or `awaiting`. An early fatal open emits neither
  Ready nor OpenFailed. Zeroize the master and drop unused memory/receivers.
  Returned success from an already-admitted open takes the checked owner before
  Ready or helper work. An error after fatal emits no OpenFailed; implicit
  partial-store close inside the open remains a mount prerequisite.
- The frontend checks the latch on collection and before exposing cached
  completions, readiness, cleanup, and prepared observations. Refuse new queue
  entries and acks on each method, including CopyGc, live prepare, finite work,
  cleanup request and cleanup-failure acknowledgement; refuse bootstrap-retire
  inference from cached readiness. Drop queued/memory records without ACKs.
  `PreparedLiveExec` owns descriptors (`ChildSpec`/`OwnedCopyContext`) for a
  future spawn and has no `ChildOwner`. `EngineCompletion` owns observations.
  A separately exec'd finite worker remains independently recoverable.
- A scoped worker owner guards its `Engine` and `CleanupGuard` slots. Its own
  `Drop` runs exact checked admission once, including Stop, disconnected input,
  failed event send, panic, and Ready-send failure. Refusal parks those exact
  slots with no destructor; success admits the normal close of those slots.
  Move a newly returned Engine immediately into this owner before any other
  stateful operation or send. `Engine::into_cleanup` would move the Engine out
  of these slots and can drop its store on success or error. Candidate Cleanup
  is refused before move in this unmounted leaf until the connection guard
  covers both close paths. Stock Cleanup uses its existing ordinary path. This
  intentionally does not claim full healthy candidate shutdown readiness.

## Stop race and physical join

Continue to use the current stock `shutdown` and blocking keeper for stock
callers. The dormant candidate stop driver gets a typed `StopOutcome::Fatal`
when its shared latch wins before `Stop` submission, while waiting for queue
room or thread completion, or before yielding a completed ordinary report.
Recheck at those boundaries and again immediately before returning a report.
The existing public stock `shutdown` may keep its original signature and fail
closed if given the dormant kind by a future miswire. Do not let a typed fatal
result pass into the stock keeper's retry/cleanup loop. At actual composition,
the candidate lifecycle replaces its stop observer with this typed outcome and
chooses the fatal path once; the rest of frontend/daemon convergence remains an
explicit dependency of the first mount.

Once the signal has tripped, `CommandEngine::into_fatal_join(self)` consumes
only the original instance and extracts a one-use thread join. Disconnect all
request/control senders and the event receiver before exposing the join. Dropping
the receiver wakes a full synchronous send, including one admitted just before
the cut. This handoff does not collect events or enqueue a stop marker. Its
result is typed joined, or failed if the worker panicked. If a previous
ordinary stop attempt already consumed the OS handle on panic, preserve that
failed result in the owner. A second consumer is impossible by move; repeated
observation of an existing join result cannot change it to success.

Reuse the existing blocking-keeper ownership pattern in `command_lifecycle` to
run that exact join while holding an `Arc<ProfileLease>` from the same lifetime.
Dropping an async observation leaves the blocking closure with that lease and
thread until the physical join finishes. The candidate orchestrator must keep
its original lease Arc independently through process exit. No runtime cleanup,
graceful report, SQL, network, finite worker signal, or redispatch is executed by
this handoff. The leaf cannot claim frontend/listener/stream/Terminal/Peer
closure or subsequent lease release is safe at the daemon boundary.

## Mount prerequisite discovered by reading the engine

`Engine::open_storage` calls `Store::create_frontend` (one keyed state
connection), can run startup recovery, and may drop a partly built store on
error. `Engine` operations can also enter `spool::load_prefix` independently
of the state store, reopening keyed SQLCipher per selected stream. The worker
check above covers the synchronous owner turn, but this is not the approved
independent open/implicit close guarantee. Before daemon composition, install
an exact candidate-only engine connection guard at its shared `cipher::open`
boundary, including temporary spool connection destruction and early-error
cleanup, with any deeper necessary startup checks. Keep stock and the
independently exec'd helper outside that guard. Do not claim this unmounted
increment qualifies those opens, state sidecars, VFS callbacks, actual
forwarding replacement, or intentional startup partial custody. Keep the
production mount closed until that is implemented and independently verified.

## Focused evidence

Use existing bounded test worker seams and one-shot channel barriers. Test
pre-open refusal (no directory and no Ready/OpenFailed), full event channel
and full control queue, and a fatal cut while a previously admitted turn is
held. Require no new admission, no late buffered result (including errors),
and no fake worker completion while the cut is held. Exercise exact OS join,
keeper-held lock after observer expiry and sticky panic including handoff after
an ordinary join already reported panic. Cover cut before queued Stop dispatch,
after Stop consumption and before checked owner close, and a completed join
before ordinary report admission. Use a drop sentinel to exercise checked
owner parking across the equivalent Stop, disconnection and send-error exits,
Ready send failure and unwinding. Test candidate Cleanup cannot take the
engine and normal stock Cleanup still succeeds; the successful in-flight
candidate `into_cleanup` test belongs to the scoped inner connection guard
because there is no candidate call to it in this leaf. Confirm all publishable
cached event categories and queue methods close on a tripped latch. Parked
handles intentionally retained to process exit belong in an isolated child
test if using real SQLite state. Verify exact candidate build and absence of
any candidate constructor call reachable from launcher/main/daemon (also a
composition prerequisite on the bead). Run strict scoped Clippy, format and
real worker/keeper regressions. Record exact source and Claude Auto verdict.
