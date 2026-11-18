# Stop the ordinary owner after dispatcher loss

This increment implements steps 3–5 of
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md) for a still-owned
ordinary `MdkOwner`, based on `65fcfae`. The caller first excludes all other owner turns and holds the exact shared
profile lease. Daemon shutdown mounting and takeover of an engine already
moved to the graceful keeper remain separate. The cut leaves MDK deactivation, journaling, recovery, cleanup and
finite-job turns disabled. Dedicated Terminal, Peer and listener completion,
startup custody, graceful-to-fatal takeover, parked storage and process exit
remain composition responsibilities.

## Begin and observe

Use one Linux AMD64 test/candidate-gated child of `mdk_adapter`. The
synchronous begin function consumes a boxed ordinary owner and requires an
active Tokio runtime. A precondition refusal returns the same owner without
mutating it; the caller still applies candidate custody policy. Check the
present, already-tripped observer from the exact direct connector, refuse
`streams.stopping` (including an owner that already gave away its engine to
graceful stop), and check any still-owned engine against the exact same Fatal
using three read-only identity predicates on Fatal, Admission and CommandEngine.
A missing engine on an owner which has not begun stop is valid (e.g. before
Command initialization or a Command profile with no configured or historical
execution principals). A consumed owner cannot start the cut twice.

The leaf is internal to `mdk_adapter`. Its checked constructor pairs the owner
with the lease supplied by its single custody owner and checks profile binding
as consistency only. The later production wrapper takes the Arc directly from
the owner's sealed startup AccountCustody, and never accepts a same-name caller
substitute. The isolated fixture creates the owner from the same test lease.
On success the must-use `PhysicalCut` holds a non-resumable parked ordinary
owner with that lease. Its fallback Drop parks both without running their
destructors. Started closure tasks may finish independently if a caller drops
the cut, but an unobserved handle is never completion evidence. `finish`
consumes the cut only after every component settles, returns the parked custody
with the final report and preserves self if still pending.

Before any await, retire Terminal binding authority, pause the publication
gate, mark the owner inactive, set stream stopping and close command admission.
Retain every existing live runner barrier, suspend its input and request
cancellation. Do not clear live state or poll worker results. Extract the
already-owned Command adapter through the existing service, disconnect its
channels through `into_fatal_join`, and start the existing lease-holding
`keep_fatal_engine`. A missing engine is acceptable only for an owner still
holding its own lifecycle, such as before Command initialization. Unexpected
refusal retains the returned engine and marks the cut irrecoverably incomplete,
while still starting every other stop. The caller must park this custody as
well. Already authorized finite jobs in separately exec'd workers survive.

Cancel connects using the existing cancellation that leaves slot semaphores
open. After the raw ABI fence is already tripped spawn exactly one SDK
relay-context shutdown task and retain its join handle in the cut, independent
of any observer timeout. At locked MDK `bdf9833`, `NostrSdkRelayClient::shutdown`
ends its underlying SDK pool, emits a local Shutdown notification and aborts
and joins its owned notification monitors. That notification gives the
already-owned notification forwarder a termination path that awaits its nested
delivery worker. Keep and observe the outer forwarder to successful termination.
Outer abort followed by a cancelled join only proves the outer task ended; its
drop guard aborts the nested worker without joining it. Any observed failed or
cancelled join is incomplete. The new helper does not use outer abort to turn
a stalled forwarder into success. Existing direct raw transport rejects read,
write, flush and shutdown after fatal, including SDK protocol cleanup bytes.
Use `reap_fatal_sockets` across all exact configured route slots; require both raw
halves to drop, and close admission semaphores only once all slots are held.
Treat timeout as pending, other errors (including slots already closed elsewhere)
as permanently incomplete, and cache success once. Timeout releases temporarily
acquired slots and retries the complete route set. A socket reap alone says
nothing about SDK or forwarder task completion. Successful outer completion
attests the nested joins were awaited, but the locked SDK does not expose
nested-worker or notification-monitor panic results.

`observe(&mut self, deadline)` polls engine, runner, SDK context, forwarder and
raw slots concurrently under a single caller deadline. Runner readiness uses
async sleep to poll the actual child-release marker. Store each component's
proof atomically with consumption of its handle so the future is
cancellation-safe. Timeout or cancellation preserves unfinished handles and
raw slot obligations. Never retry completed raw shutdown. Report the individual
`Complete`, `Pending` and `Incomplete` results, including a never-spawned
forwarder as complete. Treat a present completed forwarder as complete only
after awaiting its join successfully. The keeper's worker panic and an SDK or
forwarder outer join failure stay incomplete after the join is consumed. Retain
other pending obligations even when one has already failed. No result
authorizes native storage close, runtime cleanup, a clean report or lease
release. The later daemon consumes the parked custody for fatal process exit.

## Validation

Run existing engine channel/join and raw multi-slot barrier regressions. Add a
small fixture using the actual owner and a direct connector carrying a test
trip. Verify unfenced, untripped, already-stopping and wrong-engine-latch entry
refuse without changing the owner. A previously closed connector slot produces
an incomplete cached result. Prove Terminal binding retirement against a held
fence and stream child release through the real runner's barrier. Observe
normal pinned SDK forwarder termination and exercise timeout, cancelled
observation, retry and sticky task failure with gated actual Tokio tasks.
Ensure SDK shutdown or raw-socket completion can advance before gated
forwarder release. Hold the original lease against a second acquisition during
that cut and while the owner remains parked.

Use the existing disposable loopback TLS WSS fixture to test actual raw
connection termination, pin the relay's subscription/close transcript across
the cut, and compare the exact pre/post Journal database and sidecar bytes
without further native calls. This measured subset is scoped to the local
owner cut and reuses prior lower-level raw ABI tests for per-poll forbiddance.
It is not a claim about all traffic at daemon scope, a production sqlite
dispatcher replacement, or storage-destructor behavior. Preserve previous real
engine-channel and multi-route slot qualifications.
