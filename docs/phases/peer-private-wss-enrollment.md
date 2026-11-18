# Private WSS Peer enrollment fixture

## Scope and dependencies

Run the real Peer carriers on real TLS WebSockets in two milestones.

1. M1 verifies Offer, durable provisional Consent, exact relay ACK,
   and recipient-verified signed Delivery. It stops before native
   Welcome ingest. Record the M1 evidence under its own test name.
2. M2 extends that same socket fixture through native join and durable
   Pending. A Manual profile stops at Pending. An Automatic profile
   runs the exact local transition that a Manual approval uses and
   stops at Activating. M2 requires the independently reviewed and
   integrated minimal joined owner specified below. Until then the
   join and Pending criteria remain open, with no substitute store or
   interim native ingest.

Application acceptance, room routing, heartbeat, CLI and daemon
enablement are separate increments.

The design references Marmish `ddbf2b345c52fc9231425f8cc2a8dd6a175c03d0`
and the isolated Consent amendment at
`23acc898a7baf3a29b6a1b1d8897693ea01fae6e` (includes `cfb3410`).
Record the eventual integrated Consent and Pending prerequisite refs when
implementing. Start from the integrated branch retaining the
committed provisional binding, quota and raw socket fixtures. Reconcile
Consent-owned path helpers, visibility and schema epoch before adding the
WSS mode and M1 owner. Integrate the reviewed joined owner before M2.
Do not copy the amendment tree over the newer shared files. The lock
resolves upstream MDK master at
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`. Recheck public API signatures
against the integrated lock. The older scripted `publish_approved` tests and
aggregate-inbox proof remain separate. The latter uses a fixture-only inner
kind 24860. This fixture uses the actual Peer `Codec` throughout. Mount
`peer_private_wss_tests.rs` as `peer_invitation::private_wss_tests` under
`cfg(test)` to reuse the owner and scope types without widening public
visibility.

## Loopback relay and transport

Reuse `mdk_adapter.rs`'s existing test relay in
`terminal_relay_fixture.rs`. Add a test-only exact Peer inbox mode that
admits the two configured disposable accounts by NIP-42 and restricts
Peer writes to kind 7435, as defined by both the Peer codec outer and
raw owner carrier constants, with one recipient tag for the other
account. A bare native kind 1059 Welcome and kinds 30443 and 445 are
refused on this Peer lane.
Accept the independently signed outer event on the authenticated socket
after verifying its signature. The outer signer and the authenticated
account intentionally differ. Permit an authenticated exact `#p` for
only that same account. Observe intended account on publish without
logging event bodies. Refuse unauthenticated and foreign-recipient reads.
Return exact EVENT and EOSE for the live and history subscription IDs.
A live limit of zero replays no history and remains subscribed. Pages
respect since/until and the requested count and return newest first with
deterministic ties. CLOSE removes just its subscription. Send the exact
carrier ID in OK. Use a Peer-only change or exact mode; preserve installed
Terminal, aggregate-ingress and benchmark behavior on the same fixture.
Keep the event ceiling at 64 KiB plus only the raw publication wrapper
space allowed by `peer_raw_socket_wire::MESSAGE_LIMIT`. Require the
existing codec cap for the embedded event independently. The test may
use one short page and a three-carrier transcript. The
existing adversarial relay tests cover broader cursor faults. This mode
does not claim to prove the full production relay policy.

Bind to an ephemeral 127.0.0.1 listener with the existing generated TLS
chain and exact private CA, exact singleton WSS connector and Off route.
Use the bounded serialized `peer_raw_socket_owner::Owner` for each
actor, including raw challenge, successful AUTH acknowledgement and the
live subscription's exact EOSE. Explicitly observe both subscriptions
ready before publishing. Add a fixture-only `QualifiedOrigin`
constructor for the shared relay and keep the existing scripted
constructor. Never fall back to default relays, DNS, Tor, an SDK
aggregate inbox, direct event injection or shared command storage.
New actors own separate private profiles and keys. Use scoped deadlines
and an outer test deadline; tear down the two sockets and cancel/join
owned relay tasks even on failure. Keep output to counts, outcome codes
and elapsed durations. Use a bounded observation of client-published
frame digest through the existing raw receive/send code or a Peer-only
relay observation when asserting exact Consent bytes. Re-serialization
by the relay need only preserve signed carrier identity and event data
on delivery.

## Exact flow

