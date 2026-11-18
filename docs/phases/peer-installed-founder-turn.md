# First installed founder enrollment turn

This increment of [installed acceptance A](peer-installed-heartbeat-first.md)
is based on `9df12c8` and its MDK lock at `bdf9833e`. It qualifies one
production-shaped Linux AMD64 founder turn on a checked private WSS loopback.
The turn ends with a charged native founder image, durable signed v2 Delivery,
and an exact publication disposition. A separate phase will consume and verify
the recipient’s native v2 receipt. The installed feature remains dormant in
this increment. Existing Command and Terminal entry points stay independent.

## Dependencies and owned code

The base has a checked physical route with no socket opener, fixture-only
`QualifiedOrigin` and a dispatcher for fixed provisional slots. Compose after
the separately reviewed authenticated recipient route and capped Welcome
dispatcher land. The route plan is `peer-authenticated-route.md`, approved
design blob `2ca530c4e8c8994652f8b60b8db9d3d8829642b5`. Reuse its direct
checked-socket core and timing fence. The capped Welcome design is
`ca9900da1ad148b4337d97653c3abeef703595a9` and the independently reviewed
source is `3b2a00f29895e2d28882289acd985c7aada2c579`. The founder work owns a new founder turn
and SQLCipher store, the narrow founder grant/path helpers, a separate founder
Control port based on the composed socket core, and an explicit
`FounderCreate` plus closed `FounderInspect` operation in the quota dispatcher.
The recipient port and operations stay independently owned. Keep the new
founder turn entrypoint, registrar and native operations test-only for this
proof. The exercised path still derives scope from the real lease and checked
config. A later normal compiled, mounted owner needs reviewed exact grant,
charge, route and recovery composition before entrypoint enablement.
Local setup uses the reviewed private TLS/NIP-42 fixture and a scoped test
qualification policy at the existing seam. Its positive evidence qualifies
this fixture only. A fixture witness cannot authorize an installed route.

## Owner and grant

The proof uses a real `Arc<ProfileLease>`, the prepared profile
identity, the checked config snapshot and its execution-client grant for one
canonical recipient npub. Require `Server` or `Both`, `peer.originate = true`,
an explicit Peer relay, direct routing (`RouteMode` other than `Always`),
distinct founder and recipient identities, and the exact checked private route.
Bind the non-cloneable founder authority to the same lease, local selection and
generation, config/security digest, recipient, qualified origin, TLS trust and
route incarnation. Its constructor stays private to the owning turn. Role and
grant are read from the verified configuration. Add only the founder config
projection. The recipient-only `ReceiverCandidate` cannot issue this
authority. `bound_peer_relay` does not compare execution clients. The checked
full config digest and re-evaluation of the actual grant fence changes.
Recheck lease, owner files, role/grant, qualification, route and trusted time
before each authoritative CAS,
native open/effect and socket write. Recheck again before recording success.
Revocation, config replacement, expired scope and sticky cancellation close the
turn and preserve prior evidence and charges. Use the minimum applicable
existing scope, control flight, replay and package deadlines. Use the anchored
clock and existing bounded transport waits.

The recipient route plan owns its listener and authorization. Founder shares the
checked transport implementation after coordination but has a separate exact
incoming kind 7435 / founder `#p` subscription and its own sealed Control send
permit for the configured recipient, flight, frame hash and socket incarnation.
Pre-drain old frames as the raw fixture does. Separate complete bounded history
from live delivery, and correlate a positive relay acknowledgment to the exact
event and current authenticated socket. A raw ACK alone proves relay acceptance
only. Use checked TLS, NIP-42 and exact signed sender/recipient checks on the
same configured route. Relay qualification, CLI/daemon mounting and cross-platform
enablement stay outside this slice. Keep fixture enrollment, clock, TLS
registration and fault controls out of non-test binaries.

## Durable turn

Use one leased SQLCipher ledger, `peer/founder-ledger.db`, and a bounded single
founder slot in the proof. Open the ledger by stock delegation after the VFS
mount and before establishing a native quota scope. Scope its rows by
direction, founder, recipient and owner-assigned
generation with an exact offer revision and compare-and-swap state. Initial
claim cannot overwrite a prior unresolved row. Bind subsequent writes to the
same lease capability and exact previous state. Existing recipient state and its
SQLCipher format remain independently scoped. Derive zeroizing ledger and
native keys from `Paths`' existing profile-bound HKDF inputs. Use independent
domains `marmish/peer/founder-ledger/1` and `marmish/peer/founder-native/1`.
The ledger has empty slot info and the native image uses the reserved nonzero
generation's fixed-width big-endian u64 as slot info. The fixture singleton
`peer_founder_enrollment_checkpoint` provides transition examples.

