# Record local Peer approval as Activating

This phase extends the private Linux AMD64 receiver-candidate at `78b4d39`
from an exact, charged Pending invitation to a durable local approval. It
implements the Manual local decision in [Pending activation](peer-pending-activation.md)
after [installed Manual](peer-candidate-manual-actor-mount.md) and its near-cap
qualification. Native group egress, history, receipt publication, Active,
auto approval, Decline and leave stay closed. Stock artifacts and ordinary
Command and Terminal paths keep their existing behavior.

## Exact layout and decision

- The installed v2 successor (`peer_pending_custody_schema.rs`) has an immutable
  v2 marker, a `peer_installed_custody_pending_only` trigger and open verification
  which refuses state above 2 and any terminal record. Give the private
  approval candidate an explicitly selected new installed layout, with an
  immutable distinct marker, exact SQL inventory and a matching verifier.
  The legacy v2 schema continues through Pending with its existing guard.
  Initially create the new layout only on a fresh private candidate profile.
  Verify its inventory and rows on reopen. Treat old, mixed, partial or
  spoofed inventories as Peer unavailable on this path. Use no automatic
  migration and leave the existing SQLCipher/native stage format intact.
- The new layout permits only the existing Held → JoinStarted → Pending
  progression and a single Pending → Activating transition with an exact
  Manual approval. Keep denied direct writes, one charged Unknown slot,
  the acknowledged exact Consent reference, immutable flight/custody/native
  coordinates and no terminal rows. Retain both floor checks and exact
  source installation checks. No publication inventory or dispatch callback
  belongs to this layout, and it must refuse incompatible fixture epochs.
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
  operation. Check exact current offer revision, retained Pending pin and
  deadline, both persisted floors and no terminal or stale turn in the same
  IMMEDIATE transaction. Require decision authority immediately before the
  state change and recheck its liveness and the physical/time window after
  speculative state and floor writes, before commit. Open the existing
  owner-only write aperture transactionally and restore it before the
  post-check. Require exactly one state change and persist only the Manual
  decision, activating time and existing floors. Roll everything back on
  refusal or failed commit. Stop or Fatal observed after a successful
  commit closes the capability and reply without undoing the approval.
- An authorized retry of the same exact still-current Activating row reads
  back the durable decision and returns it without another state change,
  floor update or native effect. Reread and check the exact selection,
  authenticated v2 retained custody, Manual row, nonexpired window and
  current checked port/source. Do not infer success from a disconnected
  reply or expose a general Activating query. Stop, drift or expiry during
  retry returns a bounded refusal and leaves the durable row intact.

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
- Reuse the pairing pattern in test-only `peer_pending_ipc.rs` and
  `peer_daemon_decision.rs` with the actual candidate listener. Create an
  exclusive bounded request channel tied to the registered listener
  incarnation and actor. Match the actual stream's verified peer UID to
  the leased profile UID. Mint a non-clone, one-use authority sealed to
  that listener, exact lease, typed request, reply lifetime, common Fault
  and readiness state, deadline and cancellation. Cross-UID, expired,
  detached, closed-reply, foreign or replayed capabilities cannot open the
  CAS. A consumed authority retains only a live precommit check for that
  turn. Route requests through the sole Manual actor, never an out-of-band
  connection, replayed display ticket or separate SQL handle.
- Invalidate or withdraw a Pending projection across the decision before
  writing the success reply. Keep outstanding stale read capabilities
  bounded by the existing read-ticket contract. Stop removes the decision
  aperture with the existing read listener. Queue refusal, expiry and
  cancellation preserve the scheduler's maintenance bounds. Leave test
  mock authority, fault injection and any additional port outside binaries.

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
