# Offline Peer invitation control codec

This increment implements the offline envelope from [Peer management](peer-live.md)
and [staged inspection](peer-staged-inspection.md). It exposes an unpublished
Rust module through an integration test entry. It has no relay, durable state,
daemon or room adapter entry point. Origin and local authority remain mandatory
checks at future entry points. A decoded Offer reserves nothing. A decoded
Consent does not allocate a group. A decoded Delivery verifies an opaque nested
Welcome before staged native inspection, which alone can validate its MLS
binding and approve the new room. Candidate wire kinds and size limits require
the parent plan's separate transport and largest-native-Welcome qualification.

## Interfaces and binding

Use distinct typed `Offer`, `Consent` and `Delivery` decode results with private
construction and getters for authenticated content. `Codec::new(outer_cap)`
accepts a positive serialized outer cap of at most 65,536 bytes.
`prepare_offer(signer, client, room_id, routing, now)` creates a random
128-bit offer ID and signs the supplied 128-bit logical room ID, the
independent 32-byte native Nostr transport group ID and full configured route.
`prepare_consent(signer, &Offer, exact_package, now, expires_at)` and
`prepare_delivery(signer, &Consent, exact_welcome, now, expires_at)` derive the
signed links from those verified values. Each returns the immutable serialized
outer bytes and both event IDs, plus the typed verified flight so the issuer
can retain the reviewed context. The `open_offer`, `open_consent` and
`open_delivery` methods require explicit local signing keys, the frame and
local time. Offer also takes an optional expected server and the exact
configured route set. Consent takes the accepted local Offer and Delivery
takes the accepted local Consent. A new Consent or Delivery requires the
correct sender and recipient relationship relative to that chain.
Signed revisions are the computed rumor event IDs. Delivery exposes its
author, recipient, exact Offer and Consent revisions, package event ID
and reference, and exact Welcome bytes, event ID and digest to the stage.
Persisted context,
outbox, clock steps and replay fences belong to the future store phase.
Expose inner creation, expiry and expiry plus 900 seconds to its consumer.

Standard Nostr JSON event shapes have bounded content at each layer. The
rumor object has exactly `id`, `pubkey`, `created_at`, `kind`, `tags`, and
`content`. Parse it through a strict raw projection, require its ID, and
recompute using `UnsignedEvent::compute_id`.
Its author is the verified seal signer. Require one exact ordered
`["p", canonical_recipient_hex]` tag. Each closed bounded body has integer
`v = 1` and unsigned integer `expires_at`, with these additional fields:

| Rumor kind | Additional JSON fields |
| --- | --- |
| Offer 24865 | `offer_id` (16 bytes), `room_id` (16 bytes), `transport_group_id` (32 bytes), `relays` (one to sixteen strictly sorted, distinct URL strings of at most 512 bytes each) |
| Consent 24866 | `offer_id`, `offer_revision`, `package_event_id`, `package_ref`, `package_slot` (16-byte native device slot), `package` (exact native KeyPackage event JSON as padded standard base64) |
| Delivery 24867 | `offer_id`, `offer_revision`, `consent_revision`, `package_event_id`, `package_ref`, `package_slot`, `welcome_event_id`, `welcome_sha256`, `welcome` (exact native Welcome event JSON as padded standard base64) |

Byte identifiers are fixed lowercase hex and required, with 32 bytes unless
specified. The role and recipient are derived from the verified rumor kind,
author and recipient tag. The inherited room and relays are pinned by the
exact Offer revision. Reject missing, duplicate and unknown JSON body fields,
noncanonical encodings, aliases and self-invitations. Validate routes using
the pinned `NostrRoutingV1::new` and require its exact sorted unique set as
input. Require parsed URL strings to round trip verbatim, and reject query
strings as the application profile does. Bound and check configured and
advertised values before comparing.
The codec does not infer the actual transport origin from a claimed route.

## Authored frames and bounded opening

Use pinned Nostr `EventBuilder`, `FinalizeEvent` and NIP-44 v2 two times:
canonical author signs empty-tag kind 13, one-use independent random key signs
kind 7435. Its only ordered tags are the exact lowercase recipient and canonical
decimal expiry. Draw each of the two independent layer offsets uniformly
from `0..172_800` using unbiased OS randomness, with a separate independent
draw for the one or two six-hour retention buckets. Saturate offsets at
zero for clocks earlier than the window. Calculate retention with checked
arithmetic as the ceiling of inner `expires_at + 300` to a six-hour boundary
plus one or two six-hour buckets. Outer admission checks the shape, decimal
and six-hour alignment and never consults outer creation or expiry for
application freshness. Sender authoring enforces the bucket formula.
Keep the complete signed event bytes for future identical retries.

