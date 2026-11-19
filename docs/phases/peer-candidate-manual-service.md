# Mount authenticated Manual recipient work

This is the service-loop increment of the approved
[Manual actor mount](peer-candidate-manual-actor-mount.md), on source
`d2d014b4824d3d1d0cee3776d65df3256597faf4`. The target is an actual private
Linux AMD64 candidate daemon that can accept a v2 invitation as far as inert
Pending and show that row through the existing same-UID read socket. Keep the
existing candidate discriminator, qualified direct WSS, Ready-at-acquire Manual
Client/Both and registered quota. Maintain stock Command and Terminal behavior.
Manual approval, Auto, Active, founder, reconnect, public relay and installer
enablement remain out of scope. This work leaves Healthy Empty closed.

Land guarded ingress and one exact recovery page progression first, unmounted.
The durable selector and the daemon/Producer mount come afterward. The first
slice uses existing same-owner managed Offer/Delivery and exact cursor/port
APIs with the narrow additions below. It needs neither the existing native
expiry follow-up nor a new startup entry point. Compose private installed
qualification only after the extra work gates pass.

## Owner, deadlines and loss

- Replace the private passive probe branch with a Manual service only in the
  qualification composition. Keep the passive proof callable independently.
  Add one private `QualificationIntent::ManualRecipient` alongside
  `PassiveRecipient` in the existing qualification example and argument
  parser, with the same registered-only refusal before any stock fallback.
  No installed CLI flag or environment selector. `Recipient::start` can reuse
  `ipc::peer_read_listener::Offer::new` to create the single view pair, and
  `CandidateListener::registered` carries that Offer to the existing listener.
  `Offer::register` records the listener edge only after kernel path checks.
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
checked nonblocking `Owner::take_buffered_live` through a private port
accessor. Check current scope and use `Owner::pop_live` to account bytes.
Before History or a managed Consent send, process at most the four
already-buffered records through the same live classifier and guard. This
drain overrides the rotor's standalone one-live quota. No raw socket poll
occurs in the drain, so nothing refills it or starves the selected turn.
A burst above the four-record/byte reserve during one indivisible page or ACK
still returns typed Capacity and retires the socket. Durable facts survive.

1. Start `installed_delivery_scan(..., Recovery)` using this owner's checked
   port/source and current wall. Resume an existing durable revision with its
   original bounds and anchor. `CursorStore::scan(Recovery)` can return only
   `Some(active_or_new)` or an error under this schema. Its `Ok(None)` cadence
   branch applies only to Periodic, so treat an absent Recovery cursor as
   a local invariant failure. Never reset bounds on retry and never copy a
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
3. Add guarded owner wrappers around `installed_delivery_scan`,
   `observe_installed_delivery`, and `commit_installed_delivery_page`.
   Keep the inner custody/applied-wall checks and add a final synchronous
   guard (see below). Preserve source-compatible fixture entry points.
   Guard every legitimate Offer, invalid and exact Held receipt while
   preserving classification. True lexical conflicts still stop or roll back.
   The first slice ends with scan, guarded history/live classification and
   measured fresh-Delivery completion on one recovered owner and port.
4. Each carrier must yield one exact `Disposition.receipt`, including a
   durable Deferred receipt where the current classifier permits it. Commit
   the page once through `use_page`, under exact cursor revision, bounds,
   carrier census and current fence. Return `Ok(Some(next))` only for the
   stored successor with stable origin/anchor and incremented revision.
   `Ok(None)` after the last guarded commit is checked completion of this
   recovery run. Keep a private completion marker bound to the port,
   cursor/source generation and completed coordinates. Prohibit a second
   Recovery scan in this incarnation. Earlier per-record
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

### Commit and effect boundaries

Use one cheap prebound synchronous predicate over Ready, the shared stop,
independent forwarding verification and the current physical route token and
clock. It cannot reborrow the port, use SQL or poll the socket. Keep each
existing source, wall and transaction custody check. The predicate checks
the current route and the latched monotonic ceiling immediately before a
mutating commit or native effect. A cut after admission does not undo its
effects. The first five rows are the ingress slice. A read transaction such
as `read_installed_consent` only checks its wall floor and needs no commit hook.

| Turn | Checked boundary |
| --- | --- |
| `installed_delivery_scan` | Add a final hook to `CursorStore::scan` after each `with_wall` path (resumed or new Recovery and the existing Periodic cadence branch where used). Refusal rolls back floor and revision. |
| Offer via `record_installed_offer` | Use `commit_trusted_exact_guarded` after marker/admission/sweep and receipt writes, including cooldown skip and duplicate. Keep custody and parent checks. |
| Invalid and Held replay | Thread `commit_invalid_delivery_guarded` through malformed, expired, terminal-ancestry and inexact or aged Held cases. Use `commit_trusted_exact_guarded` for exact current Held replay. Final hooks follow lexical/ancestry checks and receipt writes. |
| Measured fresh Delivery | Supplement `Native::check_at` at measurement admission and in the final callbacks for `commit_invalid_queued_delivery`, `defer_installed_delivery` and `commit_custody_held_input`. Preserve reclassification and exact saved bytes. |
| `commit_installed_delivery_page` | Keep the `commit_page_with_fence` context check and floor update. Add a final hook after normal advance/completion and after quarantine UPDATE, immediately before each commit. |
| Provisioning and package | Guard `reserve_provisioning`, `recover_provisioning` (including floor writes), `start_stage` and `record_prepared` before commit. Keep the independent managed file/package admission windows before each native effect. |
| Managed Consent | Reuse `queue_managed_consent_with`, checked reserve and before-flush, ACK CAS and `wake_installed_delivery_fenced` final hooks with the owner predicate. Retain existing exposure accounting. |
| Saved Delivery | Its readiness probe must be read-only. `due_installed_delivery` advances floors via `due_with_fence`; add a final hook there and to the no-ACK `retry_again_with_fence` update. Reuse `finish_retry_guarded` or guarded Held/invalid final hooks. Expired rows only use separate reviewed native cleanup. |
| Held and JoinStarted | Compose the predicate with `WelcomeFence` before native effects and in `start_installed_join` and `commit_installed_pending` immediately before their guarded transition. Existing Pending replay is a checked read. |

