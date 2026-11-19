# Publish managed Consent through the registered recipient port

This increment implements the Consent turn in the approved
[managed package, Consent and Delivery phase](peer-candidate-managed-consent-delivery.md)
at source `65ad6fb`. The
[managed Offer and Consent follow-up](peer-candidate-managed-offer-consent.md)
already supplies checked admission, package checks, durable queue and
attempt writes, and checked ACK settlement. The
[Manual actor mount](peer-candidate-manual-actor-mount.md) calls for one
actor-owned port and checkpoint. Its current
`peer_installed_recipient::hold_port` intentionally holds the port idle
and stops on the first record. This increment makes the exact send
callable by that future service loop and exercises it over real private
WSS. It leaves the actor, daemon startup, scheduling, configuration,
qualification, and runtime gates untouched. It adds no stage or outbox
allocation, sign, keypackage, native Welcome, activation, or Pending
authority.

## One bounded publication

Add a Linux AMD64 candidate/test method on `PeerRecipientPort` that
accepts the actor's exclusively borrowed `RecoveredOwner`, its
registered quota, a `Selector`, and the existing readiness and stop
inputs for a native window. Keep the command inside the Peer module.
The caller supplies no frame, account, route, clock, attempt, port
identity or ACK. The method visits both `use_current_native` and
`use_accepted` on its own port. It holds the same mutable port and
owner throughout. There is no generic signer, test permit, second
checkpoint, new issuer, parallel task, reconnect or second attempt.

1. Read the exact managed queue through a narrow owner method using
   `Checkpoint::read_managed_queued`, the owner's original
   `Source`, and fresh `NativeRouteWindow` account, origin and wall.
   Absence, a settled row or a retry not yet due yields unavailable,
   without signing or any physical publication. Use an opaque
   `ManagedQueued` challenge. Build one native window from a fresh
   observation of the same exclusively held port. Clamp the signed
   end to `queued.target().deadline()` and retain the existing
   five-second native budget, readiness, stop, fatal, certificate,
   qualification and route checks. Run `reserve_managed_consent`
   to re-attest the charged package and reserve one exact attempt. An
   intervening mutation is refused by existing durable comparisons.
   Expired or invalid scope never reaches the send.
2. Derive the private `ControlSendPermit` entirely inside the same
   port turn from that sealed attempt, its actual port scope,
   exact frame, sha256, carrier and attempt sequence. Borrow the
   attempt's bytes; the owner retains the `ConsentTurn`. Pass the
   minimum of the physical horizon and the remaining original native
   budget as the absolute Tokio deadline to `publish_control_with`.
   The deadline derives from the existing continuous clock and is
   computed without starting a second window. Preserve the existing
   physical and signed wall clamps in the socket writer. Use the
   cheap memory/state and clock check on every existing send fence
   and post-ACK permit check. Check exact operation and live scope
   there. Do no filesystem reads, digesting or SQL in that
   predicate. Resumed polls use the original continuous clock and
   fail after suspend or expired readiness. Existing fatal/route
   cancellation wakes pending raw work, and the clamped deadline
   also bounds stop/readiness changes if a pending wait needs a
   timer wake. The actor retains physical stop ownership.
3. Use the existing once-only post-`start_send`/pre-flush barrier.
   From it call `managed_before_flush` with that same owner, quota
   and `ConsentTurn`. Re-digest the original staged package, check
   its witness and re-read the exact durable attempt under the
   existing SQL fences. Renew no authority after that callback.
   A failed barrier retires the port and never polls the physical
   Consent flush. Preserve the distinction between staged bytes and
   bytes that may have reached the relay.
4. On an exact positive relay ACK, call `use_accepted` on that same
   port to obtain a fresh scoped `CheckedAck`. Consume the turn
   through `settle_managed_consent`, which compares account, route,
   carrier, generation and exclusive deadline, re-digests, and
   enters the exact attempt CAS with the old window as precommit
   fence. Copy the exact attempt `Operation` before consuming the turn
   so the wake can compare it with the second scoped observation. Reuse the actual ACK wall in settlement. Scope/clock
   failures after physical exposure retire the port and result in
   possibly exposed. A negative relay ACK never settles. Lost,
   mismatched or late ACK, aborted exposure and local settlement
   failure never synthesize success, even if the frame is in
   history. Keep the existing persisted attempt, bytes and retry
   time. A later eligible owner may use the normal retained-frame
   retry only after the saved due time, using a new authorized
   incarnation. This leaf neither brings up that incarnation nor
   schedules a retry.

## Wake one existing deferred scope

Successful settlement alone returns the non-Clone `SettledConsent`.
Synchronously use a second fresh `use_accepted` observation of the
exact same port and acknowledgement and compare the copied attempt
`Operation` and exact carrier. Require the original native window
before wake and immediately before commit. One-use authority comes
from the consumed turn and durable CAS, then the consumed settled
token. The bounded relay acknowledgement is borrowed only inside
these two scoped observations and dropped on every return. During an
ACK wait the raw reader observes physical route or fatal cancellation
and the clamped deadline. Stop or readiness loss without such a wake
is detected at the post-ACK permit recheck or the original deadline.

