# Ephemeral Terminal route on the configured Marmot relays

## Scope and gate

Prove that native Marmot can exchange Terminal frames without application
persistence of bootstrap, room state, MLS secrets, native artifacts or frames.
Keep the canonical Command accounts and relay set. Give both endpoints
independently random Terminal accounts for each room. Use the unmodified
MDK revision in the native lockfile, an offline disposable relay and synthetic
frames. Obtain Claude approval before writing the proof and again before
selecting a transport for the full PTY phase. The proof uses synthetic frames.
The separate attachment confidentiality study evaluates durable alternatives.

The application boundary includes bootstrap, profiles, SQLCipher and sidecars,
journals, SDK caches, spools, logs and restart recovery. Host swap, hibernation
and core-dump collection are outside the proof. Relay storage is separate.
A relay can retain ciphertext past expiration and correlate subscriptions
and timing. The proof concerns local application persistence and never assumes
relay deletion or network anonymity.

## Selected separation and alternatives

A separate random Nostr identity gives each Terminal room's Welcome events
a distinct signed kind-1059 recipient. Give each room independent SDK
connections, adapters, subscriptions, replay state, buffering and reconnect
authority.
Leave durable Command discovery, Welcome routing and fallback, publication,
replay, acknowledgement, queues and journal recovery unchanged. Both ends
read their existing authenticated active Command binding and local grants.
The proof adds no Command wire operation, alters no Command handler and
writes no bootstrap row.

A shared canonical inbox would need to distinguish unknown Terminal package
IDs from legitimate ordinary Welcomes before durable ingest. Pinned upstream
can join a Welcome that opens an unrecorded retained ordinary package even
when the source event ID is unknown. A shared adapter would also join mutable
subscriptions, replay pins and flood budgets. Separate identities and adapters
avoid both dependencies.

A dedicated Command MLS exporter label could supply a room-shared bootstrap
key, but the pinned `exporter_secret_with_epoch` exposes only fixed native
labels and the safe exporter requires a component already required by the
group. Keep existing Command group components and native exporter use.
A public canonical-signed capability simplifies discovery, but discloses
Terminal support and leaves a relay-readable record. Choose private per-peer
capability on a derived NIP-59 inbox addressed independently of Command
and Terminal. No automated fallback or public advertisement is part of the proof.

## First probe with the pinned engine

Before building the capability route, open two production
`SqliteAccountStorage::in_memory_with_options` instances through
`SessionConfig::with_storage` in a hermetic fixture. Give each its own
fresh Nostr signer, matching runtime account ID and account identity proof
signer, fresh local device slot and independent MLS credentials. Require
explicit `temp_store_memory` and instrument file opens. Create only a local
slot lifecycle.

Use `prepare_fresh_key_package` with a publisher that signs locally and
denies all network exposure. Extract the actual pending signed event ID,
public MLS package and package reference. Confirm the held lifecycle
recognizes the pending ID without a relay acknowledgement. Supply the source
event ID using the upstream KeyPackage API. Use pinned native group creation,
NIP-59 wrapping, peeling and consent to admit the recipient without a roster
entry or enrollment. Assert the native Welcome recipient, authenticated
sender and source, used KeyPackage reference and two exact MLS leaves.
Run both orientations and send a synthetic frame in both directions. Set
60-second retention at creation and verify the signed application expiration
in both directions. Record actual device tagging and the founder's
verification. Fail if using an unadvertised KeyPackage or an unrostered
recipient requires publishing
kind 30443, adding a sibling device link, patching MDK or changing the
Command slot.

## Private capability and enrollment

Arm a bootstrap listener only for explicitly enabled, configured Terminal
peers with an authenticated, active, two-account Command binding. Use at
most two bindings per daemon in the proof, each with its own relay client
authenticated as the local alias and an exact kind-1059 `#p` subscription.
In the fixture, explicitly arm one agreed Command session per enabled peer
on each side. An absent, inactive or differing binding
reports Terminal unavailable. The full PTY design must review automatic
selection and capacity across multiple eligible sessions.

1. Take the normalized Command binding and existing role-ordered canonical
   npubs from the authenticated local room ledger. Derive the per-peer
   NIP-44 v2 conversation key using `ConversationKey::derive` from the
   pinned Nostr library. Feed it into existing HKDF-SHA256 with a dedicated
   Marmish Terminal bootstrap salt and versioned, unambiguous info encoding
   of purpose, normalized binding, recipient role and bounded counter. Map
   the 32-byte output to a valid secp256k1 scalar, retrying at most four
   deterministic counters on rejection. Reject collisions across roles
   and with canonical or active Terminal addresses. Test distinct aliases
   for different groups and peers. Keep derived address keys only in
   memory. They can be rederived across restart, but cannot derive random
   Terminal keys or MLS state.
