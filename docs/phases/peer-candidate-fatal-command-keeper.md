# Command worker and keeper on candidate dispatcher loss

This is the next unmounted increment of the approved
[physical shutdown design](peer-candidate-dispatcher-shutdown.md), after the
sticky transport primitive at `121dc73` and evidence at `617a40f`. Bead
`marmish-dwt.4.12.2` tracks composition. Scope is the ordinary Command
worker and its blocking keeper. The daemon, IPC, native Terminal, physical
Peer route, and registry remain owned by their existing slices.

## Worker contract

- Keep `CommandEngine::start` and stock callers unchanged. Add a gated internal
  construction path under the same Linux AMD64 test/candidate condition as
  `dispatcher_fatal`. It receives the shared `Fatal` and a single synchronous
  forwarding-admission function. The eventual exact registration verifier
  supplies that check and latches fatal under its own guard. Worker admission
  fails when the check refuses or when the latch has tripped before or during
  the check. A cheap latch-only check suffices for denying frontend queue
  admission and suppressing observed events. No generic app/CLI input can
  construct this path and the candidate main still cannot mount it.
- On the worker thread, gate before the worker storage/open turn and handle
  each received control, each live preparation, finite execute, and copy helper
  registration as independent admissions. No queue marker grants new effects.
  The loop checks the latch before waiting or staging another operation and at
  each completion admission, even if an effect just returned. The idle wait is
  the existing 2 ms idle poll. Nothing runs while a registry guard is held.
- Treat junk in the queue and buffered replies like memory. A `PreparedLiveExec`
  owns descriptors for a future spawn and has no `ChildOwner`; `EngineCompletion`
  likewise owns observation data. Dropping either never cancels a finite worker.
  Clear or stop exposing buffered completions once fatal. A full sync event
  channel may block a send that was admitted just before the cut. Physical
  handoff must disconnect its receiver before joining. Drop both request
  producers and the control sender at that handoff without sending `Stop` or
  consuming any event through `collect`.
- For the retained worker `Engine`, keep a single owner from return of the open
  until trusted ordinary destruction or intentional parking at fatal. Construct
  that owner before reporting Ready and do not let an event failure or worker
  panic run its destructor without a checked admission. If healthy, existing
  Cleanup remains an admitted turn; call `into_cleanup` only after that check.
  `CleanupGuard` has already dropped its SQLite connection inside that turn,
  but still owns protective locks; keep it on fatal along with the engine root.
  Stock paths destruct normally. A fatal flag seen while an ordinary worker
  stop is in progress must prevent more controls and late completion admission.

## Physical join and lease

Consume the tripped worker rather than return a graceful `StopReport`. The
existing ordinary keeper runs the stock graceful stop and profile cleanup. Add
one small fatal handoff to the latter's module, retaining the same profile lease
under a blocking join even if the async observer drops its join handle. At the
join boundary drop the disconnected frontend records, join the exact OS
thread, and return typed success/failure once the thread actually ended. Worker
panic is incomplete and cannot turn into clean shutdown. No runtime cleanup,
SQL, network, grace drain, or finite-helper process action runs in this
handoff. The candidate orchestrator will hold a second reference to that same
lease until process exit. It must preserve the physical join observer and avoid
healthy report promotion when ordinary stop races with fatal. The module
handoff alone does not prove daemon/stream/Terminal/Peer completion.

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

Use existing bounded test worker seams and one-shot channel barriers. Verify
fatal before engine open creates no engine directory and yields no Ready. Gate
an admitted turn, enqueue work/control behind it, trip, demand that the join
remains pending with profile locked, and release the turn; require zero new
admissions and no delivered stale completion. Force a full event channel and
prove receiver disconnection lets the exact OS join complete; record keeper
panic as failure. Include healthy/admission-unavailable and ordinary stock
regressions. Parked handles intentionally retained to process exit belong in
an isolated child test if using real SQLite state. The deterministic fixture
may observe admission and join separately; it must not claim to prove an
actual registered VFS displacement or finite command handoff, which remain
parts of the mounted qualification. Run strict scoped Clippy, format and the
actual Linux AMD64 candidate check. Record exact source and Claude Auto
verdict before composition.
