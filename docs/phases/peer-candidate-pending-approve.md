# Record local Peer approval as Activating

This phase extends the private Linux AMD64 receiver-candidate at `78b4d39`
from an exact, charged Pending invitation to a durable local approval. It
implements the Manual local decision in [Pending activation](peer-pending-activation.md)
after [installed Manual](peer-candidate-manual-actor-mount.md) and its near-cap
qualification. Native group egress, history, receipt publication, Active,
auto approval, Decline and leave stay closed. Stock artifacts and ordinary
Command and Terminal paths keep their existing behavior.

## Exact layout and decision

- Keep `Layout::InstalledCustodyV2` for both modes. Add an
  `ApprovalV3` kind under the installed custody `Epoch`, with a distinct
  immutable marker and exact inventory. All installed-family predicates,
  including `is_installed_v2`, still cover both kinds. The legacy sweep and
  local terminal-writing paths must continue to refuse both. The existing
  private qualification executable gains an explicit
  `--manual-approve-recipient` intent mapped through
  `QualificationIntent` to the same Manual actor and
  `RecoveredOwner::open_pinned`. This flag belongs only to that isolated
  driver. The existing `--manual-recipient` mode still selects v2.
  Choose once before Peer opens, never from an ordinary daemon argument,
  operator profile setting, environment or incoming event.
- On first attachment, require the exact recognized unattached v2 parent
  inventory and unused provisioning slot. Install the chosen custody
  inventory in the same IMMEDIATE open transaction. On reopen, verify the
  exact requested marker and inventory before custody use. Both selectors
  explicitly refuse the other installed custody marker. Refuse a mixed,
  partial, spoofed or fixture layout before attachment or mutation.
  Use no automatic migration and keep the SQLCipher/native stage format.
- Keep the existing v2 Pending-only guard unchanged. The v3 trigger enforces
  the same Held → JoinStarted → Pending progression and additionally permits
  one state 2→3 update only when `OLD.mode=0 AND NEW.approval=0`.
  Use the existing state, decision and activating time columns. The v3 open
  verifier checks those values for every Activating row as well as the
  retained Pending pin and current parent, slot and keeper invariants.
  Retain direct-write denial, the one charged Unknown slot, acknowledged
  exact Consent reference, immutable custody/native coordinates and no
  terminal records. The v3 inventory has no publication aperture.
- Add a narrow decision entry on the same borrowed `PendingTurn` and
  `RecoveredOwner` used by the installed Manual service. Reuse
  `read_installed_join_custody`, current signed v2 Delivery replay,
  `PendingClock`, managed measured readback and its opaque
  `InstalledJoinedReadback`. Reread the same durable selection, generation,
  sole charge, stage identity and file witness, Delivery/carrier and Welcome,
  native group, epoch identity and leaf. Require the existing stable
  authority checks, Manual eligibility and the actual current qualified
  port, same lease/quota, stop, readiness and forwarding checks. Do not
  re-attest a consumed KeyPackage or re-ingest Welcome. A memory
  Pending projection is only a display hint.
- Adapt `transition_custody_guarded` for an installed Manual approval
  operation. Invalidate the producer before opening the write transaction.
  The existing read ticket's reply-time check then rejects a stale Pending
  result. Check exact current offer revision, retained Pending pin and
  both persisted floors, and absence of terminal or stale turn in the same
  IMMEDIATE transaction. Use the wall sampled at the commit's checked
  `use_current_recipient` and refuse equality with the effective cutoff.
  Consume local decision authority immediately before the state write.
  Open the existing owner-only aperture transactionally, persist exactly one
  Manual decision, activating time and both existing floors, restore the
  aperture, and recheck authority, forwarding, route and time after the
  speculative writes and before commit. Roll back all three writes on
  refusal. A stop or Fatal observed after successful commit closes the
  reply and leaves the approval durable.
- Add an approval-only v3 counterpart to `current_installed_delivery` and
  `read_joined` (factor their proof mechanics when possible). First
  approval accepts only Pending. Authorized retry accepts only the exact
  Manual Activating selection, original decision and immutable joined pin.
  Both paths authenticate the same stored signed v2 Delivery and repeat
  the same charged, closed readback and current file, source, port,
  readiness, forwarding and time checks. The retry consumes local
  authority before the final exact SQL read and repeats its live fence
  thereafter. It never updates a state, floor or native original. Keep the
  existing Pending projection, join and scheduling allowlists unchanged.
  Keep exact Delivery replay through `installed_ancestry` →
  `receipt_held_replay` recognizing the authentic held ancestry even
  after state 3, including its existing expired replay handling.
- An actual authoritative positive retry recovers the original local result.
  After a lost exchange or prior unknown result, a client must treat any
  later non-positive response (including expiry, stale and Busy) as unable
  to establish the first outcome. Include that distinction in candidate CLI
  help and diagnostic wording, with an explicit recovery invocation for
  the same selector. There is no general Activating query.

