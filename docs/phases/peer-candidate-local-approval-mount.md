# Mount exact local approval on the private Manual candidate

This supplement narrows the [approved Pending decision](peer-candidate-pending-approve.md)
after the independently reviewed `ApprovalV3` storage slice at `75dc613`.
It covers one same-UID listener, the sole Manual actor and its existing charged
joined readback. Founder convergence remains a separate phase.

## Listener and local client

- Add a candidate Linux AMD64 `ipc::Request::PeerApprove` branch with a
  versioned, deny-unknown-fields request and an exact `peer_ipc_selection::Selection`.
  The success payload has the same version, exact echoed Selection and
  `Activating`. Reuse the bounded frame and exclusive response validation
  conventions of `peer_read_ipc`. Validate wrong versions, cross-kind fields,
  noncanonical coordinates and all response envelopes before treating them as
  success. Explicitly classify this as short exchange and outside Command,
  the ordinary queue, and `PeerRead`. Stock does not deserialize it.
  Candidate Passive and v2 Manual reply Unavailable before queueing.
  `daemon.rs::dispatch` also refuses this request if reached. The exact-selector
  candidate CLI retains the 15-second client budget and says “recorded local
  approval”. Its help explains reissuing the identical selection after an
  uncertain result. Restart can briefly yield Busy or Unavailable. Once
  any bytes might have been written, I/O failure, timeout and invalid or
  mismatched responses are Uncertain. A valid refusal after a previous
  uncertain exchange cannot determine that earlier decision. Keep
  List/Show Pending-only.
- The existing installed-layout qualification driver alone selects
  `QualificationIntent::ManualApprovalRecipient`. Carry one exact mode to
  the private `Recipient::start` / `Actor::spawn` / `RecoveredOwner::open_for_actor`
  path. Select v3 once before the cursor attaches, with no retry as v2.
  Continue validating actual receipt, Manual role, registered forwarding and
  profile readiness. Existing Manual stays v2 and receives no writer pair.
  CommandOnly, Passive, stock and the inert test startup get no writer pair.
- Pair the listener endpoint and non-clone worker privately during that
  startup. Store the listener half alongside `peer_read_listener::Offer` in
  `CandidateRegistration::View` only for this intent. Bind and enable it in
  `ipc::serve_with_listener` at the same actual `UnixListener` registration
  that enables Readiness. The pair retains only the exact `Arc<ProfileLease>`
  identity, expected socket path, `Readiness` and common `Fatal`. Registration,
  worker and listener drop each revoke the pair. Open no new listener, SQL
  cursor or native owner. Check the accepted stream credential from
  `verify_peer` against the UID on the current leased profile owner file,
  read from its already-open descriptor, and the current effective UID.
  The candidate accessor validates that descriptor and fails closed.
- Use `write_frame_body` in this client and keep its send half open through
  the reply. Existing `exchange` half-closes requests and stays unchanged.
  Split the exact accepted socket and share only its `Arc<OwnedReadHalf>`
  between actor and listener until settlement. The listener owns the write
  half. Both observers only peek and never consume from that read half.
  The listener watches read readiness and invalidates on EOF, extra input or
  non-retryable read error. The actor's checked authority samples that same
  read half with bounded nonblocking `recv(MSG_PEEK | MSG_DONTWAIT)` at the
  decision fences, so it can reject a completed disconnect without waiting
  for the listener task to run. Zero or any readable byte refuses, EAGAIN
  means still awaiting a reply, and other errors refuse conservatively.
  Borrow the read half's descriptor for the call. Holding the read half
  prevents descriptor reuse when the listener task is aborted. No duplicate
  fd, signal handler, test observer or fd transfer grants authority.
  If disconnect races the final check or SQL commit, the exchange is
  uncertain. It cannot establish that the transaction was rolled back.
- One pair-owned mutex slot arbitrates Waiting, Claimed and Settled or
  Revoked for the exact exchange identity. Use a reply channel for the
  result and one authoritative slot state for claim versus removal. Hold
  the slot for at most one exchange and release on settlement, expiry or
  disconnect, without reviving a stale handle. A second eligible connection
  gets Busy immediately while the slot is occupied. Never hold the mutex across
  await, SQL or native I/O. Admit only before `accepted_at + 9s`. For a
  still-Waiting exchange equality removes it and guarantees Busy without a
  mutation. A claimed one can become uncertain, never Busy. Mark claimed
  with a single deadline no later than five seconds from claim, the
  remaining exchange reserve, and existing physical and signed bounds.
  A result gets the existing write headroom and full absolute exchange
  cutoff. Validate the same claimed exchange, current readiness and Fatal
  before replying. Listener drain, actor stop and global Fatal close both
  local apertures independently of pending work.

## Actor and checkpoint handoff

- The same `ManualService::run` checks its approval worker at loop entry,
  after history and live returns, and once after a selected managed effect.
  It claims only when the current `Ingress` incarnation has completed
  authenticated history. Never poll out of an existing await or cancel
  history, live, Join or package to handle approval. At most one claim per
  pass. Following a decision, recompute any preselected work and use only
  a fresh projection ticket. A normal refusal does not change backoff,
  recovery policy, or ordinary queues.
- Pass an owned non-clone claimed token to a private approval-only method
  on the same borrowed `PendingTurn` used by `ManualService::managed`.
  It binds the exact lease, Selection, reply identity, listener incarnation
  and deadline. Consume it at the last pre-write or final retry-read
  fence. Its consumed guard still checks liveness before commit and
  response, and cannot authorize a second effect. Explicitly bracket
  charged readback and SQL with the request budget as well as
  `PendingClock::Candidate` / `NativeRouteWindow::manual_window` checks.
  In an approval-only wrapper over `manual_window`, narrow the checked
  `NativeRouteFence.horizon` to the earlier actual physical horizon and
  claim cutoff before window construction. Every native sample and both
  SQL fences then obey that same absolute deadline through
  `NativeRouteFence::is_current`. Reuse the earlier bound through any
  derived schema fence. Read the current
  enrollment from the same `RecoveredOwner` and qualified current
  `PeerRecipientPort` with exact route, signer/account, source and
  `ForwardingVerifier`. A projection entry or raw wire data cannot mint
  a token or a native witness.
