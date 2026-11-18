# Authenticated Peer recipient route

Base `422913d`, the checked route stack atop published `5e3c0a8`.
Its tree is identical to composed `31116d0`.
This delivers the recipient transport port in
[production Peer route ownership](peer-production-route.md) for the first
[installed heartbeat predecessor](peer-installed-heartbeat-first.md).
The positive end-to-end cut is an actual native v2 Offer admitted over an
authenticated private WSS connection into the existing installed Offer
checkpoint. The subsequent worker mounts the same port with quota and
durable custody. The port is compiled in normal Rust builds. This increment
does not start it from daemon startup or add an operator command.

## Existing boundaries

- `CheckedPeerRoute` owns the Ready lease/identity binding, frozen
  qualification and attestation pair, configured CA witness, cancel
  state and effective clock. Its successful recheck establishes physical
  authority, independent of role, policy and Peer cryptography.
- `PeerRelayConnector::open_probe` already uses the single-origin
  direct path and typed TLS without using an ordinary SDK client. The
  generic `RelayConnector::open_socket` and `finish_socket` must keep
  their ordinary behavior. `PeerTlsAttempt::connect` currently drops
  the certificate window after verification.
- `socket::owner::Owner` already serializes the transport. `Socket::ready`
  currently conflates AUTH and live subscription. `publish_bytes` already
  predrains and verifies the frame, but has no exact live qualification or
  current-row recheck at the write boundary. Owner currently accepts Page,
  LiveRecord and Acknowledgement on pointer equality and active generation.
- `InvitationLane::open` uses a fixture-only `QualifiedOrigin` and
  `Checkpoint::attach` (legacy). Its bounded history/live and cursor
  logic shows the existing consumption order. The installed marker uses
  the distinct v2 checkpoint path. A valid retained Offer grants neither
  Pending state, native package/Welcome authority nor permission to publish.
  The new port will have no call from the legacy lane in a normal build.

## Crate-private port

Under `peer_invitation`, add `PeerRecipientPort` wrapping the existing
serialized socket Owner. Its only live opener consumes the sealed
`ReceiverRouteAdmission`. Proposed signatures use current types and names
where possible.

```text
PeerRecipientPort::open(ReceiverRouteAdmission, Instant) -> Result<Self, Failure>  [async]
port.page([u64; 2], Instant) -> Result<Page, Failure>                             [async]
port.wait_live(Instant) -> Result<Option<LiveRecord>, Failure>                  [async]
port.use_page(&Page, FnOnce(CheckedPage<'_>) -> T) -> Result<T, Failure>
port.use_live(&LiveRecord, FnOnce(CheckedLive<'_>) -> T) -> Result<T, Failure>
port.publish_control(ControlSendPermit<'_>, Instant) -> ControlOutcome          [async]
port.use_accepted(&Acknowledgement, FnOnce(CheckedAck<'_>) -> T) -> Result<T, Failure>
```

`CheckedPage` and `CheckedLive` lend the exact indexed records, effective
wall, local account and configured origin only for the synchronous call.
`CheckedPage` includes request ID and fixed bounds. `CheckedAck` lends the
exact accepted carrier, authenticated account, origin, effective wall and
operation identity. These are checked physical observations. A callback
cannot return a reference borrowed from the observation or transfer it
as future authority. Trusted control contents and durable mutations are
checked independently by the consumer. Keep bare legacy Owner/open/record
access inaccessible to the live port consumer.

`ReceiverRouteAdmission` owns the non-clone checked route and the
`Arc<Keys>` obtained from the same `IdentityMaterial` that authenticated
the Ready profile and binary device slot. It is sealed in the port
boundary and has only a `cfg(test)` constructor in this increment. The
production open, verification and operation bodies compile normally and
the test drives those bodies. The test constructor calls the actual
checked binding and uses the actual profile signer. The future production
constructor must consume the paired installed receiver/registrar and
cursor admission described below. No caller-supplied lease, role boolean,
bare physical route alone or SDK connector can start a product socket.
The signer public key is still compared with the current route account at
open before AUTH. The signer is dropped after AUTH. No secret is attached
to buffered evidence. `Failure` preserves the existing wire errors and
adds a fixed refusal for revoked checked authority when needed. No
diagnostic includes URL, account, pin, certificate bytes or event data.

## Owner and recipient operations

1. Let the private live origin own the existing `CheckedPeerRoute`.
   Select the endpoint, signer account and single connection scope from
   that checked binding. Derive the existing direct-only bounded TLS
   policy from its held configuration/CA evidence. Reuse the
   `connect_stream` and `websocket` primitives with one owned permit.
   Keep exact URL, TLS configuration, trust root, connector, socket
   halves, cancellation and mutable binding private. No external URL
   argument, SDK client, proxy, Tor or inherited Command routes. Keep
   the existing explicitly initiated probe authority separate. Change
   the checked route to retain the same `Arc<ProfileFiles>` for the
   existing weak TLS lifetime machinery. A private route method makes
   the TLS attempt and its single-endpoint policy after recheck. Build
   CA-mode roots directly from the exact retained `CheckedCa` bytes
   frozen by `read_issued_held`. Keep the descriptor and digest checks
   independently active, with no second CA selection or path lookup to
   obtain TLS root bytes. Probe/ordinary methods retain current trust
   selection. Perform no new provider read.
