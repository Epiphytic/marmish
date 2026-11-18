# Peer custody through the public MDK runtime

Build an opt-in proof using the public APIs of the pinned MDK
`8abc51befbf71a1ae3bdbb25eb0860c731e75db2`. Reuse the signed pending
KeyPackage, separate SQLCipher stores and loopback evidence from the
[first custody proof](../validation/peer-staged-inspection-proof.md). This
increment stops at proving that the public runtime can verify a staged
receive and suppress the founder's raw Welcome. Aggregate account routing,
Offer and Consent policy, key custody, limits, Tor and deployment require
their own design and evidence.

## Inbound custody

- Open one `AccountDeviceRuntime` with `SessionConfig::with_storage` and
  a retained handle to the same separate SQLCipher provider. Provision the
  stage's own slot by returning the stage's slot ID from the fixture
  publisher's public `KeyPackagePublisher::legacy_slot_id` before calling
  `prepare_fresh_key_package`. Keep the signing-only publisher and confirm
  the exact signed artifact remains pending. Do not use `session_mut`. The
  quarantined runtime has no relay client, live registration, maintenance
  tick, startup retry or transport activation.
- Before forwarding, bind the authenticated carrier's account, exact
  wrapper bytes, Welcome ID, signed pending KeyPackage event and approved
  route to the stage. Reuse the pinned peeler's acceptance and the exact
  second SDK extraction in the first proof. Build a `TransportDelivery`
  addressed to the stage account and call public
  `AccountDeviceRuntime::ingest_delivery`. Process one candidate under its
  single owner.
- Instrument every inert adapter and publisher network method to record
  the call and return a typed refusal. Ingest drives its resulting
  publication queue and maintenance bookkeeping, so success from ingest
  alone cannot admit a stage. Refuse to bind a live route on an ingest
  failure, any recorded transport attempt or a missing or inconsistent
  durable receipt. Require an exhaustive effect allowlist. Require a
  single expected `GroupJoined` in `events`, no
  `left_object_unpersisted`, no other nonempty collection, and the
  default maintenance disposition. Inspect every field, including
  routing failures, app-message outcomes, reports, queued effects and
  convergence. The dispatcher normally returns Ok with effects that
  carry failures. A missing inbox or group route can create a
  PublishFailure before any adapter call. A failed call may already have
  mutated the stage. Keep it isolated while recovering or discarding it.
- Read the exact retained `GroupJoined` from public
  `MessageStorage::list_pending_application_events`. Require the exact
  Welcome ID, group ID and MLS-attributed `welcomer`,
  `explicitly_confirmed == false`, one
  live group, the expected purpose and Current profile, an active local
  leaf, precisely the two approved account leaves, sole approved founder
  admin, the encoded route and the consumed pending package reference.
  Use public runtime getters for the group, membership, administration,
  app component and epoch pin. Record the epoch pin and canonical local
  leaf index as the recovery baseline. The former proof's leaf hash is
  private through the runtime and never had an independent expectation.
  The consumed reference remains the binding of the native join to the
  approved package.
- Reopen before Ready and require the durable receipt. Save and verify
  Ready before calling `MessageStorage::delete_pending_application_events`
  for that exact Welcome ID. Reopen before deletion, then after deletion.
  Require unchanged epoch and two-leaf authority along with the sealed
  registry evidence. Prove that an unrelated test receipt remains
  pending when the target one is acknowledged.

The public runtime opens the private session at
`marmot-session/src/runtime.rs:581–607`. The runtime's signed ingest and
effect dispatch run at `runtime.rs:4058–4201`; the same method checks the
destination account before native processing. `GroupJoined.welcomer` is
defined as the MLS GroupInfo signer at `traits/src/engine.rs:777–788`.
The public outbox contract at `traits/src/storage.rs:389–413` defines
transactional staging and idempotent exact acknowledgement. The session
constructor and own-leaf hash look public within the dependency, but the
module and runtime's `session()` and `session_mut()` reach it only through
development feature `test-support` (`marmot-session/src/lib.rs:9–16,40–43`
and `runtime.rs:673–684`).