1. Prepare one v2 Offer with `peer_control_wire::Codec` and the owner-chosen
   room and exact checked route. Persist the exact canonical outer frame, its
   event ID and SHA-256, and the authenticated preparation coordinates in one
   transaction. Keep the freshly constructed typed `Offer` private to this
   running turn. A fresh exact permit can send only the durable frame, after a
   durable one-shot arm and current authority check. Map the checked port's
   `Accepted` to a durable acknowledgment after recheck. Record `NotSent`
   and `Rejected` as terminal fenced dispositions, and `PossiblyExposed`
   or failed post-result persistence/recheck as uncertain. Only a durable
   acknowledged Offer advances. No send after any other armed disposition.
   Byte-identical repeated intake is a no-op.
2. Open Consent with `Codec::open_consent` on the founder key and that exact
   v2 Offer. The codec checks the signed recipient, prior revision, package
   event and native reference/slot, and verifies package bytes and lifetime on
   the exact route. Persist the exact authenticated Consent carrier/body
   binding and digest before native work. The one scoped package travels inside
   Consent. Discovery, public KeyPackage publication, and unwrapped Welcome
   publication have no authority in this turn.
3. Durably reserve a locally generated `founder-<generation>.db` and the
   exact directory, filename set and charge before any native create/open.
   Install the guarded VFS before the process’s first SQLite open. In the
   checked Peer directory it refuses stock fallback for the canonical
   `founder-<generation>.db` family and its main/journal/WAL/SHM names even
   before the ledger opens. Accept only decimal nonzero generation names with
   no alternate encodings for registration. A family match alone grants no
   open. After reading the ledger, register one exact main and journal within
   that directory, with the exact inode and durable charge. Ordinary paths
   outside the reserved family use existing stock delegation. Create the reserved 0600 main exclusively in the
   checked directory, sync it and the directory, record its exact device/inode,
   and durably consume the unique native-create turn before opening the managed
   account. Check the same reserved path/inode at SQLite callbacks and closing
   readback. Keep DELETE journaling. Enforce the existing 16 MiB main and
   128 MiB combined charged envelope for mains, journal and accounted artifacts,
   including unreleased/quarantined data. Under the sole owner lease, source the
   preexisting charge from the checked on-disk ledger and finite path inventory,
   including the bounded ledger and its auxiliaries. This one-slot proof refuses
   preexisting recipient slot, custody or unknown artifacts on both Server and
   Both. It cannot share a quota owner with a recipient task or accept a
   caller-supplied other-charge number. Count an inode once and reject aliases.
   Reserve expected main/journal names
   with explicit refusal of WAL/SHM, aliases, hardlinks and fallback for the
   managed scope. The proof exercises real journal accounting on the exact
   founder call graph. It does not increase the installed combined 64-binding
   limit or claim eligibility for a multi-profile process.
4. Call locked MDK `AccountDeviceRuntime::prepare_create_group_with_audit_context`
   as illustrated by `peer_enrollment_founder_with_group`, with one exact
   verified `Consent.key_package()` and the app’s exact `NostrRoutingV1`.
   Extract one typed founding group and its one Welcome before any native
   outbox publication. Install both a counting refusal native transport
   adapter and an independent counting refusal KeyPackage publisher during
   create and completed cold inspection. Return typed refusal for every
   attempted native publication, subscription or KeyPackage publication.
   Require zero attempts through either component before advancing. Keep
   the existing native group naming contract independent of signed control
   version. Use the scoped quota guard and bounded single-poll discipline from
   the capped recipient work. A Pending poll, cap violation, changed inode,
   unexpected output, any native egress attempt or post-close failure leaves the
   charge held and prevents Delivery. Prove that the locked signer and
   Welcome wrapping reach Ready inside the quota scope. The deny-I/O adapter
   has no native endpoint size report. The v2 Control frame cap and its
   existing native-artifact bounds govern eligibility for Delivery.
   While still in the scope, use `welcomes_for_key_package` and
   `outstanding_welcome_deliveries` to require one exact tracked and owed native
   Welcome. Retain its exact bytes for Delivery and call the public
   `withdraw_welcome` on that message ID. Require `Withdrawn`, the same tracked
   ID for that package, and zero outstanding deliveries. An already delivered,
   missing or different row fences the charge. MDK's withdrawal removes the
   native fanout without changing group state. Quarantine files from interrupted
   create or withdrawal. Only completed candidates qualify for a native
   historical inspection, with recovery and live adapter unavailable.
   Close handles and verify same private image, journal cleanup, native founder
   membership and route, initial epoch and exactly the expected peer leaf.
   Use a capped deny-egress inspection of the cold image with the derived key
   to recheck the group, tracked Welcome ID and absence of an outstanding
   Welcome. Record the typed group ID, native epoch pin and leaf indices,
   Welcome ID and digest with the image attestation. No native create is repeated.