2. Recheck the existing binding before the dial and after TLS/upgrade
   before AUTH. Return an opaque connection window from the actual
   verified TLS handshake. Use the intersection of leaf and supplied
   bounded certificate validity windows and configured trust anchor
   window. WebPKI retains the chain, host, root and path verification
   decision. Checking the windows of all supplied intermediates may
   conservatively reject a certificate list with unused expired entries.
   No extra presented certificate is a trust root. No early data or
   session resumption. Recheck this window and frozen path trust against
   the same effective clock through socket use. The test CA on the real
   path is an installed profile credential verified by the qualifier,
   with no fixture TLS bypass or process-global roots.
3. Split the existing exact `Socket::ready` protocol into authentication
   and live-open operations. Reuse one bounded budget across both.
   Wait for one bounded fresh relay challenge, sign exact kind 22242
   with current origin and challenge, and require the positive ACK for
   that exact signed ID. Recheck before AUTH send, after its correlated
   ACK, before the exact recipient kind 7435 `#p` REQ, and after the
   matching EOSE before marking the generation ready. Missing or
   negative ACK, wrong EOSE, unexpected challenge, stale or mismatched
   ID, or cancelled or changed binding retires the generation. Evidence
   cannot cross a connection. No unauthenticated readiness.
4. Promote only exact recipient control live/history for this port.
   Retain frame, byte, control, page, live queue and per-turn deadlines,
   strict parser and recipient matching. An authoritative owner-local
   read/use method takes the sealed Page or LiveRecord and a synchronous
   consumer. Revalidate the checked route and certificate window, owner
   generation, subscription and peer/endpoint binding immediately
   before lending evidence and the effective wall for consumer use.
   SQLite remains outside the socket module. Do not expose a production
   bare Arrival, Page acceptance boolean, or receipt success usable
   without this check. The consumer still checks its own local cursor
   availability and trusted signed v2 Offer ancestry and only advances
   using exact durable dispositions already committed for that page.
   Revocation withdraws buffered observations. Reopening uses a new
   binding and a new socket generation and recovers from durable cursor
   state.
5. For the forthcoming worker, accept a sealed receiver admission at
   the mount: same leased Arc, Ready account/device, exact retained
   config content/inode, private direct endpoint, supported Linux AMD64
   role/receive policy, mounted quota/registrar and available exact
   durable cursor. Current `ReceiverCandidate` only rechecks role and
   normalized route, and `ReceiverQuota` has no production constructor.
   Enrollment owns that admission/cursor wiring and the native slot
   checks. Each authoritative consumer use rechecks its own quota and
   durable state. The physical port does not inspect or assert that
   state. The opt-in private test drives the port behind a
   test-only admission adapter but uses the actual checked binding,
   TLS, socket, v2 marker and persistent cursor. No test admission
   constructor is available with `installed-checks` alone.

## Exact final handoff

Implement the transport operation once, with a private consuming turn
bound to the owner incarnation, authenticated author/account, exact
recipient, origin, lane, event ID, byte digest, expected control
operation identity/generation and deadline. Verify shape, canonical
routing fields, signed outer event ID/signature and digest before
handoff. The kind 7435 outer is signed by a distinct cover identity.
The real inner author, obtained from sealed durable operation authority,
must equal the NIP-42 account. The socket must neither require that the
cover signature names that account nor infer inner authorship from it. The operation uses existing `predrain`, frame construction,
bounded packet flow and exact ACK correlation.

`ControlSendPermit` is a non-clone consuming authority with private
fields. It carries the exact bytes and borrowed synchronous row-check
capability alongside the pinned coordinates. The type has no production
issuer in this increment. The single production-compiled send core is
exercised through the `cfg(test)` sealed exact permit. The route cannot construct operation authority from
`ready_to_publish`, an arbitrary event/request, or relay ACK. A
synchronous fence owned by the operation caller checks the same durable
row and exposes no SQL handle to the socket. The test-only exact
fixture proof supplies a sealed expected operation with a mutable
one-use row witness for fault injection. It cannot be converted into a
production activation token. Preserve the existing fixture send APIs
behind test-only reachability if no installed caller needs them.
A future recipient Consent mount must supply the frozen durable
Prepared Consent authority and exact v2 flight, row generation, account,
recipient, origin, ID/digest and deadline. Existing v1
`reserve_attempt` does not authorize this path. Group requires the
separate accepted-group binding and durable `EgressAttempt`, so leave
Group module scope and production activation out of this increment.

