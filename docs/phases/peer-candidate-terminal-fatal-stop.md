# Terminal physical stop on registered dispatcher loss

This phase implements the Terminal boundary required by
[daemon composition](peer-candidate-daemon-composition.md) and
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md). Review base
`16dc1b3`. The adjacent checked Engine connection owner and registration
admission at `ad0221d` are dependencies for reuse. Terminal already retains
the tmux session on detach. Preserve that behavior, stock and healthy
candidate shutdown, and both existing Command and Terminal relay policies.

## Admission and custody

The later daemon composition supplies the exact registered lifetime's
`ForwardingVerifier`, its paired read-only `Fatal` and narrow custody-fault
reporter. Derive the latter two from the same sealed registration issuance.
Terminal gets no trip capability, route selector override, or Peer authority.
Use one private cloneable Terminal fence with synchronous
`admit` (latch, registered observer, latch) and the existing Fatal wait.
Use explicit candidate constructors, leaving stock constructors and the
process-wide default unchanged. Late revalidation refusal trips the
original latch through the existing registration observer.

- Put `Prepared::open` and Registry startup under checked admission and
  propagate a tripped preparation to the eventual daemon as fatal even
  when the stock branch would return client-only on Registry failure.
  No lazy fallback can open a second connection. Recheck before starting
  the initializer and before each distinct startup recovery turn.
- `BindingRequest::complete` and `Control::refresh` call real Command
  journal and group reads through MdkOwner. Check immediately before
  the synchronous read and before emitting the reply or snapshot. The
  existing daemon fatal loop independently gates these call sites. Shared
  IPC listener draining is the composition owner's responsibility. The
  Terminal intake, route open, Start, Confirm and Release boundaries
  independently refuse new work, including a send racing the stop wake.
- `terminal_supervisor_store::Registry` owns the separate persistent
  `terminal.sqlite3` connection. Reuse the Engine's reviewed checked
  existing-file ownership, including raw half-open custody, before keying
  and timeout setup. The needed shared API is a narrow guarded
  existing-file constructor on the connection fence and an opaque owned
  wrapper with checked scoped read/mutable SQL borrows. No handle transfer,
  public mutable dereference or duplicated SQLite FFI. Any separate
  configuration, migration and Registry method takes fresh admission.
  Map fence refusal distinctly from ordinary Registry unavailability.
  Statements and transactions are scoped to their admitted synchronous
  turn. Apart from that, destructor/explicit close independently checks and
  parks on refusal or failed close, reporting unsafe custody to the exact
  daemon latch. Preserve private file ownership, key derivation, schema,
  journal mode, stock open flags and slot inventory.
- The volatile Room is also SQLite. At the locked MDK `bdf9833`,
  `SqliteAccountStorage::in_memory_with_options` invokes a private
  sqlite memory opener and migrations. `SessionConfig::with_storage`
  still requires that concrete provider, and MDK exposes an exact closer
  only after successful open. Treat the upstream synchronous **volatile**
  opener and its internal error cleanup as one admitted primitive. Before
  adopting this rule, prove the linked build with actual registered
  forwarding and an observing VFS that its configuration, migrations and
  failure perform no file-backed pager or temp open. It receives no path,
  uses dedicated in-memory storage, and keeps temp storage MEMORY and the
  existing SQLCipher memory policy. If file-backed access occurs or this
  primitive cannot be bounded as approved, hold the candidate mount for
  a narrow upstream guarded-open aperture. Do not substitute path-backed
  storage, leak the Room or preserve the room key beyond the lifetime of
  the process as a workaround.
- Immediately after a successful volatile open, retain one scoped
  `ManuallyDrop<SqliteAccountStorage>` root before giving clones to MDK.
  Own it across setup error, timeout, cancellation, success and panic.
  It is declared/dropped after native runtime clones. On healthy retirement,
  close through the exact upstream bound closer, then release the retained
  root. On refused retirement park only that root to process exit, so other
  clones cannot destroy the SQLite connection. On close error report
  custody fault and retain the root; audit the locked close’s error
  contract, which may itself consume the raw connection. Never retry or
  claim physical SQL release after parked custody. Volatile state remains
  nondurable. The approved encrypted database fallback stays dormant.
- Check separately before each independent Room operation and after
  async boundaries before synchronous native continuations (for example
  send → outbound acknowledgement and receive → ingest → app
  acknowledgement). Shared authority covers both lane and room, with a
  biased fatal arm ahead of deadline and issued transport work. Audit
  actual Room methods and MDK runtime polls for internal guards across
  suspension. A started poll may finish synchronous work and its native
  compensation; a fatal check before the next poll and before post-await
  results prevents later independent work. Preserve ordinary Command grant
  semantics and existing cancellation ownership outside the fatal path.

## Physical teardown

1. Terminal exposes synchronous `Runtime::request_stop` and the owner
   retains the profile Arc across the entire actual join, including
   client-only, partial initializer, recovery and error paths. While
   running, await the shared Fatal in a biased branch and acquire-check
   before dispatching a ready event. In a fatal transition close local
   intake and attachment receivers, discard queued locally owned items
   through their existing guarded cleanup owners, and request supervisor
   stop before awaiting route or listener tasks. Abort and join local
   enrollment/listener/attachment tasks. Their retained lanes and rooms
   must reach accounted reapers. During an already-running graceful join,
   check/observe the same Fatal and converge on the physical path, even
   while a lane is awaiting deactivation. Keep a closure error distinct
   from routine unavailable. Reuse the existing 15-second diagnostic
   threshold while continuing to wait for actual owned joins.
