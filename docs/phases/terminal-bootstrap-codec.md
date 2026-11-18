# Private Terminal capability exchange

This increment implements and qualifies the authenticated bootstrap record and
its volatile admission state. It follows the [ephemeral route design](terminal-ephemeral-route.md).
The [in-memory proof](../validation/terminal-memory-proof.md) already admitted
unpublished pending packages in both founder orientations. A separate
[offline relay proof](../validation/terminal-offline-relay-proof.md) established the
native subscription behavior on loopback. Neither established the bootstrap
that gives those random Terminal identities a canonical remote principal.

Implement private native types and functions for the later Terminal owner.
Use an offline signed-message test and one opt-in private WSS extension after
the deterministic tests pass. This scope includes no CLI grant, profile config,
daemon registration, Command wire change, writable application store, new
KeyPackage publication or PTY spawn. Production routing, recovery and resource
qualification follow separately.

## Inputs and binding

Accept one already verified Command binding, with version, 16-byte Command
session ID, bounded native group and transport group IDs, and role-ordered
32-byte canonical client and server keys. Validate distinct valid account keys,
positive bounded ID lengths and local signer agreement with the supplied
role. The test constructs this input from a real pinned Command group and
checks that the two owners derive identical normalized material. The local
authority and configuration checks belong to the supplying owner, and only
a successfully revalidated existing Command binding may supply this value.
The normalized shared tuple omits local UID, profile name and the
perspective-dependent authority digest. Native authority stays with the
supplying owner and must be revalidated before handling each record.

Hash a dedicated version label followed by those versioned fields with
unambiguous role and length encoding. Derive separate receiver aliases for the
two roles with a NIP-44 v2 conversation key between the two canonical keys
and HKDF-SHA256 under the dedicated versioned bootstrap label. The HKDF info
binds the normalized digest, receiver role and a one-byte counter. Limit the
counter to four scalars and reject an alias equal to either canonical key,
the opposite alias or another active local alias. Use `IdentityMaterial::signer()` and borrow the pinned
`Keys::secret_key()` inside alias derivation. Keep NIP-44 v2
`ConversationKey` and the HKDF expansion in that scope, put each trial
scalar in `Zeroizing<[u8; 32]>`, and return the accepted alias secret as a
private `SecretKey`. The pinned conversation key is Copy without a wipe
guarantee, and the pinned SecretKey drop uses `non_secure_erase`. This
design claims volatile application ownership and no plaintext output, not
a proof about compiler copies or physical memory erasure. Do not derive
Debug or serialization for the alias secret holder.
This is shared alias addressing. The authenticated sender remains the pinned
canonical signer. Both parties can derive the two public aliases. A third
party's valid NIP-59 gift to one of those aliases grants no authority.

## Closed wire contract

Wrap a version-1 application rumor under the normal pinned NIP-59 builder.
Use Marmish bootstrap rumor kind `24861`, distinct from the Peer delivery
fixture kind `24860` and the existing Command and Stream application kinds. Scope the 32 KiB outer limit to these bootstrap wraps. The
native Terminal route's 64 KiB Welcome precheck remains unchanged. Before
decryption apply the outer frame cap and cap the encoded outer Content at
31 KiB and the encoded seal Content at 8 KiB. Cap rumor JSON content at
2 KiB before allocating handshake state. Wrap the decrypted receive buffers
in `Zeroizing` for their local decoding scopes. Use only the
following authenticated fields.

| Layer | Required checks |
| --- | --- |
| Outer | Signed event ID and signature, kind 1059, exactly one bare `p` tag naming the receiver alias. Refuse expiration and all other tags here. The unrelated ephemeral signer has no sender authority. |
| Seal | Decrypt via pinned `nip44::decrypt_to_bytes` using the borrowed alias secret and outer author. Decode an Event, verify ID and signature, require kind 13 and zero tags, and require the seal signer to match the expected canonical remote account. |
| Rumor | Decrypt with the alias secret and verified seal signer. Decode the UnsignedEvent, require the allocated kind and exactly one bare `p` naming the alias, and require rumor author equal to seal signer. Compute a fresh ID without the supplied ID and require equality if one was present. |
| Direction | The client receiver can accept only Challenge and Ready from the canonical server. The server receiver can accept only Probe and Confirm from the canonical client. The receiver role and alias are selected before decryption from the validated local binding. |

The SDK extractor discards the seal, so use the lower-level pinned
NIP-44 decrypt and Event verification for the complete receive checks.
Use the narrow pinned alias secret-key borrow in the receiver. Keep its
owned relay authentication key within the alias lane. Handshake expiry
clears the live transcript. The alias codec stays available for a fresh
Probe until revocation or owner teardown. A future Terminal owner must
set an explicit lifetime for the alias lane. The pinned `nip59` Cargo
feature enables `nip44`. Compile against the actual direct
`ConversationKey::derive` and NIP-44 decrypt APIs.

