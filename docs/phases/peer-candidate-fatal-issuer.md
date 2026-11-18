# Issue the candidate fatal signal from the registered dispatcher

The [candidate daemon design](peer-candidate-daemon-composition.md)
requires one process-wide cut when exact forwarding cannot be proved.
The existing registration detects an unsafe default and persists
Indeterminate. The Command prerequisite and raw transport need that
same failure signal. This phase connects signal provenance to the
registry. The actor and the daemon stop path remain separate phases.

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
or registers the token wait and checks the flag with Acquire again
after waking. Raw I/O keeps its check-then-poll order.
A cut between those checks therefore returns promptly or wakes the
registered waiter. Cancellation means the atomic cut already happened.
The cut flag never returns to false.

Use one small owning registry guard for the production check and
register paths. It records Indeterminate with the previous keeper
and marks the cut while locked on all failure paths. It retains
the guard in an Option and clones the exact keeper while locked.
Its destructor takes and drops the underlying mutex guard, then
performs idempotent token notification. On unwinding with the mutex held it records
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

## Sealed construction

The registry module owns `RegistryMint`, with private fields and
a private constructor. Neither the mint nor the Command fence implements
Clone, Copy or Default. The only production call to
`Trip::registered(RegistryMint)` is the registrar after the physical
post-install checks succeed. `Trip::new` for an independent pair is
`cfg(test)`. The keeper's `Arc<Trip>` is never exported. A crate
caller cannot build `RegistryMint` directly.

`ForwardingVerifier` exposes a read-only `Fatal` for observation
and its guarded recheck. The verifier's tripper is private. The
actual route's single successful transfer, after the final global
bracket, adds its read-only signal to private `RegisteredRoute`
alongside route and signer. The reviewed raw fence's candidate
entry takes only this sealed route. Unpacking it supplies the exact
signal to the raw dial. The fixture-only path is compiled separately.

The registered receiver also retains one private
`RegisteredCommandFence` created at the successful registration
handoff. It takes the verified original scope's observer and its
narrow custody fault handle. Only the module defining
`RegisteredReceiver` can construct it. The registrar has already
verified original local scope before this handoff. Expose a single
take on `RegisteredReceiver` before ordinary initialization, after
a fresh global check. Do not require a second local lease check at
this point. The approved startup order must keep ordinary work
available when subsequent Peer-only checks fail, and attaching the
actor waits for ordinary readiness. A second take is refused. A global failure on take returns
`RegistrationFailure::Indeterminate`. A registered startup that cannot take
the fence propagates the refusal. Stock admission is available only when no
candidate registration succeeded. The fence authorizes only use of the exact registered forwarding
monitor for ordinary setup and custody, never Peer SQL or relay use.

The Command composition consumes this one sealed fence in
`Admission::from_registered`. The admission uses its exact
`Fatal`, recheck and custody fault handle. The latter permits only
a one-way cut, and cannot be constructed from a read-only `Fatal`.
Independent raw `Admission::new` becomes test-only in the composed
candidate, including when the Command leaf offers the temporary
paired `Arc<Trip>` constructor. The underlying transport may take
a plain latch internally, with no production constructor accepting
a plain latch for a registered receiver. The issuer may implement
the registered Command fence before the engine leaf merges, leaving
consumption unmounted until the two reviewed changes compose.

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
latch. Candidate non-test probes also reject direct mint creation,
`Trip::new` and the raw `Admission::new`. Test that an already taken
route and a Command admission from its registered owner both observe
the cut. Before full composition the same proof can use the sealed
Command fence's observer directly. Run stock and candidate all-target strict Clippy. The installed
stop path still needs its own join, lease and cross-owner proof.
