# Durable Peer Offer and Consent checkpoint

Parent design [Peer live enrollment](peer-live.md). This increment qualifies a
client-side checkpoint in the existing SQLCipher cursor test target. Inputs
are exact raw records from a trusted, already authenticated origin owner and a
local client signer. Outputs are cursor dispositions, a durable invitation and
decision, or immutable encrypted Consent outbox work. Raw WSS connection
handling, native stage custody, relay ACKs, Delivery, application acceptance,
daemon and CLI integration remain separate increments.

## Authority and inputs

Use the existing hardened, keyed cursor connection and immediate transactions.
Attach the checkpoint to the cursor's exact client account and origin set.
Build the registry from canonical explicitly configured WSS URLs validated by
the existing relay parser, with 1–16 distinct entries. Bind each URL to a
version-domain SHA-256 origin ID. A private source witness belongs to this
profile, exact canonical URL and current in-process owner. Validate it against
the registry before parsing. The fixture supplies it in place of the reviewed
raw socket owner. Wrong-owner witnesses yield no cursor receipt. This offline
fixture leaves OS credentials, definitive socket authentication, qualifying the
clock and obtaining the profile lease to the production integration.

After validating the witness and raw byte cap, decode the outer record as a
Nostr Event and verify its signature and computed ID. Compare its ID and
randomized stamp with the raw index. The Offer codec then strictly decodes the
same bytes, verifies the outer, seal and rumor, and requires the exact
configured URL. Its carrier ID must match the independently verified outer ID,
which commits the outer signer and timestamp. An unparseable or invalid outer
is terminal-invalid. A parsed index that contradicts socket attribution yields
no disposition. Check distinct server and client identities. The checkpoint represents the client
direction only. A caller supplies an independently qualified wall for each
operation. Check the cursor origin's existing high-water mark and a persistent
checkpoint-wide monotonic wall in each authority-bearing transaction.
Advance only the exact operation origin's cursor wall. The checkpoint wall
never rebases. A cursor rebase can fail closed at the owner layer until fresh
qualified wall meets its persistent watermark. If a trusted future time was
supplied in error, older input cannot recreate pruned authority even after a
cursor rebase. Production qualification must detect unjustified forward jumps
before calling. Exercise cursor rebase below this watermark. Use only qualified
wall for expiry sweep. A sweep touches at most 16 invitation rows, their 16
dependent outboxes and 8,192 fences. The Offer must be live at admission and decision
with expiry exclusive. Use the authenticated replay deadline from the codec for
fences, including skew and safety margin. The outer timestamp serves raw
record identity and cursor bookkeeping.

Construct local authority from the fixture equivalent of a verified local UID
matching the profile owner UID. Bind this private capability to the current
owner and local account. Prompt and remote inputs cannot mint it. Policy is a
local immutable owner snapshot. Manual is the default. Automatic is explicit
local policy and approves all newly received nonblocked verified Offers in
both capacity classes, including unknown senders, using the same approved
state as manual. Switching to automatic does not upgrade already pending rows.
The configured known or pinned set controls reserved capacity only.
Exact local server blocks take priority over modes and reservations. Off
refuses new effects and hides prior outbox work. Reopen with changed policy
rechecks current block and mode at actionable operations. Manual permits
completion of a previously durable automatic approval. The local owner can
discard matching rows even when blocked or off.

## Transactions and bounds

Use v1 STRICT owner tables with a singleton manifest, one invitation indexed
globally by the canonical signing server, bounded authenticated revision
fences and one outbox row per invitation. Persist exact origin, sender,
recipient through account binding, Offer ID, authenticated revision,
original carrier ID, signed frame, signed expiry and replay deadline.
Deduplicate by exact origin and authenticated inner revision. Different signed
outer wrappers of the same verified revision receive their own raw cursor
dispositions and share one effect. Check fences before active capacity. A
different revision from the same sender, including another configured origin,
cannot displace the outstanding row. Keep eight reserved and eight unknown invitation/stage rows.
Reserve up to 256 fences per origin in each of these two classes, at most
8,192 overall. Retain fences after discard and expiry, deleting at a qualified
wall at or beyond the exact authenticated replay deadline. At most 16 Offer frames and 16 Consent frames may remain, each
capped at 64 KiB, independently of cursor deferred capacity. Check version
and account at attach. Require an exact parent invitation for outbox
ownership. No table stores native private keys or stage paths.

