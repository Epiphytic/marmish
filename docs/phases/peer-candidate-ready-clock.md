# Start the candidate clock at the second readiness edge

The [Manual actor design](peer-candidate-manual-actor.md) gives the
private receiver at most 15 seconds for local recovery and its first
authenticated route. The existing startup handshake knows both readiness
edges but stores only their boolean completion. A worker scheduled late
must spend that delay from the same allowance.

Store `std::time::Instant` in the Ready variant of the existing watch
state. Sample it under the watch update, only when the second distinct
edge completes readiness. Duplicates preserve the original instant.
Stop still replaces every state and remains absorbing. Store no time
for incomplete readiness and create no additional readiness channel,
thread or timer. The process monotonic clock is shared by actor and
listener threads on independent runtimes.

The worker's Ready observation borrows the existing watch and retains
the recorded instant. Expose a candidate-only `startup_remaining` helper
on that observation for the actor's initial local and WSS checks. The
helper samples the process monotonic clock and returns None when
readiness has been revoked, when the observation predates the edge, or
at and after the 15-second boundary. A private same-clock instant input
enables deterministic tests of that calculation. Both local and WSS
checks measure the remainder against the original edge. The actor will
use that duration together with independent liveness, signed expiry and
TLS validity checks. The latter use wall time across suspend. This
budget is an admission guard. It does not interrupt blocking operating
system or database operations. The first native five-second turn and
its own liveness checks stay separate. This prerequisite adds no
authority, database operation or socket, and leaves the worker unmounted.

Extend the readiness tests with both edge orders, delayed observation,
unchanged budget after duplicates, and immediate revocation. Test the
boundary with supplied Instants inside the owning test module to avoid
sleeping or cross-runtime clock emulation. Rerun the existing
startup and private WSS lifecycle regressions, stock and candidate
all-target Clippy. Preserve the existing loss tests and behavior in the
stock shipping binary.

Claude Code Auto approved the design at `2cea9fb` in session
`a8d71ed3-be89-43a4-999c-00e98a134442`.