Use strict JSON with integer `version=1`, exact
`protocol="marmish/terminal/bootstrap/1"`, `type`, `sender_role`,
`mode="lossless"`, the 32-byte binding digest, and integer `expires_at`.
Use four concrete variant bodies with exact field sets, reject unknown and
duplicate fields, and use lowercase exact-width hex for the binding digest,
each present challenge or key, and each present previous rumor ID. An
initial Probe carries no previous ID. Other records carry the exact
preceding recomputed ID as `previous`. No private material, subscriber ID,
relay credential or local path is sent. Do not accept a capability event on
a canonical or session inbox.

Use the authenticated rumor's creation timestamp and require
`expires_at == created_at + 30` using checked arithmetic. Receiver age is
inclusive from -5 through 35 seconds at every accepted step. The local
30-second monotonic handshake deadline still governs even if every signed
record is fresh. The wrapper and seal use the SDK's randomized timestamps.
A relay acknowledgment does not advance the handshake.

Use four fixed records in one direction pair.

| Record | Sender | Required confirmation |
| --- | --- | --- |
| Probe | Client | Fresh unpredictable 128-bit client challenge |
| Challenge | Server | Exact client challenge and fresh unpredictable 128-bit server challenge |
| Confirm | Client | Both challenges and the client's fresh random 32-byte Terminal public key |
| Ready | Server | Both challenges, the same client session public key and a distinct server session public key |

Every record binds the normalized Command digest and explicit terminal mode
and roles through its version and record type. The only supported mode in
this increment is lossless. Each step extends the transcript with the exact
prior authenticated rumor ID. A record can advance only the matching state
under the current monotonic deadline. The initial Probe can allocate a small
bounded liveness challenge only. At Confirm, the server can reserve native
runtime capacity and supply a generated Terminal public key. At Ready, the
client receives evidence permitting it to start the separately qualified
native KeyPackage exchange. The bootstrap confers no terminal shell grant,
MLS room, PTY rights or durable activity record. The next phase binds the
package reference, withheld event ID and confirmed random accounts to the
confirmed transcript.

Each `Exchange` allows one live transcript for its binding. The fixture
closes the alternate binding's exchange before starting the main handshake.
A future Terminal owner's registry must cap live exchanges at one per
role-ordered canonical pair, including across distinct bindings.
One Probe per minute per binding includes failures. In a live state, an
exact duplicate yields no new transition, challenge or runtime reservation.
Use the recomputed rumor ID, independent of randomized outer wrapper ID.
Reliability retries use the original remembered signed wrapper and rumor.
After expiry or restart, a still-timely Probe can solicit one newly generated
liveness challenge subject to the current run's rate rule. Restart clears
both the rate and dedup caches. A party that simply recorded network
ciphertext cannot read the Challenge. No runtime can be reserved from this
Probe alone. A captured response, Confirm or Ready cannot satisfy a new
challenge. A revoked or unavailable
grant tears down the local exchange before its next transition or send.
Interfaces expose opaque bindings and fixed error categories. Timers, caps,
dedup and rejection paths stay in memory.

## Validation and CI

- Exercise real signed wrapper and seal extraction with both canonical
  endpoints and role alias isolation. Generate negative vectors for each
  row of the validation table using real signatures. Construct non-13 or
  tagged seals manually using pinned NIP-44 because the builder always
  creates kind 13 with no tags. Cover malformed and wrong recipients or
  signatures, mismatched seal or rumor signer, forged inner ID, duplicate
  and unknown content keys, uppercase hex, skipped and reordered
  stages, separate valid outer wraps with the same rumor ID, challenge replay after
  restart, wrong binding, oversized input, expired state and grant
  revocation. Require zero runtime reservations on every refusal.
- Trace alias and binding equivalence for two live Command owners in a
  private disposable fixture, plus separation for a second Command group
  and for a different canonical remote. No test passes private hex keys or
  protected transcript bytes to stdout, diagnostics or persistence.
- Round trip the four messages through the existing opt-in exact NIP-42
  loopback relay with role alias authentication. Send correct signed gifts on
  an issued but nonmatching Command route and require no durable ingress.
  Check that pending publications are volatile and no bootstrap KeyPackage
  is sent. Qualifying the supplied relay and Tor is outside this increment.
- Name the deterministic signed positive and wrong-binding negative
  `terminal_bootstrap::core_tests::signed_exchange_binds_current_command_room`.
  Include `cargo test --locked -p marmish-native --lib terminal_bootstrap::core_tests`
  in the trusted core workflow only if the full warm selection excluding
  build stays below one minute. Keep the loopback, native resource and benchmark tests opt-in.
  Report cold key setup separately from warm signed-wrap-to-check latency
  and compare the four-record local lifecycle with loopback latency.
