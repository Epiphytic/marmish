# Authoritative native leave from terminal Peer custody

A manually declined or timed-out Pending invitation already has an immutable
local fence. This increment qualifies one native leave from that fence and
requires independent observations of the recipient's removal before it can be
recorded as confirmed. It builds on the
[terminal custody design](peer-terminal-custody.md) and isolated
[SelfRemove recovery proof](../validation/peer-pending-unrouted-leave-proof.md).
Use the integrated upstream master lock and two disposable SQLCipher-backed
native accounts carrying the genuine Peer Welcome. Start with the test-owned
terminal profile and a controlled in-process sink. The deployment and live
Peer transport qualification remain separate.

## Binding at the closed terminal boundary

- Open the terminal checkpoint through the exact profile owner and check
  class, generation, founder, account, Offer revision, Delivery and Welcome
  digest, stage path and inode, group, stored route, retained local leaf and
  terminal reason. Require Fenced, matching held stage and no alternate room
  owner. Treat Intent, FilesRemoved, Released and Blocked as closed. Leave uses
  a separate exclusive terminal turn which cannot activate or drive a normal
  pending invitation. The default no-egress owner can still retire locally.
- Sequence the shared schema after the private WSS activation increment.
  Bump the integrated custody epoch from 4 to 5. A fresh supported Prepared
  Consent inventory can install it. Reject epochs 3 and 4, partial inventories
  and mismatched guard definitions before mutation. Retain the activation
  inventory and run its opt-in tests and the epoch-3 no-egress selectors at
  epoch 5 with no hold. Begin implementation after M5, with an explicit
  integrated recheck of the activation inventory, terminal guards and due
  query against this design.
  Add one `peer_terminal_native_leave` row keyed by the terminal
  `(class,generation)`, with explicit state, immutable bound account,
  stage witness, route, clock deadline, evidence columns and closed guard
  writers. Include RecoveryParked in the state enum, `recovery_attempts`
  constrained to 0 through 3 and a `recovery_not_before` wall timestamp.
  Ordinary recovery writes increase attempts and move the floor forward.
  Only the explicit owner-recovery writer may leave RecoveryParked.
  Insert the row in the same IMMEDIATE transaction as the exact
  terminal decision when the test-only reconciliation plan is enabled.
  For each terminal row without a hold, preserve A's existing immediate
  fallback. Store seconds throughout. Use checked arithmetic for the exact
  immutable `hold_deadline = decided_at + 86400`, with SQL checking equality
  and `hold_deadline > decided_at`. Expiry is `wall >= hold_deadline`.
  Use this condition for the cleanup filter and Unconfirmed writer. Require
  Fenced and a qualified wall strictly below it for each leave turn and the
  Removed writer.
  Validate local and exact origin floors. Stop, cancellation, rollback,
  reconfiguration and deadline deny publication. The complete joined
  reservation remains charged until the A release sequence.
- Filter an unexpired hold from `terminal_cleanup_due` and disallow a
  live hold's unconditional Intent or Blocked shift in the same SQL
  journal. Additionally check eligibility at `retire_terminal` entry.
  A holder can advance only a correctly bound witness mismatch to
  Blocked, or use sealed Removed evidence for Intent. Serialize native
  leave and cleanup through one owner lease with one in-process stage
  token, with an explicit no-active-stage check for retirement. At
  expiry choose Unconfirmed only through the shareable wall-guarded
  Intent writer, never through a separate filesystem path.
