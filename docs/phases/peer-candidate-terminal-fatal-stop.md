# Terminal physical stop on registered dispatcher loss

This design follows [daemon composition](peer-candidate-daemon-composition.md)
and [dispatcher shutdown](peer-candidate-dispatcher-shutdown.md). The
review base is `16dc1b3`. The guarded Engine owner on adjacent
`ad0221d` is a future reuse dependency. Stock Terminal retains its relay,
storage and tmux behavior.

## Implementation scope

**First change** closes actual Terminal relay Lanes and accounts for their
physical stop. It builds on `16dc1b3`. Scope is
`terminal_enrollment_driver.rs`, `terminal_enrollment_transport.rs` and
their private test fixture, plus narrow connector cancellation and raw
slot custody in `transport.rs` if needed. Compile for the Linux AMD64
candidate and tests. The only route constructor attaching this new
fence is a test aperture in this change. It exercises the production
Route connector, real signed-auth Terminal Lanes, SDK subscriptions and
private WSS without running `Owner::open_bound`, `Room::prepare_runtime`,
the Terminal supervisor, the Registry, the Command engine or a daemon.
Neither the fixture nor a candidate binary can use this aperture to
open a native Room. Alias watcher and detached aliases belong to the
next change, when those owners can be exercised with safe Room custody.

**Terminal ownership and storage** must be in place before candidate
mount. There are three independent mount conditions.

1. Exact physical connector stop from the first change, then complete
   Terminal owner, supervisor and alias accounting using the same latch.
2. Checked, physically owned persistent Terminal Registry, using the
   guarded Engine existing-file aperture owned by daemon composition.
3. Checked volatile Room SQLite open, turns, and final custody.
   Its retained root is mandatory before the first fault test or real
   candidate stop involving an already-open Room.

A test that stops two Lanes fulfills only the first change. Report real
attached Terminal displacement and tmux survival only after all three
conditions are exercised together.

## Physical relay implementation

Use the existing exact read-only `Fatal`. In the future mounted route
it comes from the one registration issuance, independently revalidated
before every separate synchronous owner admission. The first change
does not derive or widen that registration authority. Its private
fixture substitutes a synthetic latch only at the constructor and
uses the existing root-scoped loopback trust.

- Bind the same Fatal to both alias and room connectors through
  `Route::connector`, before SDK construction, bootstrap or dial.
  Both the ordinary constructor and the test-only roots branch pass
  through the same attachment point. Existing
  `RelayConnector::connection_phases` already biases Fatal ahead of
  dial, TLS and WS handshake. The existing `FencedStream` beneath TLS
  blocks reads, writes, vectored writes, flush and shutdown. Preserve
  exact permit lifetime on that raw stream structurally using the
  existing `with_slot` ordering, in addition to the SDK sink and
  stream closures. Unfenced SDK paths remain unchanged.
- Check the same latch at the Lane's `Authority::check` and within
  `Authority::run`, with a biased fatal wake and before the next poll
  of any guarded future. Both checks participate before bootstrap and
  any SDK protocol operation, including after awaited readiness. One
  previously admitted poll may complete its synchronous work. The
  raw guard independently prevents another socket poll at the cut.
  Keep existing deadlines, signed-auth checks, readiness and Command
  attachment fences.
- Transfer connector and SDK custody into the Lane before awaiting
  bootstrap. If bootstrap is refused, canceled, or unwinds, the
  partial owner follows the same stop path. Account for a guarded
  live Lane from before its first SDK operation that can open a
  socket until exact reap, using the existing `Reaping` accounting.
  Ownership moves into background cleanup before local ownership
  can end. `Route::finish` is called only once all Lane-producing
  tasks are joined or canceled and accounted for. The stock path
  preserves its existing healthy route availability and setup.