After source and raw verification, call CursorStore::commit_trusted with a
synchronous owner-only transaction. Check owner wall, expire eligible rows,
insert a pending or automatically approved invitation and exact replay fence
together and remove the matching deferred record through the cursor helper.
A replay uses the same hook without reapplying the invitation. A rollback returns no acceptance receipt. The closure reports the existing
typed cursor backlog error for invitation capacity, sender contention and
fence capacity. All three yield a durable defer receipt only after the
existing cursor helper persists exact bytes and signed expiry with
FlightVerified provenance. This proof uses cursor backoff. Prerequisite
wakes are an integration step. Other transaction errors return no receipt. Keep one serialized checkpoint owner across
classification and defer. Failure to defer returns no advanceable receipt.
Re-examination runs full verification. A permanently invalid attributed
indexed record may receive the cursor's existing exact invalid disposition and
acquires no authoritative fence. A contradictory signed ID or stamp yields no
disposition. Expiry, blocked and off records receive terminal-invalid receipts. This
intentionally consumes the disposition on the current pass under local policy.
A later full scan may rediscover a still-live Offer after a policy change.
Ordinary malformed records do not mutate checkpoint rows. Separate cursor
code advances a page after durable receipts.

Require a local capability and exact selector (server, Offer ID, revision)
for user approval or discard. Resolve stored origin from that row, then
recheck exact ownership, current policy, authenticated expiry and stage
inside the immediate transaction. Use the cursor's private connection and
with_wall, without a trusted ingest callback. Look up the stored exact origin
to open the transaction, then compare it again inside. A stale selector leaves other rows and
work alone. Discard removes only that invitation and its outbox and retains
the fence. Approval of an already approved live row is idempotent. A restart
between admission and decision leaves a manually pending row. Automatic
admission persists its approval in the acceptance transaction.

## Immutable Consent

After a durable approval, allow the local owner to query the exact material
needed for that stage. Generate a disposable signed native package through the
same upstream engine and publication contract as the staged proof. Mount the
existing codec as a child of the test checkpoint, which includes its existing
tests and adds no production codec dependency to the cursor store. The
fixture generates its package separately to leave that existing codec fixture
private and avoid shared changes. Generation stays outside the checkpoint
transaction. The native owner disposes of unused package material and
reconciles a crash before outbox commit in a future increment.

Queue verifies the local capability, selector and stored exact origin. If a
live outbox entry exists, return the original bytes after commit, including
after the Offer deadline. A new outbox requires a live approved Offer. Reopen
the signed Offer, compare its persisted exact binding and prepare Consent
using the existing codec and exact package. The local package owner supplies
the desired signed Consent expiry. The codec independently bounds it by
Consent's creation plus 24 hours and signed package expiry, and requires a
currently usable package. Persist one immutable signed frame, carrier ID,
revision and expiry before returning bytes. The synchronous write transaction
performs no external I/O.

Outbound work stops at exact signed Consent expiry, exclusive. Receiver skew
does not allow retries on the sender. Retain the stage and outbox reservation
until the later of Offer expiry and queued Consent expiry, or exact owner
discard. Without queued work, retain the invitation until Offer expiry.
Shorter Consent stays reserved through the Offer deadline and cannot be
regenerated. The original Offer always governs replay fence duration.
A restart recovers the exact live bytes with absent or different input.
Pending, blocked, off, wrong-owner and wrong-selector calls return no work.
Peek uses the same guards and exact row. Queue success records a local write.

## Acceptance

- Test the same signed body under a different outer signer, a different
  revision from the same server, the wrong configured origin, unrelated
  source owner, another recipient, forged raw metadata and corrupt bytes.
- Test manual pending and exact local approval, automatic parity, reserved
  and unknown saturation, off and blocked policy, stale selectors, wrong UID
  or owner and authenticated deferred-to-accepted replay. Check fence budget
  and expiry, cursor receipt ordering, qualified-clock rebase and fence-pruning bounds.
- Deliberately abort effect and outbox writes using local SQL and reopen at
  the acceptance, decision, outbox and discard boundaries. Verify rollback
  exposes no receipt or work. Exercise panic unwind in the owner callback. Check plaintext SQLite and key and owner
  mismatches and absence of persisted IDs or frames in raw database bytes.
- Open Consent with the original remote signing identity and check exact
  native package bytes and Offer revision. Verify byte-identical retry after
  restart, invalid native material, duplicate calls and expired work.
- Report focused offline tests and an ignored peer_consent_checkpoint_benchmark for verified
  admission with durable sync, approval and outbox preparation and recovery.
  Keep all checks off public relays.
- Add one test-only child mount in the cursor store and a nested mount of
  the unchanged codec. No production entry
  point, runtime dependency, new wire format or GitHub job.