1. Open both owners and publish a real signed Offer from the founder to
   the recipient's exact WSS subscription. On the client, hand the
   current `Owner` arrival to the same SQLCipher `Checkpoint::admit` and
   raw-origin checking used by `InvitationLane::live_step`, using the
   Prepared Consent fixture attach epoch. Share a small checked-arrival
   seam where possible. No database seed or caller-supplied origin,
   approval signal or reservation coordinates substitute for this
   admission. Recover only the selector derived from the admitted signed
   record. Exercise normal per-profile cap, generation, block and
   replay fences on this path.
2. Use `reserve_provisioning`, generation-bound path/witness and
   `InvitationState::prepare_provisional` from `peer_stage_binding.rs`.
   Reuse `PreparedConsentOwner` for attestation, queue and persisted exact
   recovery. Neither Manual nor Automatic supplies legacy Offer approval.
   Adapt the existing `peer_prepared_consent_owner.rs` test-only
   `FixtureSink` boundary narrowly. Production-shaped owner reserves a
   current exact Attempt, checks custody and readiness before its send,
   sends only its stored bytes with `Owner::publish_bytes` and settles
   only after `Owner::accepted` validates the current scope, positive
   acknowledgement and exact carrier. Retain existing due retry,
   before/after custody and owner-local compare-and-swap checks, with
   no SQL transaction held across I/O. Keep synthetic ACK paths private
   to their existing unit tests. Socket reopen cannot settle an ACK
   from its predecessor. A relay ACK is transport evidence only.
3. Founder receives the exact Consent over its own scoped socket and
   opens it with `Codec::open_consent` against the locally issued exact
   Offer. Use its verified KeyPackage directly for only that group.
   Reuse the public-runtime founding setup in
   `peer_staged_proof_tests.rs::found_room` and its
   `prepare_create_group_with_audit_context`. Form
   `marmish/peer/1/<offer-room-id>` with the exact signed route, founder
   sole administrator and exactly both account principals. Open a private
   encrypted founder store with its own physical network denied adapter.
   Extract the one typed `FoundingGroupCreated` Welcome obligation and
   build `Codec::prepare_delivery` using its exact bytes. Publish only
   that Peer envelope via the founder's raw owner. There is no physical
   MDK Welcome or KeyPackage fanout. One founder driver owns one
   Offer and founds at most one room. The driver's in-memory handling
   does not establish founder restart or production replay semantics.
4. Recipient reads the Delivery on its current exact socket. Existing
   `Codec::open_delivery` takes a `Consent` openable by the founder.
   Factor its checks against a private bound representation that also
   accepts a recipient-owned `VerifiedQueuedConsent` capability. Mint it
   only after the existing durable transcript and exact signed Offer
   and stage artifact checks. Reuse the actual signed package binding
   and package lifetime, consent revision and lifetime, verified issuer
   and peer keys, room and route. Include origin, selector, generation
   and stage witness at the owner boundary. The recipient cannot
   decrypt the Consent sent to the founder. Never pass it the founder's
   private key, borrow the founder's decoded object or use a fixture
   transcript disconnected from the queued SQL row. Keep expiry and
   signed replay handling independent of an original Offer that has
   aged past its signed window. For this fixture settle the ACK before
   issuing the founder Delivery, without using that ACK as
   authentication. Factor a crate-private classifier from
   `Codec::open` and `seal::open_signed_seal`. It authenticates the
   outer and signed seal, validates the exact inner recipient and
   inspects inner kind before body decoding. Dispatch 24865 to the
   existing Offer path and 24867 to the typed Delivery path. Invalid
   signatures, client-inapplicable Consent (24866) and unsupported
   kinds get the bounded Invalid disposition. Before any stage lookup,
   bind Delivery to its signed canonical
   server and admitted exact offer ID and revision. Use the existing
   per-server in-flight bound, no search over stage files. Apply this
   dispatch to live and page records; only authenticated Offer kinds
   enter the Offer admission logic. A valid Delivery cannot become an
   Invalid Offer receipt or advance a page before exact durable
   disposition. In M1 a valid Delivery stops the page commit because
   there is no durable Delivery transition yet. Leave its page progress
   unchanged and keep the exact record retrievable from the relay.
5. M2 starts only when the reviewed joined owner below is integrated.
   Persist exact bounded Delivery custody/fence in the client Pending
   owner's SQLCipher checkpoint before opening the original provisional
   store. Do not keep the prepared store digest as an equality check
   after the authorized native write. The recipient proves a matching
   initial witness and package before the ingest. Bind the known
   nested Welcome to `TransportDelivery` AccountInbox with the current
   qualified endpoint and independently verify native provenance
   against the canonical inviter, wrapped Welcome, exact route and
   package ID. Use public `AccountDeviceRuntime::ingest_delivery`
   against the original isolated encrypted stage. Only this typed ingress
   can enter the isolated runtime. Open against a fresh physical
   deny adapter with no underlying network client, reuse the public
   proof's quarantine shape, and give the KeyPackage publisher no
   physical relay client. Latch attempted native callbacks by category.
   Call only open, exact ingest and readback. Never activate or sync the
   account, drive adapter receive, open group subscriptions, serve
   history, drain restored fanouts or call maintenance/replacement
   routines. Reopen starts with the same empty physical permission.
   `ingest_delivery` may process retained work. The adapter refuses
   any such transport request. Distinguish native retained obligations
   and reported failure from physical sends. Unexpected effect or
   unresolved runtime error leaves Pending closed; no native bytes
   escape on either outcome.
