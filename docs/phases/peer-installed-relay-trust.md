# Installed Peer relay TLS and local qualification evidence

This increment starts at `b36e6fdce47eee2f4ccdac60637f97fa8d379082`. It follows
the [installed Peer design](peer-installed.md#private-relay-qualification)
and the [Peer configuration parser](peer-installed-config.md). Implement
the private trust and qualification-record boundaries before the foreground
relay probe. Ordinary Command and Terminal retain their current transport and
readiness. Peer remains dormant in this increment. No external relay, release
platform allow-list, network qualification, daemon owner, status command, or
Peer enable command is part of this increment.

## Exact production configuration input

Use the parsed typed `peer_intent::Relay` and `Trust` in
`command_config.rs`. Expose only a private accessor that accepts one canonical
`wss` origin and trust pair from that parser. Use the same canonical scheme,
host, port and root slash for the connector, TLS SNI or IP verification, raw
socket scope and qualification. Match the complete origin before DNS or TCP,
including a nondefault port. Reject proxy, alternate host or port, and
noncanonical wire aliases. `tor=always` remains `routing_unsupported` for this
direct path, including a syntactically valid saved Peer pair. Ordinary
`tor=auto` and `off` leave the ordinary route unchanged. Neither an ordinary
relay, SRV result, environment variable, HTTP redirect nor invitation supplies
a Peer destination, trust anchor, or fallback. A Peer absent/Off profile
keeps current startup behavior and opens no Peer files, TLS policy or socket.
Future explicit show/probe commands can inspect a configured pair while both
directions are Off. Those commands require their own reviewed paths.

Create a private `BoundPeerTls` only from those parsed settings and the exact
leased profile. It contains the canonical origin and a constructed validation
policy. This value proves local TLS settings only. It carries no relay
qualification, platform eligibility, installed egress or group authority.
No production entry point accepts an arbitrary root store, permissive
verifier, test certificate/clock injection or an already opened socket.
Keep the fixture root constructors and `QualifiedOrigin::for_*_fixture`
under `cfg(test)`, including with `installed-checks`.

## Trust models

| Config variant | Exact interpretation |
| --- | --- |
| `spki-sha256` | Verify the leaf with the existing bundled WebPKI public roots, current validity, requested DNS name or IP SAN, and normal handshake-signature validation. Acquire the live leaf from the completed secure stream and compare SHA-256 of its complete DER SubjectPublicKeyInfo (the signed SEQUENCE including algorithm and key). Digest scope is the presented end-entity certificate, with a nonempty bounded chain and one bounded, fully parsed leaf. Compare the digest before WebSocket upgrade or other application data, even on each reconnect. Public roots alone give no access to a Peer origin. |
| `ca-sha256` | Read one bounded DER private root from the fixed `peer-relay-ca.der` file under this profile. Check SHA-256 of exact DER file bytes before loading the certificate and require a fully parsed X.509 certificate, `CA=true`, `keyCertSign` if KeyUsage is present, self-issued DN and a valid self-signature. With this pinned sole trust anchor, Rustls enforces its normal chain, hostname/IP, validity of end entities/intermediates and handshake signatures. Check the anchor validity window separately during policy construction and each TLS connection because WebPKI trust anchors do not get normal leaf validity checks. A public WebPKI chain cannot substitute for the private root. |

Use one complete DER object, no PEM, bundle or trailing bytes. Bound the
configured root and inspected leaf to 16 KiB each. Before use, require
1–8 certificates and at most 128 KiB total encoded bytes in the presented
chain. This is a post-handshake consistency check. The locked Rustls
64 KiB handshake-message cap is the tighter wire resource limit. Use the already locked
`x509-parser` 0.18.1 as an explicit dependency with only the required
signature verification support. Use its raw DER SPKI span as parsed
from the original leaf. Reuse `sha2`. Do not add a handwritten ASN.1 parser
or Rustls custom certificate verifier. The CA mode validation of the root's
self-signature is independent of the configured digest match and only
accepts the intersection of the locked WebPKI and parser verification
algorithms with SHA-256 or stronger and modern key parameters. Allow
ECDSA P-256/SHA-256, P-384/SHA-384, Ed25519, and RSA PKCS#1
SHA-256, SHA-384 or SHA-512 with RSA keys of at least 2048 bits,
subject to the locked verifier. Refuse SHA-1, legacy RSA keys, and
unrecognized algorithms or signature parameters.
Require a fresh Peer TLS handshake every connect. Disable resumption and
early data on the Peer config, set no client authentication, and apply all
checks before handing the stream to the WebSocket upgrade. A failed pin,
CA file, certificate, SAN, wrong chain or unsupported certificate algorithm
closes that attempt. Ordinary TLS defaults, resumption and logs are unchanged.

For the certificate file and qualification file, open a descriptor for the
already leased profile root with `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`, verify
same owner, exact private 0700 mode and the directory identity used by the
lease before accessing its fixed names through that descriptor. Reuse the
descriptor patterns in `peer_stage_retirement_files.rs` and the file checks
in `identity::open_private`. The current lease retains `owner.lock` rather
than the root descriptor. Add a narrow Peer helper that opens and retains
the root on first authorized use. Verify the fixed `owner.lock` entry is
the exact held private lock by device and inode. At binding, reread the
ready manifest and parsed Peer settings via the root descriptor under
existing read caps. Compare the stable profile ID and the exact canonical
Peer projection (both role intents, routing mode and origin/trust pair)
with production equality on that projection. Before each connection or
replacement, recheck the live directory and lock identities. Recheck the
CA file at each connection. The later owner must invalidate its bound
policy before replacing the config or lease.

For a later owner, put the root descriptor, the live `Arc<ProfileLease>`
and a cancellation guard in one private owner handle. Connector clones
keep only a weak reference to that handle. A connection upgrades it for
the bounded lifetime of the attempt and checks cancellation before dial
and before upgrading TLS to WebSocket. The owner cancels and drains any
bounded attempt and closes its sockets before dropping the handle.
Matching a file's identity establishes which lock it is; the owner
handle and connection guard establish that it is still held.
Add no file I/O at ordinary profile startup. The generic path-only helper
is insufficient for the record writer. Use `openat` with
`O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK`; inspect the
open file, requiring a regular single-link file, effective UID and exact
0600 mode including no special bits. Limit its actual read to the cap plus
one. Re-open the CA via this helper and revalidate the configured digest at
each future connection; the selected in-memory DER is the one installed in
the attempt's TLS configuration. An owner may provision those exact DER
bytes by a private atomic file replacement. A changed pin or mode requires
new configuration and requalification. The toolkit does not discover,
copy or enroll private roots automatically.

## Transport integration

Add a private opaque `PeerRelayConnector` over the existing
`RelayConnector`, which only the bound trust constructor can create. Its
internal connector uses one URL, `RouteMode::Off` and the existing
leased `ArtiDirs` arguments without starting Arti, even when the ordinary
path uses `tor=auto`. Keep the inner ordinary connector private.
Reuse its singleton policy construction, readiness and one-socket permit,
`open_socket -> connection_phases -> connect_stream -> finish_socket`,
existing direct total timeout/cancellation/frame bounds and existing
`websocket` upgrade. Match the bound canonical URL and direct route before
acquiring a connection. Keep the existing bundled-root path through
`tls_stream_using_roots`; factor private client config/finished-stream helpers
only as needed for the Peer-specific checks. The TCP/TLS/WS connect remains one
bounded direct attempt. `client_async_with_config` has no redirect dialing.
Return fixed `peer_tls_pin`, `peer_tls_ca` and `peer_tls_certificate`
categories through the existing connector reason projection without
echoing host, certificate subject, pin, path, or credential. The raw
`Owner::open` already requires a singleton exact connector. In a later
reviewed increment its production `QualifiedOrigin` constructor must
require the opaque `PeerRelayConnector`, so a plain Command connector
cannot become Peer authority. In this
increment a production-style unit harness exercises the exact connector over
a bounded loopback WSS listener, using normal private CA file loading.
For SPKI, deterministic tests use the same private verifier logic with
locally generated chain anchors that replace only the WebPKI root store
inside the Peer-specific policy. These tests must traverse the exact pin
check and cannot use the ordinary `fixture_roots` branch. There is no
production root-injection API.
Check generated candidate, typed local trust, parsed CA, and actual
successful/failed TLS before the application bytes boundary.

## Owner-local qualification record

Reserve fixed filename `peer-relay-qualification.json` directly below the
leased profile, 8 KiB maximum. A closed Serde schema denies duplicate,
missing, extra, null, untyped and unknown-version fields at each level.
Typed internal data, serialized by a single writer, compare canonical
projections. Exact expected fields for schema v1 are:

| Field | Validation |
| --- | --- |
| `schema` | Integer `1`; unknown values fail closed. |
| `profile_id` | Exact canonical stable ID already bound to the leased profile by `ProfileLease::profile_id`; enforce its current syntax. A copy across profiles fails. |
| `origin`, `trust` | Exact canonical string origin and exact canonical typed `spki-sha256` or `ca-sha256` configured digest; any scheme/host/port or trust policy/pin change is stale. |
| `probe_suite`, `bounds_version`, `relay_contract` | Closed versioned identifiers from this artifact's supported suite, limits and exact relay security contract. A capability mismatch or unresolved version leaves the record unusable. |
| `mdk_revision`, `target` | The revision is 40 lowercase hex digits matching the locked upstream MDK commit. The target is the exact embedded Rust target triple, with an ASCII length cap of 128 and only letters, digits, underscores or hyphens. The triple binds OS, architecture and ABI. Build provenance comes from `Cargo.lock` and the existing build-time target. Read the workspace lock at build time with `rerun-if-changed`, require the same upstream commit for all consumed MDK crates, and embed it from the reviewed lock; no runtime file, environment override or Git checkout discovery. Missing or ambiguous provenance cannot qualify. The independent reviewed platform-enable gate still applies. |
| `probe_evidence`, `operator_evidence` | Nonzero lowercase SHA-256 digests of two separate bounded evidence documents, one exact completed probe report and one exact current operator assessment of controls and retention outside the probe window. Digests give local provenance, not independent remote attestation. The later probe must supply their verified typed completion, with explicit validity deadline and scope, before writes are reachable. A caller-supplied digest pair alone never writes this record. |
| `qualified_at`, `last_observed_at`, `expires_at` | UTC Unix seconds with range 1–253,402,300,799 inclusive. Require `qualified_at <= last_observed_at` and `qualified_at < expires_at`, with checked duration up to 2,592,000 seconds and capped by typed external evidence's deadline at issuance. Future owner writes advance the observation floor only. A persisted floor at or beyond expiry is terminal. |

Strict comparisons include canonical re-encoding of parsed identities,
origin and fingerprints, with integers in bounds before comparison. A valid
local record missing the current exact scope is `stale`; missing or unsafe,
corrupt, unsupported schema, or absent issuer evidence is `unqualified`.
For a matching record with a persisted terminal floor, return expired even
after clock rollback. Otherwise invalid or uncertain time and a sample
below the floor return clock-untrusted. A sample at or beyond expiry
returns expired and the future owner must persist that sample as the new
terminal floor before displaying durably observed expiry. At any uncertain
write deny work and keep the running process fenced pending a new checked
turn. The eventual reader can present local metadata separately from
availability. Logs use reason codes. In a future owner, sample a trusted
wall under the lease and revalidate at startup, immediately before the
first authoritative operation and later distinct wall-second turns, and
after awaits before socket use. Durably advance the floor if time
advanced before admitting work. A same-turn monotonic guard prevents
using an old lease, socket or cached floor after reconfiguration, expiry
or rollback. A new qualification writer cannot lower a previous floor or
revive an expired record without a fresh completed probe.
The local record assumes
trusted owner filesystem state and wall clock; copying or rolling back the
entire owner-controlled store can evade a persisted floor and is outside
the model. This increment emits no installed network effects and needs no
clock writer hooked into the daemon; deterministic tests inject wall samples
only into private pure validation.

A later complete probe will hold the same profile lease throughout
validate-and-replace. The record writer must use one fixed dedicated
`peer-relay-qualification.new` staging name and
`O_CREAT|O_EXCL|O_NOFOLLOW` under the held descriptor, fully write and
sync it, then atomically `renameat` to the fixed record name and sync the
parent. Recheck bound scope and existing approved clock floor in the same
serialized turn. Refuse malformed environment/paths, unexpected existing
file type, owner/mode, links and failed I/O. Clean up only the current
writer's staging file on failure before rename. A later leased turn may
unlink a crash leftover at that exact reserved name only after verifying
the same normal private single-link file metadata and rechecking inode
identity. Its payload is never read as a record. An unsafe leftover
requires owner intervention. A failure after rename is uncertain and cannot grant
authority in that turn. This increment can implement the private bounded
codec, readback validator and atomic primitive with test-only typed examples.
No CLI, fixture or production constructor writes or accepts qualification as
authority until the reviewer approves the real typed producer and consumer.
An old file is never automatically repaired or renewed. No keys, private
profile payload, event bytes, free-form operator text, or credentials belong
in either persisted format.

## Tests and phase exit

- Use deterministic disposable public-key and certificate fixtures around the
  public-API Rustls handshake. Include private root success and trusted-chain
  SPKI success with matching leaf; wrong digest, anchor, SAN, chain, time,
  self-signature/CA constraints, arbitrary trailing DER and excessive size
  refuse. Same public leaf key reissued under a valid chain can match SPKI,
  a rotated leaf key cannot. A known CA root with a digest for different DER
  fails. Tests confirm no WebSocket upgrade or Nostr write after TLS/pin
  refusal, no proxy/mismatched exact endpoint dial, and a new TLS check on
  reconnect. Use generated material and only local loopback, never public
  relays, real profiles or root certificates written to the system trust
  store.
- Record fixtures cover canonical round trip, strict schema, size edge,
  different profile/origin/trust/platform/revision/versions, unknown schema,
  expired equality and rollback including an older observation within the
  original issue window. Exercise fixed-file missing/unsafe cases, regular
  file/symlink/hardlink/owner and mode, descriptor root mismatch, bounded
  reads, writer failure before rename/after rename, and exact freshness.
  Assert an unapproved fixture/local fabricated completion cannot mint any
  production authority. Type or compile checks retain `cfg(test)` fixture
  scoping even in installed-checks builds.
- Run targeted TLS/record tests and existing parser, ordinary transport and
  raw Peer proof regressions, formatting, diff check, strict native
  all-target Clippy, and release-boundary/source inspection. Use standalone
  loopback timing only if it helps distinguish successful known-root/pin
  handshakes from negative checks. Record exact artifact/test, local result
  and scope; there is no relay probe measurement in this increment. Keep
  the core CI selector intentionally small and avoid external capacity.

Future plan order is explicit operator relay contract and evidence schema,
the bounded foreground probe with its exact completion token, production
qualification readback and clock fence, then an independently reviewed
installed owner and profile activation. No test-only evidence recorded
here can satisfy any of those gates.
