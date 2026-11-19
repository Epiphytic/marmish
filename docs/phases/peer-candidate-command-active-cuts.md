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
2. Reuse the current owner admission and its nested native result. Add one
   test-only, owner-scoped observation callback for named boundaries bearing on
   the tests below. The callback takes a closed location and no application
   or SQL data. Give each observed native operation a pre-poll visit and an
   inner-Ready visit immediately before its existing post-poll recheck. Count
   visits in the bounded test fixture. A callback may displace the default
   in that fixture. It cannot grant authority, bypass the verifier or supply
   an outcome. Stock and candidate non-test builds contain no callback field
   or calls. Retain the original candidate lease, registration and custody.
3. Test per-record compensation on two real solo-member native groups. Reuse
   the authored `CreateGroupRequest` pattern from the MDK owner group fixture
   with unique valid transport route values and no invitees, so group creation
   attempts no Welcome or network publication. Use the owner’s real
   `native_group_union` ordering to select the first group. Intercept the start
   of the second iteration and displace the real registered default before
   verifying its admission. Assert the first native forget reached its checked
   completion, the second did not run, and the post-cut account and Journal
   families match their exact images at displacement. Use actual verified
   pre-cut native counts and an independent cold readback of the uneffected
   group if it can occur after normal stock recovery without triggering an
   ordinary startup orphan cleanup. Otherwise use exact pre-cut membership
   reads plus byte-for-byte family evidence and the per-record observation.
   The test must not reopen native storage through the displaced candidate.

## Focused checks

- Use the existing fresh-subprocess registration harness for real
  displacement. Extend it only with explicit bounded test selectors. Confirm
  that target callbacks really ran and record the reached inner poll/Ready
  visits, the first blocked next effect, and immutable state evidence captured
  before the cut. No public relay, credentials, arbitrary paths or mount.
- Exercise admitted inventory with no Command service before a cut (inner
  `ServerUnavailable`), and on a cut before the query and after an existing
  query result (outer dispatcher loss). Use the same real candidate owner.
- Prioritize two actual async mappings that can run using the inert connector;
  select native account activation/recovery or transport-group sync and the
  receiver wait. Test a cut on the next poll after a real pending point where
  possible and post-Ready before normal mapping or follow-up. Record any path
  that still requires actual private WSS. The existing synthetic Pending
  wrapper fixture remains a useful primitive proof but cannot establish those
  actual call sites by itself.
- Exercise the actual gate and publisher on default displacement before any
  signing/sending. Use the existing typed minimal request and exact refusal
  assertions and confirm the native registration caused the cut. Use an inert
  connector and show no sockets or state changed after displacement.
- Run exact candidate and stock all-target strict checks. Keep tests opt-in as
  required for the registration subprocess. Review the exact source and the
  resulting tests independently with Claude Code in Auto.

Real WSS ingest, native-pending acceptance versus replay ACK, native room
creation follow-through and byte-exact retained Welcome and application
publication restart still require independently exercised cases at the next
increment. Parent daemon composition owns request allowlisting, fatal-first
reply and status dispatch, reporting R/A, and physical custody. This local
owner cut grants no new ordinary Command, Terminal or Peer authority.
