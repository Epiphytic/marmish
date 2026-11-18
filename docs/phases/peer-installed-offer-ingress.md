# Installed recipient Offer admission

This increment of [the installed recipient owner](peer-installed-recipient-daemon.md)
closes the gap between the authenticated physical route and the installed
custody successor. The current successor rejects the generic
`Checkpoint::admit` entry. An existing fixture seeds a v2 Offer in the
predecessor and upgrades it, which proves migration and cold recovery. The
qualified owner must receive an Offer after opening the successor. This
increment covers that write and recovery. Daemon startup, provisioning,
send, Delivery, and native Group work have separate proofs.

## Interface and authority

1. Add a borrowed `CheckedCarrier` for one record with private fields and
   read-only getters visible within `peer_invitation`. Only the recipient
   port creates it, in a synchronous callback after accepting an exact
   page or live record from its active socket and reobserving the physical
   route. A page selector is an index in that page with a stamp inside the
   requested bounds. Use the authenticated account, normalized origin,
   qualified wall sample, and exact raw JSON bytes, ID, and stamp. The
   successor can depend on this narrow socket observation. It cannot take
   a caller-supplied wall, body, or origin. The port refuses foreign and
   retired-socket observations before the callback.
2. The separate successor entry requires the `InstalledCustodyV2` layout,
   same `Source` instance, matching account and raw normalized origin,
   and direct matching of `Some(ReceivePolicy::Manual)`. AutoAny,
   AutoFounders and Off are local ineligibility, even when the configured
   founder would otherwise fall back to Manual. Use `Context::verify` on
   the installed successor epoch, durable wall floors, and exact origin
   quarantine before any classified result or write. Read per-origin cursor
   wall as part of the same transaction checks. Local binding, index, and
   storage failures return an error before any durable disposition. No live
   observation supplies a reservation or decision. Set `reserved=false`
   and `decision_at=None` regardless of ordinary Command trust or the
   fixture's reserved and legacy policy-mode fields.
3. Verify the outer signature and exact authenticated index with the
   existing v2 codec. Authenticate the inner flight classification before
   applying an Offer. A verified Delivery or an unsupported, malformed, or
   undecryptable carrier returns to the later combined owner as
   `NotOffer`, with no receipt. Malformed Offer bodies also return
   `NotOffer`. Exact index mismatches return an error. For an Offer candidate, verify the v2
   signed flight and exact configured route, founder, recipient, and
   interval using the existing v2 parser. A stable Offer-policy refusal
   returns `Invalid` with no receipt. Blocked, self, wrong signed route or
   expired Offer cases are remote refusals after local scope checks.
4. A verified current Offer uses `commit_trusted_exact` on the capped
   stock cursor. Verify the exact installed successor via `Context::verify`
   and its `Epoch::verify` before the cooldown,
   installed replay sweep, and bounded `rows::admit`. Reuse
   `installed::mark` only on Applied. Recheck the successor and exact
   retained parent before commit. This sweep preserves charged ancestry and
   bounds retained unreserved Offers. An authenticated replay uses the
   existing signed replay fence and cannot rearm provisioning. Create no
   slot, legacy decision, Prepared Consent, or outbound work.
5. The narrow result is `OfferIngress` with
   `Recorded(Disposition)`, `Invalid`, `NotOffer`, and `Backpressure`.
   Only `Recorded` carries a receipt and it applies to Applied,
   AlreadyApplied, and SkippedCooldown. Return `Backpressure` only for
   the existing Backlog result, including same-founder occupancy. A
   `SQLITE_FULL` or other local storage failure rolls back and returns
   an error. Do not call `finish_admission` or `admit_installed` from
   this path. No invalid or unclassified receipt, deferred write, or
   page/live cursor advancement occurs here. A mixed page containing one
   of those outcomes therefore stays uncommitted in this proof. The later
   combined owner must classify remaining records and commit its own exact
   bounded invalid or deferred receipts for the entire page. Check
   retained rows, SQL state, cursor wall/revision and deferred state on
   each receipt-less path.
6. Scope the entry to the existing test-only successor layout. Keep the
   generic admit, sweep, approve, retirement, and legacy guards. The
   future startup owner still needs the live lease, Ready binding, exact
   files and bounded cursor inventory. This proof mounts no installed
   runtime and qualifies no end-to-end enrollment.

## Validation

Open the empty v2 predecessor layout with the existing capped SQLCipher
opener and upgrade it without seeding an Offer. Use the real private checked
WSS fixture, with an authenticated page and live record from separate
founders. Observe the v2 marker, exact retained signed bytes, and original
proof through a cold reopen. Test exact replay against unchanged ancestry.
Use one unhandled carrier and assert it cannot yield an Offer receipt.
The test may expose the existing capped opener to this Linux AMD64 test
module, with no change to the normal artifact.

Use focused deterministic cases for absent writes or receipts on foreign and
retired scope, wrong account and origin, stale source, auto and off modes,
blocked, self or misaddressed carriers, wrong signed route, expired Offer,
quarantine, and regressed clock. Test protocol negatives at their earliest
refusal, including self or wrong-recipient outer envelopes refused by the
port. Verify Unknown despite configured legacy pins and automatic mode,
cooldown with a guarded test row, replay against the same charged ancestry,
bounded row count and rollback. Lower the page cap in a disposable test
connection for a stable write failure if otherwise impractical. The
SQLCipher test's scope must leave no legacy decisions, outbox, new slot,
deferred row, or Group effects. Run strict native Clippy and nearby Offer,
cursor, and authenticated route regressions. Keep the live test opt-in and
use fast bounded tests for core coverage. Obtain independent Claude Auto
approval before code and on the exact composed result.
