# Checked Peer physical route binding

Base `4ae715a`. This is the next bounded increment of
[production Peer route ownership](peer-production-route.md) after schema 3
issuance. It prepares the exact physical trust binding for a future live
owner. Installed Peer traffic still requires the reviewed durable recipient,
quota/cursor and role admission work. This increment adds no dialer, socket
opener, subscriptions, publish path, daemon or CLI mount, or configuration
surface. Command and Terminal relay selection and the existing disposable
`peer relay qualify` probe keep their current behavior.

## Existing seams

`PeerRelayOwner::bind_with_limits` in `peer_relay_trust.rs` currently makes
the TLS policy for the disposable qualification probe. `ProbeAuthority`
checks that exact profile and endpoint before its probe opens. The crate's
`transport::PeerRelayConnector` exposes only that authorized probe opener.
The `peer_invitation::QualifiedOrigin` constructors use fixture authority
under `cfg(test)`; neither the owner nor the probe can make a production
origin for invitation traffic. Leave those boundaries intact.

`ProfileFiles` already keeps the exact private directory, checks the
held owner-lock inode, Ready manifest inode and parsed manifest, and opens
bounded no-follow files relative to its directory descriptor.
`command_snapshot` returns the parsed configuration and full inode/digest
witness. `bound_peer_relay` selects the single configured private endpoint
and trust and refuses Tor-required routing. `ca_snapshot` verifies the
declared CA pin, root and current CA window, or uses the explicit SPKI
policy without a CA file. `qualification::read_issued` authenticates
the closed schema 3 record, whose exact build, MDK, target, profile, relay
and evidence are already MAC-bound. It checks the managed attestation
against that evidence. Its result is a point-in-time observation. The
present `IssuedQualification` carries the evidence inodes but cannot
prove those same bytes still occupy the paths in a later owner turn.

## Implement the inert binding

- Put the non-Clone, non-Debug checked route under `peer_relay_trust`
  (private child module if helpful). Its constructor borrows the actual
  prepared `IdentityMaterial` and receives the same already-held
  `Arc<ProfileLease>`. Use `ProfileFiles::open`, with no new profile lease
  acquisition, setup, provider call, private state or database creation.
  Check the material's account and binary device slot against the
  checked Ready manifest, and derive the issuer key from
  `material.sqlcipher_root()` using the same private, factored derivation
  as `ProfileLease::peer_qualification_key`. Keep the manifest's existing
  profile/account/device encodings byte-identical to schema 3. Derive the
  key only for the readback, zeroize that buffer on drop and keep neither
  signer nor credential or root in the route. Existing qualification CLI
  still uses its bounded selected-provider read and probe-only signer.
- Refactor `read_issued` around one checked read, for example
  `read_issued_held(profile, key, now)`. It reads the first checked
  config descriptor, derives the configured private endpoint and trust
  directly from its bytes, then reads the CA when present, the
  record and the managed attestation. It verifies schema 3, HMAC and
  pair correspondence using those objects. For each first-inspection
  descriptor return its inode and digest calculated from those exact
  bounded bytes. Preserve the final path and byte comparisons already
  used in the check before returning. Keep the current issuer
  `read_issued(profile, relay, key, now)` as a narrow wrapper that
  requires the same expected relay and discards the held witnesses.
  The route consumes only the held read's endpoint and witnesses,
  with no separate earlier or later config, CA or file observation
  used to establish authority. A non-Peer config edit also closes the
  route. The HMAC already covers probe and attestation evidence.
- Freeze the report window `last_observed_at..expires_at`, the
  attestation window `valid_from..valid_until` and the CA validity
  window when present, using the authenticated first inspection.
  Recheck their original bounds with the effective clock and prove
  the original files remain unchanged. Retain no issuer key and
  perform no re-MAC or record re-decode on recheck.
- Keep the exact returned regular descriptors for config, qualification,
  managed attestation and, in CA mode, the trust anchor. The existing
  lease holds the lock descriptor. `ProfileFiles::open` opens and keeps
  the directory after matching that lock and the manifest. Keep the
  checked manifest descriptor for this binding to prevent its original
  inode from being reused. Reopen fixed paths relative to the profile
  directory for each check, compare device/inode with held files,
  verify mode/owner through the existing opens, and compare bounded
  content to the original digest. Preserve current pathname, full Ready
  manifest and lease checks. A same-byte rewrite in the same inode
  remains a content match. A replacement with even identical bytes in
  a different inode closes the scope.
