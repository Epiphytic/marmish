# Capped Pending refusal evidence

This addendum to [the capped Pending bridge design](peer-installed-capped-pending-bridge.md)
covers the refusal matrix from independent review `e3c8b15e`. Its source
baseline is accounting commit `7d725d0` on `d9ea132`. The proof remains
an opt-in Linux AMD64 test. Reuse the private NIP-42 WSS run and its existing
Held, JoinStarted and cold Pending subprocesses. Run focused pure test cases
only where the live path cannot construct the negative input by design.

## Cases and observations

For each executable case, take a fresh `Effects::read` snapshot immediately
before the operation under test and read it again after the typed refusal.
Assert deltas for actual `JoinTurn` future polls, closed native success and
committed custody rows. Check the retained stage/custody and existing relay
publication counter for cases touching the real Held row. Report proof
boundaries rather than combining zeros from unrelated tests.

| Required refusal | Exact exercise and expected effect |
| --- | --- |
| Legacy or uncapped witness | Source inspection demonstrates the private constructors and by-value `ArmedWelcome` plus quota identity guard prevent a legacy/uncapped native result from entering either installed CAS. The current installed checkpoint policy also explicitly requires a v2 Offer and Manual inactive custody. Test a real v1 Delivery at the local negotiation guard if constructible with the existing signed fixture without introducing a bridge constructor. A legacy fixture passing its own native test establishes no installed effect. |
| Wrong signed ancestry and generation | The existing child measures wrong generation at Start. Extend its same genuine Held path with a changed signed Offer revision selector and reread the exact Held. The adjacent `peer_installed_delivery_recovers_after_lost_ack` case rejects a competing authenticated carrier and a signed Delivery for a swapped package. A changed selector alone proves exact row selection, while the competing signed carrier case proves authentication and ancestry mismatch at ingress. |
| Wrong owner, lease or family | Exercise a foreign lease against the original `ReceiverQuota` at `PendingReceiver::open`, without a second quota registration. At the native boundary exercise the existing measured CleanFiles begin refusal using the wrong family (or changed physical stage identity on the existing Held file, restored before positive Start). For a foreign checkpoint owner exercise exact current Held evidence in a freshly reopened checkpoint's local owner check. All refuse before any Welcome poll or custody transition. |
| Deadline and route or role loss | On an actual retained Held, require refusal at its exact signed effective deadline with existing fixture wall or a narrow test-only internal clock argument to the existing fence and checkpoint read. Use real port retirement and repeat actual Start with that port. If the existing fresh physical/config observation can be invalidated by changing only the owner-controlled receive config, isolate a role-loss example from a fresh same-origin scope before Start. Do not conflate the already-tested explicit stop flag with role loss or time expiry. |
| Missing or mismatched negotiation | `Delivery::status_negotiation` is absent for v1 and derives the v2 coordinates solely from authenticated Offer/Consent/Delivery. Test mismatch by comparing the exact sealed Held against an independently otherwise valid signed competing v2 Delivery or by asking the existing guard to reopen independently signed but mismatched retained transcript. Do not mutate the private typed Delivery or create an alternate source of negotiation. |
| Swapped or unconsumed package | Cite the signed swapped-package ingress test plus its `Invalid` result and unchanged custody. The bridge's cold pre-effect and capacity cuts already test zero polls and no qualifying Pending from an unjoined image with an unconsumed KeyPackage. They do not isolate the native consumed-bit predicate. If needed, give the real joined artifact reader a disposable SQLCipher snapshot carrying only a cleared native consumption field, through existing storage APIs, then call the same checked readback and assert no qualification. |
| Wrong roster or epoch on cold readback | On cold Pending, use the normal checked readback against a deliberately wrong stored expectation of epoch/identity in a disposable fixture row and assert exact mismatch fails with no rewrite and zero Welcome polls. The first JoinStarted readback gets its epoch pin from native storage and has no separately stored expected pre-join epoch. For roster, the native checker requires an active exact two-member roster, exact founder and local leaf, and exact sole founder admin. Exercise its shared checked predicate on an independent MDK alternate-roster or wrong-founder witness if reachable without adding native authority. A self-invented group ID does not test the roster. |
| Uncertain after Start | Reuse the positive joined round, or add one bounded scenario only if necessary. Run the actual original MDK Welcome once and close its scoped VFS. Inject loss of its result at the final native-to-custody handoff for one call after the checked native closure, so the bridge caller observes an error and cannot report success or move to Pending. Measure one real poll and one observed closed native success below the handoff, zero custody writes or relay publication in that window, durable JoinStarted and refused rearm. Reopen cold with no old witness and qualify only if the independently checked native state exactly matches the founder. This is ambiguous to the custody caller. It does not claim the inner MDK future returned Pending or the VFS failed. The existing pre-effect process cut and real quota refusal retain their separate evidence. |

Keep case inputs narrow and never add a public mutation API, installed mount
or transport permission. Test-only helpers may be local children of the
existing private receiver/fixture modules. Use existing clock, SQLCipher
and native verification seams. A direct test of a dynamic guard may satisfy
one matrix row where a full WSS replay would only repeat ingress. Count
zero only when the real tested operation is invoked and instrumented. Any
unreachable row must distinguish type evidence from runtime behavior.

Managed rollback is qualified independently by
`managed_recovery_hot_journal_and_stock_wal`, source `6a8846ec` and
[measured evidence](../validation/peer-receiver-charge-recovery.md).
It executes journal-sync/main-write interrupted native SQLite and keyed
playback using the same managed quota/owner. Preserve that prerequisite and
do not describe the WSS fixture's clean process exits as hot-journal recovery.

The accounting owner retains the existing WSS file and counters. A small
one-line child module mount and call sites can be composed with that owner;
a test author owns the new sibling file and any narrow test-only seams.
Re-run only the selected WSS and pure negative cases plus focused strict
installed/stock checks, with an exclusive warm Cargo target. Get a fresh
Claude Auto review of the resulting code and observed measurements.
