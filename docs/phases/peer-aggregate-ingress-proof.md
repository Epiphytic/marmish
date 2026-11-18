# Aggregate native ingress fixture for staged Peer rooms

This proof follows the [staged custody design](peer-staged-inspection.md)
and the [public runtime proof](peer-public-runtime-proof.md). Exercise the
actual pinned SDK adapter and a minimal test owner in an opt-in in-crate
`mdk_adapter::peer_aggregate_ingress_proof_tests` child module. Reuse
`terminal_relay_fixture` and keep production dispatch, journal schema and CLI
unchanged. The test-only kind 24860 uses the earlier fixture envelope. It
exercises separate store ingress and replay ownership without promising a
production control protocol. Egress of Peer
application payloads and production invitations need independent evidence.

## Native adapter contract

Use exactly one registered receiver account, one live receiving adapter
and one owner of all its receive and replay-pin operations. The owner uses
`activate_account` once and `sync_account_groups` with the complete eligible
group union. The latter replaces the account's desired group set.
`deactivate_account` is account-wide. Never pass the adapter or Marmish's
delegating `PublicationGate` to a child.

Re-host the ordinary Current-profile account runtime in this fixture instead
of constructing the production `MdkOwner`, whose initialization owns its
own adapter. Instantiate this runtime, one active Peer runtime and one
quarantined incoming stage with separate SQLCipher stores. Compare ordinary
native group delivery and effects with the same setup and fixture-only owner
before a Peer stage exists. This qualifies the adapter seam, not identical
production daemon/Command behavior. The existing independent native Command
test remains the production regression control.

Give each child a scoped adapter and an owner-readable routing snapshot.
The ordinary and Ready fixtures submit a bounded desired-state notification
when native sync calls their adapter. Validate the matching account and each
child's exact already-authorized proposed route, then record the desired
snapshot and return success. No underlying sync occurs in this method.
The canonical owner reads the snapshots, intersects them with its validated
generation-bound application registry, and submits the aggregate union.
No child proposal can add a route by itself. An inert quarantine has an empty
routing snapshot and refuses a nonempty native sync. All children receive a
typed refusal with a sticky counter for child activate, deactivate, receive,
and outbound publish. Counters are separate from the accepted sync request
path. Pin release is an inherent method visible only to the canonical owner.
KeyPackage signing can be local and child network methods are denied. Send
fixture artifacts from a separate sender. Quarantine owns no active group
filter before durable Ready.

Register the native union by the normalized endpoint ID, full transport group
ID and full MLS group ID. Reject a duplicate transport or MLS group ID within
the same account even on disjoint endpoints. The account's proper inbox and
separate groups may share a configured relay. Only the canonical owner adds or
removes filters. Revocation first invalidates the application mapping, advances
the local generation, then requests native sync. A delivery already queued
through a valid subscription cannot survive the generation recheck.

## Bounded routing and final disposition

Use two disposable loopback WSS origins and the existing private CA
fixture. Extend the test-only trust constructor to authorize a bounded
list of exact loopback URLs with corresponding explicit roots. Preserve
the original one-URL trust behavior and mismatch tests. Extend the relay
fixture's bounded observer to count actual REQ and CLOSE for exact inbox
and group routes. Reuse its stored replay, live provision and test injection
under a specific issued subscription. This stays under cfg(test).

Read `TransportDeliverySource.transport`, `.plane`, `.endpoint` and
`.subscription_id` from actual relay handling. Require Nostr transport, the
expected inbox or group plane, a present normalized allowed endpoint, a
present currently issued subscription ID serving the exact account and route,
and an exact wire/event binding. Deny a missing endpoint or subscription,
the synthetic `local-publish` subscription, or wrong plane/transport.
Pinned native group replay carries Group plane, endpoint and subscription.
Prove it matches current ownership before dispatch. Inspect the SDK's
deduplicated inbox event path and raw group-event path separately. If an
ineligible B delivery causes an otherwise eligible A copy of the same
inbox event ID never to reach owner dispatch, this qualifies as an
availability failure. The stage must remain inactive for that configuration.
Retain a separate valid event from A as positive control and document
which measured boundary drops the same-ID copy. A distinct wrap cannot
qualify same-event multi-relay delivery. A later fix may align the authorized
origin set with the canonical inbox or use a reviewed raw ingress boundary.
A failing origin test may not be repaired by accepting B or bypassing
native eligibility checks.

For group ingress perform the exact subscribed transport ID, native
group, authorized endpoint, current ownership revision and receipt
checks before a child can write. Authorize the group's active canonical
role, leaf and epoch through the existing [Peer custody proof](peer-public-runtime-proof.md)
and pinned native code. Unknown, ambiguous or stale group routes do not
reach any runtime.

