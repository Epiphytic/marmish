# Production Peer route ownership

Base `97a1cad`, the published qualification source and validation.
This implements the route prerequisite in
[installed Peer heartbeat A](peer-installed-heartbeat-first.md) and
[installed enrollment](peer-installed.md). The current native relay CLI
qualifies disposable probes. `QualifiedOrigin` in `peer_invitation.rs`
still has only fixture constructors over a generic connector. The raw
recipient socket is compiled but dormant. Its Group implementation is
`cfg(test)` and imports a Terminal fixture fingerprint. The independent
daemon worker, UID IPC and Prepared Consent owner are also test-mounted.
Promoting a TLS opener alone cannot safely start an installed subscription.

## Owner and current qualification

- Keep the one direct pinned origin selected by `PeerRelayOwner::bind`,
  from the checked profile lease and the exact full `config.json` inode
  and digest. Reuse its private directory descriptor and immutable
  origin/CA or SPKI policy. Extend `ProfileLease::match_peer_manifest` to
  compare the full leased Ready manifest, including profile ID, account,
  device slot and backend/key references. A changed file, lock,
  configuration, TLS evidence or cancellation invalidates the active scope.
  Unrelated edits to `config.json` also close the scope. Preserve current
  Command/Terminal relay discovery and defaults. Missing/Off/role-ineligible
  intent performs no Peer allocation or network operation. Tor-required,
  public defaults, redirects, fallback and URL input from a remote flight
  are outside this owner.
- `qualification_status`/`show` stays read-only with no credential access,
  network, creation or ability to return a live capability. For otherwise
  current records return `unqualified` for schema 1, `requalify_required`
  for schema 2 and `current_metadata` for schema 3. Keep existing distinct
  stale, expired and untrusted-clock statuses. This supersedes the earlier
  schema 2 visibility rule in the first installed heartbeat plan. After an
  authenticated durable readback, the successful `qualify` action can
  return `qualified` directly. It must no longer assert a `show()` result.
  Require structurally canonical schema 3, including exact lowercase tag,
  on authenticated runtime readback. Keep the same TTY affirmation
  and supplied attestation grammar. After successful disposable probe and
  final snapshot checks, the issuer promotes the canonical attestation
  to an exact 0600 no-follow owner file `peer-relay-attestation.json` and
  writes schema 3 of `peer-relay-qualification.json` with an issuer tag.
  Preserve separation of the digest-only record and private attestation.
  Store no profile signer material, raw relay frames or probe secret.
- Authenticate the canonical closed record excluding the tag using
  HMAC-SHA256 with a Zeroizing, domain-separated HKDF key derived from the
  existing profile SQLCipher root, using account, profile and device
  binding. The record already binds the canonical origin, trust, suite,
  bounds, contract, exact MDK revision, build target and timestamps.
  Schema 3 adds `MARMISH_BUILD_COMMIT`. Extend `check_build_scope` to
  reject malformed and all-zero commits at both issuance and authenticated
  readback. The zero fallback from a git-less, unstamped build cannot
  qualify or consume a live record. HMAC covers both independently
  computed evidence digests. Only the sealed
  completed-probe + current affirmed-attestation path can issue a tag.
  Add `PeerRelayOwner::bind_live(Arc<ProfileLease>, &CommandConfig,
  &IdentityMaterial, ReceiverAdmission)` for the eventual mounted consumer.
  Only the admission owner creates `ReceiverAdmission` after checking
  client/Both intent, platform eligibility and quota mount. It has no
  `Clone` or `Default` implementation and
  cannot replace the config or credential check. Borrow material from
  that consumer's actual prepared profile identity, compare public key
  and device slot to the checked manifest, clone only the account signer
  for NIP-42 and discard the derived key after initial authenticated
  readback. Keep probe binding separate, with no managed identity signer.
  Frozen bytes and the matching inode witnesses preserve authenticated
  readback across later rechecks without retaining the SQLCipher root.
- Add a new read-only Ready lease credential method with no `prepare`
  call. Thread the already validated native `layout.helper` from the
  qualification CLI into `qualify`. After acquiring the checked lease and
  validating its snapshot and attestation, run the bounded file/keychain
  provider SQLCipher load in `spawn_blocking` using its own deadline,
  before printing the TTY enrollment prompt or opening probe sockets.
  Reuse the helper exchange's deadline, child termination and reap.
  Retain the lease through task completion and do not rely on timing
  out the async wait to stop a blocking provider call.
  `qualify` now requires an available selected profile credential. A
  locked or denied Keychain fails using the normal typed provider error
  before the operator registers disposable relay principals. Zeroize the
  root after deriving the purpose key. Keep only that derived key through
  the probe and promotion. The issuer never constructs a profile signer,
  creates a secret, opens SQLite or falls back. `show` uses no key and
  operators configure no separate credential.
