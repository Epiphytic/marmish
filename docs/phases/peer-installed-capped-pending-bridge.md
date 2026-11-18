# Compose the v2 recipient's capped Pending proof

This Linux AMD64 opt-in proof is the native bridge required by the approved
[installed recipient Pending design](peer-installed-recipient-pending.md)
and [recipient charge contract](peer-receiver-charge-scope.md). Start from
`e29270e`. End with one Unknown/Manual row in the exact installed-v2
successor test layout proving it joined the founder's native group.
Application membership remains provisional. No daemon, CLI, package,
schema, wire, policy, or default CI changes belong to this increment.

## Prerequisites

- **Signed ingress and settlement.** The sibling recipient owner (Beads
  `marmish-dwt.4.4.3` and qualified v2 Delivery admission) must
  open the real checked private route, durably prepare one exact v2
  Consent, settle the matching authenticated publication under that
  same route and owner's turn, and admit the founder's exact v2
  Delivery as Held. Use `peer_invitation::socket::owner::recipient::PeerRecipientPort`
  and the installed test successor's signed replay. A fixture ACK,
  synthetic custody row, supplied raw Welcome, or native group alone
  cannot start this bridge. Require a qualified fresh owner/origin
  guard from that owner at native and SQL boundaries. Retain v2
  marker and exact status-room negotiation across replay and recovery.
- **Managed recovery and charge.** Require the sibling receiver's
  `marmish-dwt.4.4.2`
  same-lease inventory-derived clean and managed hot-journal scopes.
  The fixture must register `ReceiverQuota::bind_receiver` for the
  exact `Arc<ProfileLease>` before opening the stock recipient cursor.
  Require independent proof of pre-open admission, fixed 49 MiB
  cursor reserve, exact stage/generation/inode, main plus DELETE
  journal and copy allowance, 16 MiB closed main limit and strict
  sub-128 MiB aggregate bound. Clean scope and keyed rollback scope
  convey different authority. Use no fixture width, independent
  `ProfileSlots`, raw VFS recovery, C2 scope or borrowed Founder quota.

The existing v2 SQL cold-reopen tests and legacy capped Welcome proof
establish component behavior. They leave this composition unqualified.
Before building the integrated opt-in, record the exact reviewed
commits and tests for the two prerequisites. Preserve legacy scope and
its existing evidence. [Custody validation](../validation/peer-installed-recipient-pending.md)

## Implementation boundary and order

1. The private fixture keeps the `Arc<ProfileLease>` in its outer
   scope. Its holder in `peer_invitation::receiver_inventory`
   borrows that Arc for the full lifetime of `ReceiverQuota<'lease>`
   and `RecoveredOwner<'lease>`. Bind the quota before
   `RecoveredOwner::open` opens the stock cursor. Add a narrow
   callable turn on this private module, leaving the individual
   holder fields and recovery helpers private. Implement it in a
   child mounted from `peer_receiver_inventory.rs` with
   `cfg(all(test, target_os = "linux", target_arch = "x86_64"))`.
   `peer_invitation::prepared_consent::wss::pending_join_owner` remains the
   legacy path. Reconstruct the exact v2 Delivery from the
   checkpoint's authenticated retained bytes via
   `read_custody` and `Codec::reopen_custodied_delivery`. Bind
   the resulting opaque `HeldEvidence` and signed Delivery to
   each other by package, Welcome ID/digest, origin, founder,
   recipient and signed bounds. Hold the configured receive role
   and live route fence. A replacement process acquires a fresh
   lease and route, then checks the persisted coordinates anew.
2. Factor the existing concrete validation and snapshot mechanics in
   `peer_capped_join.rs`, `peer_stage_joined.rs`, and
   `peer_quota_vfs::dispatch`. Keep the legacy fixture constructor
   and result isolated. Introduce installed-family-only non-Clone
   prepared and joined evidence, private constructors and no
   conversion from an uncapped or legacy proof. Bind both to
   the exact leased receiver, source/selection, generation and inode,
   carrier, package, signed v2 transcript, operation and deadline.
   Tie prepared evidence to the private concrete managed receiver
   scope. Compare pointer identity of its exact `ReceiverQuota`
   instance at use, as the dispatch reservation does. Use disjoint
   scope and checkpoint borrows. Move the fixed prepare, original
   join and joined readback mechanics into the managed receiver
   scope using `RECEIVER_SNAPSHOT_NAME`. Each operation acquires
   the existing file-turn mutex. No `ProfileSlots` wrapper or
   live file turn crosses the SQL CAS.
   Prepare by inspecting a guarded disposable copy. Close and verify
   it before the SQL transition. A path, raw byte string, receipt from
   a peer, or mere presence of a group is insufficient.
3. In `peer_pending_custody_checkpoint.rs` add two installed-only
   internal writer apertures. The first borrows the sealed prepared
   evidence for the exact currently Held row and reuses the existing
   Start CAS to return a non-Clone `JoinStarted`. Recheck the same
   owner, durable ancestry, Manual mode, expiry and fresh live fence
   after preflight. The owner passes the fresh prepared evidence and
   Start witness together by value to the same receiver quota turn.
   No SQL transaction crosses the native call. Require a final live
   fence and original identity/digest check immediately before the
   original keyed open and effect. Poll the concrete no-I/O Welcome
   operation once. Withhold success on Pending, refused quota, closed
   route, mismatched file, uncertain native outcome or bad closure.
   Drop the future, SQLite handles and dispatch scope before
   returning. A durable Start attempt may have had an effect and
   cannot be reconstructed as a permission to ingest. Expiry in
   JoinStarted leaves charge intact and prevents qualification.