- Factor a scope-limited sibling of
  `Checkpoint::read_installed_join_custody` and
  `current_installed_delivery` for v3 Pending and exact Manual Activating.
  Existing v2 / scheduler / projection paths retain their state allowlists.
  Reread exact current Offer and acknowledged Consent, authenticated signed
  Delivery and Welcome, charge/file identity and original stable group,
  epoch authenticator and recipient leaf. Use the existing measured
  `InstalledJoinedReadback` on a closed, disposable copy with exact same
  quota and authority. No original write, KeyPackage re-attestation or
  Welcome ingestion. Activating uses its original Pending pin and deadline.
- Factor the private pre-write/commit path from
  `transition_custody_guarded` around its existing IMMEDIATE transaction.
  Before it starts, invalidate the projection with its existing producer.
  Inside verify the approved marker, same owner, immutable Offer revision,
  retained signed Delivery and pin, terminal absence, lease/source/route
  match and both existing wall floors. Check exact current request and
  source again before the keeper aperture opens. Open the existing writer
  guard for exactly one guarded state-2-to-state-3 update with
  `approval=0` and `activating_at`, restore the guard, and advance both
  floors with the same checked wall. Verify both floors and owner
  invariants before commit, resample actual route/forwarding, file identity,
  request and clock, and reject wall regression or cutoff equality. The
  current route facts can be checked inside the SQL closure through the
  exclusive port's checked native window, with no recursive port borrow.
  The transaction preserves the original limits and rolls back all three
  writes on any precommit refusal.
- A retry uses a fresh exact joined readback and consumes a fresh request
  before a final read-only same-selection SQL verification of the original
  decision and both floors. A replacement, stale, terminal or expired row
  grants no positive result. SQL must not increment floors, reapply a
  decision or mutate the stage during retry. Emit success only after a
  known committed decision or exact durable readback and a final live
  exchange/lifecycle check. Commit I/O ambiguity closes this Peer
  generation and yields Uncertain if the reply is live. Lost reply or
  transport failure after possible request delivery is uncertain to the
  client. A later refusal cannot disprove a prior commit. Keep separately
  observed actual lifecycle faults and integrity/clock regressions distinct
  from request/native proof refusal, which leaves the actor available.
  Shared Fatal always retains the existing global-stop precedence.
- Keep the approval result typed through the nested
  `use_current_recipient` result or the equivalent checked native callback.
  The outer port failure follows a fresh lifecycle check. The private
  transaction distinguishes request/native refusal, persisted integrity,
  clock regression, recorded decision and ambiguous commit. In particular,
  an error from `tx.commit()` is always ambiguous for this operation.
  Do not collapse the inner result into `Error::Source`, infer transaction
  state from generic SQLite errors or call `ManualService::effect_failed`
  for a local decision. The service independently calls
  `ManualService::check` and rechecks its actual port before mapping
  the typed result to the reply or stop. Test these categories separately.

## Implementation and qualification boundaries

Land candidate-only wire/paired listener tests and unmounted native witness/
SQL decision tests as independent cuts. Compose both with the qualification
driver and Manual rotor before making this intent callable. Scope ownership
to `ipc.rs`, `identity.rs`, `peer_read_listener.rs`, candidate CLI and
new approval protocol/intake modules. Startup and actor cuts own
`daemon.rs`, `daemon_candidate.rs`, `registered_qualification.rs`,
`peer_installed_recipient.rs`, `peer_manual_recipient_service.rs` and
`peer_receiver_inventory.rs`. The decision cut owns the exact candidate
schema-epoch promotion in `peer_pending_custody_schema.rs`, a sibling of
`peer_installed_pending_checkpoint.rs`, and minimal hooks in the shared
custody checkpoint, `peer_receiver_v2_pending.rs` and
`peer_physical_route.rs`. Keep all promotion limited to the Linux AMD64
candidate gate with its existing test alternative. Leave stock, v2, read
projection and ordinary commands intact.

Use deterministic barriers and injected clocks for claim/removal races,
disconnect or pipelined bytes with the listener task delayed, immediate
Busy from a second client, UID/path/lease/incarnation mismatch, stop/Fatal,
and the exact accept-derived Busy cutoff. With actual local Unix sockets,
verify wire rejection, the case where client deadline precedes listener
deadline, and typed outcome categories for native refusal, persisted
integrity, clock regression, commit I/O ambiguity and a known commit.
Use the existing charged private registered Manual fixture on a freshly
provisioned v3 profile with a separate ordinary Command relay and actual
authenticated founder v2 history. A v2 profile is an incompatibility
regression fixture only. Freeze history while a newer signed Offer can supersede the
selection, and cut at an existing native turn and each SQL guard. Verify
read projection invalidation, no duplicate queue effect and no ordinary
queue admission. Read true SQLCipher state, both floors and keeper across
precommit rejection, ambiguity, real restart and committed-but-lost reply.
Verify the original charged file and same native pin, exact replay with no
second Join, sole-slot retention in Activating after the cutoff, ordinary
Command through harmless refusals, and no Group, receipt or status egress. Report private installed decision and complete
round-trip latency separately. Retain only deterministic core tests in
ordinary CI. Until exact candidate composition and review pass, keep v3
staged and unreachable by the executable.