2. Use the pinned Nostr NIP-59 wrapper with a dedicated closed, versioned
   application rumor sealed by the canonical sender and addressed to the
   intended bootstrap alias. Verify the outer signature, exact recipient
   tag, seal kind/signature, seal and rumor author match, recomputed rumor
   ID and exact inner schema. Both peers can derive the alias. Canonical
   sender authority comes from the seal and the existing authenticated
   Command binding and configured role. Reject the canonical Command inbox
   and native Terminal inbox as capability destinations. Do not use
   randomized wrapper time as freshness evidence.
3. The request carries an unpredictable challenge and expires after at most
   30 seconds. A qualified response echoes it and adds a random responder
   challenge. Require initiator confirmation of both challenges with exact
   binding, purpose and roles. Enforce one outstanding probe per peer and
   one probe per minute including failures. Reject expired and previously
   seen rumors without response or allocation. After restart, the still-fresh
   initial probe can at most solicit a bounded fresh liveness challenge.
   It conveys no positive capability or admission until the initiator confirms
   that new challenge. Old confirmations and all later stages produce no
   response or allocation without an exact current challenge. Check live
   grants on both sides before allocating a Terminal runtime or sending
   a package. An old build never subscribes to a bootstrap alias, responds
   or receives an attempted Command control record. Timeout reports
   Terminal unavailable.
4. Bind the two independent random Terminal account public keys to the
   canonical roles, Command binding and live challenges in the same
   authenticated private lane. Exchange only the invitee's public MLS
   KeyPackage bytes, reference and withheld signed kind-30443 event ID.
   Reject a random Terminal identity matching a canonical account or
   conflicting live binding, a package account/ref mismatch, or a replayed
   transcript. The actual signed Nostr artifact stays inside the memory
   runtime. The public MLS bytes keep their native signature, which cannot
   authenticate a Nostr publication. Authenticate a versioned transcript
   digest covering both random npubs, package reference, withheld event ID,
   purpose, binding, roles and both challenges. Before exchanging Terminal
   frames, confirm the actual MLS credential identities equal the bound
   ephemeral npubs and check the purpose and exact transcript digest under
   protected group state. Recheck current local canonical grants.

The qualifying peer can learn Terminal support. Relays see opaque bootstrap
recipient tags, ciphertext and traffic timing and can link repeated use of
an alias. Compromise of either canonical private key can expose recorded
bootstrap metadata and public packages. The separate random Terminal account
and MLS secrets stay outside the derivation. The fixture must find no
bootstrap identifiers or keys in profile files or logs.

## Runtime, grant and receive boundaries

Use a fresh account, slot, production memory SQLite store, runtime, SDK client,
adapter and routing state **per underway invite and resulting room** at each
endpoint. One runtime can serve one invite and one room only. The pinned
runtime can reuse an existing pending KeyPackage for its slot, so serving two
invites in one runtime would risk duplicate IDs and shared account linkage.
No package, slot, account or runtime survives room teardown or serves another
peer. All connections use the same configured relay URLs and direct/Tor
policy. Pause Terminal maintenance before activation. The scoped Terminal
egress allows exact native room traffic and Welcome only. Deny package
publish, republish or withdrawal, account registration, device linking,
history sync and generic inbox publication. Instrument every denied call,
including any replacement made due when a pending package is consumed. Keep
one package reservation per runtime. Runtime creation and
bursts of peel work must run outside the Command actor.

Precheck configured relay, issued subscription, signed recipient and size
before a Terminal kind-1059 peel. Require the native verified Welcome sender
and source package ID to match the live reservation, then check the native
used reference again at join. Account for both the precheck peel and the
engine verification. Kind-445 group delivery uses exact active native `#h`
routes bound to the Terminal engine and endpoint. Check native group identity
before effect. Register the founder's route before publication. The joiner
may drop early traffic for an unknown route and retry within the bounded
handshake. Neither receiver delegates to durable ingress. Prove Terminal
sync, deactivation and reconnect affect only its own subscriptions. Each
adapter releases pins for its own final dispositions, and deferred ordinary
deliveries keep their existing acknowledgement behavior. Use memory-only
trackers and no durable spill for Terminal or bootstrap. On the ordinary
path the required assertion is a drop in the pinned adapter's `routes_for`
and `deliveries_for` path, **before** `MdkOwner::receive_one` invokes
`runtime.ingest_delivery`. `routes_for` matches the exact registered
recipient or group destination and relay. Instrument each durable call site
at that seam with a test sink and positive ordinary delivery controls.
MDK can persist an ingress dedup identifier even when an invalid or wrong
recipient Welcome is refused, so engine-level rejection is already too late.

