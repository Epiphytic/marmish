# Preserve registered daemon custody across a healthy-to-fatal stop

This private Linux AMD64 increment implements the stop and process-disposition
part of [registered Command service](peer-candidate-run-admissions.md).
It builds on the reviewed IPC drain, native startup custody, active turn
admissions, checked Command startup and physical owner cut. It needs those
sources composed before implementation. The ordinary public daemon keeps
the existing stop entry. A registered qualification driver will own the
service from successful registration and may request this stop after
startup failure or after entering its live loop.

## Owned handles and monotone transition

The registered stop owns the exact profile Arc, optional boxed MdkOwner,
optional listener with its drain sender and JoinHandle, and exact Admission and
Fatal from the same registration. Keep state explicit from Starting or Active
into HealthyStopping and then optionally FatalPhysical. Once stopping, close
the local queue and producer authority and signal the listener once. Observe
the same JoinHandle in either disposition. After its return, read the Fatal
again because the listener can return successfully on either explicit drain
or fatal. A panicked listener is incomplete. Carry any observer with its
original resource across expiry and cancellation.

The stock Command keeper and detached Frontend stay in the stock path.
In the healthy registered path, keep the engine inside the owner. Close
engine and stream admission, capture independent runner barriers, and give
the existing graceful stream operations a bounded turn before cancellation.
Use the already guarded projection methods for each new durable effect.
Await stream and relay operations only while their exact Admission permits
continued effects. Put each resumed operation and its final completion behind
the checked effect boundary. Do not run the full active tick during stop.

Drive the existing synchronous Command engine stop in short, bounded turns
on a blocking observer. A blocking turn returns the same engine even when
its typed result is Fatal or failed; do not detach it or run an automatic
retry loop. If it has to outlive the active observer, retain the blocking join
and the engine custody within the stop state. On return, put the engine back
into owning position before another admission. A successfully joined engine
can be represented by its existing already-joined handle until final owner
release. A panic without trustworthy thread or child ownership is uncertain.
The stop must never discard that uncertainty or reinterpret the returned
error as proof of physical completion.

Separate graceful account deactivation from transport termination. Admit
the actual MDK deactivation and each resumed durable effect independently.
Acquire explicit SDK shutdown and nested forwarder proof and connector
quiescence through the same owning fields used by the physical cut.
Avoid the stock convenience shutdown when it consumes or aborts a join needed
by the candidate. Once a task is launched, its join belongs to custody until
settled. Mark each settled result once and do not repeat physical shutdown
after passing that checkpoint. Verify exact Admission once more before
closing Journal, before native account close, and before trusted runtime
cleanup. Close and remove only after each prerequisite completes. If a
trusted close itself reports indeterminate, preserve the remaining roots.

## Fatal and process exit

The existing physical cut begins with an owner that has never entered
graceful stream stopping. Add an explicit consuming transfer from the
registered healthy stop, using its proof of retained original engine, runner,
SDK and forwarder ownership and any already observed barriers. Do not loosen
the convenience physical cut to accept arbitrary stock stopping state. Both
entries share effect-free physical cancellation and barrier observation.
After fatal there is no new MDK deactivation, database operation, native
close, or runtime cleanup. Physically start every remaining barrier before
waiting for any of them. Neither an engine logical outcome nor an SDK outer
task's cancellation demonstrates that its child or nested task ended.
A detached finite worker continues its existing independent responsibility.

Send or close the startup reporter as a failure before retaining fatal
custody, so the parent receives prompt failure. Emit sanitized per-barrier
diagnostics after a bounded observation and continue observing pending
live-child obligations in one-second turns, rate-limiting repeated
diagnostics. If a barrier becomes permanently uncertain with a possibly
live child, keep the original process and flock alive and expose that
outcome through diagnostics. External forced termination leaves it
unresolved and must never be represented as a successful stop.

When the native roots have been parked and every owned child is positively
observed reaped or proved never started, terminate the qualification process
nonzero with the disposition. Use ManuallyDrop or forget for untrusted
roots and any retained unresolved non-child handles. Do not return through
main or Tokio runtime teardown on this path. An incomplete barrier never
produces a success report, even if process exit is possible. The OS releases
the original flock at process exit.

## Qualification

Use fresh disposable native profiles and the registered subprocess driver.
Exercise the healthy stop with and without a started engine, real recovery
from a subsequent stock process, reader drain, and a verified startup error.
Use actual dispatcher replacement during a graceful effect and while
waiting for the engine's stop turn. Check stale successful IPC and reporting
paths, retained flock and exact native database/sidecar images after the last
admitted call, original engine and child ownership, and actual listener and
connector shutdown. Synchronize a held child after cancellation: verify the
process stays up with the lease and an incomplete diagnostic, then releases
only after the observed child release marker. An injected failed observer
must not be mistaken for completion. Preserve the existing stock Linux and
macOS stop regression coverage. Obtain exact Claude Code Auto review.
