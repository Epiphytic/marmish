# Terminal attachment confidentiality

Beads `marmish-dwt.1`, under `marmish-dwt`. This proof gates the
[native PTY phase](interactive-pty.md). The smallest proposed native path
uses one purpose-specific two-leaf room, volatile per-attachment X25519
keys with a per-frame hash ratchet, MDK epoch-pinned sends, and explicit
retirement of terminal-only storage. Prove it with a fake endpoint and no
shell. The [interactive roadmap](interactive-peers-roadmap.md) currently
calls for an ephemeral MDK payload route.

## Approval and scope

The user approved encrypted local retention up to 24 hours, 256 MiB of
charged wire data per room and 4 GiB per profile. PTY plaintext and its
decryption keys stay out of the database. Expired rooms must be reclaimed
before the lane reopens after restart. The 60-second relay expiry is a hint.
The installed Terminal path uses volatile MDK rooms. These limits bound any
later durable fallback.

The user requested interactive Marmot sessions, tmux reattach, and the choice
of takeover or simultaneous read. This proposal preserves those behaviors and
the existing five-minute keepalive and eleven-minute endpoint timeout. It
changes the roadmap's storage rule. The outer MLS records, inner ciphertext
and public handshake fields may enter native durable storage while raw PTY
bytes and the material needed to decrypt the inner layer stay in volatile
application memory. MDK `8abc51befbf71a1ae3bdbb25eb0860c731e75db2`
retains some outbound work and persists an authenticated receive output with
MLS state. An ephemeral inner layer is a way to use that native path while
keeping the terminal content itself out of its storage.

If a later phase uses the approved durable fallback, a sender's
native `Sent` rows retain the exact signed kind-445 wire bytes for echo
classification until the room is retired. The online worst case is 24 hours
and 256 MiB of charged wire data per room, with up to 16 live or retiring
rooms and 4 GiB of charged wire data per local profile. SQLCipher rows,
indexes, pages and WAL add physical overhead. An offline daemon can leave
rows on disk beyond 24 hours. It must reclaim expired rooms before enabling
the lane on restart. The separate signed relay expiry is 60 seconds and
depends on relay enforcement. Terminal rooms enable component `0x8005`
and keep it out of Foundation and Command rooms.
A later durable implementation requires reviewed quota, expiry, cleanup
and replay evidence. The live writer uses a separate volatile MDK runtime and
does not use durable ciphertext storage.

## Fixed room authority and dispatch

Add `RoomPurpose::Terminal` to the fixed-purpose router. The exact room name
is `marmish/terminal/1/<32 lowercase hex digits>` with description
`marmish-terminal/1 terminal transport`. The initiating client is the accepted
founder and sole MLS administrator. The founder role supplies MLS authority
only. The server is the sole source of interactive grants. Require exactly two
active, distinct canonical accounts with bound leaf indexes, Current protocol
profile, the exact signed purpose and allowed components, and configured
signed routing. Verify the recipient's local role and expected remote account.
Use the ordinary known-peer Welcome flow only. Before passing a wrapped
Welcome to native processing, use `NostrMlsPeeler::peel_welcome_with_provenance`
to verify its NIP-59 seal and require that signer to have known local command
trust. This precheck cannot establish group purpose or MLS authority. Bind
the observed signer to the exact unchanged outer wrapper, then require the
same signer to equal the MLS-authenticated `GroupJoined.welcomer` and the
confirmed founder before registering Terminal routing. Missing provenance
or a mismatch fails closed for the Terminal route. The default-off server
`terminal.clients` grant is a list of canonical keys already present in
`execution_clients`. Neither that existing allowlist nor executable
patterns or generic shell-command settings grant an interactive session.
Clients pin the server through existing trusted-remote and destination
configuration. Check both independently of `check_group_for`'s existing
shared `self.allowed` set. Follow the PTY design's rejection of privileged
server execution.