Before each parse or decrypt, enforce the relevant raw or encoded bound.
The fixed additional provisional caps are 63 KiB encoded outer content,
49 KiB decrypted seal, 48 KiB encoded seal content, 30 KiB decrypted rumor,
28 KiB rumor content and 20 KiB per decoded artifact. Base64 decoding first
checks padded length and uses strict padded standard representation, then
checks exact decoded length. The final serialized cap applies on receive and
send even when nested inputs meet their own bounds. Using a tighter outer
cap must not permit building a frame larger than that configured cap.
Reject malformed ID/signature, distinct wrong kind/recipient, nonempty seal
tags, wrong author and trailing JSON. A strict standard event projection
rejects duplicate and unknown fields on Peer outer, seal, KeyPackage
and Welcome. The projection for the unsigned rumor has the same checks
without a signature. Require canonical lowercase fixed-width hex for IDs,
public keys and signatures, and JSON unsigned integers for times and kinds.
Limit each event to eight tags, at most 128 values per tag and 512 bytes per
value within its raw layer cap. Use pinned `Event::new` and `Event::verify`.
Use an isolated internal pinned
`open_signed_seal` helper for the two inner operations, with layer limits
and optional known seal author. The helper keeps outer kind and recipient
policy separate and can later be unified with Terminal without changing
Terminal's wire acceptance or optional rumor-ID policy. This offline
increment temporarily implements the common checks at the local seam
because Terminal enrollment owns its current source. Production integration
retains the parent plan's extraction and regression gate before activation.
Zeroize transient decrypted byte spools. Errors are stage categories without
raw values.

## Authenticated time and nested artifacts

Admission checks each authenticated current flight with
`created_at <= now + 300` and `now < expires_at + 300` using checked
arithmetic. Each signed body has `created_at < expires_at`; Offer expiry
equals creation plus 86,400 exactly. Causal intervals do not grant skew.
Consent creation is within the exact Offer's half-open window; Delivery
creation is within the exact Consent's half-open window. Consent expiry
is at most its creation plus 86,400 and the signed KeyPackage's
`not_after`. Delivery expiry is at most its creation plus 86,400,
exact Consent expiry and package `not_after`. Delivery can be accepted
after the earlier Offer expires. Exact prior stage evidence is mandatory
but its historical Offer freshness is not rechecked. On authoring use
`max(now, parent.created_at)` for either descendant and reject beyond
`now + 300` or outside the exact parent window.

Package interval is inclusive at `not_before` and exclusive at `not_after`.
Independently verify it against authenticated Consent creation. At Consent
preparation or receipt, also require `not_before <= now < not_after` for
the supplied clock and pass the pinned host-clock-dependent native check.
No skew applies to package lifetime. Native lifetime failure has its own
category covering elapsed host time, missing lifetime or unacceptable native
duration. Delivery uses the already validated Consent fields without
re-running package validation. Require the package interval at explicit
Delivery receipt and creation even when general flight skew would allow
an expired package. Future outbox retries reuse retained immutable ciphertext
and typed evidence under separate replay, authority and expiry policy.
Persisted verified package metadata must be authenticated by its owner
before reconstructing typed evidence. No persistence operation is in scope.

For the native KeyPackage, verify exact event kind 30443, computed event ID
and signature by the canonical Consent account. Require the JSON bytes to
equal compact SDK serialization of the parsed event, and its signed native
publication time to be at or before signed Consent creation. Require the
exact seven tags from the Marmish native publisher in matching order: `d`
(a 16-byte lowercase hex Marmish slot), `mls_protocol_version` (`1.0`),
`i` (MLS reference), `mls_ciphersuite` (lowercase four-digit hex with `0x`),
`mls_extensions`, `mls_proposals`, and `app_components`. Reuse pinned
`NostrKeyPackagePublication::to_event_at` to construct the expected shape,
matching the extensions, proposals and private app components from metadata
as in `mdk_adapter.rs`'s local publisher. The future native stage still
checks actual target room capability requirements.
Decode its signed content into `cgka_traits::engine::KeyPackage` with
`ProtocolProfile::Current`. The pinned
`cgka_engine::key_package_metadata` independently validates the MLS
signature, structure, account identity proof and profile. Compare metadata
credential identity, suite, MLS reference, and signed lifetime. The exact
artifact emitted by pinned `AccountDeviceRuntime::prepare_fresh_key_package`
is last-resort. Require that native marker in this shape, and admit that
artifact only in the sealed Consent bound to a single Offer. This does not
make the underlying OpenMLS bundle single-use. A future stage must retain
the established isolated store, exact single-offer binding, consumed
lifecycle evidence, replay fence and targeted withdrawal. The distinct
single-use sibling device package API does not implement this lifecycle.
Unit-test the interval helper at fixed times and use a
freshly generated real native KeyPackage for deterministic offsets relative
to host time. No bypass or alternate metadata factory appears in the module
API. Verify all signed links independently on receiving the Consent.

For the native Welcome, verify the digest of the exact received bytes with
the strict signed event projection, signature and computed event ID,
standard GiftWrap kind 1059 and exactly one tag `["p", consent_author_hex]`.
Preserve exact bytes for future pinned native peeling and
staged authority validation. The offline Delivery cannot establish native
seal signer, actual KeyPackage consumption or MLS room membership.

## Validation

Prove strict stage linkage, keys, privacy geometry, independent issued
clock offsets, size checks at each boundary, newly signed tampering cases,
Consent time bound by the real native signed MLS lifetime, and valid
Delivery before and after the Offer expired. Use deterministic private
timestamp and bucket inputs through module-private test helpers without
introducing predictable values into authoring. Use independently generated keys.
Construct signed synthetic opaques for Welcome size geometry, and retain
the real pinned artifact acceptance gate in the parent plan for activation.
Exercise the NIP-44 padding cliff at 28,672 and 28,673 raw rumor bytes and
record the effective artifact maximum for each fixture.
Include an opt-in repeatable release benchmark for prepare and verification
and serialized bytes for all three flights. Keep small deterministic core
tests separate from the benchmark. Submit final code and reproducible checks
to Claude Code in automatic permission mode before integration.
