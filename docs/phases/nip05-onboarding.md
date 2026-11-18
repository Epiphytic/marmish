# Verified identifier onboarding and profile publication

This increment of [the roadmap](interactive-peers-roadmap.md) precedes Peer
room admission. Only the existing explicit setup path can add execution grants.
The identity owning daemon remains offline with respect to directory lookup and
continues to use canonical configured keys at startup.

## Operator interface

- `marmish setup --grant-identifier name@domain` resolves one candidate in
  foreground setup using the selected profile's effective relays and Tor
  policy. Show the source and complete npub over the controlling terminal.
  Require the operator to compare it with the intended peer independently and
  type that complete npub, or supply it up front as `--confirm-npub`.
  Noninteractive invocation requires the latter. A mismatch, lack of terminal,
  conflicting config, or directory failure leaves configuration intact.
  An existing config requires `--update` before lookup. Role checks still
  apply. No unattended replacement.
  Recommended refresh is repeating the check with the originally confirmed
  npub as expectation. An intentional rotation uses explicit revoke and grant.
- `marmish profile publish --nip05 name@domain` attaches the identifier to the
  selected local identity's public Nostr metadata. Optional `--email` publishes
  a public contact separately and has no discovery or grant effect. If supplied,
  it takes one address with a valid ASCII dot-atom local part and DNS domain,
  with at most 254 bytes overall and at most 64 for the local part. This
  increment requires `--nip05` and does not remove fields. The preview shows
  the exact changes and describes public relay visibility. Foreground users
  type an exact publication confirmation after seeing the preview. Headless
  users opt in with `--confirm-public`. Only then open signing credentials.

## Directory and relay boundary

Follow [NIP-05](https://github.com/nostr-protocol/nips/blob/master/05.md).
Accept the NIP-05 lowercase local-part alphabet with at most 64 bytes and a
canonical ASCII DNS hostname with valid labels. Exclude IP literals, URL
syntax, credentials, ports, and single-label local names. Submission is
operator-only. Fetch the fixed well-known HTTPS path with the encoded name.
Factor out the existing Rustls public-root and Arti stream seams without
changing direct relay resolution. The HTTPS path has a dedicated direct TCP
resolver and does not consume a relay slot. Initially reject onion directory
names under the public-root trust policy. Existing onion relays still work.
Use bounded Hyper HTTP/1 over that stream, HTTP 200 only, no proxies,
redirects, credentials, or reuse. Use a 15 second total lookup deadline after
route readiness, a bounded readiness window under the existing Tor policy,
and at most 64 KiB each of HTTP headers and body. Cancel the connection when
the request completes or times out. Fail closed under forced Tor. Before a
profile is initialized, prepare private disposable Arti state and cache under
an owned temporary directory, with checked bootstrap and an isolated client
handle for directory circuits. Drop the connector and its tasks before
cleanup. No local DNS for Tor hostnames and no clearnet retry. Direct DNS resolves with a bounded time, then connects only to the
selected validated public IPs so rebinding cannot change the destination.
Reject mapped or special/private addresses, including loopback, link-local
and unspecified. Transport fault categories omit URL, identifiers and payload.
The deliberately shorter HTTP deadline has its own finite error category.

Use Hyper `=1.11.1` (`client,http1`), Hyper-util `=0.1.20` (`tokio`) and
HTTP-body-util `=0.1.5`, with default features disabled. These sources are
cached locally. Review the lockfile, licenses and new transitive dependencies
and validate the locked build offline after prefetch. CI retains its standard
Cargo fetch. TLS fixtures issue a disposable, test-only pinned CA and certificate
with dev dependency rcgen `=0.14.8` (only `ring`).

Read the single requested entry in `names` as a valid lowercase hex Nostr
public key; reject duplicate keys at security-relevant object levels. Do not
use relay hints. Query exactly that author and kind 0 on configured relays.
Require bounded successful EOSE on at least one relay, and validate full
signature, event kind and author. Select latest by NIP-01 ordering and reject
conflicting identifiers at the newest equal timestamp. Require the selected
event's NIP-05 field to match. Cap relay count by existing policy, events per
relay at 16, metadata content at 16 KiB and the query deadline at 15 seconds
after route readiness. Complete EOSE for every relay used as evidence; surface
partial coverage or failures without treating failures as evidence. Pre-credential
relay reads use a disposable signer for NIP-42. A relay that requires a
specific identity can refuse this query.

## Publication and persistence

Acquire an existing ready profile lease and read its manifest public key.
An active daemon holds the lease, so publication requires stopping it. Bootstrap
only the configured relay connections, without creating rooms, publishing
KeyPackages or starting execution. Verify the NIP-05 HTTPS mapping points
to the local public key. Read kind-0 metadata with the disposable reader,
compose the preview, collect consent, and verify the mapping again before
opening the real signer.
Preserve unknown fields and existing tags. If coverage
is incomplete, an event is malformed, or distinct latest records tie at the
same timestamp, fail without publishing. A fresh profile is established only
when every configured relay returns successful complete query with no event.
An unavailable configured relay prevents publication.
Do not auto-retry against concurrent external metadata writers. Recheck the
selected base immediately before sending, and explain that Nostr has no atomic
compare-and-swap. Use existing SDK targeted sends and report per-relay ACK
coverage; a partial ACK may already have published publicly. Use bounded
attempts and do not mint future timestamps to force replacement.

Resolved keys use the existing setup config lock and atomic commit. Resolve
against proposed effective options before preparing credentials, then confirm
before constructing the final grant set. Record only canonical approved npub
semantics in existing authorization configuration. No directory document,
public profile bodies, domain-to-key cache, or email is written to the private
application journal. No new database schema.

## Validation and review

Provide test-only native HTTPS/relay fixtures with disposable keys, a pinned
fixture trust root and loopback injection absent from release builds. Cover
happy path and old direct-npub setup, mismatch and headless behavior, exact
key verification, duplicate fields, malformed/bad-signature/incorrect kind and
wrong author metadata, stale and equal-timestamp conflicts, missing EOSE,
redirect and size bounds, non-public DNS destinations, rejected onion directory
names, and no direct fallback for forced routing on a fresh profile. Cover
successful signed publication with field preservation, local-key mapping
mismatch, preview/consent gate, malformed or incomplete profile reads,
and partial relay ACK. Exercise real CLI and verify authorization uses the pinned
key rather than later domain responses.

Run the local short parser and authority tests with
the `profile_directory::core_tests` and `profile_metadata::core_tests` filters
and the exact `identifier_proposal_cannot_grant_until_the_full_key_is_confirmed`
test for the self-hosted `interactive-core` workflow. Reuse the single
Linux AMD64 runner for these portable tests, then qualify other native platforms
separately. CI runs only for trusted master events.
Keep native TLS/relay integration in the documented full suite and benchmarks opt-in.
Record warm verification and fixture lookup p50/p95/p99 over at least 100
successful samples with failures reported separately. Relay fixture timings
do not imply public-network or Tor performance.