6. The Pending owner reads back typed persisted native join and
   KeyPackage-consumption evidence using the exact room, Welcome and
   inviter. Verify native group identity, peer purpose and capability,
   two roster leaves, founder sole admin and singleton absolute
   signed route. Verify the exact persisted origin, stage witness
   and generation. Use `storage.list_pending_application_events()`
   for the matched native `GroupJoined` receipt's Welcome ID, inviter
   and GroupId. Read `runtime.live_group_ids`, `group_record`,
   `canonical_group_membership`, `admin_pubkeys`, `app_component`
   and `epoch_pin` for exact group authority. Read
   `storage.key_package_lifecycle` for the signed artifact and consumed
   package reference. Keep those checks on recovery, with the native
   store and the custody checkpoint read independently. Persist Pending
   only after all checks. A handled duplicate or a crash after native
   join and before the Pending transaction must allow reconciliation
   of that same proof and prohibit re-ingest, a new reservation or
   package creation if the underlying public MDK state survives
   reopen. If the locked MDK cannot supply exact proof after reopen,
   keep the row fenced, refuse another ingest and hold M2 acceptance
   open. A consumed package without the typed durable receipt, a
   partial group or a dedup marker with uncertain outcome all keep
   the retry gate closed. Do not infer
   authenticated inviter from unauthenticated storage fields or a
   successful ingest.
7. Observe durable Pending before either case can advance. Manual stays
   Pending without typed UID-authorized approval. Automatic invokes the
   same generation-bound pending-to-Activating operation once for a
   newly validated row, with the local mode, block and expiry still
   valid. Reopen state and confirm exact same generation. No native
   history or PostJoin egress permit, application projection, status
   disclosure, acceptance message, Command or Terminal authority is
   available. Use the durable, exact Pending owner below.

## Minimal durable join owner

Use the Pending-slice checkpoint API if it supplies this contract. If it
has not landed, implement its minimal test-mounted slice under the same
hardened `Checkpoint` schema, coordinated with that owner. Do not add a
parallel registry to the harness. Land these interfaces as one composable
increment before running the WSS selector.

- Add one bounded joined-custody row per occupied reservation. Key it by
  the existing exact profile-local class, generation, origin and selector,
  with unique exact Delivery carrier/revision and a restrictive foreign
  key to its held stage. Add a checked schema mode for tests with no
  released constructor. Attach validates existing rows and never reseeds
  a malformed or unsupported state. Require the exact source, current
  policy/origin, signed Delivery window and prepared-stage witness at
  initial insertion. The Preparing/Prepared states keep their existing
  guards. Record typed `DeliveryHeld` with authenticated Delivery carrier
  ID, revision, expiry and replay boundary, exact outer bytes, nested
  Welcome ID and digest and the verified package event/ref. Save the
  original signed origin and selector plus the expected signed room and
  route by binding to the existing Offer and Consent rows. Hold the
  stage charge and keep the Consent retry payload from resending after
  Delivery custody. Only one competing exact Delivery can claim it.
  Duplicate resumes that exact row. A different candidate never takes
  its place.
- A checkpoint operation returns non-cloneable ingress custody for an
  exact row after independently reauthenticating its signed bytes,
  refreshing current local policy/clock and checking physical witness.
  Check the original file before first ingest and after closing native
  handles. Refuse any mismatching store, uncertain joins, unsupported
  effect, quarantine or policy change. Read the already persisted
  exact `GroupJoined` and matched resource before any ingest on recovery.
  A matching receipt consumes the import right. Commit Pending with a
  full compare-and-swap on DeliveryHeld and the exact stage and local
  generation. Persist native group ID from the joined local store and
  the native pin (epoch and local leaf) after validating the group.
  Preserve the durable native outbox receipt until the Pending commit and
  keep it for this fixture's reopen check. Set Pending expiry to the
  earlier of the signed Delivery expiry or first Pending wall plus
  24 hours. Neither retries nor restart extend it. A consumed package without
  that authenticated receipt keeps the authority and retry gate closed.
  Keep a durable fence when policy fails after a native join. Preserve
  the hold for leave cleanup rather than treating it as pre-native.