## Local IPC ownership

- Preserve `PeerRead` and its Pending-only List/Show shapes. Add a separately
  versioned candidate-only Approve request using the existing typed
  `Selection` (full canonical npub, exact Offer ID/revision, nonzero
  generation). Keep closed wire shapes and bounded request/reply. A positive
  reply contains the exact echoed selection and `Activating` only. Use
  bounded busy, stale or expired, unavailable and uncertain outcomes.
  A client with a lost reply can retry that same selection. Expose only an
  exact-selector candidate CLI action alongside List/Show and describe its
  response as recorded local approval. No aliases or remote decision input.
- Pair to the actual registered listener using the pattern in
  `peer_pending_ipc.rs` and `peer_daemon_decision.rs`. Verify the real
  stream UID against the leased profile UID. Exclude Approve explicitly
  from `ipc.rs::is_command` and from the stock and ordinary read queues.
  A non-clone, one-use capability binds the listener incarnation, lease,
  typed request, reply lifetime, readiness, common Fatal and cancellation.
  Socket waits and reply are bounded by the existing 15-second exchange.
  An unaccepted, expired, detached or cross-UID request has no authority.
- Use one bounded local waiting position. Actor admission occurs only at a
  rotor boundary with a reserve of at least six seconds on the exchange.
  Its single inclusive decision window is at most five seconds and is also
  fenced by reply liveness, the signed exclusive cutoff, actual qualified
  route and readiness. Before admission, Busy means no mutation and no
  later admission of that request. Actor claim and listener removal/expiry
  must arbitrate the same one-use position. A claimed request that loses
  its reply is uncertain to that client. Check at loop entry and immediately
  after each completed history or live turn, and once after any selected
  managed effect before the next loop. At most one admitted decision per
  rotor pass. Do not select a decision inside history, live, Join or
  package awaits, or cancel those turns to obtain a slot. Defer a fresh
  refresh or recompute selected work after a decision so no old projection
  is revived.
- Local bad selection, stale/foreign/expired request or refused native,
  stage or path proof yields an IPC refusal while the actor remains
  available, like existing projection `ProofRefused`. Explicitly recheck
  the actual port plus readiness, stop, local route and shared Fatal;
  use their existing lifecycle outcomes to stop when applicable, never
  infer actor shutdown from the generic `effect_failed` mapping for a
  refused local request. Stop closes both local apertures. Only the actor
  holds the SQL owner. Fault injection and extra fixture transports stay
  outside installed artifacts.

## Validation and subsequent gates

Test closed shapes, exact local UID/capability pairing, full/expired queue,
old and mixed schema refusal, direct SQL write denial, exact old/new layout
regressions and floor rollback at each state/commit cut. Test wrong/stale or
foreign selection, native pin and file witness changes, expiry equality,
disabled Manual, readiness/route/stop/Fatal before state write and after
speculative write, duplicate request, and cross-socket or reply mismatch.
Use one actual private registered daemon and independent qualified v2
founder with a genuine Welcome. Observe Pending, issue one exact CLI
approval, lose one reply after commit, restart the daemon and recover the
same Activating state with no second write. Verify native stage, charge,
Consent and Delivery remain exact, List/Show displays no approved Pending
row, and the relay records no Group egress, receipt, status or unrelated
native publication. Prove ordinary Command continues through approved
local refusal paths and record approval-to-durable-decision and IPC latency
against the existing Manual timing evidence. Keep hosted CI on deterministic
core tests and run private installed tests opt-in.

Exercise requests arriving during history, Join and package effects, two
concurrent clients, disconnected route, non-Command routing and continued
List plus ordinary Command after an invalid or foreign request. Exercise
legacy/new opener refusal in both directions and Auto mode refusal at SQL
level. On restart assert the scheduler idles the same Activating row and
that exact signed Delivery replay completes without another join, change
to the custody row or actor exit. The effective Pending cutoff limits
decision and retry. Activating has no cleanup transition in this phase
and continues to occupy the one charged slot even after that cutoff.

A later activation phase must design an independently authenticated
founder convergence proof. The current isolated `postjoin_verify` functions
require a second local founder `AccountDeviceRuntime` and check its processed
outer event ID. They provide useful proof conditions for tests. A real
recipient has no access to the founder runtime. Relay EOSE, relay ACK,
scope metadata and the existing read projection cannot supply that proof.
Only after that review should the exact bounded Group history, closed
native maintenance and PostJoin publication journal be considered for an
installed owner. Founder acceptance remains separate and needs the exact
native signed receipt and its durable publication and verification. Decline
and expiry-connected native cleanup need their own terminal aperture and
must never borrow approval authority.
