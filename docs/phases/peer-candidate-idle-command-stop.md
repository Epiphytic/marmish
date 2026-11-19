# Quiesce an idle registered Command engine before owner stop

This amends [registered owned stop](peer-candidate-owned-stop.md) at source
`9dea56c`. It applies to a private Linux AMD64 registered process that has
entered the active loop and then requests a healthy shutdown. It does not
change stock stop, startup-failure stop, or the first registered client
qualification with no Command engine. Active or uncertain finite jobs retain
their existing independent lifecycle. This phase grants no new IPC, command,
relay or filesystem cleanup authority.

## Ownership and budget

Start at `daemon_candidate::finish` after `serve` has relinquished the sole
request receiver and no request handler remains. Signal listener drain once
and retain its original join. Close `CommandService` admission and apply the
existing Terminal and runner input retirement in the same exact ownership
transition as `HealthyStop::begin`. Keep the engine inside `CommandService`
until the prelude reaches a disposition. The existing `begin` checks tying
exact owner, startup profile lease, engine admission and connector fatal to
one registration must run before the first prelude effect. Factor that entry
once. A NeverStarted engine bypasses the prelude. Startup or active-loop
error and an already tripped admission go directly to the existing stop.

Use one **six-second total prelude deadline**, measured from starting
listener drain, across Begin, polls, time spent queued for the blocking pool
and observing their results. Pass the same monotonic instant to the blocking
turn and convert it with `tokio::time::Instant::from_std` for the async wait.
The first poll and the poll after measured progress may run immediately.
Count an actual new row registration, completion commit, internal ACK, cursor
advance or consuming enqueue as progress. After a Draining poll without one
of those changes, wait ten milliseconds or until fatal or expiry. Never spin
on an unchanged state or replay delayed timer ticks. This admission and
acceptance budget is separate from the one-second physical observation slices.
After a proved prelude, transfer the exact owner and lease synchronously and
return pending physical evidence. The next observation gets a full physical
slice for the single graceful attempt.
The existing 15 s incomplete diagnostic still measures from shutdown entry,
including the prelude. The engine's five-second native acquisition budget,
SQLCipher busy waits or scheduling can outlast the remaining prelude.
Nothing depends on interrupting those calls or on a hard exit by six seconds.

The first implementation gate is a compile-time assertion that
`Box<MdkOwner>: Send + 'static` in the private candidate configuration. At
this revision only the engine uses that blocking handoff. If the whole-owner
assertion fails, stop and revisit the design without adding an unsafe Send
wrapper or touching source outside this phase.

Own exactly one blocking turn at a time. Move the existing guarded boxed
owner, without replacing the Journal or engine with temporary state, into a
`spawn_blocking` turn that returns it and its typed result. Keep the owner
under `ManuallyDrop` in the turn and preserve the original lease in the stop
coordinator. A queued closure checks the deadline and exact registration
before the call. Retain the original join across timeout, fatal notification,
and retries of observation. Model the prelude as `InOwner`, `InTurn` with
the retained join, and `Disposition` with restored owner and `Proved`,
`Failed` or `Expired`. A lost ownership proof is a separate parked possible-
live state, with no returned owner or disposition. Observation/reporting,
complete and destruction must handle each state explicitly. Dropping an
observer may not drop the join or native owner. The listener is independently
driven during the prelude.

Use fatal-biased async observation and re-read the sticky latch after
receiving the join, even when a result and fatal arrive together. Check time
and admission immediately before and after each synchronous call. Expiry or
fatal closes admission for further prelude turns. An already-running turn
can finish or remain pending. A cut during an admitted synchronous call is
indeterminate, so no later trusted effect can rely on that result. Exact
Journal and worker storage fences remain in force. While the turn holds the
owner, no stop observer may access it, release it, claim graceful completion,
start SDK/forwarder shutdown, cancel runners or actively reap raw sockets.
The listener and raw transport's existing fatal signal remain independent.
Fatal during a turn leaves the still-owner-held engine possibly live. Once
the same join restores the owner, extract with `take_engine_for_stop` and
use the existing `can_join_dispatcher_cut` then `into_fatal_join` checks
before running the remaining physical barriers. On panic or unreturned
ownership keep possible-live disposition and the original lease, with
existing bounded observations and diagnostics. Never create a replacement
owner, engine, stop turn, or cleanup proof to get past a missing join.

