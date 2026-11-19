# Retire an authenticated expired managed Delivery

The Linux AMD64 candidate must retire an authenticated expired Deferred head
before starting the Manual recipient scheduler. Exact dormant source
`35db4bb` selects one row by due time including expired rows. The live
managed package proof refuses at expiry, so that row otherwise remains
charged and prevents later rows from progressing. The existing
[installed Delivery contract](peer-installed-delivery-ingress.md#exact-dispositions)
and [managed package design](peer-candidate-managed-consent-delivery.md)
already specify separate retirement-only historical authentication.

## One bounded retirement turn

- Keep due acquisition order, cap, one-row limit and existing SQL wall
  behavior. Its successful selection is a separate transaction that advances
  the configured origin and Consent floors. Retirement failure must preserve
  the floors, row and its revision from immediately after that acquisition.
  Use the same registered owner, cursor, Source and qualified port. Renew the
  current physical observation and require exact account and origin.
- Factor the existing read-only `Checkpoint::observe_saved_delivery`
  classification. Produce a private non-Clone `ExpiredSaved` for managed
  observation only when its fresh checked wall is at least the saved cutoff.
  Keep existing live and legacy fixture dispositions. The witness borrows
  the exact opaque saved Snapshot, records the current port witness, exact
  queued Consent and outer signed Delivery, and records the checked wall.
  `read_installed_consent` checks both floors without advancing either.
  Reauthenticate the exact outer ID, stamp and signed ancestry. Require
  current Manual v2 Unknown, no Held or terminal replacement, and
  `Snapshot::require_current_flight` against the exact Consent prerequisite.
  Reuse the stored checked cutoff only as a candidate pending independent
  rederivation. Unsettled or acknowledged Consent may own an expired row;
  neither state authorizes other effects during retirement. This depends on
  the installed layout retaining the exact Offer, queued Consent and original
  stage while the Deferred row owns that ancestry, including after Offer or
  package expiry. The present installed layout refuses its general sweep. A
  future reaper must preserve or explicitly resolve that ownership first.
- The same port opens a borrowed expiry-only checked route callback from that
  witness. Verify the current port scope exactly as `use_delivery_native`
  does. Privately construct a fresh five-second cleanup window capped by the
  real physical and qualification horizon, Ready, sticky stop, dispatch
  Fatal and cancellation. It runs on current anchored wall. The signed
  cutoff cannot cap the cleanup window since the cleanup starts at or after
  that cutoff. Keep this window inside the retirement owner and require it
  before every effect. Expose no general new route or historical time knob.
- Borrow the exact quota/profile and reuse the charged encrypted package
  snapshot mechanics in `peer_candidate_package_owner.rs`. Add a private
  expiry-specific read operation dispatched only through
  `ReceiverFiles::inspect_snapshot`. The copied native store must open
  with the same no-create managed SQLCipher settings and guarantee no
  local group. Call only the existing read-only
  `OpenStage::read_provisional` at the sealed historical sample
  `cutoff - 1`, after checked subtraction. The dispatcher/open/copy,
  charge, closure, cleanup, original full re-digest and all file identity
  checks use the actual current bounded window. Retain exact native package
  ID, digest and expiry equality with the committed Artifact, plus original
  file/charge/source identity. Reuse the shared copy mechanics for live and
  retirement with private typed read operations. The retirement operation has
  no signer effect, issue/finalize operation, or original native open.
- Rebind the retained exact incoming Delivery using the current queued
  Consent signed frame, retained Offer and the exact copied package via
  `Transcript::verify_delivery_with_package` wire verification at that same
  historical sample. The retirement branch never calls
  `read_managed_queued`, `confirm_managed_queued`, `window.require_offer_limit`,
  `verify_exact_delivery`, `Native::require_same_turn` or
  `PackageProof::require`. Those live paths keep their existing cutoffs.
  Recompute the complete `delivery_expiry` with signed Delivery bounds,
  replay horizon, retained Consent/Offer bounds and verified package
  lifetime. It must exactly equal the saved cutoff. Keep the ephemeral
  verified Delivery and package representation inside this verifier and
  return a deletion-only proof from `Snapshot::reverify` with
  `RetryFinding::Expired`. This authority cannot create
  `PackageProof`, `MeasuredDelivery`, Held, Welcome or a page receipt.
  A wrong, corrupted or unavailable ancestor refuses retirement and leaves
  the row charged. No fallback from uncertain ancestry to invalid.
- After snapshot removal recheck the original extent, inode and full digest
  under the same quota/current window immediately before the write. In the
  same bounded current-port turn use `finish_retry_guarded` with source,
  both floors, exact snapshot and exact queued row revalidation including
  attempt and ACK fields. The commit wall comes from a fresh actual window
  sample at or above the checked observation, both floors and recomputed
  cutoff. Advance both floors only with the exact deletion. Recheck the
  same current window immediately before commit so any refusal rolls back
  deletion and both floor changes. Never apply a historical wall to quota,
  route, SQL or scheduler effects.
- Return a categorical durable retirement result. No ACK, retry deadline
  rewrite, transport publication, Welcome dispatch, stage deletion or
  custody transition is authorized. The next ordinary due acquisition can
  select the later row after successful retirement. If the signed cutoff is
  crossed while an existing live turn runs, let that turn lose its current
  authority and re-observe the row through this branch on its next turn.
  Use four categorical results. `Retired` records durable deletion.
  `Reobserve` covers a stale row or an observation still below the cutoff.
  `Transient` covers lost window, cancellation, temporary quarantine, SQLite
  busy, and clock refusal, including a fresh window below the physical
  observation. The actor cools transient outcomes through its existing
  bounded policy without treating them as permanent disablement.
  `Stop` covers missing, corrupt or unverifiable ancestry, package or local
  authority. It forbids a sweep past that row. These results give the eventual
  actor enough information to avoid an immediate reselect loop. Daemon
  mounting and broader scheduling stay out of scope.

## Scope and qualification

Own the installed Delivery retry/factored managed owner, private checked
saved-route aperture, and narrow managed package/dispatcher read leaf.
Keep layout, schema, stock retirement, wire, installer and actor unchanged.
Use one isolated private candidate proof. Select a genuine signed founder
Delivery with its effective cutoff a few seconds ahead after the fixture
is ready. The signed expiry includes the existing 300-second safety margin.
Persist live Deferred first, observe exact cutoff through real time, then
retire via real registered WSS, quota, SQLCipher and MDK. Count no original
native open, signing, Welcome, outgoing Peer event or new stage change.
Verify after retirement that an independently due row remains eligible,
the retired ID is absent, and both floors are current. Use a distinct valid
Consent and charged stage for a second FlightVerified row. Record total and
bounded native turn elapsed times as local observations. The deterministic
standalone matrix uses a fixture-only hermetic wall to cover exact equality
at cutoff, after `latest_delivery` and after package lifetime, without a long
sleep or a production clock override. Route/window and SQL checks in that
matrix must use its current sample and the native artifact authentication
only its sealed historical sample. Do not infer those coverage points from
the short elapsed WSS case. Include stale replacement and rollback of both
floors on a precommit refusal. Exercise missing stage or queued ancestor,
wrong owner/port, block/cancel, artifact or retained lexical/cutoff drift,
and current-window loss before commit. Keep the failed row charged and
prove the failure did not delete peers' rows. Prior seven-scenario WSS and
stock strict checks remain relevant. Require independent Claude Code Auto
review of exact source before handing off for composition.