Hold the owner exclusively across predrain and until the result. After
predrain and frame validation, drive readiness, `start_send` and the
first `poll_flush` in one `poll_fn` call when readiness is immediately
available. Fresh-check the exact row, live route, cancellation and
deadline before readiness and again before each write-driving poll.
If readiness is pending, repeat checks before the next poll. The actual
sink's futures `With` and `SplitSink` can buffer at `start_send` and
delegate the first underlying EVENT write only on `poll_flush`. Treat
attempting the first flush that can drive the EVENT as the conservative
exposure boundary. On later flush polls recheck everything again. Drop
both halves immediately on refusal. Do not call `close` to retire an
uncertain write because it may flush its buffered frame. Keep exact caps
and the transport's cancellation and deadline armed through flush and
ACK reception. The synchronous checks read bounded files at poll
boundaries. Validate frame size before handing it to the adapting sink
so its limit cannot be the first refusal. Do not infer no exposure from
an erased SDK I/O error after the first write-driving poll.

`ControlOutcome` is `NotSent(Failure)`, `Rejected`, `Accepted(Acknowledgement)`
or `PossiblyExposed(Failure)`. Local failures before the first
write-driving poll, including known withdrawal and cap failure, are
NotSent. If a private typed adapter reports that its own limit or
cancellation guard failed before delegating any EVENT write, it is also
NotSent. Other sink errors at or after that poll, missing or invalid ACK,
late timeout, cancellation and scope withdrawal are possibly exposed
and retire the socket. Exact current negative ACK is Rejected. Exact
positive ACK is just a scope-bound transport witness and is revalidated
before synchronous caller use. No automatic retry,
durable state advancement from a stale ACK, or claim that a late
failure can recall transmitted bytes. A dropped pending operation
retires the generation under existing `ActiveOperation` ownership.

## Ownership and exit evidence

Transport owns `transport.rs`, `peer_relay_trust.rs`,
`peer_physical_route.rs`, recipient `peer_raw_socket*.rs` and
a narrow `peer_invitation.rs` origin/port boundary coordinated with
recipient startup. Any private relay harness change is test-only and
coordinated with the current Terminal fixture owners. Enrollment owns profile role/quota/cursor/consent,
daemon mounting and product status. Cursor keeps the reviewed native
quota/capped Join scope. No configuration/schema changes in this
increment. Preserve Command and Terminal routes and privacy policy,
and existing Peer qualification behavior.

- Add the test-only `peer_authenticated_route_tests.rs` consumer. It
  opens the actual leased `Paths` and `HardenedPeerConnection`, attaches
  `Checkpoint::attach_installed_v2`, binds the exact source and runs
  existing available/admit/page commit operations through checked port
  observations. Cold reopen reads `installed_live_offer` and stored
  scan coordinates using a fresh Source. It does not use legacy
  `InvitationLane`. In a disposable Ready profile, qualify the private WSS relay with
  existing production qualification, close and cold-bind, open actual
  authenticated v2 recipient scope, receive an exact signed cold
  history Offer and a distinct live Offer, and verify installed marker
  and bounded cursor after native SQLCipher reopen. Count the probe and
  live sockets separately. Exercise stale/mismatched incarnation use
  and prove that invalid/wrong-recipient/v1 traffic confers no marker or
  native custody. Prove no different relay is contacted.
- Drive missing, negative and wrong AUTH ACK and a mismatched EOSE on
  actual WSS. Verify no receiver further frame on failure. Exercise
  mismatch material/profile, config/qualification/CA replacement
  between phases, closed owner, retained certificate-window
  boundaries and cancellation. Prefer deterministic clock/phase cuts
  beside representative real WSS checks.
- Prove exact receiving relay EVENT counts and correlation. Positive
  private fixture send sees exact signed bytes once. Wrong bytes,
  route, generation, revoked row during predrain or sink readiness,
  and expiry at or just before the boundary produce zero EVENT sends.
  A forced lost ACK after exact receive produces one possibly exposed
  result and a retired generation. Same carrier on another incarnation
  and a reused turn refuse. Assert no stale ACK advances the local
  operation. Test the pre-sink distinction and zero-event-on-lapse
  with a `cfg(test)` injected counting sink/stream using the same
  boxed sink interface. Drive Pending before readiness and during
  flush, with independently counted real handoffs and simulated
  cancellation. Also keep actual private WSS positive and lost-ACK
  checks.
- Run exact native private fixture and regression selectors for prior
  Peer raw socket, installed v2 marker, qualification, ordinary
  Command/Terminal connector paths, formatter and strict all-target
  Clippy. Run an ordinary release-feature compile with
  `installed-checks` to prove fixture-only entry points are unavailable.
  Report private relay timings and distinguish transport/cursor proof
  from installed worker and cross-platform qualification.