2. Keep the existing Native initializer join and both
   `LiveSupervisor::join_if_finished` joins. On fatal, refuse native
   control and PTY admission before touching queued starts, confirms or
   releases. Switch to physical reaping before normal expiration,
   departure, recovery, takeover advancement or allocation. Gate new
   helper spawn centrally at the actual tmux command spawn and checked
   attachment launch, including reconciliation's kill-session call.
   Guard repeated independent PTY writes/resize/installation (the I/O
   thread's wake and existing stop/retire path can be reused). Flush
   pending replies and parked replacement envelopes without granting
   them. `Attachment::shutdown` closes its own PTY and joins/releases
   its owned attachment child. `Manager::drain_pending` kills and reaps
   only an already-owned helper. Has no need to issue a fresh tmux
   command or edit a row. Existing exceptional Drop transfer sends to
   a reserved per-child reaper retaining the lease. Before claiming the
   terminal barrier, explicitly account for any such transfer possible
   under fatal/initializer failure; if unreachable on the fatal branch,
   prove by source and fault test. No timeout allows early lease
   release. The base-shell tmux server remains available for later
   normal reattach.
3. Construct both the bootstrap alias and room connector through
   `Route::connector` with the same Fatal below TLS, before SDK client,
   bootstrap or TCP open. The raw stream fence already refuses reads,
   write/vectored, flush and shutdown and wakes pending operations. This
   also cuts protocol CLOSE, auth, TLS close-notify and writes caused by
   reading. No extra wire-level check can substitute for the raw fence.
   Install Lane custody before its first SDK/bootstrap await so
   cancellation always transfers a counted cleanup owner. Audit the
   second room connector construction which may occur before a Lane
   exists; before any I/O it can simply drop.
4. `Lane::close_inner` retires authority when applicable, aborts the
   pump, and in candidate mode checks at deactivation admission and
   races in-flight deactivation against Fatal. On fatal skip further
   MDK deactivation, including an existing alias-detach task. Cancel
   new connects before SDK shutdown while keeping the exact connector
   semaphore open. Reuse `reap_terminal_socket` to prove both halves
   of the actual stream are gone. The timeout in SDK shutdown permits
   checking exact slot ownership, never reporting early success. The
   generic semaphore-closing `cancel` is reserved until reap succeeds.
   Keep the same physical proof for `Lane::drop` and startup failure.
5. `ReservedRoom` already transfers room permit and owner to a
   `Reaping` guard before it starts normal close. The existing
   `Route::finish` waits both reaper counts. Count the alias watcher
   from spawn until its Lane is returned or its own tracked Lane Drop
   has taken ownership, and count detached aliases until their exact
   close. Tokio JoinHandle drop alone detaches its task and is not a
   join. Establish transfer ordering so `Route::finish` cannot see
   false zero between a parent dropping and its child taking custody.
   Wait after all join-set producers drop, preserving the permit until
   actual room/alias teardown. Avoid introducing a second owner runtime.

## Validation and boundary

Begin with one production-shaped private Terminal transport stop and
owner plumbing pass if the shared SQL aperture is still under review.
Candidate live daemon mount waits for all three boundaries. No workflow,
daemon, registrar, ordinary Command or public route changes in this
phase. Scope additions to Terminal files, the existing connector's narrow
token-only method, and the small Engine cross-crate aperture if separately
coordinated. Prior runtime/fixture tests continue to use ordinary entry
points. Terminal cannot enable the candidate, open a listener, issue Peer
authority or choose a new relay. No privileged runner is needed.

- Deterministic latch cuts at pre-open, partial startup, post-native
  binding before reply, queued Control, PTY wake, and graceful-close
  deactivation. Count entry into the actual corresponding effects, not
  only declared task completion. Assert queued starts/Confirms do not
  advance and helpers do not spawn; retain a blocked actual helper/owner
  and verify lease and room accounting before and after release.
- Real isolated encrypted Registry and in-memory MDK connection cases
  on Linux AMD64. Prove no later SQL turn or stale application result,
  exact healthy close, no unsafe close on cut, and preserved persistent
  Registry bytes after refusal. Run observing VFS displacement/failure
  tests in isolated children. Verify volatile input never resolves a
  file-backed pager/open and independently cover error before returned
  custody, explicit/canceled close and post-cut root Drop. No global
  SQLite test displacement in the parent libtest process.
- Real private loopback TLS WSS with normal alias/room attachment and
  actual tmux. Cut one lane during startup and after an established
  writer with both lanes exercised. Deliberately hold SDK or relay work
  across the cut, verify no further forwarded raw I/O or application
  event after the completed cut, and hold a separately retained actual
  socket half to prove the join cannot finish early. Include fatal
  crossing graceful shutdown. Then verify the existing tmux session
  survives and normal Terminal detach/reattach still runs over the same
  ordinary route. Separate kernel bytes already in flight at the cut
  from post-cut poll effects.
- Run focused stock Terminal regressions and strict stock and candidate
  Clippy. Use the existing explicit native WSS/tmux fixture, disposable
  profiles, bounded clocks and an outer watchdog. Keep slow cases opt-in
  and avoid new blanket CI jobs. Record actual test selectors, source,
  timings and any unproved setup separately from positive evidence.
