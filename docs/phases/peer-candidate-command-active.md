# Admit active Command turns on a registered candidate

This design refines [daemon composition](peer-candidate-daemon-composition.md)
for the private Linux AMD64 Command qualification. Source base `c990cf9`
combines the checked native owner at `c033631` with the reviewed
[Command startup adapter](peer-candidate-command-startup.md) at `a992a18`.
See also the outer run admission plan at
`e072a5a:docs/phases/peer-candidate-run-admissions.md`. Use the existing candidate build gate and track composition in
`marmish-dwt.4.12.2`. This phase defines owner-side turns and their required
caller contract. Production mounting remains a separate reviewed composition.

Only Status, finite Command exchanges and existing authorized ClientInventory
are exposed during this private qualification. Reject generic
OpenConversation/Send/Received, Stream intake, Terminal attach and Peer before
they reach owner work. Stock keeps every existing operation. A profile may
retain generic, Command and Stream rows, sessions and pending transport
obligations from stock operation. Shared maintenance must still guard these
operations. Transport does not become another profile's policy and there is
no new purpose, native format, store migration, or fresh replay ciphertext.

## Authority and execution boundary

- The registered daemon derives one owned Command `Admission` from its
  original registration. Use clones from that exact value for startup,
  the engine and the active owner. Stock takes the existing path. The
  candidate owner retains the checked admission associated with the owner
  as it moves out of startup custody. Do not add a setter callable on a
  stock or already active owner. The eventual caller retains the same
  profile lease, the read-only `Fatal`, and the selected physical custody.
- Use a small shared synchronous check and an async poll boundary based on
  the same exact admission. At each independent owner effect, check before
  evaluating its closure or polling its future for the first time. On each
  actual later poll, repeat the check before handing control back to the
  suspended effect. Use a lazy async body for a call with eager arguments.
  Check after a ready result before returning authority or entering a
  distinct decision. The normal path has no extra registry work. One
  already-admitted synchronous body or in-progress upstream poll may
  finish. This promise gives no cancellation inside a Rust poll, SQLite
  operation or kernel syscall. Release no mutex across an await, add no
  worker, sleep, executor, or downstream fork, and preserve all existing
  deadlines and budgets.
- Keep a checked outer select biased to the exact `Fatal::tripped()` in
  activation, listener, dispatch, tick, room work and reporting. The
  narrower checks still matter when several nested boundaries run in one
  poll while the daemon's outer select cannot run. A refused continuation
  returns the existing fixed `candidate_dispatcher_lost` storage category.
  Do not turn it into transport degradation, capacity backpressure,
  a room retry-cache entry, or a success reply. Preserve unrelated stock
  and healthy candidate error classification. Report custody failure
  through the existing exact `Admission` when needed to select fatal
  disposition. Do not continue the ordinary operation to generate a reply.
- Raw transport fencing is independently required. SDK dials, subscriptions
  and the notification forwarder can outlive a given future poll. Only a
  connector already fenced by the same registration may activate, and returned
  transport resources are retained before further fallible checks. Inner
  publication and subscription calls use existing `PublicationGate` and
  transport when wrapped as an admitted upstream call. Guard a next owned
  route/transport command independently and require the physical connector
  cut on fatal. Dropping a pending owner future is a cancellation request,
  not proof that raw I/O or an SDK task has exited. The separate physical
  disposition revokes and proves known tasks/sockets/workers drained and
  parks lost store custody. No graceful deactivation, Journal advance or
  purge occurs on that disposition.

## Concrete checkpoints

