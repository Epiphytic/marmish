# Mount authenticated Manual recipient work

This is the service-loop increment of the approved
[Manual actor mount](peer-candidate-manual-actor-mount.md), on source
`d2d014b4824d3d1d0cee3776d65df3256597faf4`. The target is an actual private
Linux AMD64 candidate daemon that can accept a v2 invitation as far as inert
Pending and show that row through the existing same-UID read socket. Keep the
existing candidate discriminator, qualified direct WSS, Ready-at-acquire Manual
Client/Both and registered quota. Maintain stock Command and Terminal behavior.
Manual approval, Auto, Active, founder, reconnect, public relay and installer
enablement remain out of scope. This increment leaves Healthy Empty closed.

## Owner, deadlines and loss

- Replace the private passive probe branch with a Manual service only in the
  qualification composition. Keep the passive proof callable independently.
  `peer_installed_recipient::run/attach/hold_port` already owns the thread,
  material, lease, registered quota/route, recovered cursor/Source, and paired
  raw drain. Reuse those instances. Use the existing listener and its exact
  nonzero generation for the single Producer/Reader pair. Carry the sole
  Producer to the actor and the memory-only Reader to the existing candidate
  listener registration. Preserve both Ready edges and current startup report
  ordering, with no service before checked registration and port open. The
  listener's stopped edge and Producer drop stop the view independently of
  native work. Check both pair/listener generation and actor liveness before
  serving a reply.
- Preserve the one 15-second deadline from the second Ready edge through
  attach, recovered cursor reconciliation, route transfer, authenticated
  NIP-42 open, live REQ and the post-open check. Start service after that check
  succeeds. For subsequent work, check Ready currentness, the shared actor
  stop, global forwarding/Fatal and exact port scope. Do not continue to call
  `startup_remaining` or reset that startup allowance. Route wall/continuous
  samples, TLS/attestation horizon, stored signed expiry and durable floors
  govern later turns. Cap page I/O at five seconds and live polls at 500 ms,
  shortened by remaining route and maintenance deadlines. Preserve the
  existing 15-second schema and separate five-second native windows, and
  the managed Consent derived send/settle deadline. Never reuse an expired
  window, or convert a stale clock/route into a harmless timeout.
- Use the same actor-owned stop state for the async stop and synchronous
  `NativeWindow` predicate. The actor handle latches the same atomic flag
  before waking the async cancellation token, including Drop. Preserve
  Ready's revocation watch and physical route/Fatal predicate. Recheck
  forwarding and exact cursor binding before and after work, and compose
  existing effect guards with the synchronous predicate where they enter a
  mutating path. Check at source/cursor admission, before a durable commit
  or native effect, and after returns. No guard may recurse into an already
  borrowed SQL connection, or use a stale cached wall. Current exact effect
  checks still own the permissible cut at a started write. In particular
  Consent’s reserve, before-flush and ACK settlement guards own exposed
  outcomes. Finish that bounded protocol attempt or retain its already
  durable uncertain state. Never infer nonexposure by dropping a future.
- On local loss, stop the Producer, drop the operation/port, revoke and reap
  the exact paired raw route with the existing `revoke_and_reap`; keep lease,
  quota and recovered cursor until cleanup is observed. Recheck the independent
  global invariant even after a local error. Global loss uses the reviewed
  physical-only stop path and performs no logical cleanup writes. An incomplete
  socket/thread join remains incomplete. No replacement socket opens on a turn
  refusal or relay failure.

## Authenticated ingress and the recovery scan

Reuse `PeerRecipientPort::page/wait_live/use_page/use_page_carrier/
use_live_carrier/use_current_native`. Port opening installs the distinct live
subscription before history. `Owner::page` already predrains and buffers
interleaved live on the same authenticated connection, requires exact EOSE
and closes the history subscription. `read_ack` also buffers live. Add a
guarded, nonblocking take of one buffered `LiveRecord` next to `wait_live`
if required. It still uses private `Owner::pop_live`, accounts bytes, and
checks current socket scope. Do not return bytes from an arbitrary owner.
Only one operation polls the port at a time. Give queued live a turn before
starting a new history/ACK operation that cannot fit the remaining reserve.
The existing raw backlog/frame limits and checked failures stay authoritative.

1. Start `installed_delivery_scan(..., Recovery)` using this owner's checked
   port/source and current wall. Resume an existing durable revision with its
   original bounds and anchor. Never reset bounds on retry and never copy a
   prior port's in-memory completion. The first service increment drives this
   one finite recovery scan, then continues live, durable and saved work.
   A future periodic recovery can use `ScanStart::Periodic` without changing
   these rules. Existing count/byte/timestamp-tie caps are hard refusals.
