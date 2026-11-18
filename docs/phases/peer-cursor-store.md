# Offline Peer page disposition and recovery store

This increment implements the durable bounded scan and deferred retry portion
of [Peer activation](peer-live.md). The dedicated carrier owner will supply
already attributed page boundaries and verified EOSE. The Peer owner will
supply qualified wall time and definitive control classifications. This slice
uses SQLCipher in an independent offline test target. It opens no socket,
processes no MLS state, and grants no authority. A returned cursor or retry
row cannot authenticate a source or authorize an invitation.

## Store boundary

`CursorStore::attach(connection, account, configured_origins)` consumes a
private `HardenedPeerConnection`, created only through the startup helper
using `marmish_engine::initialize_storage`, SQLite `NOFOLLOW`, and the pinned
`storage_sqlite::open_hardened_sqlcipher` with `SqlCipherKey::raw` and
`SqlCipherHardening::live_cache`. On the shared schema connection enforce
`busy_timeout=2000`, `synchronous=FULL`, `foreign_keys=ON` and journal mode
`DELETE`. Verify the returned journal mode and the effective settings,
including `secure_delete=1` and `temp_store=2`. Authenticate the keyed
connection by reading the schema. Use a private temporary file in the
fixture and keep full hardening in tests and benchmarks. Production path
validation and key derivation remain owned by the later profile store.
Use versioned namespaced schema tables and an account binding.

An origin is a fixed 32-byte identity assigned by the owner from its exact
configured route. Store and validate the exact static set of one to sixteen
distinct origin IDs at attachment. Configuration migration belongs to the
later profile store. All origins share one connection so trusted application
effects, exact origin replay fences and deferred removal can eventually share
one transaction. Every operation checks the attached origin. Up to sixteen
origins allow at most 4,096 retry rows and 256 MiB of exact event bytes for
the whole database. SQLite pages, indexes and journals add storage overhead.
SQL errors surface as closed categories. No diagnostic includes frame bytes.

One persistent state row per origin records a per-origin revision and a
qualified wall high-water mark, optional active anchor and its original
qualified reference, lower and upper cutoffs, current inclusive `until`, up
to three boundary IDs, a visible count, the last completed anchor and wall,
and a latched `RelayContract` outcome with its quarantine deadline.
Use SQLite nonnegative integers with checked conversion, BLOB length
constraints, and validated snapshots. Each mutation uses an immediate
transaction. No result claiming persistence is returned before commit.
A wrong account or wrong key prevents reopening the fixture.

`check_wall(origin, wall)` is part of every operation with a time argument,
including scans, pages, deferred admission, due reads, retry, wake and
expiry. A successful observation records the per-origin high-water even
when no row is eligible. Any lower wall returns `ClockRegressed` without
performing the requested change. An explicitly qualified owner-only
`rebase_wall(origin, wall)` abandons the old active scan, clears the old
periodic cadence while preserving its last completed anchor as telemetry,
and establishes the new high-water. The old cursor revision is retired.
Make any persisted retry scheduled after the corrected wall immediately
due and bump affected row revisions. Preserve failure steps and exact signed
expiry evidence. Every expiry and retry eligibility is then reverified
under the corrected wall. After the rebase, explicit recovery may start a
fresh full-window pass without waiting for the old clock. Rebase receives
freshly qualified time from the clock owner. A caller cannot use it as a
routine admission path.

## Scan and page API

`scan(origin, start, wall)` returns the exact active `Cursor`, if any, or
creates one from a qualified `wall`. `start` distinguishes explicit recovery
from periodic catch-up. A periodic start may create a new anchor only at
least 600 seconds after the last completed pass wall second. Recovery may
start earlier. An unfinished pass always retains its original anchor,
cutoffs, qualified reference, inclusive boundary and ledger on resume.
Both first and resumed carrier subscriptions still need fresh socket
qualification at integration. The API does not infer wall qualification
from schema content or accept a completed anchor as the next lower bound.
For anchor `T`, compute lower `T.saturating_sub(259_500)` and upper
`T.checked_add(300)` at start. The initial `until` is the upper cutoff.
On a qualified time representation overflow, fail without progress. The
per-origin cursor revision increases on start, commit and rebase and never
resets across completion, explicit recovery or reopen. A restart in the
same wall second still receives a distinct cursor revision.