| Owner path | Admission boundary |
| --- | --- |
| Native and open-time | Keep `native_preflight::check`, `MdkOwner::open_candidate` and its checked connection and startup custody sequence from the native phase. Keep the Ready binding check and exact lease. Admit `reconcile_client_inventory` before its SQL turn and apply `set_retention` as ordinary in-memory setup before first maintenance. Independently admit the startup adapter's history, configure, binding, recovery and worker entry. No second preflight, identity, connector, registry or journal. |
| `activate` | Guard bootstrap, notification forwarder creation (retain handle immediately), each configured relay's connect turn, KeyPackage republish, `activate_transport`, first native drain, application effect projection, `replay_pending`, `submit_unsubmitted` and final Active. Route connect waits and native calls recheck after suspension. No successful status, parent readiness, or external listener authority is committed merely because the transport connected. |
| Status/exit/readiness | Guard `command_count_exit_ready` before its synchronous Journal/native read set, `status` before SQL, and independently check before exposing the result, honoring count exit or declaring Active. Keep the original `ServiceLifetime` and signal/limit ordering. Listener status cache and queued replies independently recheck at the actual IPC write poll even when `serve_owner` is scheduled out. Preserve explanatory activating/degraded state on healthy paths, with no stale success after fatal. See reporting below. |
| `dispatch` | Guard dispatch before `client_inventory`, `command_acceptance`, `command_local` and Busy-to-`command_enqueue_preparation`. Cover submit/consume/poll/cancel and fixture-only acceptance where reachable. Atomic, synchronous nested reads/writes in one admitted operation may finish. Check again before a separately selected mutation or submission and before returning a response. Keep short exchange expiry, ownership validation and exact retry behavior. |
| `tick_inner` | Guard each existing Command-completion/Stream-progress pass, source reconciliation, retention, route rebuild and independent recovery. Split recovery at forwarder replacement, each SDK reconnect initiation, reactivation, catch-up receive and `PublicationGate::resume`. Keep retries and the existing retained forwarder on an unfinished dial. Guard subscription synchronization, native drain, effect projection, receive, retained work and each publication pass. Recheck before the second completion/maintenance pass. Never start any of them because an earlier health result was good. |
| `receive_one`, `apply_effects` | Separate wait/read, native ingest, projection, adapter replay ACK and transport-group sync. After ingest or projection returns, begin neither ACK nor sync without a fresh check. Durable projection stays ahead of its corresponding ACK and preserves storage backpressure/error typing. Between independently processed records or batches, recheck before the next projection, pending deletion or invalidation write, including any command/stream/ordinary branch. |
| `command_open_room_turn`, `create_command_room` | Separate orphan reconciliation and selection, each deliberate acquisition/discovery drive, original token reservation and native `create_group`. After discovery cannot create based on an earlier admission. Post-native `accept_command_group`, any conditional forget/confirm, projection and subscription sync each need their own admission. If cut after preparation, ID, token and native interpretation remain subject to existing restart reconciliation. No fresh room, Welcome, or retry just because a result was interrupted. |
| Retained replay and retention | In `replay_pending` guard subsequent row disposition, in `drive_retained` each orphan or original Welcome redelivery and targeted `advance_application_convergence`. Separate existing Journal claim from native send, later bind/project, and each later row in `submit_unsubmitted`, `submit_command_publications` and `submit_stream_publications`. Distinguish native read, journal claim and nondurable retry bookkeeping from new sends. Protect fanout/queued intents/native invalidation witnesses and retain their normal order. In `maintain_journal` recheck before invalidation writes, Command maintenance and general maintain, and in `progress_streams` before preparation, completion/runner launch, observation and reclamation. Do not compensate or ACK on refused admission. |

Pinned upstream MDK `bdf9833` illustrates why guarding only the outer daemon
method fails. `AccountDeviceRuntime::republish_key_package` may fall back
to fresh publication, `drain` drives already queued output and retained
fanouts, `create_group` awaits native creation then publication, and
`advance_convergence` awaits exact fanout retry before newer convergence.
Guard these presently public upstream calls on each poll as above and keep
the raw transport fence. A review finding that a required effect escapes
that ownership boundary blocks mounting until addressed. It does not justify
treating these APIs as synchronous or weakening the promised cut.

## Reporting and integration

`StartupReporter::ready` currently takes the one channel and uses
`spawn_blocking` for R/ACK. Canceling its join future leaves the actual
writer running. The candidate caller must use an owned nonblocking equivalent
on the same validated channel, check before the actual one-byte R write
poll and ACK acceptance, and honor the existing bounded timeout and observed
termination. Preserve the one-report rule, existing stock protocol and
detached parent ownership. R sent while admitted cannot be recalled. If fatal
arrives before ACK acceptance, the child records no successful activation
and enters owned fatal cleanup. The parent can already have seen R and sent
ACK, so do not promise retractable parent-visible success in that race.
Normal foreground reports admit immediately before Active status publication.
IPC framing, reply guards, Terminal production stop and daemon assembly are
neighboring ownership scopes. The owner-side implementation must not mount
another listener or modify those modules without coordination.

## Validation and exit

- Run scoped controlled cuts with real keyed Journal and native state and
  ordinary Command requests. Block before independent steps and stop while
  an async call is pending, release its wake after displacement, and assert
  no second poll, durable row, native effect, replay ACK or success reply
  from the post-cut portion. Include activation after real KeyPackage
  acknowledgment before native drain, received delivery before ingest and
  after ingest before projection/ACK, status and count exit, dispatch with a
  queued reply, room discovery before preparation, preparation before create,
  retained Welcome and claimed Command publication before native send and
  the second pass of maintenance. Use sealed observation barriers for tests,
  not invented production fault flags.
- Check retained ordinary and Stream publication/recovery or demonstrate a
  precise covered call boundary with durable fixture rows. Verify recovery
  reopens a healthy interrupted profile and reuses original IDs/ciphertext
  without fresh work for the same obligation. Check real default-dispatcher
  displacement at decisive integrated cuts. Include same-UID IPC Status and
  Command replies racing a cut, detached report before R and while waiting
  for ACK, and real physical join or explicit held custody. Use bounded
  serial disposable private WSS and report cut outcome plus latency.
- Healthy composed qualification runs a finite byte-preserving Command over
  actual WSS, then a clean restart, with stock behavior and noncandidate
  builds unchanged. Run strict candidate and stock checks and exact Claude
  Code Auto review. Keep this heavier qualification opt-in. Publish no
  release or active Peer/Terminal mount on the strength of owner cuts alone.