In `mdk_adapter::apply_effects`, classify the native group purpose before
`is_command_join`, `project_join`, the generic `pending_joins` branch,
`project_message`, and generic rejection and invalidation. The Terminal
router owns joins, messages, invalidations and cleanup for that lane. Use
a distinct inner kind, proposed `30101`, an exact `marmish/terminal/1`
protocol marker and a closed message schema. A message in a pending Terminal
join is retried only within the pre-admission budget. A permanently invalid,
ungranted or expired Terminal room is fenced and retired by this lane. A
transient inability to classify or validate a possible Terminal room
defers it without a legacy fallback. Reject terminal kinds in Command,
Stream and Foundation decoders before logging or journaling payload, and
reject their kinds in Terminal. An approved room still needs a valid
attachment before terminal data can be accepted.

Freeze the existing `check_group_for` canonical authority digest through a
terminal-scoped equivalent of `bind_authority`, including role mapping,
configured signed route, fixed components, admin and sorted account/leaf
identities. Validate via the same pattern as `rebuild_routes` on admission,
restore, every send and receive, and after state changes. Removal, changed
leaf or account, rejoin, founder or route change, component change, lost
grant, or application invalidation involving a live attachment fence the
affected session and retire compromised authority. An ordinary canonical
MLS epoch advance with the same frozen authority preserves the attachment.

Use `AccountDeviceRuntime::epoch_pin` and
`SendIntent::AppMessage { expected_epoch: Some(pin), .. }` for every handshake
and sealed frame. Upstream's pinned-send tests prove an unsettled or
mismatched send is refused before queueing or publishing. At receipt require
the event's authenticated epoch number, sender, and leaf index, and require
the carried pin to equal the receiver's current runtime pin before opening
a frame or confirming a handshake. Each confirmation and frame authenticates
that full current-pin value in its transcript or AAD. Missing leaf evidence
fails closed. A still-current refused frame may retry with identical bytes.
If an epoch changes after a frame
has been sealed and the old sequence cannot be delivered safely, close the
attachment and reattach for a fresh redraw. An idle ordinary epoch advance
allows the next frame to use a fresh sequence under the new pin and the same
attachment. Verify live grant, generation and lease again at the actual PTY
write in the later phase.

This pins application acceptance to current authority and a valid current-pin
proof from the already authorized endpoint. A stale frame with an old pin
fails, including after a same-number fork, and changing its pin breaks its
AEAD or confirmation. The pin itself cannot prove the exact source branch of
a deliberately constructed envelope from a peer that controls both valid MLS
and live attachment keys. At the current MDK revision, `app_payload.rs`
computes `authority.source_context` from the same authenticator as
`EpochPin.identity`, while `reporting::requires_source_authority` exposes it
only for moderation kinds. A Terminal event uses neither moderation kind.
The independent technical review must explicitly accept the current-pin
application proof for this threat model. If exact native source-branch
provenance is required, obtain a reviewed upstream generic app-event source
pin or an equivalent verified ingress API before enabling this lane. With
that evidence available, require the event-source pin to equal the current
pin too. Do not infer source provenance from the event's epoch number alone.

## Ephemeral exchange and seal

Use a direct RustCrypto `x25519-dalek` 2.0.1 dependency with the `zeroize`
feature and `EphemeralSecret` with OS randomness. Use `hkdf` 0.12 with SHA-256, `hmac` 0.12 with SHA-256, and
RFC 8439 `ChaCha20Poly1305` from `chacha20poly1305` 0.10.1. Reject an
all-zero shared secret using `SharedSecret::was_contributory`. Generate one
ephemeral share per endpoint and exchange. Use an independent exchange
instead of an MLS exporter because native persisted state or retained
exporter material may be available after restart. Fresh public shares may
persist and cannot reconstruct the exchange once its secrets are erased.

