# Native aggregate ingress proof

This opt-in fixture exercises the pinned MDK Nostr adapter and Current
profile locally. It uses one activated receiving account, loopback WSS with
NIP-42, independent SQLCipher stores and a separate sender. The ordinary
runtime in this test exercises the native group path. Existing Command
tests separately exercise the production daemon owner. No public relay,
Tor, or production feature is involved.

Run the selected fixture with the workspace's normal native test build.

```sh
cargo test --locked -p marmish-native --lib \
  mdk_adapter::peer_aggregate_ingress_proof_tests -- --ignored --nocapture
```

## Observed origin constraint

On a canonical inbox registered on A and B, the same signed kind-1059
event arrived first from B. Its source was B's authenticated connection
and exact issued subscription. The fixture denied it because its
application route allowed A only. It then sent the exact same event on A,
followed by a fresh eligible event on A over the same socket and
subscription. The fresh event reached the owner. The earlier same-ID A
copy did not reach owner dispatch through the ordered control and
subsequent bounded wait. The wire observer recorded two accepted inbox
REQs and zero inbox CLOSEs before shutdown. **Multi-origin inbox
qualification failed.** The split-origin case never activates a Peer
stage. The shared-origin test configures a canonical inbox on A only.

In the pinned rust-nostr SDK at `48801bf6f2f1`, the default
`ClientBuilder` uses the pool-wide `MemoryEventsTracker`
(`nostr-sdk/src/client/builder.rs:216–223`). Relay
`inner.rs:1677–1726` verifies the signed event, runs the optional admit
policy, checks the event ID and calls the database save path. The memory
tracker at `nostr-sdk/src/events_tracker.rs:44–68`
tracks seen IDs across the pool. An ID already marked Saved suppresses
the normal event notification. At MDK `8abc51b`,
`crates/transport-nostr-adapter/src/sdk_client.rs:1799–1832` consumes
that deduplicated path for kind 1059. The raw branch at lines 1851–1868
only observes kind 1059, while group intake uses the raw event path. The
negative control exercises the assembled system. The pinned source
explains why the eligible copy stops before owner dispatch.

A later design can evaluate the pinned SDK's
`ClientBuilder::admit_policy` hook (builder.rs:260). The
`AdmitPolicy` context supplies the verified event, relay URL and
subscription ID (`nostr-sdk/src/policy.rs:82–92`) before the global tracker. A policy could reject an event at an ineligible
origin when authenticated consent or trusted local routing establishes
eligibility before MLS ingest. It must authenticate the full signed body
before inspecting it, since the SDK can hit its verification cache. If
source eligibility requires opening an MLS stage, a policy alone cannot
qualify the route. The pinned MDK reader uses a single replacement
notification stream, and the native inbox placement pin has no
source-qualified key. A later design would need bounded pre-MDK custody
of raw per-source copies or a dedicated native raw inbox route with
source-scoped ownership. Separate pools would add inbox lifecycles and
replay coordination. These alternatives have no implementation here.

## Signed isolation and ownership

The split-origin test also rejects missing endpoint, missing subscription,
local publish echo, wrong transport and wrong plane. The shared-origin
case verifies the route's registered transport and MLS IDs, signed WSS
group replay, canonical account, founder leaf and numeric epoch. It
compares the current epoch pin before and after ingest. Full source fork
identification remains unqualified. The owner reconstructs the activation's
inbox REQ ID from its configured endpoints and MDK's current attempt. Group
dispatch uses MDK's current REQ ID for the registered transport group. The
loopback relay's observed IDs are kept as a wire check.

The negative cases include wrong destination, wrong founder, wrong
pending-package binding, unknown token, malformed control, stale
generation, a raw native Welcome referencing a withheld Peer package,
and an additional genuine MLS leaf. The wrong package reference and stale
generation cases compare otherwise valid carriers against mismatched local
authority. A signed three-leaf Welcome may
mutate its isolated stage. It produces no Ready, ordinary ingress marker
or active Peer subscription. A signed frame with an unregistered
transport group ID is also suppressed. Revocation invalidates a queued route before native sync in the single
owner task. A genuine ordinary signed Welcome traverses the shared inbox
classifier while a Peer carrier is held. Ordinary signed group traffic
progresses after the Peer filter is removed. Concurrent dispatch/revoke
and a precise tally of all ordinary group filter renewals remain open.
The generation-fence assertions run in the disjoint route case, leaving
four ignored test functions for the five plan scenarios.

The child scope refuses activation, deactivation, receive, unauthorized
sync, publishing, and KeyPackage network actions. A real signed Welcome
carrying the retention component triggers a native retention-change
effect alongside the Join. The acceptance allowlist excludes that
effect through the same admission function used for the positive stage.
There was no child network request during signed ingest. The separate
direct child calls exercise stub API refusals.
The ordinary inbox classifier delegates to the pinned
`held_publications` authority for current, pending and retained
material. The wired ordinary positive exercises a signed pending package.
A full retained-package rotation on this same adapter remains
unexercised.

## Scope of persistence and timings

The fixture reopens stage or ordinary stores while the canonical adapter
remains alive. The observed replay pin is account-wide. Its release
requires the signed carrier ID, and a nested Welcome ID did not clear it.
The test records accepted or denied dispositions by tenant, generation,
plane, endpoint and event ID. The original encrypted stage could not be
opened while missing or with a different encrypted provider substituted.
The exact buffered delivery was reconsidered against the current
generation and ingested after restoring the original provider. An
injected duplicate carrier did not displace the positive control on the
same connection. The owner did not rely on the adapter to redeliver the
held inbound item. The stage was reopened again before native pin release. Reopening the runtime with
a pending Join also loads that event into the engine queue. The fixture
verified the exact durable Join, committed its Ready and carrier
disposition, acknowledged the known Join, and opened the active runtime
against the cleared queue. The receipt write and native stage write use separate SQLCipher
transactions. Whole-process restart, a crash in that write gap, native
inbox redelivery, unacked replay following revoke, independently tunable
native relay cursors and production multi-origin admission remain
unqualified. The fixture also has no per-store physical write measurement
or capacity-boundary qualification.

Times printed by the test are monotonic local observations for one
warmed ordinary round trip alone and one while holding a Peer inbox item.
The components report round trip, dispatch checks, native ingest, receipt
commit and the acknowledgment call duration. The separate positive
pin transition is observed through adapter replay status. The reported
call duration measures a local method call. The single samples and
run-to-run noise cannot establish a throughput or overhead difference.

The Linux run of the four named cases passed in 33.78 seconds. The
observations below are single samples in milliseconds from that run.

| Ordinary sample | Round trip | Owner dispatch | Native ingest | Receipt commit | Pin acknowledgment call |
| --- | ---: | ---: | ---: | ---: | ---: |
| Alone after warmup | 226.64 | 0.288 | 27.71 | 82.74 | 0.124 |
| While holding one Peer item | 200.64 | 0.293 | 26.89 | 67.83 | 0.148 |