- `accept_pending` checks current exact row, generation, native proof,
  original local mode and expiry in the serialized owner. Manual entry
  accepts only an opaque UID-scoped local capability. Automatic entry
  accepts only the owner-local newly verified Pending witness tied to
  that exact row, with Automatic mode still valid. Persist the
  eligibility decision with new Pending if recovery between the two
  transactions is supported. Reopen may resume only that recorded
  Automatic decision under matching current policy, and cannot newly
  promote a Manual Pending on a mode switch. Consume either entry
  through the same CAS into Activating. Retain the package/custody hold,
  keep all application and transport routing gates closed, and do not
  allocate Active or Accepted in this increment.

## Acceptance and gates

- Run ignored exact M1 selector
  `peer_invitation::private_wss_tests::peer_private_wss_offer_consent_delivery`.
  Verify it stops at authenticated Delivery, with no consumed package,
  native group, cursor-completion claim for that Delivery or acceptance.
- Run ignored exact M2 selector
  `peer_invitation::private_wss_tests::peer_private_wss_manual_pending_and_auto_activating`.
  Run Manual and Automatic
  sequentially with separate keys, store and relay. Both check exact
  Origin, authentication account and recipient filter, all three relay
  carrier IDs, client Consent byte equality, native current lifecycle
  consumption of the original KeyPackage and exact Pending evidence after
  reopen. Verify per-client one stage and one retained application room,
  client group subscriptions zero, 30443 writes zero, native 445
  writes zero and the three admitted control publications only. Exact
  same-ID replay uses the exact WSS history page on a fresh owner
  and remains inert. Capture an Undelivered or duplicate disposition
  through the same recipient dispatch and assert the exact durable
  Delivery carrier fence before the page can advance. Relaying a valid
  signed Delivery for a different Consent or presenting stale socket evidence
  must not reach native import. Compare native ingestion count, custody
  evidence and unchanged staging generation. Keep negative traffic
  bounded and account for its extra carriers separately.
- Enforce closed-stage extent and sidecar checks from `peer_stage_files.rs`,
  exact profile-class custody and one ten-minute reserve charge without
  recharging retry. The existing 16 MiB aggregate VFS proof uses a
  process-wide default admitting one stage and rejecting the cursor and
  founder stores, so that exact helper cannot enforce the live sum in this
  multi-store process. Keep the standalone quota proof and require a
  qualified live-cap seam before deployment. Do not describe a before/
  after size observation as the live sum. A production-capable follow-up
  needs named/scoped VFS selection or reviewed isolation of the cap
  worker and exact join operation.
- Fixture prerequisites are reviewed, integrated Prepared Consent
  queue/attempt/ACK, a recipient-owned authenticated Delivery capability
  and, for M2 only, the independently approved and integrated durable
  joined Pending contract above. Coordinate the minimal checkpoint
  changes with the Pending owner. Missing exact public MDK evidence
  may require a narrow upstream read interface. No direct SQL mutations, raw storage
  promotion, test-support `session_mut`, spliced native runtime, or
  release of a rejected stage to satisfy assertions.
- Production availability additionally requires explicitly qualified
  registered-writer, recipient-scoped auth, size/rate/storage/retention
  and deterministic-page contract at the exact configured origin.
  The fixture may use the current unpublished test trust capability
  and must never create a deployment constructor. Live aggregate stage byte
  cap, durable pending decline/expiry and native routing re-drive,
  exact scoped activation history and PostJoin rotation, authenticated
  acceptance receipt and current accepted native route are separate
  release gates. No app/command/status behavior can depend on this
  fixture.
- Run the ignored test serially on native Linux at integration and repeat
  on supported targets during opt-in qualification. Run the existing
  raw socket, Prepared Consent, staged join and Pending tests, plus
  formatting, lint and fixture diff checks. Keep the real WSS selector
  out of routine CI. Conduct independent Claude Code Auto review of
  the integrated patch before publication.

## Heartbeat reuse

Keep only the listener, trust-on-loopback factory and bounded observation
utilities shared with Terminal. Later heartbeat qualification may open
the same fixture mode after durable accepted authority is independently
proved and exact room subscription/publication is permitted by a
reviewed extension. Reuse the encrypted founder and client stores,
native route and current connection ownership from that later owner.
The terminal relay's generic 445 filter checks only route shape. It
does not establish accepted membership or a confidential route. This
enrollment fixture denies 445. A later case must prove exact room and
audience scope plus native authority, with no host statistics reaching
Manual Pending, Activating or revoked peers.
