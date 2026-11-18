# Peer invitations

Proposed design for the server invitation increment of the
[interactive peers roadmap](interactive-peers-roadmap.md). This phase adds a
nonexecuting peer channel and local invitation inventories. It provides the
authenticated channel that the later private heartbeat phase will use. A known
server is a server the client accepted. Reachability remains unknown until the
heartbeat phase has evidence.

The ordinary first-Welcome path remains blocked under the pinned MDK. The
[staged inspection design](peer-staged-inspection.md) evaluates separate custody.
Production invitation handling remains in design.

## User interface and authorization

- Client and both-role profiles default to `manual` invitations. An additive
  `peer_invitations` configuration setting accepts `manual`, `auto` or
  `off`. Older configurations load as `manual`. Setup can change the setting
  through the existing explicit update and atomic config commit. Server-only
  profiles have inbound peer invitations disabled.
- An operator runs `marmish peers invite CLIENT_NPUB` on a server or both-role
  profile. The daemon admits the request only while that exact canonical key is
  present in `execution_clients`, differs from the local key and has capacity
  to start a peer room. The persistent intent uses that pinned key. KeyPackage
  discovery uses configured relays only. No usable package leaves an outgoing
  pending entry with a bounded retry schedule. Revocation or a role change
  immediately stops that intent and prevents publication and later status
  delivery.
- `marmish peers servers` and `marmish peers show SERVER_NPUB` read the local
  client's inventory. Each row shows the full canonical npub, optional local
  alias, state and local state-change time. Pending, accepted, blocked and
  revoked are distinct. An expired invitation drops from the pending list.
  Pending is an authenticated invitation awaiting approval. An accepted entry
  identifies a known server. A blocked entry identifies a key whose new
  invitations are suppressed. Display an alias only when it comes from local
  configuration and resolves unambiguously to that key. The phase exposes no
  contact fields or claimed host names.
- `marmish peers accept SERVER_NPUB INVITATION_ID` approves exactly the
  displayed offer for that key. The ID binds the validated content and staged
  revision. A stale selection fails without approving a newer offer.
  `marmish peers block SERVER_NPUB` rejects future offers by that key and
  removes its active channel. `marmish peers forget SERVER_NPUB` removes the
  channel and visible invitation history while preserving short replay
  fences and any explicit block. An explicit unblock permits fresh offers.
  These operations affect peer state only.
- `marmish peers clients` and `marmish peers show CLIENT_NPUB --client` report
  configured client keys and outbound peer invitation state on the server.
  Distinguish grant present with no invitation, waiting for a KeyPackage,
  Welcome acknowledged by relay, and accepted by client. The later private
  status phase adds command policy details and last successful client access.
- Same-UID IPC is the only local control plane. Provide a separate typed
  `PeerRequest` path before the command classifier in `daemon::dispatch`.
  Paginate on canonical key with a bounded opaque cursor and a page size of
  at most 64 within the existing 64 KiB `ipc::FRAME_LIMIT` envelope. A missing
  daemon produces the normal local readiness error. List and show perform
  bounded reads without making network requests.

The policy setting grants a peer channel only. Membership, manual acceptance,
automatic acceptance, receipt of any peer message and alias lookup can never
write `execution_clients`, `trusted_remotes`, the command engine allowlist or
terminal authorization. A manual confirmation applies to a single invitation
revision and does not enroll future invitations from that server automatically.

## Group and message authority

Use one peer room per local profile and remote canonical key in each direction.
A both-role identity can hold one local-client room and one local-server room
with the same peer. Version 1 uses a dedicated MLS room purpose
`marmish/peer/1/<128-bit-room-id>` and exact description
`marmish-peer/1 peer transport`. Use a fresh identifier and random Nostr
transport group ID. Bind the purpose to the authenticated initial metadata.
Reuse the creation request's routing component and exact configured relay
intersection. A valid but unconfigured relay never establishes a route.

The server is the authenticated MLS Welcome signer, the NIP-59 seal signer
and the sole initial MLS administrator. The recipient occupies the client
leaf. Require the Current protocol profile, exactly two active leaves for
two different canonical accounts, an active local leaf, and an ordinary
account Welcome. Check that the destination is this device's eligible
KeyPackage, that the authenticated relay hint is consistent with the MLS
route, and that the complete route is checked before subscription. A retry
of a published Welcome must preserve the original room and offer. A second
device leaf, replacement/rejoin or a second Welcome attempting to replace
an accepted room requires a later protocol revision or explicit local
forget and a fresh room.

Split the existing structural part of `mdk_adapter::check_group_for` from
purpose-specific admission. Existing Foundation and Command branches still
require their existing `allowed` keys and permanent role derivation. Peer
admission uses its own policy and table, never the global `allowed` set.
Bind the peer record to local account, remote key, local role, MLS group,
transport route, room ID, initial app components, administrator and sorted
leaf account/index identities. Revalidate authority before registering or
restoring a route, emitting or accepting a peer record, and after every
membership or metadata transition. Refuse administrative or role changes,
additional/duplicate leaves, prohibited components, local removal,
rejoin/branch replacement and purpose changes. Revoke and unregister the
room on an authority change. Do not revoke solely on a transient inability
to read state. Pause use of the route and retry the state read.