4. Use a new process and a freshly reacquired lease to exercise cold
   verification. In JoinStarted, permit only the sibling's exact
   managed rollback if required, followed by a separate clean
   readback. Restored unjoined state is insufficient. Copy the closed
   original under the receiver charge, then independently open and
   close the bounded DELETE-mode copy in the scoped dispatcher.
   Rebuild `Delivery::status_negotiation()` and require its sealed
   v2 result. Bind its exact app-room and receipt coordinates,
   Consent and Delivery revisions and routing into the installed
   proof alongside native coordinates; rederive and compare at
   the pending writer. Keep the app-room distinct from native
   MLS group identity. Reuse the native joined verifier for the
   exact persisted Welcome receipt, signed welcomer and purpose,
   group/epoch, expected roster and local leaf, and consumed
   package. In the fixture compare the verified group and epoch
   to the independent founder's output. Preserve the source
   main's identity and digest, check both file families and account
   for possible replica writes during hydration. A changed or
   unqualified original withholds all authority.
5. The second installed writer accepts only the joined evidence
   produced by that same fresh receiver scope. Reuse the existing
   Qualify CAS to compare the exact current JoinStarted row,
   complete v2 ancestry, native coordinates, deadline and durable
   wall floor. Read a fresh wall, owner and route fence before this
   commit. Record Pending without native calls, application/receipt
   publication or reverse Command grants. A replayed Pending never
   runs native ingestion. To report Pending after restart, rederive the
   negotiation and require fresh scoped joined readback matching the
   stored group/epoch/leaf via the existing Pending comparison. A
   failed CAS, absent stage or uncertain recovery remains charged
   and fenced. At or after the stored Pending deadline, withhold
   Pending before attempting readback, even when the retained v2
   Delivery could still decode.

Limit new entrypoints to test Linux x86_64 builds. Keep native proof
constructors private and expose their sealed results within
`crate::peer_invitation` only where checkpoint/owner composition needs
them (allow crate-local visibility solely for the existing test
path-inclusion boundary, if necessary). Legacy
`start_custody_join` / `commit_custody_pending` keep refusing the
installed layout. Witness type and scope identity exclude legacy
and uncapped evidence. Incompatible registered path families
conflict in `Dispatch::bind`. Equal same-lease paths can bind again,
so exact instance identity and the file-turn mutex enforce the
separate proof and file exclusions. No installed activation or
generic writer is opened.

## Required result and evidence

Use a private TLS/NIP-42 loopback relay. Reuse the legacy WSS Pending
test's bounded driver and cut points and use the exact supported v2
founder runtime. The founder and receiver use distinct profiles and
subprocesses because FounderQuota and ReceiverQuota exclude one another
within a process. Pass public coordinates and fixture signals between
processes. Do not transfer private signing material, cursor/stage
files, quotas or native handles. Retain only sanitized counters and
timings in evidence.

First run small cases proving refusal for the legacy/uncapped witness,
wrong signed ancestry, wrong owner/lease/family or generation, expiry,
route/role loss, and repeated Start. Include missing or mismatched v2
negotiation, a swapped or unconsumed package, a wrong native roster
or epoch on cold readback, and an injected uncertain outcome after
Start. Assert which cases allow zero polls and which allow one attempt
with no subsequent effect or retry. Then run the opt-in real WSS
sequence through exact Consent ACK and signed Held. Use a cold restart
after actual one-poll native commit and before the Pending CAS as the
positive case. Also cut after Held, immediately after durable Start
before native effects, and after the Pending commit. The pre-effect
Start cut stays charged with no retry or Pending. The Pending restart
must provide a matching fresh readback without a native ingest or
Pending rewrite. Exercise an actual managed VFS cap refusal, and replay
one exact Delivery after Pending without further native or SQL custody
effects. Use the sibling's opt-in observed journal-sync/main-write crash
and keyed recovery evidence for managed rollback. A clean process cut
alone establishes no rollback qualification. Report unavailable or
indeterminate cuts by category. Count native polls, committed effects,
SQL custody transitions and relay publications, including attributable
zeros, and verify handles are closed. Measure prepare/join/readback
separately against the existing five-second continuous turn ceiling
(including copy, cipher open and close) and tighter signed deadlines.
The following windows begin after the authenticated ingress has
durably committed Held. Receipt and application publications stay
at zero in all windows. An inbound relay ACK or NIP-42 bootstrap
does not count as Peer application egress. Track them separately.

| Window | Welcome polls | Committed native joins | Custody writes |
| --- | ---: | ---: | --- |
| Read exact Held after restart | 0 | 0 | None |
| Recover pre-effect Start cut | 0 | 0 | None |
| Start and successful poll before positive post-effect cut | 1 | 1 | One Start |
| Recover positive post-effect cut | 0 | 0 | One Pending |
| Restart after Pending and replay exact Delivery | 0 | 0 | None |
| First managed quota refusal or uncertain poll | At most 1 | 0 or withheld | At most one Start, no Pending |

A follow-up readback of an indeterminate outcome can qualify only on
the separately verified, exact cold joined state. Uncertainty alone
does not prove absence or successful join. Custody counts exclude
allowed cursor bookkeeping and independent native snapshot readback;
record their presence so zero-effect claims remain auditable.

Run affected strict Clippy and confirm normal binaries and
`installed-checks` retain the existing dormant entrypoints.
Obtain separate exact integrated code review before counting the proof.

The earlier plans already authorize this isolated composition. A
private installed daemon mount is a separate approved deliverable with
its own prerequisites and review. Adding new runtime authority, new
policy or persistence semantics, or widening exposure beyond existing
approvals requires a separate exact design and human approval.
