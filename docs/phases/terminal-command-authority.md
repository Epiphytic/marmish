# Terminal authority handoff from the Command owner

The first live writer needs a typed way to obtain authority from the existing
Command actor. This increment establishes that handoff and revocation against
real accepted Command rooms, following the [interactive roadmap](interactive-peers-roadmap.md)
and [local fence contract](terminal-ipc-fence.md). It adds no shell or relay
connection to the Command owner.

## Admission and identity

A local Terminal launch supplies peer, locally configured relay policy and exact
returned session. Use `command_ready_session`, including its exact fresh
preparation branch, and validate all three inputs on the actor turn. An
earlier owner may use non-fresh preparation to find a session when no exact
session has been selected. The authority handoff never calls
`open_command_room` or `open_prepared_command_room` and never discovers
or creates a room. A pending Welcome or fanout (`Ok(None)` while selecting
or `Ok(false)` for an exact session) means retryable not-ready under the
caller's deadline. The actor checks the matching non-revoked client journal
row and exact current route again. The ready selector itself calls
`command_room_drained`, then `command_context`, and can revoke a binding.
On the server the actor offers a
snapshot of current incoming accepted Command bindings, filtered by the
current per-key Terminal grant, capped by the admission owner's fixed
candidate capacity. No local caller can produce or enlarge the server
snapshot with a peer-supplied endpoint or digest.

`AuthenticatedCommandContext` already binds local and peer canonical
accounts, session, direction, MLS and Nostr group IDs, UID and the protected
authority digest. Return canonical protected relay endpoints from the same
`check_group_for` result used to establish authority. Apply canonical URL
validation without choosing a transport mode. The separate admission owner
intersects these endpoints with the profile's real mode and policy. The
digest already includes the exact protected routing application component.
The actor revalidates the whole verified context and exact endpoint set
before restoring a fence. Existing Command source ownership and journal
queries keep their durable formats.

Only the Command owner holds a bounded table of at most 32 live binding
generations. At 32, return typed capacity for a new binding and keep
existing registrations. Ordinary Command room capacity remains 64. Index by exact MLS group ID and store the verified Nostr
transport group alongside it. Use a process-local monotonic nonzero
generation counter and refuse overflow. Return a clone of the immutable
binding and an `Arc<AttachmentFence>` for each positive registration.
Retiring a generation removes it from scheduling and invalidates every
shared holder immediately. The actor owns an RAII table whose destructor
retires every remaining fence without IO. The server scheduler and dialing
owner see immutable contexts and their fences. A signer capability is
provided separately and never exports canonical secret key material.

## Mutation boundary

Enter Suspect before polling a native effect that can change membership
or protected metadata. In `receive_one` (`mdk_adapter.rs:2349`), finish
waiting for the delivery before raising the fence. The 20 ms receive
wait is outside it. For `ingest_delivery` at line 2371 use the exact
transport group only if the envelope maps unambiguously to a registered
MLS binding. Stage all for unknown or non-group envelopes, or unscoped
native work. Revalidate after the successful native result and successful
application projection, before replay ACK or resubscription. An
unpersisted envelope or another uncertain ingest result cannot restore
a fence. The proof uses the current protected context, exact endpoint set,
journal and grant. Projection currently has no await internally. A native
error, timeout, `ingest_deferred`, `journal_backpressure` or uncertain
result retires all staged generations even when the ordinary operation
can retry. Keep the existing error and ordinary retry unchanged.

Route removal and explicit cleanup are immediate actor boundaries:

- Route every owner-side Command revoke through a wrapper that retires
  first and calls `CommandJournal::command_revoke_room`
  (`command_rooms.rs:114`). Route every `routing.remove` through an
  owner helper that retires matching registrations first, including
  `project_command_join`, `command_forget_and_confirm` and `command_context`
  (`command_adapter.rs:113,155,340`), join or cleanup projections, route
  rebuilding and the orphan path (`mdk_adapter.rs:1121,1384,1772,1974,2020,2237`).
  Any non-revoking error from a Terminal-side `command_context` check,
  including `group_state_unavailable`, retires that existing registration
  before returning.
- Retire on entering transport recovery (`mdk_adapter.rs:2306`).
  Notification loss can hide a commit. Recovery cannot restore a generation.
  Retire before taking the engine for stop, beginning Command cleanup
  (`command_runtime.rs:39,55`) or awaiting shutdown. The running daemon
  reads typed config at engine creation and has no grant hot reload.
  Any future reload must retire affected bindings before replacing the
  policy. Every actor tick rechecks grant and Command allowlist before
  effects, within at most one existing actor tick, and all admission and
  resume revalidates the current grant.

Audit the native call sites against pinned MDK `8abc51bef`. Stage all for
`runtime.drain` in startup and tick (`mdk_adapter.rs:1447,2464`). Stage the
named group for `advance_application_convergence`
(`command_runtime.rs:827`), which can apply pending commits, and
`redeliver_welcome` (`mdk_adapter.rs:2265`), which can publish runtime
effects. Retire matching registrations before `forget_group_local`
(`mdk_adapter.rs:1051,1934`, `command_adapter.rs:118`). For
`runtime.send(AppMessage)`, the account runtime calls `session.send` then
`publish_session_effects` and checks post-join rotation. Stage the named
group around that full path, including Command, ordinary AppMessage and
stream submissions. Creation starts a new group via session
`create_group` and publishes its effects. Use all-group staging if any
creation path uses unscoped runtime work or the pinned contract does
not isolate its publication effects. Classify `sync_transport_groups` and
`activate_transport` as subscription-only after checking their pinned
definitions. Uncertain transport state goes through retirement.
`acknowledge_published_app_messages` only inspects and deletes completed
stored fanouts without touching the roster or route.
`delete_pending_application_events` deletes stored projection events;
`reconcile_command_sources` synchronizes the Command source journal.
Any route removals inside projections still use the owner retirement helper.

Keep all work in the existing actor. No fence mutex crosses an await. At
each potentially guarded call, test table emptiness and take the existing
path directly when empty. No added await, native work, or reordering on
that path. Existing reactor deadlines bound a Suspect pause, and an
abandoned token retires. Healthy connected measurements must show a
p95 pause under 50 ms on the loopback fixture and no pause may exceed
the actor operation's current deadline plus one bounded in-progress
effect. Measure ordinary calls with and without the empty-table hook.
If healthy drain cannot meet the target, leave production enrollment
inactive and revise the staging design. No Terminal network awaits
are added to the Command actor.

## Evidence

Deterministic tests cover event-to-binding selection, bounded registration,
exact equality, no renewal of retired generations, failed projection or
cancellation, empty-table behavior, and actor destruction. An opt-in local
real-WSS test proves actual Command preparation, equivalent canonical
context export and a membership transition that retires the prior fence,
plus ordinary work after a separate normal interaction. Exercise both
directions and a server grant refusal. Measure registration and normal
actor tick pauses with and without bindings. Keep real WSS, benchmarks
and broader workload validation as local jobs. A later integration test
demonstrates PTY effects and frontend cleanup through these actual fences.