- Use the existing stage descriptor and quota checks, private one-link
  original SQLCipher file and DELETE journal. Keep a single mutable runtime
  for this stage under the local profile owner, close before any file
  retirement, and recheck stage identity and retained terminal generation
  after each native boundary. Dedicated terminal ingress accepts only
  one authenticated kind-445 event per retained commit slot from the exact
  source, route and group. Cap the candidate at the prior 64 KiB bound,
  verify canonical outer ID, signature, and route before MDK ingest, and
  deduplicate by stored exact ID and digest. Deny other candidates. Drop
  application effects. After an interruption with a DELETE journal,
  recover the exact stage under the same owner, checked descriptor and
  quota before retirement by reopening SQLCipher and closing. This is
  mandatory even at deadline and grants no transport authority. On
  failed recovery retain the journal and charge, with no deletion of
  unverified sidecars. Run this before the existing retirement capture.
  A lingering non-hot DELETE journal or recovery I/O interruption leaves
  the Fenced row retryable and charged. These outcomes never arm or enter
  Blocked. A later turn opens only the exact protected stage and uses one
  owner-authorized write transaction. In DELETE mode and with durability
  settings verified, execute `PRAGMA user_version = N` inside an IMMEDIATE
  transaction with `N` equal to the exact, validated existing value. This
  rewrites the header through SQLite's ordinary rollback-journal protocol
  without changing the schema version or application data. Close the handle,
  recheck the original inode and sidecar set and proceed to capture only
  when the DELETE journal is absent. Prove that the same-value transaction
  exercises the journal before enabling this path. An unrelated replacement
  or unexpected WAL/SHM remains a witness mismatch.
  Persist a recovery attempt reservation before opening the stage under
  the terminal and journal guards. Set `recovery_not_before` in the same
  transaction to reservation wall plus 1 second after attempt one and
  plus 2 seconds after attempt two. A reservation interrupted before open
  is spent and retains the floor. Run at most one due recovery in a daemon
  turn. Exclude future and parked rows from both native and filesystem
  cleanup selection. An unsuccessful third attempt, including interruption,
  parks the charged Fenced row. Explicit recovery is a test-only operation
  invoked by the held fixture owner with the exact terminal selector. It
  does one local recovery under the same descriptor and quota checks.
  Failure preserves the parked row and does not reset the counter. Success
  advances to the next eligible guarded step with the spent counter
  unchanged. Handle a recorded recovery failure as a completed bounded turn
  so expiry and cleanup of another class still run in that daemon tick. At
  or after hold expiry any attempted recovery is local-only. Failed recovery
  still precludes Intent and file deletion. After an explicit verified
  recovery the ordinary path may commit only Unconfirmed at that deadline.
  The ordinary runtime,
  activation, history, receipts, status, KeyPackage and unrouted stage paths
  grant no capability to this turn.

## Exact Proposal and founder convergence

1. Read same-stage native authority through a read-only view. Require stable
   current branch and epoch authenticator, caller member and non-admin, exact
   own MLS leaf index and signing key, correct founder/admin roster and
   route, no disband, pending commit or queued convergence. A durable
   deferred PostJoin obligation may remain from the joined stage. It is
   never polled or permitted here. If the exact leaf
   was already removed and the native reference and identity remain valid,
   continue to removal verification and record its cause independently.
2. Capture a single native Leave through a closed adapter. Reuse the existing
   `SendIntent::Leave` API and the SelfRemove verifier. OpenMLS must identify
   current-epoch authenticated Proposal by the recipient leaf while
   verifying the outer Nostr signature, retained exact Sent row, group
   context, local leaf credentials and durable LeaveRequest. Check failures
   as pre-send; no transport acknowledgement can establish native removal.
3. Use the indexed exact message ID after it is recorded. This specific
   isolated re-staging proof advances the earlier test boundary which
   only recovered the denied candidate. The bounded Sent scan and
   re-staging remain test-only and non-installable. The exclusive owner
   forbids concurrent active stages and primary operations. It checks
   the verified stage before and after deny-only seeding. Allow the
   sequence LeaveRequest/Sent durable before candidate lookup, candidate
   evidence committed before fanout stage, then frozen readback before
   any physical permit. Restart at each cut uses the exact checked
   state. Before the ID is recorded,
   the fixture may scan only its independently bounded current-epoch
   Sent inventory. Refuse rows of unknown size,
   shape, routing or ownership and any ambiguous candidate. If no fanout
   exists, seed precisely one `PublishWork::Proposal` under the closed
   adapter. Cap both the serialized stored candidate and the signed
   Proposal at 64 KiB for this fixture. Refuse unrelated or duplicate
   native fanout and outbox work before bounded public
   `resume_outbound_fanouts`. Never mint a replacement proposal in the
   same epoch. Egress permit binds account, stage, terminal generation, expiry,
   full epoch pin, exact signed bytes, ID, proposal, Group target kind,
   exact room route, single required acknowledgement and sole frozen
   endpoint. The guard compares the normalized request including route
   before consuming the one use. Store verified candidate and raw digest,
   permit generation, started/uncertain/accepted state and bounded
   attempt count in the encrypted leave row. A crash after started is
   unsure. Within the current hold, controlled retry recovers the same
   signed object and uses a new permit. Allow at most three attempts for
   this captured Proposal, at least 1 and then 2 seconds apart and never
   before the retained native fanout due time. The same signed object
   retains its native retry status. Keep at most one
   captured Proposal and one 64 KiB signed founder commit per hold.