Use MLS-authenticated `Hello`, `Reply`, `Finish`, and `Ready` records in
that order. The client generates a fresh 128-bit exchange ID, and the server
supplies the opaque 128-bit session and attachment IDs and the writer
generation. `Reply` proves the server knows the transcript key, `Finish`
confirms the client, and `Ready` confirms the server validated `Finish`.
Separate HMAC keys and labels for all three confirmations. Allow no
plaintext release, resource creation or input before the role-appropriate
confirmation. A retry preserves bytes and identity. Restart or abandonment
requires a fresh exchange and attachment IDs.

Encode the transcript with a fixed ASCII domain
`marmish/terminal/1 transcript`, a one-byte version, and then the client and
server account keys in that order, a u16 big-endian length plus raw MLS group
ID (1–255 bytes), the 32-byte frozen authority digest, the u64 big-endian MLS
epoch, 32-byte branch identity, 16-byte exchange, session and attachment IDs
in that order, a u64 big-endian writer generation, a one-byte writer or
observer role, and the 32-byte client and server public shares in that order.
Use the transcript SHA-256 as HKDF-Extract salt and the X25519 result as input.
Each output uses an exact ASCII HKDF-Expand label beginning
`marmish/terminal/1 `, followed by `chain c2s`, `chain s2c`,
`confirm reply`, `confirm finish`, or `confirm ready`. Each confirmation
MACs the 32-byte transcript digest. Complete a handshake on one epoch pin.
Discard the shared secret, ephemeral key, HKDF state and confirmation keys
when their handoff completes or the handshake terminates.

Each direction starts at sequence zero. For sequence N derive a 32-byte
AEAD key and successor chain key by HMAC-SHA256 keyed by the current
chain key, using distinct exact ASCII inputs
`marmish/terminal/1 frame` and `marmish/terminal/1 next`. Erase the old
chain and one-frame key as the sequence is committed. The 12-byte nonce is
four zero bytes then the u64 big-endian sequence. The frame AAD is the
32-byte transcript digest, one-byte direction, one-byte type, u64
big-endian sequence, u64 big-endian current epoch, 32-byte current branch
identity, and u64 big-endian authenticated creation time. Bind that time
to the outer authenticated application event. A frame is sealed once per
sequence and retry uses identical bytes only. A receiver opens only the
next frame. It may buffer at most 16 out-of-order sealed frames within
the limits below, keeps a bounded recent digest window to identify exact
duplicates, and never derives or keeps old frame keys. A tag failure,
conflicting duplicate, out-of-window gap or unresolved gap after 30 seconds
detaches the affected attachment and emits no plaintext from the failing or
later frame. Final authority checks precede forwarding. Claims cover prior
erased frame keys after loss of the current chain state and expired keys
after restart. Live key compromise can expose the rest of that chain.
A fresh exchange is mandatory on attach, takeover and budget exhaustion.

Observers on the same local client daemon can each run their own handshake
in the same two-leaf room. Their IDs, keys and leases are independent, and
their authenticated role grants output only. The server must encrypt output
separately for each attachment. Bound the first multi-attachment release
to one writer and three observers per session and retain separate queues
and permits. Measure duplicate encryption and relay bytes before raising
this cap. This proof does not add a second MLS device leaf.

Hold key types, decoded content and queues in zeroizing buffers. Keep
`Debug` for these types redacted, never enter secrets or raw frames into
argv, environment, diagnostics or audit state, and avoid immutable plaintext
copies. Set and verify an inherited zero hard and soft `RLIMIT_CORE` in
processes that hold terminal keys before accepting an attachment. Use
best-effort `mlock` for the small owned key slots and zeroize before unlock.
Account for RAM capture, swap and independently configured tmux scrollback
separately from application persistence.

## Limits, cleanup and restart