Admit before creating runtime storage or opening new connections. Permit
one underway invite per daemon and count it in a **four-room maximum** for
the proof. Permit at most two rooms for a peer and one unresolved frame
publication per sender per room. Cap serialized events at 64 KiB for
kind 1059 and 32 KiB for kind 445, decoded protected content at 16 KiB and
individual frames at the attachment-plan limits. Cap the ingress queue at
eight events per route and 64 events or 2 MiB daemon-wide. Budget pre-peel
work per recipient and URL independently of any claimed sender. Apply
authenticated rate and dedup caps per peer after verification. Pinned
in-memory SQLite exposes no page/cache cap. Four is a test ceiling, pending
measurements. Measure the live process at zero, one, two and four rooms while
running ordinary commands. Record incremental RSS and locked memory, migration
latency, `RLIMIT_MEMLOCK`, descriptor and socket counts, and ordinary p95
latency against the zero-room baseline. Require the fourth reservation to
succeed, the fifth to be refused and a second underway invite to be refused
before allocating resources even below the room cap. Require headroom
under the measured host memory/lock/descriptor budgets and no ordinary route
starvation at the four-room ceiling. If the measured ceiling is lower, cap
the route there and report the failed four-room qualification. Leave
SQLCipher's process-wide memory security enabled.

The pinned `RelayConnector` grants one live socket per configured endpoint;
cloning it into a second SDK client would wait behind the ordinary connection.
Leave that Command slot and its cancellation owner intact. Give the experimental
transport its own per-endpoint ceiling of six sockets, including at most two
bootstrap aliases and four room runtimes. Enforce the same counts before
dial or reconnect in direct and Tor modes, across at most 16 configured
endpoints. Reserve the existing ordinary socket independently; the proof's
maximum per-endpoint combined count is seven. Use the existing relay URL and
route decisions. The full Tor qualification must reuse the existing Arti
bootstrap with an isolated experimental stream context, independently scoped
socket permits and no direct fallback. Exercise permit exhaustion and cleanup
with an injected Tor connector and measure live Arti circuits and ordinary
delivery before enabling this path on Tor. An offline direct fixture can prove
protocol handling but cannot qualify Arti resources. Production per-profile
admission limits are a separate PTY design decision.

Require the current canonical grant and attachment generation before
admission, output, input, takeover, simultaneous read or resume. Use synthetic
effects in the proof. Revoke queued authority before it can take effect.
Alias possession and valid ephemeral membership cannot independently grant
canonical access.

## Relay permission qualification

The offline relay must enforce NIP-42. Verify the challenge, relay URL and
signature in the connection's authentication event. For the experimental
connections, reject kind-1059 reads before AUTH and require each such filter
to name exactly the authenticated recipient in `#p`. Reject filters for
another recipient and broad inbox reads. Use the same checks for bootstrap
aliases and room accounts. Admit group reads by their exact route and verify
native group authority in the client. Leave Command relay permissions intact.

The qualifying relay must admit AUTH from previously unknown bootstrap aliases
and room accounts. It must also accept the pinned native kind-1059 and
kind-445 writes whose valid outer signatures use fresh, previously unknown
keys distinct from the connection signer. Locally enforce the scoped route,
kind and size policy before egress. Verify positive deliveries under this
policy, with no account allowlist seeded from the generated experimental keys.
Record what was admitted without displaying private keys. Test a restrictive
mode denying unknown connection accounts and a separate mode equating write
authors with AUTH signers on experimental connections. Both must fail Terminal
qualification with the ordinary control still available. Require explicit
qualification of a relay configuration before enabling the route. The offline
proof confers no compatibility with the default public relays or other
deployments and offers no relay, permission or direct-transport fallback.

## Expiration and restart

Set 60-second application retention at Terminal group creation through
the reviewed attachment-plan component. Require signed expiration on real
kind-445 application publications. Pinned MDK adds no expiration to native
Welcome wrappers, commits or proposals and accepts late events. Check the
authenticated inner time and bound exchange deadline in the Terminal lane.
Exercise a 5-minute keepalive and an 11-minute inactivity timeout with an
injectable clock. Drop stale, duplicate, expired or revoked effects.