Only a small versioned client-to-server `Accepted` message is supported in
this phase. It binds both accounts, room ID and the accepted offer. Check
the authenticated sender account and leaf, current epoch authority, exact
peer purpose, and fixed schema before recording the acknowledgment. Use the
existing native application-publish proof and retry pattern with an idempotent
receipt. Relay delivery acknowledgment and an authenticated application
acknowledgment are separate states. Replays cannot change either accepted
peer identity or authorization. Reject all regular command, stream, copy,
terminal and Foundation messages through an explicit peer dispatcher.
Use no fallback when a purpose is unrecognized or a pending room is absent.

## Welcome boundary and upstream gate

Pinned upstream revision `8abc51befbf71a1ae3bdbb25eb0860c731e75db2` exposes
these seams:

- `transport-nostr-peeler/src/peeler.rs:219–305` exposes
  `peel_welcome_with_provenance`. It verifies the outer wrapper and the
  NIP-59 seal, associates the seal sender with the rumor author, computes
  the rumor ID, and checks recipient and required tags. It returns the
  authenticated seal sender, candidate MLS Welcome bytes and KeyPackage
  event ID. It validates the rumor's relay list internally, so expose the
  authenticated values through the new admission seam wherever they must
  be compared with the verified MLS route. At this point group purpose,
  founder, roster and application components are still unverified by MLS.
  The outer Nostr author's key is ephemeral and cannot identify the server.
- `cgka-engine/src/group_lifecycle.rs:1557–1715` stages OpenMLS and validates
  the MLS signer and group proof. Its initial-join path does not generally
  require the NIP-59 seal sender to equal the MLS signer. The equality
  check near line 1699 applies to dated-reset replay protection.
  `GroupJoined.welcomer` is the MLS signer.
- That engine transaction writes the joined group, pending application
  event, replay markers and KeyPackage consumption before Marmish receives
  `GroupJoined` (lines 1932–1977). The existing pending rejoin branch at
  lines 1823–1841 rolls back only to request consent for replacement of a
  local group. It provides no pre-commit admission API for a first Welcome.
  `AccountDeviceRuntime::ingest_delivery_with_observer` in
  `marmot-session/src/runtime.rs:4058–4111` reports phase timings after
  mutation and offers no admission control.
- In Marmish, `MdkOwner::receive_one` sends deliveries directly to that
  mutation path before `apply_effects`. `forget_group_local` after rejection
  can remove an installed group but does not undo initial KeyPackage
  consumption or create a safe staging transaction.

Obtain and review a mainline MDK revision that exposes authenticated,
rollback-only first-Welcome inspection or an equivalent transactional
application admission API. The API must return the verified MLS signer,
canonical two-leaf roster, recipient leaf and KeyPackage, metadata and route,
and a digest/token identifying the exact candidate. It must support a
deferred decision without committing group state, advancing epochs,
consuming KeyPackages, emitting events, changing subscription state or
marking the content permanently processed. Acceptance must revalidate the
same wrapper, metadata, signer, route, token/revision and current eligible
KeyPackage inside the commit transaction. Both the inspection and final
acceptance enforce seal/MLS signer equality for Peer rooms. Invalid candidates
produce no persistent engine artifacts. A first-Welcome approval API also
needs an error outcome that distinguishes deferred consent and retryable local
failure from terminal rejection.

Do not enable incoming peer invitations or found peer rooms until this
dependency is proven by storage and retry tests. The pin does not provide
this API. At upstream `0ad3c610`, `do_join_peeled_welcome` still enters the
transaction without a first-join consent policy. The invitation generation
is saved with the installed group. Pending consent requires a previous local
copy. Do not implement copy-and-swap of the live databases, fork and
reimplement OpenMLS joining, or join a live unknown or unapproved candidate
and later forget it. These alternatives alter key custody, consume a public
KeyPackage prematurely or leave persistence and publication gaps. If upstream
has an equivalent newer API, document its actual transaction and test it
before changing the pin. The existing restricted legacy MDK patches apply
to a separate Python reference build and do not supply these capabilities.

## Bounded admission and recovery

1. Keep the existing configured inbox relay policy. Before inspecting an
   unknown invitation, apply a separate bounded Welcome work budget and
   ciphertext size cap. Use ciphertext event IDs for early duplicate
   checks. Reserve work before NIP-59 verification, then apply per-sender
   limits only once the seal identifies the actual sender. Check client
   role and policy, block status, candidate freshness and explicit resource
   availability. Classify by authenticated MLS purpose after preview. Carry
   a safe pre-commit admission hook through this path even for a key already
   trusted for ordinary commands.
2. Proposed initial limits are 64 KiB per encrypted offer, at most one
   pending offer per authenticated server, 16 outstanding offers and 1 MiB
   of pending offer data per profile, 64 accepted client-side channels, 64
   outbound pending/active channels on a server, and 128 explicit blocks.
   Bound processing to two candidate previews per daemon turn and four
   per minute per authenticated sender, plus one shared token bucket per
   profile for candidate work. Persist a bounded short-lived per-sender
   counter only for retained candidates. Use a bounded in-memory rejection
   cache for other senders and a bounded durable dedup set for staged and
   accepted offers. Capacity rejects the newest work and preserves existing
   entries. A shared inbox may still be congested by remote traffic.