For shared inbox ingress, authenticate the signed NIP-59 inner record
before selecting the target. Prove an exact fixture Delivery record binds
the preapproved stage token and generation, canonical account and founder,
configured origin, withheld package reference, outer signed carrier ID and
exact nested signed native Welcome. Move only the nested artifact into its
stage. Reuse public stage receipt and publish-attempt/effect allowlist
checks. Stage decline, wrong route, malformed known control, wrong or
unavailable stage, unknown token, superseded generation or a raw nested
artifact with a Peer pending reference yield no ordinary fallthrough.
Distinguish explicit recognized controls from ordinary required early-Welcome
behavior, including the legitimate retained ordinary package fallback.
The exact prospective store type is classified by these authenticated facts,
not by the signer's claimed name or optional tags. Unknown or malformed
input cannot select a stage. The malicious ordinary-granted member targeting
an ordinary package with an MLS purpose visible only after native decrypt
remains outside this flow, as described in the staged design.

Record a small durable application disposition or associated receipt with the
exact delivery ID before calling the canonical adapter's inherent
`ack_replay_delivery(account, delivery.message.id)`. The ID is the 32-byte
signed carrier event ID, not the nested Welcome ID. The method returns unit
and can silently no-op. In a controlled single outstanding replay case
record the `take_replay_status` pending transition. Drop the status read
guard before releasing the pin, and observe progress via `replay_changes`
and post-release status. Include a wrong nested-ID call that does not clear
the carrier's pin before the correct one. If the live path has no pin,
report that and do not infer release from unit return. Pin release
is not an independent per-tenant cursor. A
transient child store failure must preserve the exact work in bounded replay,
or in a durable recoverable owner handoff. Ordinary delivery must still
progress. The proof uses the pinned adapter's unfloored replay and independent
application watermarks by tenant, generation, plane, origin and event ID.
It makes no claim of an independently tunable native fetch floor per tenant.
Use bounded fixture slots, deadlines and no unlimited persistence. Unknown
pre-approval deliveries fail closed and may release their native pin after
their final decision. Avoid a shared cache that can suppress a currently
eligible origin before checking its route.

## Negative, restart and measurement cases

The [validation report](../validation/peer-aggregate-ingress-proof.md) records
the merged generation-fence case, the retained-work limits, and the approved
scope of the implemented fixture.

1. `ordinary_and_peer_routes_are_disjoint`. Start with a real ordinary Current room and execute a signed ordinary
   positive before any Peer subscription. Keep the ordinary store's exact
   source and route counts for comparison. Submit the ordinary positive
   again while Peer ingest is paused and after stage revoke and removal.
2. `invalid_stage_inputs_never_reach_ordinary`. Exercise signed wrong-destination, wrong reference, wrong founder, extra
   leaf, wrong-origin, stale generation and malformed Peer records. Count
   every attempted application dispatch by target and exact source. Assert
   every rejected carrier and raw message ID has no ordinary inbox or
   group-dedup marker while open and after reopening. For a transient
   unavailable or swapped stage, require no Ready and retain only the
   bounded, recoverable candidate needed for retry.
3. `registered_route_generation_fences_queued_work`. Activate the Peer route only after the validated staged receive and
   registry commit. Use room- and endpoint-specific signed positives and
   deliver real events over actual filters. Try duplicate registrations
   and use a valid subscription ID with a different signed group route as
   a negative. Hold one accepted delivery across revoke before dispatch
   and require rejection with zero child ingest. Assert the native sync
   changed only intended group filters and kept the canonical inbox.
4. `exact_inbox_origin_and_replay_disposition`. Repeat after deterministic interruption before durable app receipt,
   after receipt but before pin release, and immediately after revocation
   but before native sync. Retained work must be reconsidered using the
   current tenant and generation. Deliberate replay and duplicate copies
   must preserve ordinary and Peer routing fences and receipt identity.
5. `child_transport_refuses_ownership`. Attempt all scoped-adapter operations from a child as negative controls.
   Exercise an unexpected automatically generated effect during signed
   initial Welcome intake if reproducible. Otherwise trace the pinned
   first-Welcome path in evidence with exact file/line references. A signed
   Welcome with component 0x8005 can add a disallowed retention-change effect.
   Do not claim that is automatic publish. Record reachable effects, any
   unerased qualification limits and the review disposition before activation.

Cap the fixture at two relay origins, three receiving runtimes, four
registered group routes, eight retained records per route and 32 total.
Use a five-second limit per async step and a 180-second outer test deadline.
Count actual REQ and CLOSE operations, stale/late and split-origin
dispositions, and per-store writes. Report local monotonic latency for
owner dispatch, native ingest, receipt commit and PIN release separately.
Compare ordinary round trips alone and during bounded Peer backlog using
the same fixture and warmup. Latencies are report-only. Keep the production
ordinary tests as separate controls. No external relay, Tor, production mode
or GitHub-hosted runner is required. The in-crate fixture uses normal public
MDK runtime methods only, but compiles with the repository's usual test
feature graph. Keep the existing named custody proof as the independent
public-only build evidence. Keep routine CI at its existing core selection.
