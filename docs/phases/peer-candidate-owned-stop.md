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

Track engine custody explicitly as NeverStarted, InOwner, InTurn with a
join, Joined, or Uncertain. InOwner and InTurn both own or account for the
same engine. A temporarily empty CommandService slot is not Join evidence.
Drive the existing stop_outcome in bounded blocking turns. Continue promptly
after an ordinary incomplete result, so event collection releases a worker
blocked on its bounded queue. Discard all drained requests/completions
without publication or acknowledgment. On Fatal or Err, stop driving the
ordinary turn. The blocking observer returns the engine with its typed result.
Cancellation of an observer retains its join. If the join panics before
returning the engine, classify the handle as possibly active and retain the
profile. After the normal observation budget, a stuck worker keeps the
process in an incomplete HealthyStopping state with periodic diagnostics.
A later cut interrupts stop_outcome through its existing sticky check and
transfers the in-flight turn before converting the returned engine into
into_fatal_join, or retaining it explicitly if conversion refuses.

Separate graceful account deactivation from transport termination. Admit
actual deactivation and each resumed durable effect independently. Race
bounded async progress with the Fatal wake, and re-read the sticky flag
after every await, including when mark_cut precedes the wake. Keep all
started resource handles outside cancelable futures and in custody.
Cancellation without such ownership is uncertain. Obtain SDK shutdown and
nested forwarder proof and connector quiescence through the fields used by
the physical cut. Do not use the stock shutdown that consumes or aborts
those joins. Preserve the exact runner, SDK, forwarder and raw observations
on the fatal transfer. A held join is always transferred and never launched
again. Check exact Admission and Fatal before Journal close, native account
close, and the final trusted runtime cleanup. Re-read the latch after each
effect; a mid-effect cut retains indeterminate disposition even if that
already-admitted synchronous effect ran. There is a check-to-unlink window
in cleanup_runtime because it uses plain filesystem calls. It is the last
effect and its outcome can only be published after the post-effect check.

## Disposition rules

| Obligation | Complete evidence | Incomplete behavior |
| --- | --- | --- |
| IPC listener | Owned join returned normally and socket intake revoked | Listener panic or drain error is failure. This task owns no child in the registered Command path. Preserve unresolved socket evidence. |
| Command engine | Absent with NeverStarted proof, or actual engine join with retained outcome | Pending blocking turn or lost engine/worker ownership is possibly active. A normal worker thread join returning engine_thread_failed is a failed stop, and is exit-eligible only with independent child evidence. |
| Stream runner | Finished marker plus snapshot without uncertainty | A pending marker, try_exit failure, or caught panic with uncertain snapshot can mean a live child. Preserve the process and flock until actual reap evidence. |
| SDK and nested forwarder | Actual successful shutdown and nested forwarder join | JoinError or lost handle is incomplete. A pending or lost task is retained as possibly active until there is independent end evidence. Outer abort alone proves neither. |
| Raw transport | All selected raw ownership slots confirmed released | Retain unresolved socket evidence. Incomplete raw state produces failure even after all child and task obligations have positive evidence. |
| Journal, account, runtime | Each trusted operation guarded and completed without a post-cut observation | Preserve remaining database roots, runtime evidence, and lease until process exit. Report an error even if every owned execution obligation is physically complete. |

Partition every settled result into complete, failed but no remaining
child/task possible, or uncertain with possibly active ownership.
Healthy stop can report success only with all required proofs. Even without
a fatal trip, a failed but exit-eligible stop retains roots and the flock
through nonzero process exit. An uncertain healthy stop holds the lease
and continues bounded observation. A later actual dispatcher loss transitions
the same remaining ownership to FatalPhysical. A Journal/MDK operation that
reports an indeterminate close must use that leaf's trusted-retention and
failure signaling path. Incomplete never authorizes destructors while
default trust or physical ownership is uncertain.

## Fatal and process exit

The existing physical cut begins with an owner that has never entered
graceful stream stopping. Add an explicit consuming transfer from the
registered healthy stop, including explicit engine custody and the same
runner, SDK, forwarder, raw and listener observations. A missing engine
requires NeverStarted or actual Joined proof. A pending blocking turn stays
an observed engine barrier. Do not loosen
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

When the native roots have been parked, every owned child is positively
observed reaped or proved never started, and no possibly active task retains
its authority, terminate the qualification process nonzero with the disposition. Use ManuallyDrop or forget for untrusted
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
only after observed child release. Inject a runner exit-observation failure
or panic and ensure an uncertain runner cannot authorize exit. Interrupt an
engine turn in flight and between turns, inject a panic that loses engine
ownership, force a full event queue during healthy stop, and trip after the
SDK task starts. A failed observer never supplies missing child or nested
task evidence. Assert that no independently handed-off finite worker inherits
the daemon flock descriptor. Preserve existing stock Linux and macOS stop
regressions. Obtain exact Claude Code Auto review.
