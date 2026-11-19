# Exercise the active Command owner at the next effect boundary

This bounded follow-up applies the owner-exit requirements in the
[active Command design](peer-candidate-command-active.md) to the exact reviewed
first owner increment `776487b`, with evidence at `1015da4`. Track the work in
`marmish-dwt.4.12.2`. The candidate owner remains unmounted.

## Scope

1. Add a candidate-only `MdkOwner::checked_client_inventory` entry in
   `command_runtime.rs`. Return `crate::Result<Result<ClientInventoryReply,
   InventoryError>>`. Check the owner's exact admission before the current
   stock query and independently before returning its result. Keep
   `client_inventory` available to its existing stock caller. Domain errors
   (including `ServerUnavailable`) remain inside the nested result. Dispatcher
   loss remains the outer `Error::Storage("candidate_dispatcher_lost")` and
   takes precedence over a previously returned value or domain error. The
   candidate daemon caller must separately recheck before queuing and writing.
   The method is private to test/candidate builds. Its admission is the existing
   no-op on a stock owner even when a test builds both modes.
2. Reuse the current owner admission and its nested native result. Create one
   shared, initially empty observer slot in the test-only portion of
   `ActiveMode::from_startup`. The slot is retained by every owner, gate and
   publisher clone. Store a `Send + Sync` callback taking only a closed
   observation location. Clone the callback under the slot lock and invoke
   it outside the lock. Visits cover the receiver wait before each real poll
   and immediately after inner Ready before the existing final admission,
   the orphan union before each synchronous per-record admit and after
   checked native forget completion, the inventory entry and just after
   the query returns, and publication gate/publisher entry before admission.
   The callback can count, inspect already available test evidence or
   perform the fixture's real displacement. It supplies no operation
   result or authority. Stock and candidate non-test binaries contain no
   observer slot or calls.
3. Test per-record compensation on two actual solo-member native groups
   without inviting participants or calling `accept_conversation`. Verify
   both are live in the admitted owner, in the order returned by
   `native_group_union`. Capture a pre-compensation account image. Assert
   the first record reaches checked native forget completion. Before the
   second record's admission, capture its exact account and Journal families,
   displace the registered default, and verify no further completion. The
   changed native image and exact ordered visits show progress on the first
   group. The retained native and Journal families must exactly match the
   cut images after the operation refuses. Journal equality shows no
   additional writes after the cut. These journal-less groups do not
   produce an existing Journal row to update during the first forget.
   Keep the owner parked through the comparison. Take no native read after
   displacement, and avoid normal cold reopen in this assertion because
   startup reconciles the very groups under test.

## Focused checks

- Use the existing fresh-subprocess registration harness for real
  displacement. Extend it only with explicit bounded test selectors. Confirm
  that target callbacks really ran and record the reached inner poll/Ready
  visits, the first blocked next effect, and immutable state evidence captured
  before the cut. No public relay, credentials, arbitrary paths or mount.
- Exercise admitted inventory with no Command service before a cut (inner
  `ServerUnavailable`), and on a cut before the query and after an existing
  query result (outer dispatcher loss). Use the same real candidate owner.
- Exercise the actual adapter notification wait in `receive_one`. Poll it
  to Pending before a real notification can arrive, then cut on the next
  pre-poll visit. In a separate child, let a short deadline resolve the
  inner timeout and cut just before its result would normally be mapped.
  Assert that the lost dispatcher wins in both cases. Use only the existing
  refused loopback relay configuration and compare socket and byte evidence
  against the instant of displacement. Ordinary route connect can still
  create loopback tasks before the cut. The existing wrapper proof with a
  synthetic Pending future complements the real receiver-map test.
- Exercise actual `PublicationGate::publish` and the publisher's first
  effect entry with typed requests after real registration displacement.
  The gate refuses with `RetryableUnavailable` and `pre_send: true`.
  The publisher refuses before signing. Check socket and byte evidence
  against the cut. The gate's delegation-only receive surface and physical
  deactivate remain unchanged.
- Run exact candidate and stock all-target strict checks. Keep tests opt-in as
  required for the registration subprocess. Review the exact source and the
  resulting tests independently with Claude Code in Auto.

Successful inventory with a started Command service, a cut between real
key-package prepare and prepared publish, the journal maintenance and pending
cleanup loops, and any reachable invalidation reconciliation remain open.
Whether connect, republish, activation and drain reach Pending under a real
private relay remains to be proven. Real WSS ingest, native-pending acceptance
versus replay ACK, native room creation follow-through and byte-exact retained
Welcome and application publication restart remain subsequent gates. Parent daemon composition owns request allowlisting, fatal-first
reply and status dispatch, reporting R/A, and physical custody. This local
owner cut grants no new ordinary Command, Terminal or Peer authority.