- Guard native adapter deactivation separately because normal Lane
  close retires the attachment authority first. A cut before
  deactivation skips that operation. A biased Fatal arm interrupts it
  while pending, with no next native poll. SDK shutdown may still
  unwind local tasks, with the raw gate closed. After a cut block
  reconnect immediately, without closing the semaphore used for the
  proof. Cancel the pump and actually await its existing `JoinHandle`.
  Retain that handle in the Lane while an awaiting close is
  cancelable, or transfer it to counted cleanup. Dropping a handle
  is not task completion.
- Reuse `RelayConnector::reap_terminal_socket` as the sole success
  proof. The SDK shutdown timeout permits beginning this proof, and
  does not prove release. Both underlying stream halves must drop.
  Avoid calling the generic semaphore-closing cancel before exact
  reap. The existing reaper keeps retrying while the slot is held
  and reports incomplete ownership if its actual cleanup is lost.

## First change validation

- Run real, independently authenticated alias and room SDK Lanes on
  the same private TLS WSS, using two connectors from the same Route
  and two distinct established subscription sources. Take the cut
  while both are active. Verify new work refuses, both live SDK
  sessions lose their subscriptions, and both reapers finish.
  Before the cut record actual relay auth/subscriptions. Stop
  fixture traffic, observe EOF after the cut and compare exact relay
  protocol observations where supported. Use the existing raw
  delegate-poll tests for the explicit zero-post-cut-I/O assertion.
  TCP bytes already emitted before the cut may still be received.
- Prove startup cancellation on a real already-connected socket for
  this connector using the existing fixture `peer_test_open`. Hold
  the actual returned sink and stream halves separately and start
  the real SDK Lane on the occupied single-route slot. Wait until
  the Lane has transferred startup ownership before tripping. The
  startup future may return promptly. The route reap must still be
  pending after only one half is dropped, and finish only when the
  other half is released. No synthetic slot token serves as proof.
- Exercise a deterministic blocked poll of the same production
  deactivation guard, distinguishing the entered poll, the Fatal
  wake, future destruction, and absence of a second poll. Also
  cancel an awaiting close with an active pump and observe the
  tracked physical join. Report injected boundary evidence
  independently from the real WSS outcomes, including whether a
  pre-cut protocol CLOSE was already in flight.
- Run focused stock enrollment/connector regression tests and strict
  stock and candidate Clippy. Use disposable profiles and a bounded
  outer watchdog. Slow WSS stays opt-in. Record exact selectors,
  source and timings under validation. No public relay, listener,
  release, privileged test runner or CI schedule changes.

## Remaining Terminal owner and registered admission

The future Terminal owner receives the exact registration's
`ForwardingVerifier`, paired Fatal and narrow custody-fault reporter.
Only the reporter can set the latch, and only for unsafe custody
release. Use synchronous admission (latch, observer, latch) and the
existing fatal wait. No selector override, trip capability or Peer
authority is delegated. The existing daemon fatal loop independently
gates shared listener/Command work.

- Keep the profile Arc across the actual Terminal join in compiled
  and client-only paths. Add synchronous `Runtime::request_stop`.
  Bias Fatal ahead of dispatch and check before handling already
  queued events. Close local intake before waiting. Abort and actually
  await enrollment/listener/attachment producers before draining
  Route counts. Join the Native initializer and both supervisor
  threads. Retain the 15-second diagnostic threshold without treating
  it as permission to release the lease.
- Stop before native control confirmations, expiry, reconciliation,
  recovery, allocation, departure, or takeover advancement. Gate
  queued attachment admissions and actual tmux helper spawn
  centrally, including reconciliation's kill-session command.
  Revalidate before independent PTY installation/write/resize where
  synchronous effects can follow a wake. Existing attachment and
  Manager drains close or reap existing owned children. They cannot
  issue a fresh tmux command or edit a row on the fatal branch.
  Resolve reserved thread transfers explicitly before claiming an
  actual join. Those reapers hold the profile Arc, but can outlive a
  supervisor thread. Base tmux server and shell remain resumable.
- Check `BindingRequest::complete` and `Control::refresh` before
  actual MdkOwner journal or group reads and before replying or
  updating a snapshot. Check queued Start, Confirm and Release
  independently of the select wake. A fatal `Prepared::open`
  refusal must propagate as fatal even where stock would choose
  client-only on ordinary Registry failure. No recursive opener.
