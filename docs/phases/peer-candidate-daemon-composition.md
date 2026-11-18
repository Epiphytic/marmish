# Compose the private candidate daemon startup

This is the startup and ownership slice of the [private candidate
mount](peer-candidate-daemon-mount.md). The source inventory is from public `a14b8c8`.
It already has the candidate artifact marker, CLI List and Show, and one same-UID socket
whose candidate read branch returns `peer_unavailable`. Do not compose an affirmative
mount until the independently reviewed handshake, Ready registration, and charged
Pending bridge with cold readback are available on the composition base. Handshake
source `35f08c2`, registration design `b619d1f`, charged bridge composition `d9ea132`,
and Pending view design `7fa52a2` are separate private checkpoints at this design cut.
Before implementation, record their incorporated revisions and the reviewed non-test
actor's exact start, stop, join and read types. The charged test bridge alone cannot
supply that actor.

Keep the private Linux AMD64 candidate gate, Ready-at-acquire, Manual, Client/Both,
exact qualified direct WSS and existing Unknown cap. Use the sole ordinary daemon,
identity, profile lease, listener and signal owner. The scope ends at the checked
Pending read view. Keep local approval, native application access, activation, heartbeat
and distribution outside this slice.

## Compose in startup order

1. In `daemon.rs::open_inner`, use an explicit run intent from `run_inner` for the
   installed candidate branch, excluding `daemon::open` and the test `PeerFixture`.
   Retain one `ProfileLease::acquire`, existing config selection, overrides, relay
   match, prepared material and `ServiceLifetime` checks. Keep first-run and excluded
   profiles on the existing mutable path. After those checks and before
   `native_preflight::check`, give the local preflight the same prepared lease in an Arc
   and the original identity material. An excluded or independently untouched outcome
   drops all temporary witnesses then unwraps that Arc into the original lease. If
   unwrap unexpectedly fails, startup fails with a fixed ownership diagnostic. Never
   reacquire the profile or redo provider selection. Keep the ordinary connector and
   entry points unchanged.
2. `ReceiverPreflight::prepare/register` alone checks scope and installs the verified
   default forwarding dispatch and complete reservations. For a verified
   `RegisteredReceiver`, preserve the exact Arc and retain the original identity
   material for the prospective actor. Apply the failure table below. Registration
   performs no ordinary native database open and supplies no network authority.
3. Preserve `native_preflight::check`, `MdkOwner::open`, retention and inventory
   reconciliation in the existing order. Recheck the registered dispatcher immediately
   after open before the remainder of the SQL setup, and again before transferring the
   candidate owner. At the existing `mark_ready` call site, the registered branch runs
   `ProfileLease::check_opened_binding` on the actual `owner.opened_binding()`. It
   preserves ordinary error precedence and the binding error, and leaves `profile.json`
   unchanged. The unregistered branch still calls `mark_ready`. Finish ordinary Terminal
   and Command setup with the same lease and identity when the failure classification
   permits. Retain the verified dispatch check even if a Peer-local scope refusal has
   dropped the actor option. If post-open validation fails while the owner is still
   being assembled, destroy the owner before releasing its lease. Do not expose a
   listener or an Active report after that error.
4. `run_inner` owns the existing Terminal setup and normal return path. After ordinary
   preparation succeeds, create one `peer_candidate_startup::pair` and start the
   candidate actor only with the exact lease, moved prepared material, registered
   witness, WorkerStart and the publisher of the candidate Pending view. The sole
   actor's current-thread runtime starts idle. Its handle owns cancellation and actual
   join, and its read handle carries no worker sender. A thread start failure
   permanently closes the pair and removes only Peer intake. It does not retain a
   partial actor or make Command/Terminal startup fail. Keep the existing fixture
   bootstrap, `ipc::peer`, fixture stop/test connector and owner separate. Use private
   types at these call sites rather than adding unused production stubs or promoting an
   entire fixture module.
