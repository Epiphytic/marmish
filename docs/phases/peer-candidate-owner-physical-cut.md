# Stop the ordinary owner after dispatcher loss

This increment implements steps 3–5 of
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md) for a still-owned
ordinary `MdkOwner`, based on `65fcfae`. The caller first excludes all other
owner turns and holds the exact shared profile lease. This increment neither
mounts daemon shutdown nor takes over a Command engine that has already moved
to the graceful keeper. It never deactivates MDK or advances a journal, recovery,
cleanup, or finite-job turn. Dedicated Terminal, Peer and listener completion,
startup custody, graceful-to-fatal takeover, parked storage and process exit
remain composition responsibilities.

## Begin and observe

Use one Linux AMD64 test/candidate-gated child of `mdk_adapter`. A synchronous
`MdkOwner::begin_dispatcher_cut` checks the fatal observer already installed in
the owner's exact direct connector before changing anything. The caller may
only begin once, exclusively. The returned must-use `PhysicalCut` retains the
mutable owner borrow, physical handles and the original shared profile lease
while observation is pending. The caller retains its own copy of the lease
and parks the owner when observation ends. The cut has no methods for normal
owner work and cannot authorize clean shutdown or release of parked storage.

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
open; start exactly one SDK relay-context shutdown, after the raw ABI fence
is already tripped. At locked MDK `bdf9833`, `NostrSdkRelayClient::shutdown`
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
A socket reap alone says nothing about SDK or forwarder task completion.

Observe engine, runner, SDK context, forwarder and raw slots concurrently
under a single caller deadline. Timeout preserves each unfinished handle
or slot obligation for retry. Store proof as each step actually completes.
Classify `Complete`, `Pending` and `Incomplete` distinctly. A permanent join
failure never becomes success after the handle is consumed. Finish independent
proofs even if one has already failed. Pending is only an observer outcome.
The integration caller must retain live or unproven custody and skip profile
cleanup, including if the observer is dropped. Do not make `Drop` run
SDK/MDK/Journal work. The caller must keep its exact lease (and owner, or park
it for fatal process exit), regardless of the observation.

## Validation

Run existing engine channel/join and raw multi-slot barrier regressions. Add a
small fixture using the actual owner and a direct connector carrying a test
trip. Verify live or unfenced entry refuses before modifying state. Verify
normal forwarder joined termination and independent timeout/retry with a
gated actual Tokio task. Make its branch hold completion through the first
deadline and observe engine, SDK and/or raw-socket barriers progress before
unblocking it. A panic or cancellation must remain incomplete across repeated
observation. Where existing fixtures can be reused without production hooks,
exercise an actual live runner barrier and private local WSS. Any component
already covered separately remains a separate result from combined daemon,
storage retention, or full real dispatcher displacement.
