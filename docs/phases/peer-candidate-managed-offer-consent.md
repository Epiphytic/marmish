# Managed Offer and Consent authority

This increment implements the store and borrowed authority portion of the
[managed receiver design](peer-candidate-managed-consent-delivery.md) on
`0b1f944`. It provides Linux AMD64 candidate leaves before the daemon
actor mount. Pairing this authority with the same live socket and consuming
settlement as a Delivery wake require their own detailed review.

## Offer and package

- Mount only the located Offer ingress for the candidate. Split the
  library's fixture macro so the standalone `peer_cursor_store` target
  still substitutes an empty mount and the existing Delivery ingress stays
  fixture-only. Compile `CheckedCarrier` and its located page/live borrow
  and route/account/wall/raw-event projections. `CarrierWitness`,
  `CarrierCoordinates`, witness matching and saved Delivery remain
  fixture-only. Decoding a Delivery enum branch classifies the wire and
  performs no Delivery effect. Keep raw fixture admission, conversion from
  caller input and fault hooks confined to their existing test/internal
  boundaries. Retain exact page/live receipt, signed v2 carrier, current
  Manual Unknown policy and reservation, and both time floors.
- Keep a single `RecoveredOwner` and its `Source`. The inventory's borrowed
  managed Consent functions use that owner, its exact registered receiver
  quota and a fresh `NativeWindow`. No cursor, register, or port is opened
  here. The separately reviewed caller will construct the window while it
  holds the same checked port. This increment does not assert pairing a
  detached `NativeWindow` with any port.
- Use `attest_queued_package(&ManagedQueued, ...)` so the caller retains
  the original opaque full-row witness. Internally copy only the sealed
  `PackageTarget` after checking exact queued state before and after the
  charged copy. Add `PackageProof::require` (owner and exact target/expiry
  operands, returning only borrowed native package bytes) and require the
  same checkpoint owner and quota, exact stage and artifact, current
  native package lifetime, and the proof's effective exclusive deadline.
  Callers cannot substitute raw package bytes and no proof is duplicated.

## Queue, reserve and settle

- For a fresh queue, check there is no durable outbox before calling the
  existing staged KeyPackage issuer. Use the sealed Prepared reservation
  and exact retained signed Offer from the current checkpoint. Re-digest the
  original immediately before signing and again before the queue write.
  Add proof-taking managed signer/verification entry points and share only
  the input-size, canonical transcript construction/verification and signing
  core. Keep legacy `NativeAttestation` signer/verifier and raw fixture
  helpers test-only. The managed producer is limited to installed v2 Unknown
  Prepared. Preserve row and wire formats.
- Use the existing queue transaction's exact Prepared revalidation and
  durable row insertion. Before its actual commit, after tentative writes,
  run a synchronous memory-only stop/time fence. Re-digest again and read
  the exact queued witness before returning. A failure after commit
  produces no in-memory authority and recovery reuses the stored outbox,
  with no new KeyPackage generation or signing.
- For a survivor, attest and re-digest while retaining the original
  `ManagedQueued`. Reserve in one immediate checked transaction. Verify
  the current exact authenticated v2 Offer, matching checkpoint and
  Source, Manual Unknown policy, not custodied, the exact stage/artifact,
  complete queued row including attempt/retry/ACK, both floors and current
  exclusive deadlines. Preserve the old compare-and-set transition and
  retry scheduling. A deferred retry, prior ACK or error drops the
  transaction without committing either floor or any row. On a changed
  row that qualifies, save both floors and run the final memory-only
  stop/time fence before commit. Preserve the legacy fixture API and its
  existing no-op semantics.
- The borrowed Consent authority retains the non-Clone current `Attempt`
  and the charged `PackageProof`. A later checked transport can project
  only the exact outgoing coordinates and invoke a fresh file re-digest and
  exact current-attempt read immediately before flush. This increment
  adds no socket initiation, writer or retry loop.
- Move the pure `CheckedAck::exact_attempt` and exact operation projection
  to a sharing boundary as needed. Use only a genuinely checked ACK from
  the owning port callback; the private fixture may pass the actual
  `CheckedAck` obtained using that existing mechanism. No unchecked ACK
  constructor. Immediately before settlement, check the exact ACK
  account/origin/operation/carrier and re-digest the metered original.
  In one immediate transaction enforce the same installed/manual/source,
  deadline, authenticated Offer, stage/artifact, exact current attempt and
  unacknowledged row guards as reserve. Apply the existing acknowledgement
  transition and both floors, then run the memory-only stop/time fence
  before commit. A successful transition returns the existing sealed
  `SettledConsent`. A stale attempt, replayed ACK, existing ACK, stop,
  deadline or file drift cannot settle earlier authority. Delivery ancestry,
  retry wake and saved payload are outside this cut, including for testing
  the new managed settlement.
- Any library-only mount coupling the real PackageProof to checkpoint
  internals uses the existing native fixture-macro boundary pattern. The
  standalone cursor harness compiles the shared SQL/wire portions with
  an empty macro. No substitute concrete proof, mock ACK or production
  raw/admission bypass is required.

## Proof

Use the opt-in Linux AMD64 registered SQLCipher subprocess for the actual
quota/owner path and the existing checked private WSS receipt path when
exercising settlement with a genuine ACK. Compare exact logical rows and
both floors, verify the original encrypted stage bytes remain identical,
and check signature count on refusals. Test two observations of one outbox,
prior ACK, stale attempt after a different reservation, replayed settled
ACK, deferred retry, foreign or restarted owner, custody-owned tuple, and
exclusive equality separately at Offer, Consent and artifact expiry. Use
actual rows and a bounded fixture clock/state perturbation at the named
cut when necessary. Exercise the last-guard rollback and post-commit
refusal/reopen preserving byte-identical signed outbox without re-signing.
Keep checked attempt and ACK construction private. Run exact stock and
candidate strict all-target lint, cursor/store regressions, and positive
candidate plus fixture-only negative compile checks. Record Linux timings
and file sizes, including any synchronous check invoked by the async port
fixture. Socket pairing and per-poll/no-flush proof are separate acceptance
criteria for the future transport increment.
