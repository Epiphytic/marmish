# Managed Offer and Consent authority

This cut implements the Offer and Consent section of the approved [managed
receiver design](peer-candidate-managed-consent-delivery.md) on `0b1f944`.
It makes the bounded leaves available to the Linux AMD64 candidate owner.
The existing actor remains the only future caller. Delivery ingress and
the daemon mount have their own reviews.

## Ownership and order

- Reuse `CheckedCarrier` from the same `PeerRecipientPort` to admit
  an authenticated v2 Offer. Compile only located carrier observations
  needed for this entry. Keep the raw `OfferInput` conversion constructor
  and common classify/record methods inside the checkpoint family, with
  a separate test-only `admit_offer_fixture`. Preserve page and live
  receipts, signer, route, binding, policy, quarantine, both wall floors
  and existing Manual Unknown reservation.
- Use the owning `RecoveredOwner` and its one `Source` for the entire
  turn. The inventory child borrows that owner and its exact registered
  `ReceiverQuota`. Use the native window created by the same checked
  port immediately before this operation. The checked transport method
  obtains the window internally while it owns the single mutable port
  borrow. Callers cannot substitute a different port's window. Accept
  the existing readiness and stop witnesses and the exact signed limit.
  Before each file/SQL effect sample the window and require the signed
  Offer/Consent/package exclusive deadline and open socket wall; use the
  earlier monotonic horizon for publication. Memory-only guards check
  stop/fatal/time on each write poll.
- Split mechanical review/qualification into store and checked socket.
  First share the queued store operations and a sealed
  `receiver_inventory::managed_consent::ConsentTurn`. Retain
  test-only `NativeAttestation`, `MatchingAck`, test fixture wrapper and
  raw signing helpers. Use `PackageProof::require` for the package
  operand from the charged copy, requiring the same checkpoint owner,
  exact stage, artifact, native validity and proof deadline.
  Only exact installed v2 unknown Prepared may use this managed producer.
  Package bytes are borrowed locally for the existing capped signer and
  exact transcript verifier. Keep old signature and row formats.
- For a fresh queue, require no outbox before issuing native material.
  Use existing staged generation/attestation and `inspect_prepared_consent`
  to recover the exact signed Offer and bound Prepared candidate. The
  leaf accepts a sealed `Reservation` and its current `PackageProof` and
  rechecks the original immediately before signing and again before
  `queue_prepared_consent`. Reuse the exact queue transaction and
  a memory-only guard before commit. Recheck the original and freshly
  read the exact queued witness before returning. A post-commit refusal
  returns no proof. A later turn can recover the durable outbox without
  generating or signing again.
- For a survivor, retain `ManagedQueued` through charged package
  attestation and re-digest. Then `reserve_managed_prepared_attempt`
  consumes it and checks the exact local owner, Manual Unknown result
  and installed signed v2 transcript. Compare the selected authenticated
  Offer and complete row (including attempt/retry/ack) inside the same
  write transaction used by the existing reserve CAS. Advance both
  floors only in that successful transaction. Failed check, ineligible
  ACK state, and deferred retry return no attempt. Factor a guard into
  the attempt change transaction for final memory-only stop/time
  checking before commit. Preserve legacy helper semantics.
- The inventory `ConsentTurn` retains a non-cloneable reserved pair
  of exact `Attempt` and metered `PackageProof`; only the existing
  checked port bridge reads the outgoing coordinates. Before flush,
  re-digest, check exact current attempt and resample. No sqlite or
  filesystem checks run inside the transport per-poll closure. After a
  positive route-checked `CheckedAck`, re-digest again, compare its
  carrier/operation/account/origin and current attempt, and use the
  exact acknowledgment CAS. Put the memory-only commit fence in that
  same transaction. A changed attempt, existing ACK, stop or expiry
  cannot be settled by an earlier reservation. Only a successful
  checked CAS creates the existing one-use scoped wake, and only an
  accepted re-observation from that same port uses it. A refused wake
  preserves successful settlement and ordinary due retries. Socket
  outcomes keep existing unavailable, not-sent, rejection and
  possibly-exposed categories. No second port or attempt is created
  inside a turn.

## Proof

In the actual registered SQLCipher package test harness, retain two
opaque observations across the first reservation and show the earlier
one cannot reserve or authorize before flush. Check exact row, both
floors, no new signatures and unchanged encrypted stage on refusal.
The checked socket composition must run the private authenticated WSS
fixture with the actual registered cursor and quota, exact positive
ACK, stale reserved attempt and replay of an old ACK after durable
settlement. Reuse the checked port, introducing at most a fixture-only
deterministic local state transition at the relevant boundary. Assert
wire event counts and unchanged durable state on each refusal. Never
make an alternate constructor for checked ACK or attempt. Reuse the
existing port refusal/retry/wake regressions and Cut 1 exact package
restart tests. Run stock and candidate all-target strict lint and
the fixture-authority negative compile probes at the final integrated
candidate boundary. Capture elapsed time and staged bytes in the real
Linux cases. This cut supplies no Delivery write or daemon entry point.