A restarted endpoint has a new unpredictable Terminal identity and fresh
MLS state. An old native Welcome still addresses the destroyed recipient
and an old native group event addresses the retired route. The daemon never
subscribes for either destination again. A relay cannot change the signed
recipient tag without invalidating the event. Capture actual correctly signed
Terminal kind-1059 and kind-445 events and have the fixture relay deliver
them intact using valid, active Command subscription IDs for nonmatching
destinations. Do this while the Terminal route is live, after expiry and
teardown, and after each endpoint restarts. Require zero durable-route
adapter deliveries, zero calls to the instrumented durable ingress sink and
zero corresponding dedup records. Deliver actual Command events on those
subscriptions as the positive control. If the Command adapter dispatches
either Terminal event, stop the proof. Leave ordinary
Welcome fallback untouched. This argument requires no tombstones or Command
key rotation, even after a crash before publish acknowledgement. The
rederived private bootstrap alias still requires current canonical
challenge/response and grants, and previous transcripts cannot authorize
new rooms. A later tmux attach requires fresh authority. An authorized
principal deliberately embedding data in ordinary Command payloads remains
outside this routing guarantee.

## Offline acceptance

Use the actual adapter, a disposable relay, separate local profile roots
and disposable identities. Add fixture support for delaying, reordering,
duplicating, misrouting and retaining events. Permit dropped acknowledgements
and a relay that deliberately ignores expiration.

1. **Native and discovery.** Pass the first probe before adding capability.
   Observe no Terminal kind-30443 publication, even after package use and
   reconnect. An authorized malicious claimant attempts to reconstruct and
   publish kind 30443 using all disclosed fields. It can sign under its own
   key. It cannot reproduce a valid signature under the ephemeral owner
   or canonical account. Ordinary discovery must continue selecting the
   qualifying persistent package, with the ordinary slot and device link
   unchanged throughout Terminal churn. Assert distinct ephemeral accounts,
   slots, MLS credentials and reservations across unrelated peers and across
   simultaneous live rooms for the same peer, each created in a serial invite.
2. **Ordinary interoperability.** Run actual unchanged Command CLI and
   server binaries, then each unchanged binary against its candidate
   counterpart. Execute an allowlisted command and compare exit status and
   exact bytes including NUL and shell metacharacters. Cover ordinary policy
   denial, a delayed Welcome across package rotation, and an ordinary
   unrecorded retained-bundle Welcome exercising the existing fallback.
   Run ordinary commands before, during and after Terminal traffic,
   reconnect, expiry and either endpoint restarting. Old builds receive
   zero attempted Command probes or new Command rows. Hold a Command
   acknowledgement during Terminal flooding and prove existing delivery
   completion and replay remain correct.
3. **Relay permissions.** Enforce the stated NIP-42 relay modes. Observe
   successful authenticated inbox reads and ephemeral writes in the qualifying
   mode. Exercise invalid AUTH, unauthenticated reads, reads as the wrong
   alias or room account, and attempts to read an unfiltered inbox. Exercise
   both restrictive write and connection signing policies, with an ordinary
   positive control after each failure.
4. **Capability and grants.** Establish the real private handshake, native
   two-leaf room and synthetic traffic in each direction. Test no public
   capability or unsupported-peer fallback. Reject wrong bootstrap
   recipient, wrong signer, role, group, purpose, challenge, generation,
   native Welcome sender/source/ref, third leaf and denied grant. Hold work,
   revoke, release and prove the stale generation is fenced. Confirm no
   duplicate registration, link or replacement publication.
5. **Crash and bounds.** Kill either daemon before and after confirmation,
   Welcome publication, join and frame acknowledgement. Replay old artifacts
   after restart, including a still-fresh request followed by recorded
   confirmations and package offers. Require no admitted capability or room
   from the replay. Exercise the exact Command adapter misdelivery seam for
   intact native events with zero tombstones. Verify independent reconnect,
   route synchronization and deactivation. Include duplicate, early, stale,
   oversize and claimed-sender flood cases. Saturate room, queue and socket
   limits under concurrent Command traffic and record RSS, memory-lock
   results, Command delivery and p95 latency.
6. **Exact no-disk checks.** Monitor opens/writes under all owned profile
   and temporary roots, including SQLite WAL/SHM/temp files, SDK, spools,
   launcher and daemon output. Instrument typed destinations before writable
   database, journal, cache or log boundaries, including deferred and replayed
   deliveries. Capture live sidecars before checkpoint and after abrupt
   termination. Open the persistent SQLCipher stores with test keys, inspect
   decoded native tables and logical journal for exact captured Terminal
   account, group, package and source identifiers, bootstrap transcript IDs,
   native artifacts and test frame markers. Scan raw sidecars and known
   encodings as supplemental evidence. Require the scanner to find expected
   ordinary command records as positive controls. Publish only sanitized
   artifact categories, counts and timing.

Run the proof under a fixed deadline on self-hosted Linux AMD64 or an
equivalent developer host. Collect sample counts, cold construction and
p50/p95 frame round-trip latency, process RSS and a Command latency reference.
Keep the full fault, load and benchmark proof opt-in. After independent
proof approval, qualify a small core offline dispatch or expiry test for
routine CI. Full PTY work requires a separate reviewed design and platform
qualification.