- Validate the strict attestation and SHA-256 of its canonical bytes,
  the record's current local scope/window and issuer tag using constant
  time verification. A schema 2 report, decoded metadata or edited
  evidence string cannot produce `QualifiedOrigin`. The MAC is local
  issuer provenance under profile custody. It adds no protection against
  a malicious effective UID that can also read provider secrets or
  invoke arbitrary local processes as that owner. Existing expiry and
  current scope reject stale records. Record the residual cross-restart
  rollback limit in the agent specification. An otherwise valid previously
  issued pair has no independent rollback counter.
  Platform eligibility remains a separate check.
- Reuse bounded atomic same-directory staging, fsync/readback and no-follow
  ownership checks for both fixed files. Separate the attestation staging
  name `peer-relay-attestation.new` from existing record staging. Check
  allowable schema 2→3 replacement and an intact old record before touching
  either file. Commit and sync the attestation first, then promote the
  report with its bound digest/tag and verify both. The
  prior report may still be visible after a cut between promotions, while
  the mismatched pair authorizes nothing. Retrying the explicit qualifier
  repairs a decodable old record with a mismatched pair. Keep the existing
  refusal for a corrupt old record. Never reuse staging owned by another
  inode or add hidden repair, root or credentials files. A fresh owner
  binds both current inodes and canonical hashes; requalification with
  even identical contents invalidates an already running scope by inode.
  Actively check both at use with the same profile and config/CA binding.
  Anchor monotonic elapsed time to system wall at open. Require
  nondecreasing wall and at most two seconds of divergence in either
  direction. Apply deadlines and expiry to the later of observed wall
  and anchored elapsed wall, so a slow or stopped clock cannot extend
  authority. Fail closed on conversion errors or overflow. Keep the
  bounds across a running scope and require new admission after it closes.
  If the caller supplies the already-managed attestation file, verify the
  exact existing inode and canonical digest and retain the attestation
  instead of replacing the source underneath the issuance checks.

## Runtime connector and exact turns

- `PeerRelayOwner` reads the exact witness and constructs an opaque
  `QualifiedOrigin` tied to its private typed `PeerRelayConnector`,
  account, lease, canonical origin and immutable qualification scope.
  The fixture-only `RelayConnector` path may exist as a test-only variant
  with disjoint constructor. A live owner gets the one socket transport
  from a dedicated typed `open_qualified` path. No generic clone, raw URL,
  SDK client or fixture roots escape. Recheck before direct connection,
  after TLS/upgrade before AUTH, and after fresh NIP-42 ACK before the
  first subscription or publish. Split the existing combined `ready`
  authentication and subscription. Bind NIP-42 signer to the leased account.
  Seal the checked CA file inode and content in CA mode. After the exact
  handshake, retain the authenticated peer-chain validity window and bound
  socket use by it and the trust anchor's window. Recheck the CA descriptor
  and live window before authoritative I/O. Dropping the checked owner
  cancels and retires the socket incarnation.
- Extract the proven socket `Owner` serialization, bounded frame parser,
  recipient exact `#p` history/live and Group exact `#h` history/live.
  Move only the production Group event verification, kind/caps and
  fingerprint calculation from `peer_private_wss_relay.rs` into the Peer
  socket wire layer, leaving fixture fault controls, oracle transcripts
  and clocks test-only. Require exact canonical Group route and one
  canonical matching `h` tag, valid event ID/signature/kind, bounded exact
  request/EOSE ordering and signed bytes, independent of relay filtering.
  Preserve strict kind 7435 Control (no bare kind-1059 Welcome). Group
  ingress must use a sealed local stage/accepted-group binding for the
  exact native group, transport route, account, endpoint and current
  subscription. The relay route and verified outer signature alone
  grant no MLS sender or group authority. Relay provenance remains
  attached to the exact current delivery and is checked before native
  ingest. Four-record history and live-queue bounds carry over, including
  byte caps. Overflow stops the scope. Broader retained history/paging
  for periodic status belongs to the status phase. Do not expose a raw
  `(route, group, bytes)` product interface.