`commit_page(cursor, page, receipts, wall)` consumes an exact, verified-EOSE
page wrapper supplied by the future carrier. The wrapper has the origin and
inclusive request bounds, plus borrowed raw events containing exact IDs,
raw seconds and opaque signed serialized bytes. For this fixture the wrapper
represents an already attributed transport result. Recheck origin and bounds,
revision, newest-to-oldest order, every stamp inclusively within the request,
one to 65,536 bytes per event, at most four distinct raw IDs and 256 KiB per
page, and at most three records per stamped second. A duplicate raw ID in
the same page is invalid regardless of timestamp. Empty is a legal short
page. Every raw item needs one exact disposition keyed by origin, ID and
SHA-256 of the received bytes. Reject extra, missing, mismatched or duplicate
dispositions, a stale cursor, or a full page with no older second. The oldest
second of a full page is
strictly below both its newest second and the request `until`. Adjacent
pages overlap only at the previous inclusive boundary. The three-per-second
cap and page limit of four make a boundary tie set complete when that set
returns. A missing prior ID is legal. Within one stamp any order is legal.
Count a repeated boundary ID once and count other raw positions with checked
arithmetic. An attributable contract violation, such as more than 256
visible records, latches `RelayContract` for the origin and neither advances
nor completes the pass. Retain the old cursor for diagnosis and record the
60-second quarantine deadline. The owner excludes this origin from page
service until explicit relay re-qualification after that deadline. The
owner-only `requalify(origin, wall)` then abandons the failed scan, retires
its revision and permits a fresh full-window scan. No automatic reset
pretends the earlier pass exhausted. Retry processing continues. The IDs
recorded for a valid full page are exactly the fully accounted IDs at its
oldest second. A short page is exhaustion only after every record is
accounted. Persist the completed
anchor and wall on success and clear the active scan. Its next scan starts
from the full new window. A previous inclusive ledger is progress metadata
and cannot pre-authorize or suppress fresh classification for any raw ID.
A cursor revision compare in the same transaction prevents duplicate commits.
At most eight pages are served per origin per scheduler turn. A yield retains
the exact cursor, and the page cap does not turn partial progress into scan
exhaustion.

There are three receipt sources:

- `commit_invalid(origin, record, wall)` consumes a fresh terminal-invalid
  classification, including expired rediscoveries. Remove any deferred row
  for those exact bytes in the same transaction. A competing row with the
  same claimed ID and different bytes retains its independent classification
  and retry obligations. This receipt creates no authoritative replay fence.
- `defer(origin, record, metadata, wall)` commits an encrypted retry row. Page
  commit independently checks exact ID and raw bytes still match the row.
- `commit_trusted(origin, record, wall, write_effect_and_fence)` calls the
  supplied synchronous closure with only a borrowed immediate transaction.
  The closure writes the authenticated origin fence and idempotent effect
  together to owner tables. Its contract forbids touching the cursor or
  deferred schema, changing transaction control, re-entering the cursor
  store, or any external effects. Its successful outcomes are `Applied` or
  `AlreadyApplied`. A storage or application error, or panic unwind, rolls
  back both the owner writes and cleanup. Remove the deferred key by exact
  origin and authenticated event ID on successful closure, regardless of
  its previously stored bytes. Return an accepted receipt only after commit.
  The fixture writes a synthetic effect and fence to prove rollback and
  idempotence.

The store hashes received bytes itself for each receipt and binds exact
origin, raw stamp and ID. Receipts are private typed boundaries for the
serialized owner. Actual flight, origin, role, policy and replay checks
remain the caller's obligations. A failed database/deferred write, callback
failure, stale cursor or saturation creates no claimed receipt or cursor
progress. A crash after trusted disposition and before the later cursor
transaction replays the page. The caller's effect/fence closure handles an
exact already committed result idempotently and returns a transient receipt.

## Deferred storage and service

Use `(origin, event_id)` as the durable deferred key and retain exact bytes,
raw stamped second, received authentication status, expiry, retry-not-before,
bounded failure step and revision, with an optional fixed 32-byte prerequisite
when verified. Exact raw bytes and stamp coalesce. No deferred row grants
authority. Retry reads expose original bytes and typed status as hints for
mandatory reverification. `defer` rediscovery cannot consume a retry step,
postpone eligibility, reset failure count or downgrade verified status.
Authenticating the same bytes may replace bounded outer expiry with signed
inner expiry and add its exact prerequisite without postponing eligibility.
The owner supplies expiration validated against the applicable envelope,
with a maximum 90-hour outer horizon from qualified admission. A persisted
verified expiry stays tied to exact authenticated bytes.

