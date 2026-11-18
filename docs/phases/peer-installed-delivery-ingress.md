# Installed recipient Delivery and mixed history

This Linux AMD64 library fixture composes checked Offer intake, the same
recipient owner that publishes Prepared Consent, the bounded cursor and the
installed v2 custody checkpoint. The output is durable `Held`, with the native
package still unconsumed. The [recipient daemon design](peer-installed-recipient-daemon.md)
defines the separate startup and release requirements. The capped join bridge
consumes the resulting cold `Held` evidence.

## Exact prerequisites and ownership

The Consent publication prerequisite is the approved design
`b9ba9076543e5bd73344bd1e9801c43b7228d6b8`
(`docs/phases/peer-installed-consent-publication.md` in that tree).
Its implementation candidate `3b8308f7b` places
`PeerRecipientPort::publish_prepared_consent` in
`peer_installed_consent_port.rs` and the exact durable ACK compare-and-set in
`peer_installed_consent_settlement.rs`. It returns unit `Settled` only after
the compare-and-set. Integrate only after that prerequisite lands with its own
exact review and tests. At implementation, link the checked-in prerequisite
plan and verify the final API against the landed source.

Use the same profile, physical origin, checked recipient port, live role,
identity, checkpoint instance and its bound cursor `Source`. Require
`InstalledCustodyV2`, Manual, and exactly one normalized direct WSS origin.
No other Peer policy, daemon entrypoint, CLI, installer, schema or wire change
belongs here. Keep the legacy `receive_custody_record` and native custody
transition refusals for this layout. Mount the native adapter only in the
Linux AMD64 test library using the same loader pattern as
`native_offer_ingress_fixture!`. The standalone cursor crate defines a
corresponding no-op macro. Shared cursor methods may accept synchronous SQL
fence callbacks and existing cursor types only. No shared source imports a
port type.

## Observed and saved authority

1. **Physical input.** Use `PeerRecipientPort::use_live_carrier` or
   `use_page_carrier`. Reuse the existing checked account, route and exact
   lexical bytes, plus the raw ID and stamp. Classify verified outer and
   authenticated client content once for a synchronous observation. Share the
   checked Offer admission behind that classification; avoid calling its
   whole public fixture wrapper and decrypting again to discover Delivery.
   Cold and fresh authority rechecks remain independent when needed. Neither
   decoded JSON nor caller-supplied bytes can serve as a port witness.
2. **Across native inspection.** The first callback returns a private
   non-Clone, owned prepared observation for a Delivery, bound to the exact
   port incarnation and observed page request/bounds/index or live record
   identity. Include account, origin, checkpoint/Source owner identity,
   ID/stamp/frame digest and owned authenticated incoming Delivery, selector,
   exact immutable Consent and native stage coordinates and deadline. Drop the
   callback borrow and SQL transaction before the bounded sealed package
   inspection. Then reenter through the same observation method on the same
   physical input to do the disposition, comparing all coordinates, exact
   frame digest and a fresh trusted wall. Recheck original native stage
   identity/digest after inspection. Use the immutable parts of the verified
   Consent and stage only. The token is consumed in this second callback.
   A socket retirement, expired observation, owner change or changed stage
   at this seam stops the turn. A conclusively stale signed frame observed
   under healthy authority takes the stable invalid path before inspection.
   No native handle, copy guard or transaction crosses a network await.
3. **Stored retry.** Add a separate Linux-test-only port callback
   `use_saved_delivery` that takes only an opaque revisioned snapshot
   from the current cursor store, checks current physical scope and samples
   wall. The owner creates the snapshot wrapper only from `due` on its own
   exclusively borrowed cursor. A private owner incarnation binds that
   wrapper to the checkpoint and `Source` across native inspection. Verify
   that identity and the current durable revision on reentry. Its private
   constructor creates a distinct `CheckedSavedDelivery`. Bytes come only
   from that snapshot; there is no conversion from `RawRecord` and no
   conversion to observed `CheckedCarrier`. Recheck exact store ownership,
   origin, auth `FlightVerified`, prerequisite, revision/stamp/digest,
   metadata and signed ancestry before disposition and in mutation transactions.
   The old socket may be gone. The new port must be independently qualified
   for the same checked owner and origin. With the matching Consent still
   unsettled, use fenced `retry_again` without a native package read. With
   settled Consent, redo the exact bounded native proof and fresh saved
   observation. The Held transaction checks that exact snapshot is still
   current and atomically removes it. Stable invalid or expired saved
   records use `Snapshot::reverify` and fenced `finish_retry`. They never
   produce a page receipt or call `commit_invalid`. For expired records,
   rederive the original signed expiry from the exact retained authenticated
   ancestry and recorded verified bounds at their valid historical time,
   then compare to the fresh rollback-checked wall. Historical verification
   grants retirement only. Unverifiable local ancestry stops the turn.