| Item | Initial bound |
| --- | --- |
| Pending handshake | 30 seconds, one per remote account and Terminal room, at most four per profile |
| Keepalive and idle lease | Five minutes by default, eleven minutes from last fresh authenticated inbound frame per attachment |
| Attachment lifetime | Eight hours, 65,536 frames or 512 MiB of plaintext per direction, whichever comes first |
| Unsubmitted, reordered or retrying frame | 30 seconds from creation or first admissible arrival as appropriate, at most 8 KiB raw payload |
| Native Terminal room | 24 hours, 65,536 attributable send and ingest attempts, or 256 MiB of offered wire payload per local copy |
| Live Terminal rooms and outstanding native data | Up to 16 rooms per profile, one unresolved native application fanout per room |
| Signed relay expiry | 60 seconds for Terminal application records |

Use suspend-aware monotonic lease and queue timers. Treat the signed
application creation time as seconds since Unix epoch. Sample wall time
and suspend-aware monotonic time at handshake start and retain the pair
for each attachment. If local elapsed wall time differs from monotonic
elapsed time by more than 10 seconds, fail the handshake or close the
attachment and require a fresh handshake after clock synchronization.
Never learn a clock offset from a peer event.

Reserve a ten-second skew allowance. Require every handshake record and
sealed frame to have observed receiver age in the inclusive range
−10 through 40 seconds, computed as receiver wall time minus authenticated
sender creation time. For send and retry, require both monotonic age at
most 30 seconds since local creation and local wall time strictly before
signed expiration. Require signed expiration equal to authenticated
creation time plus 60 seconds, with checked arithmetic. Apply the
receiver age and expiration checks before forwarding or advancing useful
attachment state. Buffer residence retains the separate 30-second
monotonic limit measured from first admissible arrival. Drop stale,
too-far-future or expired records without lease renewal. A required
sequence or confirmation that stays missing takes the normal bounded
timeout path and reports a clock-synchronization hint without claiming
clock skew as the cause.

Expiry of an individual
lease erases both traffic directions, buffered plaintext, pending confirmation
material and that attachment's slot. Lease expiry of a writer releases its
writer generation and leaves live observers with their own keys. Observer
activity cannot prolong the writer lease. Room authority loss, retirement or
daemon shutdown erases every attachment's keys and pending exchanges in
that room. Takeover erases the former writer, increments the generation before
granting a successor, and discards ambiguous input. Duplicate frames,
publication acknowledgements and another attachment's traffic cannot renew a
lease. A restart has no attachment keys and never replays old PTY input.
The later PTY phase preserves the tmux shell separately.

Enable the immutable `marmot.group.message-retention.v1` component
`0x8005` only in Terminal rooms, exactly eight big-endian bytes encoding
60 seconds. Register capability support and supply the component at creation.
Pin its bytes in the authority digest and reject wrong or removed retention
and account-device components. Foundation and Command continue to reject
its presence. Inspect the exact signed kind-445 `expiration` and the
receive event's `AppMessageRetentionDecision`. Relays may ignore the tag.
Enforce both the application age check and egress deadline locally. Require
the pre-publish `PublicationGate` to fence stale, revoked and retired
Terminal fanouts, including frozen send retries and startup recovery.
Bind a publication permit to the actual native ID, room, endpoint set,
attachment and age before the first native publish. Require that proof
also across native retry. `SessionConfig::with_storage` already takes a
`TransportPeeler`. Test a narrowly delegated wrapper of
`NostrMlsPeeler::wrap_group_message_with_metadata` that distinguishes
application records from commits, and stamps the resulting native ID under a
single fresh same-room send reservation. Only arm the reservation after the
owner verifies no unresolved same-room native application work. The wrapper
receives authenticated application time and retention but no inner ID, so
prove that convergence, maintenance and retry cannot stamp a different
application event under that reservation. Keep ordinary commits separate
from application permits. If that invariant fails, require a native hook
with exact inner-ID provenance. Restore no Terminal permit after restart.
If an old or rejected fanout blocks the room, retire it, confirm cleanup
and establish a fresh two-leaf room to reattach for a clean redraw.