2. One history turn processes one page of at most four exact carriers. Follow
   `peer_installed_delivery_fixture` for checked page/count/index and reuse a
   single actor-owned ingress function for both page index and live. Call
   `RecoveredOwner::observe_managed_delivery` with the raw aperture. Applies to
   Offer, invalid and Delivery events. A Delivery needing inspection uses the
   exact `Prepared::challenge`, `use_delivery_native`, `measure_delivery`,
   then reacquires the same carrier to `finish_measured_delivery`. No fixture
   attestation, decoupled bytes, or independent keyed cursor is accepted.
   Recheck physical scope across these borrows and after each returned result.
   Reuse the existing lexical conflict and same-event replay disposition.
3. Remount a guarded owner wrapper around `installed_delivery_scan`,
   `observe_installed_delivery`, and `commit_installed_delivery_page`.
   Use the existing `commit_page_with_fence` and custody/applied-wall checks,
   adding the current actor/route predicate inside mutating transactions just
   before commit, including direct invalid/Offer receipt and saved retry
   paths. Existing test adapters may use their existing fences. An outer
   actor check does not replace a commit check. Closures use the exact
   current in-memory stop/Ready/physical witness and independent forwarding
   verification, plus the existing SQL source/row/wall checks, without
   touching the socket or borrowing the owner recursively. Classification
   and any other SQL helpers that write must thread the same guard to their
   commit, or the runner must refuse that path. Audit each call and keep
   the wrapper private instead of creating a general transactional escape.
4. Each carrier must yield one exact `Disposition.receipt`, including a
   durable Deferred receipt where the current classifier permits it. Commit
   the page once through `use_page`, under exact cursor revision, bounds,
   carrier census and current fence. Return `Ok(Some(next))` only for the
   stored successor with stable origin/anchor and incremented revision.
   `Ok(None)` after the last guarded commit is checked completion of this
   recovery run. Keep a private completion marker bound to the port,
   cursor/source generation and completed coordinates. Earlier per-record
   commits survive a failed page commit. Cold replay uses their existing
   idempotent dispositions and exact cursor. A partial page, lost EOSE,
   conflicting receipt, overflow, stale source or timeout neither commits
   progress nor supplies completion. A live timeout only means idle.
5. Checked completion is only scan progress. Invalidate it on port/source
   loss and never turn it into a read-side Empty, including after a resumed
   earlier run or a page with a deferred carrier. A later separately
   qualified absence proof must also establish a fresh current-port coverage
   barrier, exact inventory absence and no unresolved input, with the
   foreground/live handoff held. This increment publishes a sealed positive
   Pending after the recovery scan finishes; individual received, deferred
   or absent rows provide no view on their own.

## Deterministic work selection

Use a pure rotor over `Live, History, Durable, SavedDelivery, Observe`,
with a full cycle of five positions starting at Live. Every selection advances
past the chosen position even on a refusal, duplicate or unsuccessful
progress. Skip ineligible positions once per search. Serve one live carrier,
one history page, one durable step, one saved retry, or one observation per
selection. With continuously eligible classes, each is selected within five
completed choices. No latency claim is implied while a finite operation owns
the port or a synchronous native call has not returned.

- Live gets one nonblocking buffered attempt per pass of its position. When
  no buffered item exists, poll one bounded raw live slice before advancing,
  even with continuously ready local work. The idle case waits for live,
  next known checked due time, 500 ms maintenance, or stop. Do not spin on an
  empty rotation. A page/send already in progress retains its exclusive port
  until its checked finish or failure. Use the existing time and queue limits
  to bound reads between choices. The rotor is local scheduling, not a permit.
- Add a candidate-private, bounded read-only lookup on the existing checkpoint
  and recovered owner. Return at most one exact row key, generation, step and
  next eligible deadline for local work; no stored frame or native authority.
  Use existing durable slot/count limits and semantic checks rather than
  listing all rows. Recover the occupied Unknown slot first. Its successor
  states are Bound, NativeStarted, Prepared, ready Consent, Held, JoinStarted
  and Pending observation. Choose a new eligible authenticated v2 Offered
  row only when the existing admission/cooldown/slot permits one; use
  ascending durable admitted time and canonical founder/Offer/revision
  tie-break. Restrict this first mount to the measured Unknown family;
  a reserved/known row is never silently reclassified. No wildcard
  enumeration. Revalidate all row keys, generation, deadlines and current
  masks through the unchanged guarded effect APIs before each operation.
  Read-only readiness does not advance a wall floor or reserve an attempt.
  For saved Delivery, add a matching bounded read-only due probe, then use
  `due_managed_delivery` only after choosing that class. Ensure the probe and
  operation use the same exact saved identity. A blocked exact row waits for
  its existing durable due horizon or a new applicable checked state;
  unresolved corruption stops locally. No hot retries or unbounded side list.
