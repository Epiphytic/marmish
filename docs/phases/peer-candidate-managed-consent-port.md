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
   fence. Reuse the actual ACK wall in settlement. Scope/clock
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
exact same port and acknowledgement, compare it with the original
attempt's exact carrier and generation, and consume the settled token
through the existing `wake_installed_delivery` proof. Keep the same
original native window and check it before wake and immediately before
the wake transaction commits. The wake transaction already verifies
its local owner, account, origin, selector, carrier and durable ACK
wall and updates only existing FlightVerified retries bound to that
origin and Consent carrier. Add a narrow last-commit hook to that
transaction path and preserve existing callers via a no-op hook. It
updates no rows belonging to other prerequisites and creates no
work. No opening or processing of a deferred Delivery occurs here.

Return distinct typed outcomes for unavailable, definitely not sent,
negative ACK, possibly exposed, and durably settled. A durably settled
outcome can carry a fixed wake status (completed, including zero
eligible rows, or failed). A failed wake keeps the ACK and deferred
retry time intact, including rollbacks after an update but before
commit. Preserve the periodic retry as the later recovery path. Drop
the token after a failure and never wake on a foreign or stale ACK.
If the original authority stopped after settlement, retire the port
and suppress the wake without undoing the ACK. Distinguish a failed
wake from an uncommitted or indeterminate settlement. Report only
bounded error categories. Neither the permit nor the acknowledged
or settled capabilities escape the one turn.

## Source ownership and validation

Own one new child alongside `peer_recipient_port.rs` and its
declaration, narrow accessors within `peer_managed_consent_turn.rs`
and `peer_candidate_native_fence.rs`, and a small wake fence in
`peer_installed_delivery_retry.rs` / `peer_cursor_retry.rs`. Reuse
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
  caller's bounded turn and separately report the relay observation.
- Seed an already deferred retry through the existing checked
  Delivery fixture path. If necessary only the fixture lender reads
  the saved Consent early so an independent founder can prepare the
  exact v2 Delivery ahead of the ACK. Keep its Welcome opaque to the
  recipient and perform zero recipient Welcome effects. Keep a
  separate unrelated deferred retry. A positive ACK wakes only the
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
  in this fixture.
- Run exact scoped registered WSS, existing managed checked-ACK and
  cursor fence tests, formatting and strict candidate and stock
  builds. Report actual coverage, timing, and any fixture-only
  evidence explicitly. Review the exact code with Claude Code in
  Auto mode before publication.