- Add one mutable `recheck` path for the private checked route. Start and
  finish with cancellation, clock and lease checks. Check the frozen
  config, CA and authenticated pair between them. On any observed failure
  cancel the scope irreversibly, including on the first bad check. A later
  read after restored files or clock cannot revive it. Drop cancels the
  same scope, which is local to this physical binding and independent of
  Command, Terminal and probe scopes. Retain no caller-accessible network
  connector, URL-taking operation, authorization token or reusable bare
  endpoint getter. Exercise the production implementation from tests
  through private visibility. Use a narrowly scoped dormant-code allowance
  until the next consumer exists.
- Anchor sub-second system wall time to monotonic elapsed time before
  the checked read. Compare each raw wall and monotonic sample to its
  previous reading and refuse regression. Refuse a difference greater
  than two seconds in either direction between raw wall and checked
  projected wall. Use the later of observed and projected wall and
  convert to Unix seconds for existing record bounds. Use checked
  conversion/addition and fail closed on overflow or a pre-epoch wall.
  Check the preserved windows after snapshot work and before and after
  each recheck. Record and attestation windows are start-inclusive and
  end-exclusive. Apply existing CA inclusion rules when a CA file is
  present. Suspend, clock step or clock drift can require new admission
  if it violates the threshold. TLS peer certificate and connection
  windows begin when a later approved transport opens a socket.

This detects observed drift and refuses altered current bytes. It does
not promise an atomic transaction across separately named files or detect
an edit restored before any check. The same-UID and valid historical
cross-restart rollback limits of the [issuer](peer-production-route.md)
continue to apply. A later send fence must recheck at the use boundary and
use the socket's own clock/deadline and cancellation. Successful binding
here proves neither role eligibility nor native peer/group authority.

## Handoff boundary

Leave the checked route and its constructors private until a later
reviewed consumer. The sibling quota design's planned
`ReceiverCandidate::evaluate(&Arc<ProfileLease>)` and `recheck` provide
read-only role/receiver/platform eligibility and normalized relay matching.
They supply no quota or live owner and do not yet return this route's full
config witness. The future integration must match the same
`Arc<ProfileLease>` instance, the exact config content/inode observed by
the role decision, and the account/device binding before consuming either
reservation or route. Define that common witness in the integration review,
without adding a second role check or reservation here. A newer config
parse with the same values, arbitrary role booleans, a URL, probe result
or copied report cannot replace those witnesses. The live composition
should use this physical precursor in the planned
`PeerRelayOwner::bind_live(..., ReceiverAdmission)`, with opaque binding
rather than the generic `RelayConnector` in `QualifiedOrigin`. Keep
retained descriptors, time and cancellation with the resulting scope and
recheck before any authority is consumed. This document neither defines
the sibling reservation/eligibility token nor changes its persisted schema.
No `QualifiedOrigin` or live TLS constructor is in this increment.

## Implementation ownership and validation

Own `peer_relay_trust.rs`, its optional child module and focused tests,
the internal witness reconstruction in `peer_relay_qualification.rs`,
and the shared narrow identity derivation/manifest material comparison
in `identity.rs`. Reuse existing bounded test helpers and a real
production issuer-created pair from the private WSS qualification fixture
for the positive cold bind. Do not add an authority-producing helper
available to ordinary or `installed-checks` builds. No `transport.rs`,
daemon, socket, cursor, quota, journal or custody changes.

1. After production qualification with disposable profiles, close the
   qualifier, acquire the checked lease with prepared material and bind
   from disk. A fresh scope rechecks successfully with unchanged files
   and has no effect on relay counters or persistent profile bytes.
   Preserve the actual CLI/PTY probe regression. Unissued, schema 1/2,
   forged or expired pairs, wrong profile root/account/device, untrusted
   clocks, changed private configuration/trust and missing/foreign files
   refuse binding.
2. Table-drive mutation of qualification, attestation, config, CA,
   manifest, owner lock and directory. Include in-place content change,
   identical-byte replacement, and symlink/wrong-mode refusal using
   disposable profiles. Verify sticky invalidation after restoration.
   For file replacement cuts, assert a retained original descriptor
   and compare its inode to the replacement. Test that a pre-cancelled
   route refuses recheck and that drop cancels a passive test witness.
3. Deterministically cover expiry between initial and final checks,
   small acceptable skew, positive and negative excessive divergence,
   raw rollback, stopped wall with advancing monotonic time, and
   conversions/overflow. Tests may inject samples privately. Product
   entry points have no custom clock. Test initial/cold root mismatch
   independently of public account mismatches where practical.
4. Run focused issuer/route/profile tests, ordinary binaries, strict
   all-targets with `installed-checks`, and no public-network tests.
   Verify production source has no new caller, opener or fixture authority
   and no installed `QualifiedOrigin` constructor. Use focused Command
   and Terminal regression tests. Record platform and validation limits.