## Exact worker and durable handoff

- Invoke `command_begin_cleanup` once on the admitted idle owner. Its
  `begin_cleanup` checks durable active, uncertain, worker and observation
  counts, Ready state, and in-memory completion, ACK and queues before it
  allocates a durable sequence fence. Recheck the deadline and registration
  immediately before allocating the fence after those potentially blocking
  reads. The candidate prelude also checks the
  engine's existing `copy_gc_pending` flag before any fence allocation.
  `progress_streams` can have queued `CopyGc` during active service, and
  ordinary `observation_pending` does not count it. The flag covers queued,
  executing and completed-but-unapplied reclamation. Inspect it without a
  worker poll or `take_copy_gc`. No normal progress runs after intake
  closes, so the owner cannot enqueue another reclamation during the
  prelude. Busy or refusal enters physical stop without admitting a new
  reclamation. That disposition adds no promise about whether a previously
  admitted worker control already ran or finishes before worker stop.
- Idle means no work that needs this engine to keep running, using these
  checked conditions. Unanswered request slots and unsent publications may
  still exist. They remain durable for ordinary recovery after a clean exit.
  This is neither an exit-ready/release check nor a relay publication drain.
- Invoke only `command_poll_cleanup` with that same retained owner until a
  poll returns same-fence `Quiesced`, time expires, or it refuses. The fence
  is kept by the coordinator; `Draining` is pending, `Open`, `Failed`, missing
  or mismatched fence and any error fail the prelude. If the engine leaves
  Ready or disconnects, fail immediately instead of repeatedly accepting
  `Draining` from a poll that cannot progress. A positive result
  requires exact checked admission and time on both the blocking turn and
  observation side. Carry the success as an explicit private stop
  disposition, independent of later physical `worker_joined` and
  `StopReport.quiesced`. A late `Quiesced` observed in `stop_outcome` cannot
  turn a failed or expired prelude into success.
- The existing poll performs durable Journal work. It may register one
  requestless `FinalObserve` at a time before asking the worker to observe
  the exact retained row, commit its corresponding completion, and send the
  exact internal engine `CommitAck` after commit. A fresh fence requires
  refreshing every retained server record, so work scales with retained
  history and can exhaust this budget even for an otherwise idle engine.
  One row per waiting interval would allow fewer than 600 refreshes in six
  seconds before database and scheduling costs. The Journal accepts up to
  10,000 records globally. Reopening or repeating stop does not help a
  large backlog because each stop allocates a fresh fence. Record this
  limit on the private proof; large-profile graceful stop is not qualified.
  Reuse the established per-row transaction and authority checks. Do not
  add a synthetic observation, batch SQL update or queue-loss recovery to
  reduce that cost. A stalled bounded request or ACK queue falls back to the
  existing physical disposition. Candidate prelude progress checks each
  completion commit, ACK, next registration, FinalObserve enqueue and
  consuming Cleanup enqueue at its effect boundary. If a
  synchronous subeffect has already started, post-check it and stop the
  current poll on loss or expiry. Stock poll keeps its current behavior.
- Internal ACKs differ from a Marmot ACK. The prelude reaches no relay
  publication, receive or dispatch, no ordinary replay or schedule, and
  requests no Cancel, launch, or reclamation. The empty-at-Begin checks and
  requestless `FinalObserve` chain are prerequisites. If a completion is
  unrelated to this preparation, or if it projects a response publication,
  refuse before new durable publication or internal acknowledgement. Keep
  the ordinary startup and active-loop recovery for retained requests.