5. `serve_owner` retains signal setup, `ipc::bind`, ordinary activation, status
   publication and reporter. Carry candidate registration and the read-only intake into
   the existing IPC task. Replace the stock tick-only observation with a listener
   JoinHandle select during candidate activation and service, even if Peer registration
   failed. Retain the stock and legacy fixture monitor paths. In the normal loop,
   preserve the pre-status command-limit check. After the first non-degraded Active
   status is published by `status.send_replace`, require the background reporter's
   successful parent acknowledgement if present. Keep the reporter's existing bounded
   await and error behavior. Deliver OrdinaryReady immediately afterward via an
   independent once flag, including foreground, only if shutdown, lifetime and
   process-fatal checks have no already-pending stop. The flag flips only at that
   release. Candidate waiting or local failure must not block ordinary status or
   reporting. Do not reinterpret a successful reporter write as custody or Peer health.

## Registration failure disposition

| Call and result | Disposition before issuing further ordinary work |
| --- | --- |
| `prepare: Ok(None)` or `Err` from read-only eligibility/proof | Fixed Peer-unavailable outcome. No registration was attempted. Drop temporary witnesses and preserve the original ordinary path. Existing ordinary config and identity errors remain outside this call. |
| `register: Ok(witness)` | Use only this exact verified forwarding dispatch and original Arc. |
| `register: Untouched` | The registration layer freshly proved the original stock dispatcher. Drop temporary witnesses and use the original ordinary path. |
| `register: LocalUnavailable` | The current registrar only emits this here for an already-registered process and returns no witness. This is an unexpected duplicate startup. Fail with a fixed registration-conflict diagnostic before ordinary opens. |
| `recheck/attach: LocalUnavailable` | The exact registered dispatch remains verified. Permanently close this Peer generation and retain the original lease. Continue ordinary service unless an independent ordinary error occurs. Keep a dispatch-only verification capability or otherwise prove the exact existing forwarding invariant before subsequent startup uses, without authorizing a new Peer generation. |
| `register/recheck/attach: Indeterminate` | Process-fatal. Stop before another ordinary open or turn. If handles already exist, use the ownership-preserving fatal path below. No repeat registration, unregistration or settings fallback. |
| `recheck/attach: Untouched` | Not emitted by the approved registered recheck or attach. If the interface returns it anyway, fail closed as lost dispatch authority. |

The registration owner supplies only a read-only dispatch verifier if the
mount needs to retain it separately from the consumed actor witness. Check
that exact capability on the composed base before mounting. No watchdog
may redefine any local refusal as proof of global dispatch validity.

## One listener and one actor

Keep `ipc::serve_with_peer` as the shared accept/credential/frame loop. Verify the
actual bound listener's kernel local address matches the socket path captured for the
held lease, then attach the candidate view for that listener generation before emitting
Registered. Reuse the existing private fixture path check's approach without importing
its write intake. On a local registration failure drop the candidate registration and
keep serving ordinary requests. Retain a successful registration through the lifetime of
that listener and preserve the ordinary 50 ms accept retry on transient errors. Drop
registration before listener/task drain. Keep the current test-only fixture registration
and accept fault behavior. Do not give the listener the lease, route or durable storage.
All ordinary requests retain the current UID check, 64 client semaphore, status path,
Command dispatch and Terminal transfer.

Use the installed `Request::PeerRead` shape on this listener. After the existing
same-UID check, accept-time frame deadline and client semaphore, acquire a nonwaiting
permit from the Pending view owner's four-slot local budget. That permit and the view
handle belong to the Pending view dependency. Neither exists on the public base. Hold
the permit through the bounded reply attempt. Use a direct memory-only handle to the
installed actor's independently refreshed Pending view. Under a short synchronized
access, require Live for the exact owner generation/ticket, a current handshake, fresh
view and a live local exchange. Use only the already-checked bounded typed List/Show
projection. At final local write admission recheck the same generation, sequence,
freshness and liveness. Release the short view lock before asynchronous socket write.
Invalid, Stopped, absent or stale view returns the fixed Unavailable response. A read
never waits for the native actor, enters its queue, initiates I/O, refreshes proof,
accesses a relay or falls back to ordinary Command dispatch. Preserve candidate request
shape, existing permit/deadline limits, stock listener flow and fixture IPC. This
replaces the proposed actor-mediated local read queue in the parent candidate mount.

