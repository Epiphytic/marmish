# Authenticated Peer recipient route

Base `5e3c0a8` plus checked route implementation integrated as `31116d0`.
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

## Owner and recipient operations

1. Let the private live origin own the existing `CheckedPeerRoute`.
   Select the endpoint, signer account and single connection scope from
   that checked binding. Derive the existing direct-only bounded TLS
   policy from its held configuration/CA evidence. Reuse the
   `connect_stream` and `websocket` primitives with one owned permit.
   Keep exact URL, TLS configuration, trust root, connector, socket
   halves, cancellation and mutable binding private. No external URL
   argument, SDK client, proxy, Tor or inherited Command routes. The
   no-network probe remains a distinct typed authority and uses the
   existing disposable signer only for its own probe. Require the same
   leased material signer for live AUTH and perform no new provider read.
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
   Enrollment owns that missing admission/cursor wiring and the native
   slot checks. The opt-in private test drives the port behind a
   test-only admission adapter but uses the actual checked binding,
   TLS, socket, v2 marker and persistent cursor. No test admission
   constructor is available with `installed-checks` alone.

## Exact final handoff

Implement the transport operation once, with a private consuming turn
bound to the owner incarnation, authenticated author/account, exact
recipient, origin, lane, event ID, byte digest, expected control
operation identity/generation and deadline. Verify shape, canonical
routing fields, signed event ID/signature and signer account before
handoff. The operation uses existing `predrain`, frame construction,
bounded packet flow and exact ACK correlation.

The production port keeps its send activation private to its future
durable consumer. The route cannot construct operation authority from
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
predrain and frame validation, perform the fresh row and exact live
route recheck before polling the sink. If readiness yields, recheck
immediately before the first poll that can hand off the exact EVENT,
with no intervening await between the final check and `start_send`.
Use the existing stream/sink lifetime and cancellation plus the outer
deadline. If the underlying sink internally defers the frame, keep
the cancellation and deadline armed for flush and receive. Attribute
no exposure before the handoff boundary. After handoff, missing,
invalid or late ACK, cancellation, route lapse, or sink error is
possibly exposed and retires the socket. A current exact negative ACK
is a rejection. A positive ACK is just a scope-bound transport witness
and is revalidated before synchronous caller use. No automatic retry,
durable state advancement from a stale ACK, or claim that a late
failure can recall transmitted bytes. A dropped pending operation
retires the generation under existing `ActiveOperation` ownership.

## Ownership and exit evidence

Transport owns `transport.rs`, `peer_relay_trust.rs`,
`peer_physical_route.rs`, recipient `peer_raw_socket*.rs` and
a narrow `peer_invitation.rs` origin/port boundary coordinated with
recipient startup. Enrollment owns profile role/quota/cursor/consent,
daemon mounting and product status. Cursor keeps the reviewed native
quota/capped Join scope. No configuration/schema changes in this
increment. Preserve Command and Terminal routes and privacy policy,
and existing Peer qualification behavior.

- In a disposable Ready profile, qualify the private WSS relay with
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
  operation. Exercise the pre-sink polling distinction with deterministic
  backpressure rather than timing assumptions.
- Run exact native private fixture and regression selectors for prior
  Peer raw socket, installed v2 marker, qualification, ordinary
  Command/Terminal connector paths, formatter and strict all-target
  Clippy. Run an ordinary release-feature compile with
  `installed-checks` to prove fixture-only entry points are unavailable.
  Report private relay timings and distinguish transport/cursor proof
  from installed worker and cross-platform qualification.