4. Ingest the exact proposal into the founder's real MLS runtime.
   Drive bounded native pending convergence and auto-publication
   work. Accept only the native auto-commit from that queued
   SelfRemove, never a separately issued admin Remove. Use the public
   native session convergence/publish path with the fixture adapter
   retaining exact emitted signed bytes. Process its typed delivery report
   via native confirmation or failure and require a stable founder
   successor, exact processed proposal transport ID, and exact pre/post
   roster. Use a six-second convergence bound and 20-ms polling.
   Before delivering the commit to the recipient, durably seal the
   proposal evidence, both founder pins, named leaf, exact signed commit
   ID, digest and bounded bytes. Founder observations are fixture-supplied
   supplementary evidence. They cannot create a confirmed removal outcome.
   The recovery record survives MDK clearing the recipient's LeaveRequest
   and fanout.
5. Deliver only that sealed commit through the dedicated ingress.
   Observe `LocalGroupCopyTerminated` on the uninterrupted path.
   Its process-local voluntary flag is supplementary. Across reopen, use
   the encrypted pre-ingress record, native exact processed-transport
   marker and a new terminal read-only view of durable
   `group_authority_view` facts and the selected group record. Unlike
   `read_joined`, it can read a removed copy. Derive the expected successor
   from the recipient's own pre-ingress epoch pin and the processed native
   commit, with the sealed founder pin as a cross-check. Require that epoch
   and `removed && !is_member && !account_listed` for this
   two-account fixture and no disband or unrecoverable state. Disallow
   restored membership, stale branch, different leaf or unprovable
   successor. If the exact independent native evidence supports loss
   but cannot establish the leave's cause, record the separate
   independently removed observation.
6. Update the epoch-5 terminal outcome constraint and guarded Intent
   writer for a distinct Removed value only from sealed, currently valid
   authority. Validate the leave/terminal join, evidence nullability and
   matching stage across the complete checked inventory. Keep the
   selected outcome immutable once Intent commits. Late native evidence
   cannot change an Unconfirmed decision. At or after the exact hold
   deadline only Unconfirmed may commit, including when removal evidence
   is present but the Removed Intent has not yet been committed. Sink
   acceptance by itself advances only the delivery ledger. In the same
   exclusive owner turn, open and recheck the recipient's authoritative
   terminal snapshot, then close the stage. Mint one non-cloneable token
   from that closed readback carrying the immutable sealed row identity,
   current clock and stage witness. The Removed Intent writer consumes it,
   compares the exact local SQL evidence and fresh wall inside the
   transaction, and writes only if the hold and stage still match. Then
   use the established descriptor-relative retirement and atomic
   reservation release. A held stage without authoritative removal can take
   the Unconfirmed path at its bounded deadline. Repeated decisions preserve
   original reason and evidence. All file witnesses and replay fences stay
   authoritative after a crash or replacement.

## Acceptance and measurements

- Run fast cases for wrong event, target, group, member, leaf, digest,
  epoch, signer, generation, reason and deadline. Verify denied unrelated
  PostJoin, app, Welcome, commit and KeyPackage publication. Test frozen
  retry, two attempts sharing one permit, dropped reply, expired hold,
  restart with prior delivery, and the distinctions between Removed,
  restored, disbanded and unconfirmed. Include the existing no-egress
  decline and expiry scenarios and use the existing core selectors.
  Test the daemon and exact SQL writer against a fresh hold and an
  expiry tie with a sealed removal observation. Check due selection and
  both Intent writers at deadline minus one, the deadline and deadline plus
  one. Extend the exact epoch inventory test with the added table, state
  constraints and changed due-query join. Exercise pre- and post-
  journal crash cuts, including the non-hot residual, same-value header
  rewrite, I/O failure and interruption between recovery reservation and
  open. Verify a persistently failing row is parked,
  never starves the other class or new expiry, and remains charged when
  recovery fails after its own hold expires. Exercise successful explicit
  exact owner recovery after parking. Explicitly reject epochs 3 and 4
  before mutation.
- Use an opt-in joined-custody two-account test. Exercise at least native
  leave committed before fanout, frozen fanout reopened, after proposal
  handoff with current membership still present, matched removal applied
  by both devices, restart with confirmed removal, and file release.
  A second case with the founder unavailable keeps the prior exact work
  unconfirmed and exercises bounded expiry using a controlled wall.
  Qualify the retired room authority snapshot at the successor epoch,
  persistent exact processed marker and removed flags after reopen before
  enabling the positive outcome. Cut after each durable evidence step.
- Record workstation and source lock, decision-to-capture,
  decision-to-accepted proposal, decision-to-stable removal, reopen and
  exact file release timings and peak protected storage. Record whether a
  fixture cut or actual process interruption was exercised. Publish no user
  identifiers or raw transport bytes. Obtain independent Claude Code Auto
  design and integrated source review.