The actor consumes `WorkerStart::ready` and rechecks the returned Ready and receipt of
its exact lease/material before local work. Read-only recheck the registered witness
before opening or initializing the capped cursor, then obtain its exact source. Attach
through the checked witness to borrow the quota and retain the checked route. Recover
and recheck the charged inventory against the cursor source before starting intake or
opening the physical route. Use only the charged bridge's agreed Reserved, Held,
JoinStarted and Pending operations. The actor rechecks Ready before each further native
or relay turn and stops when revoked. For a mutating turn the sole view producer
invalidates before minting the non-Clone ticket and reading any evidence. The view owner
checks ticket epoch, listener generation and sequence and carries the effective
persisted floor from the same proof fence. A disconnect can retain an exact Pending view
only while its independent scope and freshness proof remain current.

## Exit and ownership contracts

A local Peer startup, open, route or custody refusal closes that
generation and leaves ordinary service running. An unexpected listener exit
is an ordinary daemon failure through `listener_stop`: an unexpected
successful exit becomes `ipc_listener_stopped`, an IPC error survives,
and a join failure becomes `owned_stop_incomplete`. Observe its task
result once. If the ordinary queue closes first, obtain the listener's
outcome instead of returning clean success. Intentional shutdown keeps its
ordinary result unless cleanup reports a failure.

If the exact registered forwarding invariant is lost, the detecting actor
publishes a typed process-fatal signal and trips a common cancellation
fence immediately. Distinguish it from normal Peer stop or actor panic.
At the top of the ordinary loop and each storage-touching branch, check
that fence before doing work. Include `command_count_exit_ready`,
`terminal.refresh`, `owner.status`, Terminal binding completions and
other ordinary synchronous admissions. Include the fatal signal with
shutdown in activation, request dispatch, tick and room turns. Apply
the same fence to open-time inventory, retention and Terminal or Command
creation if startup is already in progress. An action already in a native
call at detection must finish or be canceled under that owner's existing
guarantees. Do not initiate any fresh durable or route work as part of
fatal cleanup.

On any exit, stop both the startup handshake and the shared read view
before requesting listener drain, aborting the listener or awaiting it.
The view's stop is available while an actor is blocked. A single serve
result block and explicit pre-drain stop covers post-spawn exits, including
SIGTERM, Ctrl-C, uptime, command limit, activation/report errors and IPC
exit. The outer run owner owns the actor and covers earlier returns and
unwind. Add a scoped candidate drain signal independent of the fixture.
After receiving it the same IPC loop drops registration and listener and
cancels its owning JoinSet, including read tasks holding permits. Await
that task once. Stop candidate and Terminal before waiting for either owned
join. Retain the original Arc through both joins and the existing final
unwrap/reaper barrier. A stuck owner retains the lease. The diagnostic
bound never permits clean completion or early unlock. Preserve the primary
service error and stock no-pair/legacy fixture semantics.

### Process-fatal physical cleanup

Use `stop_reporting` and `command_lifecycle::stop_owned` for normal verified
ordinary storage shutdown. The storage-fatal path uses a reviewed physical
subset of the same owner keeper with no additional service process or
authority. The existing keeper depends on journal and publication calls
for a clean report. A failed registered dispatcher cannot certify either.
Before mounting, audit and prove the following cuts together.

- Close IPC intake, ordinary owner admission and the shared view, wake the
  actor, and signal the independent Terminal owner before listener drain.
  Fence Terminal binding, Registry and supervisor work, transport listeners
  and stream tasks at their owner boundaries. `terminal_owner::Runtime::join`
  currently sets stop and awaits work, while `Native::finish` can finish
  a pending initializer and stop its supervisor. Confirm the fatal path
  closes outstanding transport capabilities, disallows fresh storage
  opens, new journal/recovery turns and publication, and joins the supervisor
  and route resources. If the normal Terminal finish initiates such work,
  require a narrow physical-only extension before composition. A Terminal
  loss never licenses early lease release.