Explicitly clean processed Terminal application rows through the protocol
storage trait. After the serialized in-memory decision accepts a frame
into the bounded authorized endpoint queue, or drops it permanently,
acknowledge the native event and delete its processed transport message
through `MessageStorage::delete_message` in one native transaction as
applicable. At this MDK pin the delete removes pending authority, pending
application events and the matching `cgka_messages` row. It preserves the
separate processed-transport dedup IDs and epoch state. Use this path for
expired or unknown attachments after restart without opening ciphertext.
Defer only while transient validation is possible within the deadline.
On ack/delete failure keep the work pending under a bound, and use the
volatile sequence watermark to avoid a second delivery while the daemon
lives. Prove that deleting a processed Terminal row does not break the
next MLS application receive, legitimate canonical convergence or
invalidation safety. Never delete unrelated rows or the sole live evidence
for an unprocessed or outgoing-native obligation.

MDK persists outgoing frozen wire and keeps the exact signed wire bytes in
the `Sent` row after acknowledgement. Preserve these rows for native own-echo
classification until room retirement. A delayed own echo during normal operation
or recovery must produce no endpoint input or new application work. Fence
expired room routing before deletion and prove a delayed echo after deletion
cannot reach a replacement room. Reserve a conservative wrapped-size upper
bound before asking native MDK to send. Check the exact signed bytes in the
peeler before returning the wrapped event for engine persistence, refuse any
overflow, and charge the actual signed size against the reservation. Charge
exact inbound wire size before native ingestion, including the bounded retry
path. Keep a retiring room and its byte allocation within the 16-room and
4 GiB aggregate budgets until storage cleanup is verified. Refuse new rooms
or additional application work when cleanup is blocked and the bound would
be exceeded.

Persist content-free room budget reservations in bounded batches ahead of
`runtime.send` and Terminal-route native ingestion. Count attributable
retries and incoming events before native storage can grow. Deny excess
work, fence the room and call `AccountDeviceRuntime::forget_group_local`
before admitting a replacement. The pinned SQLite schema cascades
processed-transport IDs when their owning group is removed. Verify no
unbounded residue from messages, queued intents, frozen fanouts or
retirement markers. Keep retirement tombstones within a bounded replay
window and rate-limit new room creation. Use a fresh group and routing ID
after rotation while preserving the opaque tmux session mapping. Show
that expiry and sustained traffic reclaim the logical rows and reuse
SQLite pages, including under checkpoint pressure. Key erasure supplies
the confidentiality bound even for old ciphertext outside the database.
If egress permits, row deletion, bounded retirement or replay behavior
cannot be proven through current MDK, keep the Terminal lane disabled and
seek an upstream change.

## Fixture, scan, and exit

Start with `mdk_adapter_tests::Fixture`, the two-profile offline harness
backed by real SQLCipher and native MLS. Keep its explicit unreachable
`wss://127.0.0.1:9/` sink. Parameterize the app kind/tag helper for exact
Terminal and existing Foundation records. Add symmetric Alice and Bob
reopen helpers, and capture the signed frozen kind-445 events before injecting
them as `TransportDelivery`. Inspect their timestamps, relay targets and
expiration tags. This test sends no packets to a live relay and validates
the wire's expiry instruction. Installed PTY validation uses a private
relay separately. Keep fixed private shares and timestamps solely in
`cfg(test)` known-answer tests that exercise the production KDF/encoding.
Production creates shares exclusively through OS randomness.

- Hold a real pending commit and prove a pinned Terminal send is
  refused with no corresponding queued intent or fanout. Use a single
  explicitly unpinned test-only control with already sealed ciphertext to
  demonstrate the queued `SendIntent` representation and sender reopen
  window. The production frozen-fanout window is created by pausing
  publication after an eligible pinned send. Inspect and reopen the sender
  before a publication acknowledgment. Observe the actual guarded adapter
  and show that old restart obligations are fenced and retired.