3. Store the original encrypted wrapper, verified offer ID, key,
   authentication digest, revision and local expiry in the existing
   encrypted journal. Address rewrap replay by the authenticated inner
   Welcome content digest and recipient KeyPackage as well as outer event
   ID. A new envelope alone confers no freshness or new storage slot.
   Use a bounded 24-hour pending interval and authenticated rumor time,
   with existing clock/replay-floor conventions and explicit future skew
   checks. Ensure an accepted/forgotten/expired offer remains out beyond its
   earliest possible fresh reappearance. Never use randomized outer
   wrapper timestamps for consent freshness. Expiry removes pending bytes
   and preserves the replay fence only as long as needed.
4. Manual mode stores a validated offer without a live MDK membership,
   reply or routing entry. Auto mode uses the same gates and immediately
   records a decision for that exact offer. Off mode rejects new offers and
   leaves existing peer channels visible. Block wins over manual and auto.
   Failing validation does not expand a book of durable attacker-controlled
   keys. Ack the transport replay delivery only after a durable decision
   or an intentional disposable reject; a failed journal write leaves it
   eligible for retry. A full queue cannot pin the ordinary inbox cursor.
5. Before accepting, journal a decision intent, then commit the verified
   Welcome through MDK, then bind the accepted peer and register routing.
   Reconcile a crash between these steps with the exact durable intent,
   validated GroupJoined identity and canonical authority. Project pending
   peer joins before their messages, without allowing command selection.
   Exclude pending and revoked groups from the route set. For outbound
   rooms, journal creation intent and room ID before MDK creation. The
   pinned runtime's `prepare_create_group_with_audit_context` and
   `publish_prepared_session_effects_with_audit_context` provide separate
   local creation and publication steps (`marmot-session/src/runtime.rs`,
   starting at lines 3431 and 3505, respectively). Validate and bind authority, current eligibility and
   the durable Welcome obligation before calling the publication step.
   The ordinary `create_group` publishes before returning and cannot
   enforce this ordering. Recover exact prepared Peer rooms before the
   existing `MdkOwner::reconcile_orphans` prunes untracked groups. An
   uncertain creation cannot create a second room by retry. Withdraw
   queued Welcomes before cleanup where possible. Revoke first, remove
   routing, and verify native cleanup before finalizing deletion. A
   subsequent new room uses a fresh ID. Restart must not re-enable
   invalid, blocked or unconfigured peers.

## Implementation and evidence

- Agree on the upstream contract and build a focused dependency proof
  against encrypted SQLite and a real signed first Welcome. Add positive
  fixtures for publication and manual/automatic acceptance plus negative
  tests for seal/MLS signer mismatch, wrong destination, wrong purposes,
  unconfigured relays, duplicate account leaves, extra devices, wrong
  profile, extra administrators, stale data, branch replacement and content
  rewrap. After rejected and deferred previews check KeyPackage usability,
  installed group rows, pending events, routes and replay dispositions.
- Add the journal records and bounded migration from currently supported
  format 7. Tests exercise role transitions, policy changes, expiration,
  replay-fence bounds, stable pagination, visible list/show states and
  allowed-key removal. Add crash/restart fault points around intent,
  MDK commit, durable projection, acknowledgment and cleanup. Verify
  idempotency and that command trust and execution policy remain intact.
- Integrate `peer_service` / `peer_adapter` modules with
  `mdk_adapter.rs`, `daemon.rs`, `ipc.rs` and `cli.rs`. Use
  `command_runtime::create_command_room` as a creation/reference path,
  `mdk_adapter::discover` for bounded KeyPackage discovery and the existing
  native `Fixture::join_with_route` to build malicious and valid group
  fixtures. Test that a previously unknown accepted server cannot open a
  Command, Stream or Terminal channel through its peer room and cannot
  change any allowlist by proposing another purpose.
- Run local integration with disposable identities and an explicit
  configured WSS relay or the owned onion fixture. Check the actual client
  and server inventories, manual no-join until acceptance, sealed client
  acknowledgment, auto opt-in, ordinary command interoperability and
  blocked/expired replay. Verify that an unavailable client waits and
  becomes eligible when it publishes a usable KeyPackage. Clean owned
  processes and ephemeral state.
- Record candidate verification time, publication-to-pending latency,
  approval-to-acknowledged latency and admission under the configured burst
  limit. Benchmark each stage separately so operator response time stays
  out of protocol latency. Record relay/route mode, sample count, median
  and p95. Keep GitHub Actions to focused deterministic validation of
  policy/role closure, rollback preview and a representative restart
  path on self-hosted runners. Full integration and benchmarks run under
  local or manual validation.
- Review this detailed design and its dependency proof with Claude before
  coding. Obtain Claude Code auto-mode approval of integrated code and
  tests before recording the phase as complete.