- Close Command admission and fence the Command engine before its join.
  Existing `CommandEngine::shutdown` posts a FIFO Stop control. Audit
  queued controls, copy work and opening state before using it. If Stop
  permits a fresh open or queued operation after the fatal fence, require
  a fail-stop hook that rejects it before using the same physical join.
  Cancel and retain independent stream runner handles. Capture their
  barriers without the journal-touching `stream_begin_stop`,
  `stream_stop_progress` or `stream_stop_publish` sequence.
- Destroy the MDK owner before releasing its lease. `MdkOwner::drop`
  retires Terminal authorities and cancels its connector and forwarder.
  Reuse the physical frontend and keeper handoff only after verifying that
  relevant destructor paths merely close already-owned handles. Preserve
  on-disk evidence, skip clean-runtime assertions and keep the original
  dispatcher failure primary. An already-running operation or any
  unproved join leaves shutdown incomplete.

## Code ownership and acceptance

The composition owner changes `daemon.rs` and the shared `ipc.rs` call sites and any
indispensable scoped candidate fail-stop hooks in the existing lifecycle owners. Test
adapter and cases stay beside those boundaries. The registration owner supplies the
actual private registration API. The actor owner supplies a non-test start/stop/join,
typed process-fatal path and bounded shared memory view backed by charged recovery.
Review the three exact source interfaces on a common base before editing. Keep the
candidate cfg at the integration boundary and keep fixture authority behind test cfg.
Avoid additional daemon entry points, listeners, helper processes or public feature
controls. An inert test-only actor adapter may prove the integration before the
production actor source lands. It supplies deterministic startup and join observations,
no Peer authority. This lifecycle slice may close with candidate reads still returning
only `peer_unavailable`, provided no installed affirmative mount or permissions were
exposed. The final mount review remains required.

### Lifecycle integration checks

- Demonstrate both startup edge orders and no cursor or network before readiness. Cover
  foreground Active, background reporter failure, initial activation failure, bind
  failure, an already observed stop/limit while reporting, and no second candidate
  startup. Exercise registration failure with ordinary Status and Command still working,
  transient accept retry, and unexpected listener exit with a closed ordinary queue.
- Test excluded or pre-registration rejected profiles with stock forwarding. For a
  registered path assert exact original lease identity, unchanged Ready manifest inode
  and bytes, ordinary opened binding failure classification and local post-open refusal
  with Command intact. Use subprocess isolation for default mutation. Prove
  Indeterminate stops before further native operations and the physical-only shutdown
  preserves files, joins/cancels owned work and emits no clean report.
- Verify stop-before-drain and lease retention until actual actor and Terminal cleanup,
  including all four read permits occupied, a blocked or parked actor, local failure and
  failed actor join. For the inert path verify immediate Unavailable without an actor
  request while the actor is blocked. When the view dependency is integrated, also
  verify Invalid, Stopped and stale-generation views refuse without native I/O. Exercise
  true ordinary limits/signals and the distinct private fixture mode. Reuse handshake
  unit cases and registration owner tests for their internal race and authority
  matrices.
## Separate affirmative mount gate

The integrated actor, view and charged bridge must all be qualified before an installed
read becomes available. Lifecycle adapter evidence does not satisfy this gate.

The final privately installed Linux AMD64 candidate proof needs actual private WSS v2
exchange, cold exact charged Pending readback, foreground and background readiness
across selections, same-UID List and Show on the one socket, and an ordinary Command in
that daemon with Peer both present and unavailable. Verify no active/group/approval
surface, stock/candidate packaging and protocol separation, and strict focused checks.
Keep the broader private qualification outside routine CI. Record stage-specific
evidence and explicitly distinguish local helper or fixture coverage from the composed
installed proof.
