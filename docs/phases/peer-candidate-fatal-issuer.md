# Issue the candidate fatal signal from the registered dispatcher

The [candidate daemon design](peer-candidate-daemon-composition.md)
requires one process-wide cut when exact forwarding cannot be proved.
The existing registration detects an unsafe default and persists
Indeterminate. The Command prerequisite and raw transport need that
same failure signal. This phase connects signal provenance to the
registry. It does not mount the actor or the daemon stop path.

## Ownership and ordering

Create one shared keeper during successful one-shot dispatcher
registration. `Installed` becomes Clone-only and holds the exact
dispatch, descriptor and an Arc to the keeper. The keeper retains
the `Trip` for this installation. The registry, registered dispatch
and verifier refer to the same record. The Indeterminate state
retains the existing keeper, if registration previously succeeded.
A recovered or second lease cannot replace it. Stock and the older
fixture registrar keep their respective paths. These private
components are only built on Linux AMD64 under the existing test
or candidate configuration.

One shared core contains an `AtomicBool` and the existing
`CancellationToken`. `Trip` stores true with Release before
notification. `Fatal::is_tripped` reads it with Acquire. Only `Trip`
can cancel the token, and the wake method always performs the Release
store first. `Fatal::tripped` returns immediately if it sees the cut
or registers the token wait. Raw I/O keeps its check-then-poll order.
A cut between those checks therefore returns promptly or wakes the
registered waiter. Cancellation means the atomic cut already happened.
The cut flag never returns to false.

Use one small owning registry guard for the production check and
register paths. It records Indeterminate with the previous keeper
and marks the cut while locked on all failure paths. Its destructor
drops the underlying mutex guard, then performs idempotent token
notification. On unwinding with the mutex held it records
Indeterminate and marks the cut before releasing the guard. Poison
is always fatal, and recovers the held guard only to record failure
and release it, with no database or verification work. Keep a
known verifier's exact keeper as a backstop on that path. Each
registered Indeterminate return re-notifies after unlocking, including
a duplicate register that discovers replacement or an already fatal
state. This prevents one preempted or interrupted discovering thread
from being the only waker. Failures before any committed registration
have no live signal and preserve the established error return.

Local lease, directory, Ready config and scope refusals leave
ordinary work live while forwarding remains exact. A restored default
cannot clear a cut. The narrow unsafe-close capability for the
future Command connection guard captures that same keeper and may
only mark the cut and notify. It performs no SQL or registry work.
Every later registered observation, attach bracket or duplicate
register marks the registry Indeterminate on seeing this cut even
if physical forwarding still matches. The signal carries a
stop condition. The typed result distinguishes dispatcher fatal
from normal stop or local refusal.

`ForwardingVerifier` exposes a checked read-only observer from the
keeper, with its tripper private. A separate non-publicly-constructible
custody fault handle comes from the same observer and cannot be
created from a `Fatal`. A fresh generic private latch only has
authority in tests. In particular, when the raw fence is composed,
`RegisteredRoute` has a private issuer field carrying the exact
registry observer, and its recipient admission consumes that route
and derives the raw `Fatal` internally. No installer or route
consumer can pass a separately created signal to that entry.
The generic transport's internal test fixtures may still construct
independent pairs. The future installed Command site follows the
same sealed-pair rule.

## Evidence

Isolate process-wide SQLite default mutations in subprocesses.
Extend the real registration cases to prove synchronous cut on
displacement before and after route take, no cut or wake on exact
local refusal, permanent failure after restoring the default, and
a normal stock open following local refusal. Cover displacement
detected by duplicate registration while its original verifier
and raw observers are alive. Verify the mutex is available to a
custom Waker at notification time. A distinct deterministic case
pauses the first notifier after unlock and verifies that a second
observer of Indeterminate wakes the real waiter. Test unwind and
the poison path with a known observer in isolated processes.

On the core signal, confirm a woken reader always sees the set
flag, a trip between raw check and waiter registration cannot
lose its wake, and repetition is harmless. Custody notification
must work while the registry mutex is held and remain sticky
through a later verified observer. Prove private WSS observes the
actual registered cut under the composed raw fence and disconnects
without sending a protocol Close. A scratch negative compile
proves the route admission signature rejects a separately supplied
latch. Run stock and candidate all-target strict Clippy. The installed
stop path still needs its own join, lease and cross-owner proof.