- Only the worker's `Control::Cleanup` can select `Engine::into_cleanup`
  after pending, awaiting and unresolved work are empty. That operation
  checks terminal job/process identity, obtains the existing worker/IO
  locks and closes engine storage. It does not delete retained jobs. Its
  `Cleanup(Quiesced)` event is the existing consuming engine proof. Candidate
  worker operations continue to use their exact dispatcher admission. Stop
  does not quiesce the engine. An already admitted/queued read-only
  FinalObserve or consuming engine cleanup can finish as stop begins. In
  particular, expiry after `consuming=true` can leave the engine store
  closed and held by its guard until worker join. That is no permission to
  clean up Journal/account/runtime. After physical barriers it still means
  nonzero exit and lease retained through exit. No late outcome can restore
  permission for trusted cleanup.

After the first failed/expired decision, do not actively call `fail_cleanup`
merely to clear state, change a durable invocation state, or manufacture an
ACK. Existing in-poll failures may already have called `fail_cleanup` via
`poll_completions` or `advance_cleanup`. The coordinator does not repeat it. Stop may
collect worker events to unblock its bounded channel but must not commit or
acknowledge further completions. A dropped requestless FinalObserve event
leaves its registration in the Journal until existing cold
`command_initialize` clears registrations with `operation_request IS NULL`.
If the completion commit already won, that transaction set the new
observation/revision and cleared the operation; loss of the runtime ACK
cannot roll it back. Protocol-request registrations and durable uncertainty
are not erased by this prelude. For either case rely on the existing
stock/new-frontend recovery paths, never retry Submit. A fatal or expired
result is never treated as a negative proof that the previous write did not
happen.

Continue through the same physical stop and existing three guarded trusted
cleanup effects. For a started engine add `prelude_proved` to
`HealthyStop`'s `graceful_complete` predicate alongside explicit actual
worker join with `complete && quiesced`, all other ownership barriers and
clean listener drain. An idle
prelude timeout may ultimately leave a physically complete worker but must
skip those effects and retain evidence and lease through nonzero process
exit. Existing startup failure, NeverStarted and uncertain-custody rules
continue unchanged. No graceful stream or account projection is started on
prelude failure; use physical teardown. Sticky fatal always dominates.

## Qualification

- Use the private source-matched registered executable with an idle real
  started Command engine, then with at least two actually retained terminal
  results. Observe the fence, each durable requestless refresh and internal
  ACK, production Terminal authority retirement, actual engine join,
  socket/listener/SDK closure, cleanup and exit zero.
  Reopen stock on the same disposable profile and verify retained results
  and no second launch. Retain no command output, SQL or key data in reports.
- Prove active, uncertain and pending Copy GC refusal before allocating a
  fence. The Copy GC cases include queued and already completed but
  unapplied work. Use an owned
  test gate to hold a bounded retained-row backlog or completion past the
  deadline. Cover registration committed before completion, completion
  committed before runtime ACK, expiry after the consuming enqueue, and
  late `Quiesced`. Test a mismatched or missing fence via a unit seam at
  result acceptance; the real service's sole preparation owns its fence.
  Assert no further prelude write or ACK, actual physical end as available,
  nonzero exit and no cleanup. Compare durable rows in a fresh stock process,
  including successful reopened engine storage after a late consuming turn,
  with unchanged invocation result/output/replay classification and no
  lost or duplicated launch. An independent finite worker never inherits
  the profile flock.
- Trigger actual dispatcher displacement during a poll at an identified
  subeffect boundary and while the observer owns an unfinished blocking turn.
  Verify the listener drains, ownership of the one join is retained until
  return, no post-cut durable effect begins, and the exact DB families at
  the last completed admitted effect remain available for recovery. A panic
  or unresolved owned turn cannot prove the process may exit. Exercise
  observer cancellation separately from a normal return.
- Re-run stock stop and registered NeverStarted/starter-failure handling.
  The no-engine first private qualification remains independent. Report
  prelude time and retained-row counts in opt-in qualification, keeping slow
  process cases out of the core CI subset. Obtain exact independent Auto
  code review before composing the later daemon increment.
