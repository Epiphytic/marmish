# Issue the candidate fatal signal from the registered dispatcher

The [candidate daemon design](peer-candidate-daemon-composition.md)
requires one process-wide cut when exact forwarding cannot be proved.
The existing registration detects an unsafe default and persists
Indeterminate. The Command prerequisite and raw transport need that
same failure signal. This phase connects signal provenance to the
registry. It does not mount the actor or the daemon stop path.

## Ownership and ordering

Create one shared signal during successful one-shot dispatcher
registration. The installed record and its verified observers retain
the same signal. The registry remains the sole production issuer for
this path. A recovered or second lease cannot replace it. Retain the
existing exact VFS dispatch and descriptor identity checks. Stock
and legacy fixture registration keep their own paths.

The shared signal separates a sticky atomic cut from asynchronous
notification. Under the existing registry lock a failing exact
forwarding check first records the permanent state and synchronously
sets the cut. A watcher or raw poll can observe the cut before any
later operation is admitted. Notify the same cancellation token
after releasing the registry lock so a wake cannot reenter it. The
monotonic flag is the synchronous source of truth. The token wakes
existing reads and writes and asynchronous stop waiters. The healthy
path publishes no event. If the lock is poisoned, the verifier
trips its exact already-issued signal and returns Indeterminate.
A registration that fails before it has supplied an observer refuses
startup at its existing error boundary.

The signal is one way. Local lease, directory, Ready config and
scope refusals leave ordinary work live while forwarding remains
exact. A restored default cannot clear a cut. If the separate
Command connection guard needs to report unsafe close, its exact
observer supplies a narrow capability targeting this same signal.
That notification sets the cut and wakes waiters without SQLite or
the registry. A later registered verifier treats the cut as
Indeterminate even when the physical default still matches.

Expose read-only access through the exact `ForwardingVerifier`.
When the reviewed raw route fence is incorporated, private
`RegisteredRoute` carries this signal from the registered scope.
Its candidate recipient admission consumes the route and its signal
together. No candidate call can pair an arbitrary read-only signal
with a registered route, and stock and fixture admissions keep their
existing configuration. Test-only producers may still simulate raw
failure in independent unit fixtures. Keep normal typed Peer stop,
local refusal and dispatcher shutdown distinct.

## Evidence

Isolate mutations of SQLite's process-wide default in subprocesses.
Extend the registered real SQLCipher cases to prove same signal
identity and synchronous cut on displacement before and after route
take, no signal on exact local refusal, no recovery after restoring
default, and a normal stock open after local refusal. Cover registry
poisoning with a known observer in a separate subprocess. Exercise
the independent observer's bounded wake and a failing custody
notification without acquiring the registry lock or invoking SQLite.
Prove private WSS observes the actual registered cut under the composed
raw fence and rejects further operations while disconnecting without
a protocol Close. Verify stock all-target and candidate strict Clippy.
The eventual installed stop path has its own join, lease and
cross-owner proof.