A contract-invalid checked page can persist its existing 60-second quarantine.
That timer does not clear the guard, including after restart. This increment
has no requalification call and does not open a replacement port. Missing
EOSE or current-owner refusal cannot supply completion.

## Deterministic work selection

Use a pure rotor over `Live, History, Durable, SavedDelivery, Observe`,
with a full cycle of five positions starting at Live. Every selection advances
past the chosen position even on a refusal, duplicate or unsuccessful
progress. Skip ineligible positions once per search. Serve one live carrier,
one history page, one durable step, one saved retry, or one observation per
selection. With continuously eligible classes, each is selected within five
completed choices. No latency claim is implied while a finite operation owns
the port or a synchronous native call has not returned.

- Live pops one buffered item per pass. With nothing buffered and other
  work ready, poll raw live once for up to 20 ms, clamped by route and
  maintenance deadlines. Use the separate buffered take even when that
  deadline has passed because raw `wait_live` checks deadline first.
  Idle waits until live, checked due time, 500 ms maintenance, or stop.
  Only pure page/live reads allow select-on-stop or revocation, with no
  reuse after cancelling the raw `active` read. Never preempt a page for
  another class. Managed Consent settles or retains its typed uncertain
  outcome under its own bounded write and ACK fences, with no enclosing
  cancellation select. The candidate rotor leaves the fixture's
  `Service`/`Batch` and reconnect/backoff unchanged.
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
  the view unavailable for that ownership. Latch the exact expired selector and
  generation and bar new Unknown reservations while it holds the slot.
  Use a separately reviewed expiry
  disposition only after its source is integrated; a missing disposition
  does not require inventing another retirement path here.
- `PendingTurn::borrow` consumes no new lease or cursor. For Held, arm and
  poll exactly one original Welcome as one selection under one shared
  NativeWindow and hard deadline, preserving narrower samples. Neither
  `ArmedWelcome` nor `ClosedJoin` crosses a selection boundary.
  A result loss after durable JoinStarted never schedules another Welcome.
  A separate JoinStarted choice performs exact capped Joined readback and
  guarded Pending CAS; preserve pre-effect or ambiguous custody and charge
  if it cannot prove native state. Pending observation uses the existing
  sealed `observe_pending` independently of package/ACK effects. Invalidate
  the view before an operation that may change its facts. Use only
  `Producer::invalidate(&self)` as the non-authorizing addition. Issue the
  existing producer read ticket before either observation read. Reuse the short dual-clock freshness,
  500 ms refresh due time and final before-write check. A stale/failed
  refresh results in Unavailable; an existing exact expired-proof List
  mask may expire as already specified. No synthetic Empty or raw row
  constructors enter the candidate code.

## Composition and qualification

First own only guarded cursor/port ingress, checkpoint commit paths and a
disjoint exact private WSS regression. That slice may be reviewed and
composed independently and adds no installed entry, projection or new
authority. Use current paired fixture owners, exercised on real authenticated
history and live, and a small deterministic final-admission cut. The second
slice adds the read-only due probes, fair rotor and one owner of effect
selection. Root coordinates the third slice's Producer, daemon and listener
handoff; a partial runner stays private with reads Unavailable. Keep
in-flight expiry and paired baseline files disjoint and pin reviewed composed
inputs before the installed proof. This first deployment proof can expose
inert Pending only after the full real service and same-UID qualification.

- Deterministic selector tests cover every ready combination, steady
  continuously ready work, busy live and retry, sparse/idle rotation,
  no-progress advance and exact re-eligibility. Verify no read-only probe
  advances row/floor/attempt state. Reuse milestone and custody fault tests
  for charge/cap effects. Add focused tests for effect and commit fence loss,
  failed page, exact revision resume, a quarantined restart, same event via
  both sources and refusal of a second Recovery scan after completion.
  Exercise prebuffer drain, live interleaving during both history and ACK,
  exact byte accounting and independently triggered genuine overflow.
  Assert the durable effects and socket observations alongside errors.
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
  Quiet checked recovery yields `peer_unavailable`. Prove live work and
  positive Pending more than 20 seconds after Ready. Report freshness
  misses during bounded page or native work. Route or TLS expiry stops
  the service and reaps this exact connection, with no reconnect.
  A manifest, socket authentication, fixture-only test or stand-alone
  successful publication does not qualify this installed behavior.
- Record proof wall time and existing phase timestamps for page, send to ACK,
  Delivery to Held, Welcome/Joined and local observation, with source,
  candidate type, counts and limits. Use locally scheduled opt-in tests.
  Keep GitHub's stock core subset and stock installer behavior unchanged.