## Exact dispositions

The observed adapter returns `Receipted(Disposition)`, `RetireRelay`, or
`Stop` with a fixed categorical reason. The first contains the exact
cursor receipt already obtained in the transaction. The mixed dispatcher
must obtain invalid receipts itself for observed Offer `Invalid` and
`NotOffer` when classification proves one of the stable cases below.
It cannot use an Offer result alone as proof. Saved retry has a distinct
result shape and no page receipt.

| Outcome | Condition and effect |
| --- | --- |
| Observed invalid | Complete indexable raw record with statically bad signature, envelope, decryption, authenticated kind, signed origin/version or peer-policy/time binding; a proved absent retained Offer under verified local inventory; or authenticated Delivery conclusively inconsistent with an intact exact Consent or whose signed package reference mismatches an intact exact local package. Use fenced exact `commit_invalid`. A different authenticated carrier competing for the same already deferred or Held Consent is also invalid if immutable ancestry is independently verified. Preserve the first record. |
| Relay contract, retire | ID/stamp contradiction in a parsed record, conflicting lexical renderings for one ID (observed in the page, retained in the deferral or saved as Held), impossible bounds/order/ties/count or physical framing failure. Preserve cursor revision and existing row effects. Issue zero receipts for the offending index. |
| Stop | Local corruption or indeterminate ancestry, invalid or missing expected stage, owner/role/config/route/clock failure, unavailable receipt, stale snapshot, transactional failure or true Backlog. No receipt for the affected index and no page progress. Keep valid earlier row effects available to exact replay. |

`Arrival::parse` can reject grammar or size before producing an indexable
record. Preserve existing strict framing. Bound the relay retirement and
retry for that failure and for conflicting renderings under the same
owner/origin, using the anchored clock and capped reconnect backoff (1, 2,
4, 8, 16, 32, 60 seconds). The in-memory owner/origin counter survives
socket replacement and successful authentication. Successful cursor progress
resets it. A process restart resets the counter and preserves the cursor.
Already scheduled saved retries may run on a genuinely qualified port
under the same finite `ServiceTurn` budgets. Repeated malformed history can
block that origin's catch-up until the record ages out of the 72-hour
lookback. Test two repeated failures without a hot loop, a fabricated receipt
or page progress.
Keep the existing `commit_page` count/tie quarantine behavior for proven
page-contract violations. Respect active quarantine for Consent and Held.
Do not add broader quarantine for parser failures or arbitrary invalid
records. A stale physical capability stops without mutating the cursor.

## Transactions, limits and wake

- **Held and replay.** Verify saved v2 Offer/Consent marker and exact local
  signed ancestry before checking the stage. Reuse
  `verify_custody_delivery`, installed Consent's bounded sealed package
  attestation, and `commit_custody_held`. Reacquire the current signed
  consent snapshot after observing settlement so its existing whole-row
  comparison still works. Keep package digest, delivery frame digest and
  stage inode bound. The exact Held replay path compares lexical bytes and
  replay window and returns its receipt for a current observation only.
  A saved retry can remove its exact pending snapshot with Held, without
  an observed receipt. No native Welcome ingest is authorized.
- **Deferred.** If the exact Consent was unsettled when read, use
  `Auth::FlightVerified` and its verified Consent carrier prerequisite.
  Within the same `defer` transaction, compare immutable owner, layout,
  cursor Source, signed Offer, exact frozen Consent frame/transcript/carrier
  and exact native stage/artifact. If ACK was set since preflight, accept
  that advancement without reusing the whole-row Held comparator. The
  existing 1-second first retry closes the race when wake ran before the
  deferral write. In the transaction, an identical record is idempotent.
  Allow at most one FlightVerified deferred Delivery per origin and exact
  Consent carrier, querying that prerequisite under the same transaction
  before applying the 256-row/16 MiB caps. A distinct otherwise valid
  competing Delivery gets an observed invalid disposition, without removing
  the retained record. If the first record later expires, its competitor
  was already receipted invalid and is unavailable through that receipt.
  A different rendering for the same outer ID retires
  as a relay-contract failure (including `defer` Conflict). Never use the
  replacement path. Require calculated expiry strictly in the future. Use
  the earliest applicable signed delivery, replay, Offer, Consent and exact
  package/artifact upper bound, subtract the existing 300-second margin
  with checked arithmetic, and clamp to the existing 90-hour outer horizon.
  A search for invalid/expired candidates cannot sweep concurrent or foreign
  deferred rows.