- Preserve both alias watcher and detach JoinHandles, including
  cancellation of the caller joining them. A watcher owns a Lane
  until returning it to its exact join owner. A detached alias has
  real close work until physical proof. If their parent is
  dropped, transfer handles to a counted reaper before relinquishing
  parent custody. `ReservedRoom` already owns the permit during
  its close task. There can be no false zero between producer
  shutdown and route/room cleanup. Under fatal, skip and cancel
  native deactivation even in an already-running graceful close.
  Use existing deliberate room retention limits.

## Remaining SQLite custody

**Registry.** The separate `terminal.sqlite3` uses a regular
rusqlite connection. Reuse only the Engine's reviewed guarded
existing-file ownership, including half-open raw custody. The shared
opaque owner must expose checked scoped reads through `&Connection`
and deferred transaction work through borrowed `&Transaction`,
without mutable connection replacement, handle transfer or additional
SQLite FFI wrappers. The eventual candidate open uses the shared
reviewed hardening flags for that raw open, with `NOFOLLOW`. Stock
keeps `READ_WRITE|NO_MUTEX|NOFOLLOW`. Keep Registry key derivation,
timeouts, DELETE journal, MEMORY temp, schema and exact profile binding.
Revalidate before key setup, each separately admitted statement
group, each migration and each method. SQL borrowed inside a
transaction stays within that synchronous turn. Independently check
explicit/destructor close and park on refusal or failed close. Route
the direct recovery and supervisor fixture-only connection accessors
through their own checked or stock scope. Engine and VFS ownership
live in the separate parent increment.

**Volatile Room.** MDK `bdf9833` uses a private memory open,
migration, and `from_opened` path for
`SqliteAccountStorage::in_memory_with_options`. The latter reads
the notification counter and installs group-write state, and can
close the connection on failure. `SessionConfig::with_storage`
needs that concrete provider. The public bound closer appears
only after success, takes the native connection out of the storage
slot, checkpoints, calls rusqlite close, and on error explicitly
drops the returned Connection. A reported error has consumed the
slot and cannot be retried or proven to have safely released custody.
A host-side root cannot intercept that internal fallback, or retain
a raw owner lost before a handle is returned. Current pinned MDK
therefore has no proven mounted aperture. Do not adopt the
unproven open or close merely by adding a precheck. One smaller
candidate for the later proof is `begin_close` (no SQLite), healthy
checked drop of the exact last storage root and `is_released`
observation. That follows `CloseableConnection::Drop`, not
`close_taken`. Actual last-drop checks must account for every clone
and any outstanding transaction or rollback hook. The release marker
follows the ordinary rusqlite Drop and alone cannot report a hidden
native close failure.

The later exact design must establish an upstream guarded volatile
opener and close, or otherwise prove an equivalent approved
boundary. On successful open install a Drop-deciding retained
storage root before giving clones to native setup. Declare it
after runtime clones in Room or shut them down explicitly before
dropping it. The guard safely parks on cut or abort. A healthy
checked explicit close must run only after runtime operations
are finished, on an owned blocking worker if it can block on
`DatabaseCloser::release.wait()`, with admission revalidated inside
that worker. Never block Terminal's current-thread runtime waiting
for itself to release a native object. Preserve volatile
SQLCipher confidentiality and genuine nondurability. No path-backed
Room or optional encrypted retention fallback is enabled here.

Validate open/migration/`from_opened` failure in isolated children
using deterministic native failure injection or controlled
`sqlite3_hard_heap_limit64`, plus a forwarding VFS oracle. Account
for permitted non-file callbacks such as `xRandomness` and
`xCurrentTime`, and detect all file opens, writes, deletes and syncs.
Verify exact healthy close, failure without independent native
retry, cut release and no stale application result or late
acknowledgement. Run the integrated active-Room fault, teardown
and tmux reattach only once those guards and the Registry owner
are physically in place.