Different bytes for an existing deferred ID return `Conflict` without
advancing the page. The owner can retrieve that exact bounded existing row
without waiting for retry eligibility and reverify both candidates. A valid
terminal effect for the incoming event takes `commit_trusted` and atomically
retires the old key. An incoming candidate with a newly authenticated outer
signature and computed ID may replace a competing row while preserving the
existing retry step and earliest eligibility. `replace_conflict(snapshot,
incoming, verified_metadata, wall)` requires fresh exact authentication by
the owner, matches the old revision and bytes, checks stamp consistency if
the original row was also authenticated, and checks capacity before replacing.
A terminal-invalid incoming record may instead take `commit_invalid` while
leaving independent bytes intact. If neither record can yet be authenticated,
the cursor stays suspended and the prior row remains on its finite bounded
retry schedule. A definitive old invalid or expired classification retires
it after exact verification and wakes the page. This permits a valid incoming
record to resolve a false claimed ID as soon as its own signature can be
checked, independent of package, room or command prerequisites.

Authentication status is `Unverified`, `OuterVerified` or `FlightVerified`.
Outer verification requires its signature and recomputed event ID and uses
bounded outer expiry. Flight verification additionally confirms the exact
inner envelope and uses signed inner expiry, with an optional prerequisite.
Authenticating incoming bytes can replace a competing unverified or
outer-verified row. Replacing a flight-verified row requires full flight
reverification of the incoming bytes and does not downgrade its status.

Each origin reserves 256 rows and 16 MiB of exact event bytes. Check both
limits in the write transaction after coalescence. Enforce individual row
limits. Saturation returns `Backlog` without eviction. The owner treats a
saturated held page as unavailable for scheduling and continues retry work.
Exact completion or freshly verified expiry frees a slot and makes the held
page eligible anew. On reopen the owner derives capacity from the stored
rows rather than an ephemeral counter.

`due(origin, wall)` selects at most one eligible row ordered by `(retry_at,
event_id)`. Initial defer sets `retry_at = wall + 1`. Only an actual failed
attempt calls `retry_again(snapshot, wall)` and advances the capped failure
step to schedule 2, 4, 8, 16, 32 and recurring 60 seconds. Compare the snapshot
revision and exact identity before mutation. The owner `wake(origin,
prerequisite, wall)` makes matching verified entries eligible immediately
and bumps only revisions whose retry time changes. A stale snapshot cannot
advance the failure step or delay a woken row. A rebased clock makes pending
future retry timestamps due without extending any expiry.

The retry owner obtains a private `RetryProof` by reclassifying the exact
snapshot bytes through its verifier. The internal constructor binds origin,
event ID, SHA-256 of those bytes and the revision, plus `Invalid` or
`Expired`. `finish_retry(proof, wall)` rechecks that binding and matches the
current revision. An expired result still requires the qualified wall to
meet the fresh exact expiry threshold. No persisted unverified hint alone
can delete a row. Trusted acceptance instead uses `commit_trusted`.

`ServiceTurn` applies to one origin at a time. It alternates a ready page
and a due retry when both are continually ready, with retry preferred at
start and reopen. An origin turn offers at most eight pages and eight retries
before yielding. `page_ready` means attributed work that can progress, so a
quarantined, saturated or still conflicting held page is unavailable until
its qualifying state changes. An empty or unavailable lane lets the other
lane run within its budget. The future owner visits configured origins in
round-robin order and advances to the next on yield. Readiness and turn
counts carry no authority. Owner reopen starts retry-first, then serves a
ready page after one retry if both are available. Successful retries which
run before recurrent page activity and repeated reopen leave later pages
serviceable. Older due rows outrank newly deferred rediscoveries.

## Validation

Exercise a boundary-second tie repeated across close and reopen, new ties at
that second, exact lower and upper cutoff, stale revisions, a fully accounted
short terminal page, raw invalid counted before classification, wrong orders
and bounds, missing receipts, and a visible-count contract violation. Prove
clock regression prevents each time-dependent operation from mutating and
rebase puts retry work back on the corrected timeline. Force a write failure and prove no cursor
advance. Prove the effect/fence idempotent after page re-read and exact atomic
removal of a deferred row. Check deference coalescence, encryption, all
backoff intervals, wake, verified expiry, origin isolation and conflicting
raw bytes. Fill the reserved rows with bounded small records and check that
saturation has no silent progress. Simulate sustained pages and deferred
rediscovery around reopen and demonstrate older due work and held pages progress. Add a
release-only opt-in local benchmark reporting transaction latency and workload
bytes for full page disposition and due/retry. This does not measure socket,
relay or real flight admission. The actual carrier and enforced relay contract
remain separate activation gates.