## Outbound disposition

Require a Current-profile founder and a delivery-aware native Welcome from
`FoundingGroupCreated`. Keep the founder's runtime inert while the native raw
Welcome is owed.
Use a separate loopback connection to send the signed encrypted Delivery
carrier and get a positive relay acknowledgment for its exact ID. Commit
the authenticated binding of that acknowledgment to the stage, carrier,
raw Welcome and configured route in the test registry. Keep carrier
acknowledgment separate from acceptance by the receiving client.

After reading back the committed record, call public
`AccountDeviceRuntime::withdraw_welcome` to cancel publication of the raw
nested wrapper. Retain the positive carrier acknowledgment as the app
truth. An independent rejection or cancellation withdraws the same
native obligation with its own durable app disposition. Before any live
adapter starts, require the expected withdrawal outcome and verify the
exact raw ID disappeared from the freshly loaded outstanding list. On
reopen, an already-withdrawn result is acceptable with the same durable
app binding. A missing obligation, unexpected native Delivered, a
mismatched binding or a write error keeps the stage inactive. The
inert owner has no detached publisher that could already hold the raw
event.

Crash and reopen after recording the carrier ACK and before withdrawal,
then after withdrawal and before any live transport. With the real native
adapter on a loopback relay, trigger startup from a freshly read native owed
list. Submit an exact stale retry for the withdrawn raw ID as a negative
control. Verify zero matching raw publications in both paths and one
undisposed fixture raw publication as a positive control. Never carry a
pre-disposition retry list into startup.

The public withdrawal at `runtime/welcome_delivery.rs:348–373` also
removes the exact fanout when no detached attempt owns it. The engine
changes the raw obligation without changing canonical group state
(`cgka-engine/src/engine.rs:3666–3710`). The runtime has no public method to
mark a carrier acknowledgment as native Delivered. If that status becomes
a requirement, the public lower-level engine's Delivered method
(`engine.rs:3656–3663`) validates the raw artifact, but requires a separate
engine ownership handoff, opening the same storage only after the runtime
closes and revalidating group authority before reopening the runtime. A
second concurrent engine, direct state-row writes, and synthetic relay
acknowledgments provide no valid proof of delivery. The test uses the
public withdrawal path.

## Tests and acceptance

- Run the proof in a dedicated, self-contained named integration test.
  Provide its own minimal signer and publisher using the public dependency
  APIs and reuse only the earlier proof's scenarios. Keep the private
  fixtures and production API unchanged. Compile and run that target in a
  disposable source snapshot with only Marmish's MDK development
  `test-support` flag removed. Record the exact invocation, single manifest
  diff, and `cargo tree -e features -i marmot-session` evidence that the flag
  is absent. The in-tree development run alone cannot qualify the public
  path. MDK declares the flag with no dependency additions. Existing tests
  keep their development feature.
- Exercise the correct signed candidate and verify zero attempted I/O in
  quarantine. Include wrong destination, wrong pending package ID,
  mismatched MLS signer behind a valid outer sender, extra leaf, wrong
  route and a briefly unavailable or swapped stage. Show the ordinary
  provider never receives those stage inputs.
- Exercise unexpected effect handling with a native signed message whose
  publish effect is queued into the runtime's public dispatcher. Require
  the adapter's typed denial, a sticky attempted-I/O flag and refusal to
  promote when the dispatcher reports success with a failure in its
  output. Also deny promotion with a missing native route that creates a
  PublishFailure without calling the adapter. If the fixture can induce an
  automatic effect through real signed intake, exercise that too. Otherwise
  record the missing
  end-to-end negative as a production gate and describe exactly which
  dispatcher branch the proof exercised.
- Retain the loopback positive control, the restart cut points, the
  unchanged ordinary baseline and the public-only build evidence. Report
  timings for this scenario as measurements. Suite size, deployment and
  aggregate transport behavior remain outside this proof. Run the
  targeted test locally and obtain independent design and code review
  before advancing it.