The checked wake is currently reachable only through the test-only
Delivery ingress mount. Extract its transaction into one child of
`peer_pending_custody_checkpoint` under
`all(any(test, marmish_peer_receiver_candidate), target_os = "linux",
target_arch = "x86_64")`. Use existing source and local-owner checks
and `custody_context`. In the transaction, verify the epoch, wall and
quarantine with `Context::verify`. Check the exact installed parent
with `installed::check_parent` and the exact signed queued ancestry
with `Context::exact_consent`. Reject a missing parent or queue,
terminalized or custodied selection, carrier mismatch, or missing,
different or future durable ACK wall. Reuse the existing typed cursor
error conversion. The candidate path needs none of `Ancestry`, saved
Delivery, Delivery ingress, Held construction or native Welcome
machinery. Keep the legacy test call using this same implementation
with a no-op final fence. Make the managed port call consume the
settled token after its independent checked ACK and original-window
checks.

Extend the internal cursor wake with a last-commit hook and preserve
the old signature as a no-op wrapper. The existing indexed predicate
updates only existing FlightVerified retries of that exact origin and
Consent carrier, with a retry time strictly in the future. It creates
no work and updates no other prerequisite. An injected precommit
refusal rolls back the clock, retry time and cursor revision as one
transaction. No opening or processing of a deferred Delivery occurs
here.

Return distinct typed outcomes for unavailable, definitely not sent,
negative ACK, possibly exposed, and durably settled. A durably settled
outcome can carry a fixed wake status (completed, including zero
eligible rows, or failed). If authority stopped after settlement,
retire the port and suppress the wake without undoing the ACK. A wake
transaction failure has its own outcome, including storage ambiguity.
Drop the token and preserve the ACK. A settled row never re-enters
publication, and there is no wake-only replay. After a missed wake or
interruption between transactions, the existing independent deferred
Delivery can run when its own persisted retry is due, with the current
backoff capped at 60 seconds for each reschedule. Report only bounded
error categories. No permit, ACK, or settled capability leaves the
turn.

## Source ownership and validation

Own one new child alongside `peer_recipient_port.rs` and its
declaration, narrow accessors within `peer_managed_consent_turn.rs`
and `peer_candidate_native_fence.rs`, and the wake extraction and
minimal transaction hook in the checkpoint and cursor files described
above. Reuse
private permit fields and the existing raw writer. Keep the prior
fixture and legacy ingress paths test-only. Extend
`peer_managed_consent_wss_tests.rs` only as needed, adding a separate
child test file if the new cases obscure the existing ACK proof.

- Use the existing registered, disposable private TLS WSS fixture,
  actual NIP-42 account authentication and checked live v2 Offer.
  Obtain a real registered quota and managed package, persist the
  v2 queue and run the new non-fixture wrapper on that same port.
  Compare authenticated relay Consent flow and stored exact carrier,
  bytes or digest, and attempt/ACK persisted readback. Measure the
  caller's original five-second turn and remaining margin including
  the package check, reserve, send, settlement and wake. Report the
  relay observation separately.
- Seed the matching deferred retry before opening the send window
  using `observe_installed_delivery` and `finish_installed_delivery`
  on the same managed checkpoint. Follow the existing
  `peer_installed_delivery_wss_tests::observe_delivery` and real
  stage-attestation pattern. A fixture-only lender can read the exact
  queued frame so an independent founder prepares authenticated v2
  Delivery while Consent is still unacknowledged. Check that the
  observed prerequisite is the actual managed carrier before using
  that row as evidence. Reuse the existing checked due-only retry to
  obtain a future retry interval longer than the send window if
  initial backoff is too short. Keep its Welcome opaque and perform
  zero recipient Welcome effects. Use a separate checked prerequisite
  for the unrelated retry, or a cursor-local fixture sentinel scoped
  to a distinct prerequisite and explicitly report that oracle. A positive ACK wakes only the
  exact preexisting matching row. Read back ACK, matching retry
  time/revision, and unchanged unrelated scope. Repeating the same
  selection causes zero additional Consent writes or wakes.
- Exercise the real managed pre-flush barrier with a test-only staged
  image drift after the message is staged. Prove the callback ran,
  the port retired, the relay saw zero Consent frames, and no ACK or
  wake occurred. Use the existing faulted negative/lost relay ACK
  cases on the new wrapper; assert exact received bytes once,
  no settlement/wake and durable unchanged frame/attempt on reopen.
  Lost ACK must retire the port. The fixed five-second native turn
  remains in force. Reuse the prior 60-second exact retry validation,
  without adding a new live retry round or weakening real clocks.
- Arrange one post-ACK wake refusal using a test-only cut at its
  transaction fence. The genuine ACK must remain persisted while
  both retry and clock effects from the failed wake roll back.
  Exercise stop or expiry on the original window around the accepted
  ACK with an owned fixture hook. Check persisted time floors, no
  false settled outcome, and physical stop/retirement. Reuse the
  existing exact stale and foreign ACK tests for dictionary-free
  proof of scope. Assert no native recipient Welcome work anywhere
  in this fixture. Reuse the existing
  `fixture_late_acknowledgement` pattern for an owned accepted-ACK cut.
- Run exact scoped registered WSS, existing managed checked-ACK and
  cursor fence tests, formatting and strict candidate and stock
  builds. Report actual coverage, timing, and any fixture-only
  evidence explicitly. Review the exact code with Claude Code in
  Auto mode before publication.