- Require a one-use private send turn from the existing durable exact
  operation witness at the consumer mount. For Consent reuse frozen
  selector/flight/account/recipient/bytes/dedicated target and exact
  current row, for Group reuse the sealed `EgressAttempt` or equivalent
  validated `check_armed` custody/fanout witness. Bind owner/incarnation,
  lane/route, operation/generation, exact destination, event ID and digest
  plus deadline. Hold the serialized active operation across predrain,
  final revalidation and `EVENT` handoff. Check authority and deadline
  after the predrain await at the `exact_publication`/`publish_flight`
  handoff and immediately before first sink poll. A synchronous owner-local
  fence checks the exact current durable row (`check_armed` or its Consent
  equivalent) and live scope. The socket never owns SQLite. Keep the
  operation local and serialized. Withdraw on close/expiry, with
  cancellation armed while writing. Keep publication callable only from
  a sealed internal test proof until its durable consumer exists. Never authorize
  egress from `ready_to_publish`, a constructed `TransportMessage`, relay
  ACK, or a `CurrentMetadata`/report. After first handoff a late timeout,
  revoked scope, lost ACK or cancellation is uncertain, retires the socket
  and grants no retry. Do not advance a cursor or commit application
  effects using an expired or retired observation. New native egress
  recovery policy remains with the durable lifecycle owner.

## Smallest installed consumer and validation

The first product use of this route is the already planned client/Both
receiver in `daemon::run_inner`: actual `Config` and Ready identity,
platform and quota admission before the first SQLite open, independent
Peer worker ownership and existing same-UID `peer` IPC. Initially use
manual receive with durable checked custody before local Pending. Keep
ordinary readiness independent of a stalled or unavailable Peer. Start
no production Peer socket until the sibling integration mounts production
identity and worker, persistent recipient permissions and cursor SQL,
quota dispatch and platform admission. Keep each egress lane closed
until its sealed durable send witness is mounted. Server-side
invites, automatic policy changes, accepted-room activation, process
scheduling/retries, startup status expansion, metadata and periodic
heartbeat are separate consumers. This prerequisite adds no new CLI
verbs and changes only qualification issuance visibly. It is not a new
platform-eligibility decision and must not report installed Peer Ready.

Ownership here is `peer_relay_trust`, qualification, narrow `cli`/`identity`
provider access, `transport` and Peer socket/Group wire extraction plus
bounded fixtures. Keep daemon, quota dispatcher, cursor/provisioning SQL,
recipient stage paths and custody lifecycle edits in their designated
increments. The boundary handoff is the leased typed owner, role-bound
receiver route, serialized current-incarnation observations and private
final-send fence. Keep test-only provider/relay authority impossible
under a non-test build, including `installed-checks`.

1. Test schema 1/2 metadata, invalid hand-built record/tag, cross-profile
   and swapped-attestation cases, unchanged owner attestation and mode,
   expired and wrong build/suite, backward/forward clock, manifest/CA
   and inode replacements, pre/post promotion cuts and all report labels.
   The all-zero build fails both issuance and authenticated readback.
   A syntactically valid forged schema 3 tag may display `current_metadata`
   and must fail owner admission. Cover exact managed attestation reuse.
   Cover Keychain SQLCipher-only worker success, locked/denied and timeout
   with the existing test-only helper seam, asserting no Nostr lookup,
   fallback, probe connection or prior evidence replacement on failure.
   Tests include a native CLI private WSS issuance using the normal
   `marmish` binary, stopped lease and controlling TTY, followed by cold
   owner readback with a new ordinary lease. No public relay is needed.
2. Exercise the same typed private TLS and actual NIP-42 socket path on
   loopback. Verify no connect on invalid readback, no AUTH if scope
   changes before AUTH, exact recipient and Group history/live with
   bounded EOSE and private validated route, old incarnation rejection,
   wrong route/kind/ACK/byte limits and cancellation. Change qualification
   or configuration, or advance time, after admission and before the first
   event poll to prove zero wire publications. Exercise ambiguous ACK
   loss and observe one signed write with no second physical send. Use the
   existing fixture modes and no fake production trust or clock entry.
3. Run targeted format, no-test plus `installed-checks` compile,
   fixture-authority compile-fail, native owner and transport tests and
   strict Clippy. Rerun ordinary CLI Command/Terminal and shared relay
   regressions. Installed product lifecycle and supported runner-platform
   qualification remain separate from the local route proof.