- Use existing provisioning/managed APIs: `reserve_provisioning` once,
  `recover_provisioning` by exact selector, then
  `start_package -> prestage_package -> issue_package -> attest_package`.
  The uninterrupted package creation chain is one bounded owned operation
  with the established narrower subwindows. A cold NativeStarted only tries
  checked attestation of the exact file/artifact, never generating again;
  Prepared re-attests the existing artifact and queues managed Consent once.
  The queue step and due send are distinct choices.
- Use `queue_managed_consent` and the same port's
  `publish_managed_consent` including the existing checked attempt, ACK CAS
  and bounded Delivery wake. The wake is a hint, not native ingress
  authorization. Saved Delivery uses `use_saved_delivery`,
  `observe_managed_saved_delivery`, `measure_delivery` and
  `finish_measured_saved_delivery` under exact current source and route.
  Once expired, do not keep selecting a row whose checked native challenge
  refuses its deadline. Preserve its charge, block resurrection, and keep
  the view unavailable for that ownership. Use a separately reviewed expiry
  disposition only after its source is integrated; a missing disposition
  does not require inventing another retirement path here.
- `PendingTurn::borrow` consumes no new lease or cursor. For Held, arm and
  poll exactly one original Welcome under the existing capped NativeWindow.
  A result loss after durable JoinStarted never schedules another Welcome.
  A separate JoinStarted choice performs exact capped Joined readback and
  guarded Pending CAS; preserve pre-effect or ambiguous custody and charge
  if it cannot prove native state. Pending observation uses the existing
  sealed `observe_pending` independently of package/ACK effects. Invalidate
  the view before an operation that may change its facts. Issue a producer
  ticket before either observation read. Reuse the short dual-clock freshness,
  500 ms refresh due time and final before-write check. A stale/failed
  refresh results in Unavailable; an existing exact expired-proof List
  mask may expire as already specified. No synthetic Empty or raw row
  constructors enter the candidate code.

## Composition and qualification

Split the candidate-only service/rotor and exact guarded wrappers from the
small `peer_installed_recipient.rs` and daemon/listener composition. The owner
of managed dispositions implements the read-only lookup and commit checks
alongside existing checkpoint and cursor code. The runner owns scheduling,
typed outcomes and stop propagation, using the exact borrowed owner.
Root coordinates one Producer/read mount. Keep in-flight expiry and paired
baseline patches disjoint. Prior to runtime composition pin their compatible
source, the sealed Pending projection, and the currently reviewed exact
managed turns. Clean read visibility can mount with the real service after
source-matched qualification. A partial runner may remain private with reads
Unavailable. There is no terminal publication of this increment until the
actual installed path is verified.

- Deterministic selector tests cover every ready combination, steady
  continuously ready work, busy live and retry, sparse/idle rotation,
  no-progress advance and exact re-eligibility. Verify no read-only probe
  advances row/floor/attempt state. Reuse milestone and custody fault tests
  for charge/cap effects. Add focused tests for effect and commit fence loss,
  failed page, resumed exact revision, same raw event via both sources and
  any added buffer-take scope/byte bounds. Assert durable effects and socket
  observations, not only an error return.
- Run one source-matched private installed Linux AMD64 candidate on distinct
  loopback authenticated Peer and ordinary Command WSS. Use a disposable
  founder. Hold both Ready/report edges and demonstrate zero pre-open Peer
  work. Present at least one full checked page and terminal short page with
  an authentic Offer and send a verified Delivery live, including live
  buffering during history/ACK. Check one cursor, port, selector, package,
  exact correlated Consent ACK, stored Held, one native Welcome and exact
  inert Pending. Read List/Show through the actual same-UID client. Check
  command byte equality during the active sequence and after local route loss.
  Compare Peer connection/publication counters and verifiable durable cuts.
  A cold candidate restart must observe/prove the same Pending with no new
  package, Consent or Welcome. Hold a partial/lost EOSE or wrong ACK and
  check the barrier or Held respectively stays unavailable; force local
  loss and confirm the paired raw drain. Reuse reviewed global loss/interrupt
  tests and add one focused registered-stop seam check for the new runner.
  Quiet checked recovery still yields `peer_unavailable`, not Healthy Empty.
  A manifest, socket authentication, fixture-only test or stand-alone
  successful publication does not qualify this installed behavior.
- Record proof wall time and existing phase timestamps for page, send to ACK,
  Delivery to Held, Welcome/Joined and local observation, with source,
  candidate type, counts and limits. Use locally scheduled opt-in tests.
  Keep GitHub's stock core subset and stock installer behavior unchanged.