- **Shared transaction seams.** Factor `CursorStore::defer` and
  `commit_page` to accept a single synchronous owner fence in their current
  transaction. Existing callers keep their behavior. Use the same narrow
  pattern for invalid, retry/retirement and wake mutations exposed by the
  installed adapter. `with_wall` and existing cursor checks cover cursor
  wall, revision and scope. The owner callback requires `Context::verify`
  and `rows::advance_wall` within that transaction, including the checked
  Consent ancestry where the mutation references a Consent. The page callback
  verifies the exact owner context and still-present Deferred receipts,
  rather than selecting a single Consent for the whole page. In the existing
  committing count/tie quarantine branch, verify the owner and advance its
  wall before setting quarantine in the same SQL transaction. The existing
  owner check refuses when it finds an already-active quarantine. Roll back
  cursor wall, consent wall and row/page changes together on failure. Keep
  the live observation borrow until the synchronous transaction returns.
- **Wake.** Extend only the integrated test publication boundary with a
  private single-use settlement witness created after successful durable
  ACK CAS. Bind it to the current port/owner, `Source`, normalized origin,
  exact Consent carrier and sampled settlement wall. A narrowed post-settlement
  owner method consumes it, samples fresh wall, rechecks durable ACK
  at or before that wall with exact Consent ancestry and owner fence, then
  calls the existing prerequisite-keyed cursor wake under the same
  transaction checks above. The witness gives no authority to write Held.
  If wake fails after CAS, preserve settlement and record only the wake
  failure. A crash or failure falls back to the existing 1-to-60-second
  retry path. Negative, late, wrong and uncertain ACK results cannot wake.
- **Page completion.** Process at most the existing four records in order
  and collect exact receipts. Reobserve the same physical page before
  `commit_page` and compare owner, request, origin, bounds, order and exact
  bytes; preserve the cursor's existing rules for revision, ties and count.
  Page transaction must see every Deferred receipt still present. Serialize
  settlement/wake and saved retries between page turns, so they cannot
  remove a deferred row before the active page receipt check. Hold the sole
  exclusive owner turn across the page, including native inspection, and
  require the same exclusive turn for settlement/wake and saved retries. Use
  existing finite history/retry scheduling. A partial sequence commits only its
  successful row effects, which are replayable from the unchanged cursor.

## Acceptance

- Demonstrate private qualified TLS/NIP-42 loopback with independently
  signed founder Offer, Consent and v2 Delivery. Exercise checked live
  and mixed history containing another founder's Offer, a stable invalid
  frame and early Delivery with exact lexical bytes. A relay fixture
  retains the exact Consent and admits the founder's authenticated Delivery
  from it while dropping the first Consent OK. Retire the sender socket
  and use a fresh qualified history cut to commit Deferred while the
  Consent is still queued. Cold reopen SQLCipher. Permit the existing
  real retry interval and deliver a valid correlated ACK for the exact
  same-byte Consent resend on a new port. The exact saved retry must
  commit Held, atomically remove that snapshot, and preserve signed
  parent and stage/package in cold readback. Replay once through history.
  Compare measured receipt-to-defer, ACK-to-Held and cold readback times
  and sizes from the bounded local run.
- Exercise unrecoverable grammar/size failure before any Arrival, a bad
  decrypt/indexable wrong selector that still permits page progress,
  conflicting renderings, same-prerequisite cap, wrong package, expiry and
  retry at its boundary, changed route/account/clock, corruption,
  stale/foreign retry snapshot, owner retirement between inspection and
  commit, ACK-between-inspection-and-deferral, wake loss/crash, negative
  or stale ACK, missing/extra receipts, real cap rollback, and no hot-loop.
  Observe exact rollback of both SQL wall floors and unchanged page
  revision where required. Verify no native package inspection while an
  unexpired retry waits on ACK, no Welcome poll or membership changes,
  no Pending, Group use or application publication, and no installed
  fixture entrypoint. Use bounded native readback or audited counters to
  prove the package stays unconsumed. Count decryption calls for one mixed
  observation to pin the single classification pass and verify the exclusive
  owner scheduling with a turn-order test or a compile-time borrow check.
- Run formatting, strict all-target Clippy, standalone cursor and ordinary
  transport tests on exact reviewed code. Keep private WSS cases opt-in
  with no change to GitHub Actions. Obtain independent Claude Code Auto
  design and integrated code review.