- Ingest the captured authenticated wire event at the receiver. Hold the
  event before it is acknowledged, inspect the persisted projection, and
  reopen Bob across that window. A reopened fixture with the original
  profile and available MLS state may recover the signed outer application
  bytes. Prove that it has only public exchange metadata and the inner
  sealed record, and cannot reopen that inner record. Exercise a successful
  fresh handshake and authenticated frame after cleanup.
- On owned disposable copies derive the correct `mdk` and `journal` keys
  through the production SQLCipher derivation. Inspect typed application,
  queue, fanout and journal stores and scan physical snapshots with a
  test-only SQLCipher-4 page reader. Verify page authentication before
  scanning decrypted main pages (including freeblocks and freelist),
  and decrypt every captured WAL frame including superseded page images.
  Inspect the shared-memory WAL index as raw metadata and verify rollback
  journal and temporary files are absent or scanned when present. Snapshot
  files while held at each relevant commit/checkpoint boundary. No
  plaintext export or key leaves the temporary fixture. A normal keyed
  query or raw grep of encrypted pages alone cannot establish the result.
- Seed distinct UTF-8, quoted, and binary sentinels through the existing
  Foundation receive path on the positive control profile. Require the
  typed decoder and page reader to find the intended encodings. Seed and
  delete a test-only marker with secure deletion disabled to demonstrate
  freelist and WAL coverage. Check that the live Terminal fixture's
  negative scans find no plaintext, equivalent stored encodings or
  test-observed plaintext key material in either database, its sidecars,
  captured logs, diagnostics, and spawned process argv or environment.
  Capture the production-shaped fake-endpoint process without putting
  sentinels into its argv or environment.
- Test low-order shares, transcript swaps, wrong signer and founder, lost
  grant, a third leaf, another branch at the same epoch, late prior epoch
  frames, normal settled epoch advancement, a stale writer, duplicate and
  reorder, truncated AEAD frame, wrong authenticated type and a modified
  tag mid-stream. In the streaming failure case verify that earlier valid
  frames remain ordered, no bytes from the corrupted or later frames
  reach the fake endpoint, and retry requires a fresh attachment.
  Exercise timeout, detached writer with a live observer, bounded durable
  cleanup, ordinary command regression, and renewed room creation with no
  old-generation input delivery.
- Advance the real native epoch while a previously valid frame is
  unresolved. Show that stale delivery cannot reach the endpoint, its
  attachment closes, and a new exchange under the current pin delivers a
  fresh frame once. Advance the epoch between `Hello` and `Finish` in a
  separate case. Verify the old handshake slot and keys are discarded,
  then retry with fresh IDs and shares under the new pin.
- Run the actual pre-ingest Welcome peeler against an admitted known signer
  and the exact original wrapper through native processing. Verify equality
  with the authenticated native welcomer and founder. Exercise an unknown
  signer, missing provenance and a signer-to-welcomer mismatch. Require no
  Terminal route or endpoint work from any rejected case.
- Verify the inclusive skew edges for all message types, observations one
  second outside each edge, exact signed expiration, a monotonic send
  attempt past 30 seconds, a local wall-clock step, and a reordered frame
  whose residence timer runs out. A dropped record must never renew a lease.
- Inspect sender `Sent` wire bytes after publication acknowledgement and
  after reopening the sender before cleanup. Verify delayed own echoes in
  both windows, then verify room retirement removes the charged records and
  an old echo cannot enter the replacement room.

The deterministic KDF and room dispatch smoke is the routine CI subset.
Run the full offline SQLite and MLS proof on Linux amd64, Linux arm64 and
macOS arm64 using the qualified self-hosted test lane or an equivalent
native host. Record the actual targets and signed-wire, row and page
inspection results in sanitized evidence. Exit requires human approval of
the storage boundary, separate Claude design and integrated-code approval,
positive-control scans, successful terminal-only routing and confinement,
verified pinned egress and cleanup across both restart windows, and
complete passing native proof on the three targets. The real PTY,
tmux, live relay and latency benchmark remain in the next increment.