5. From the exact in-memory authenticated Offer, Consent and verified Welcome,
   `Codec::prepare_delivery` produces one v2 Delivery. Store exact outer frame,
   event ID and hash, and the durable `StatusNegotiation` projection (group,
   route, app room and signed revisions) with the existing reservation and
   Welcome evidence before physical egress. These stored coordinates carry
   negotiation evidence only. Move through the same one-shot arm and observed
   relay ACK or uncertainty steps as Offer. Positive publication records only
   relay acceptance and keeps the group dormant.

A restart readback validates the exact encrypted ledger schema, generation,
current charge, pinned path/inode and canonical frame hash and outer signature.
A completed candidate also requires an independent guarded cold native reopen
to verify group, epoch, leaves, route and the withdrawn Welcome's tracked ID
with zero outstanding deliveries. The completed image opens in the proof
with the same separate transport and KeyPackage publication refusals.
Incomplete rows have no native reopen permission.
The private SQL store/writer binds the projection to the original authored
payload in the same transaction. The founder cannot open an outbound gift wrap
encrypted for the recipient with its own key. This slice intentionally limits
restart to read-only historical evidence. A process cut before Consent, during
native create, or before the atomic Delivery freeze fences that generation
without resetting it. A cold Armed row means possibly exposed, even before
socket I/O is observed. No reseal, re-create, network retry, release or
replacement follows an ambiguous cut. Preserve charged incomplete artifacts,
including a main file created before its inode CAS, for controlled later
reconciliation. A normal completed readback still grants no new live turn.
Establish restartable authored transcript/uncertain reconciliation separately
before operator-facing enablement. This avoids reconstructing typed Offer,
Consent or Delivery from mutable SQL columns.

## Receipt boundary

The follow-up consumes the authenticated native v2 receipt after the recipient’s
real accepted PostJoin path. It must bind the durable founder reservation,
current grant and checked route to the scoped native processed event,
authenticated recipient leaf, matching post-rotation epoch and exact v2
Offer/Delivery, Welcome and app-room coordinates. The existing
`peer_accept_receipt_verify::pending` and singleton founder checkpoint show
native-before-durable ordering for v1. They do not supply an installed v2
receipt authority. This increment emits no acceptance/application ACK and
opens no status room, heartbeat, approved status or command permission.

## Qualification and evidence

- Run a dedicated Linux AMD64 process with the checked quota delegate
  installed before its first SQLite open, then open mixed ordinary/charged
  native stores. A separate cold run refuses SQLite opens of canonical founder
  paths before opening the ledger, and proves an unrelated path still delegates.
  Use both independent native identities and a bounded private TLS/NIP-42 WSS
  fixture. The actual recipient verifies exact v2 Offer, produces signed
  Consent for its unpublished native package, and verifies exact signed
  Delivery and Welcome coordinates. Inspect relay traffic for only the
  authorized Control frames. Prove scoped recipient-only reads and correlated
  publication, and reject preloaded/stale/wrong event acknowledgments.
- Enumerate refusal at wrong role, disabled Peer, absent/removed execution
  grant, self or foreign recipient, foreign route, stale qualification,
  cross-version Consent, different revision, wrong package, duplicate and
  expired flights. Include a grant/config change after arm before actual
  write and a grant removed between verified Consent and native create. Test a
  Both profile with recipient charges refuses first. Compare stock Command
  success/readiness before and after the proof.
- Exercise actual main/journal boundaries and stock delegation, wrong and
  displaced inode, symlink/hardlink, forbidden auxiliary names, unexpected
  native Pending/egress and bounded refusal cleanup. Verify the exact
  withdrawal. Count zero native transport publications, zero subscriptions
  and zero calls to the independent KeyPackage publisher both during native
  create and on completed cold reopen. Verify the persisted key derivation
  with group/epoch/leaf readback under the same profile and wrong
  key/generation refusal. An owner crash/readback test covers each meaningful
  durable edge: before file create, before inode
  persistence, after native-create consumption and before Delivery freeze,
  frozen before arm, armed before write, and possible exposure with lost ACK.
  A restart never generates a replacement Offer, account or frame. Persist
  no ambient test capabilities in the normal Linux binaries.
- Run focused offline locked tests, strict scoped Clippy and a normal bins
  check plus installed entrypoint refusal. The normal binary checks establish
  that they expose no founder turn. Keep the private fixture opt-in.
  Record loopback Offer-to-Consent, charged native create, Delivery-to-ACK and
  cold readback times separately, alongside refusal and charged-file evidence.
  macOS, Linux ARM64, third-party relay eligibility and live v2 receipt
  acceptance require separate evidence.
