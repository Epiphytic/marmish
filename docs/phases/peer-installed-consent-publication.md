# Installed Prepared Consent publication

The [installed recipient design](peer-installed-recipient-daemon.md) requires
an exact durable Prepared Consent to cross the authenticated WSS port before
Delivery can advance native custody. The existing outbox durably reserves an
owner-local `Attempt`, and settlement compares and sets `ack_at`. The real
`PeerRecipientPort` checks the TLS route and classifies exposure. This Linux
AMD64 library test increment connects those owners. The daemon mount, charged
native stage admission, Delivery and Welcome remain separate.

Claude Code Auto approved this design at `b9ba9076543e5bd73344bd1e9801c43b7228d6b8`,
session `1d9275f2-33c9-49f1-8a7b-e41a796a468b`.

## Authority and order

1. Add a test-only `PeerRecipientPort::publish_prepared_consent` orchestration.
   It borrows the installed fixture owner and the same mutable port, and takes
   an exact Offer selector and transport deadline. It returns `Unavailable`,
   `NotSent`, `Rejected`, `PossiblyExposed`, or `Settled`. `Unavailable` means
   there was no eligible reservation. Local error types should preserve fixed
   refusal categories such as Expired, Stale, binding and storage failure. The
   caller supplies no frame, wall, origin, OK packet, or arbitrary send fence.
   Use the port's private `check_observation` inside the method before touching
   the outbox. Compare the port account, raw normalized origin string and
   `Route::configured(origin)?.id` to the checkpoint scope. On the checkpoint
   side require `Layout::InstalledCustodyV2`, `Some(ReceivePolicy::Manual)`,
   exactly one route, same `Source`, and that route's current custody epoch and
   durable wall and quarantine fences before recovery or reservation. Keep the
   installed checks on every authority turn. Existing generic outbox and
   `PreparedConsentOwner` entry points keep their scoping.
2. Add a Linux AMD64 library-only child of `PreparedConsentOwner` with an
   installed publication fixture opener. Open a capped existing cursor on
   the same `Paths`, account, route and lease, using the witnessed normal
   stock opener and installed v2 custody attach with Manual. Use that
   fixture's `InvitationState::prepare_provisional_with_quota` with the
   existing disposable fixture quota after a real checked Offer and
   reservation. Use `SnapshotArea::read` from the child to create the existing
   sealed `NativeAttestation` with the observed native package, inode witness
   and image digest. Only the test child can obtain that attestation in this
   layout. It is explicitly a disposable snapshot read, separate from the
   future live metered read. Queue the stage with the existing
   `inspect_prepared_consent`, `sign_prepared_consent`, and
   `queue_prepared_consent` sequence and verify the exact saved transcript.
   Keep the old `attest` refusal of installed custody and test it. Reopen the
   same capped cursor with a fresh `Source` for retry. No fixture constructor
   or path becomes available outside the library test; no daemon can reach
   this test proof.
3. Recover the exact queued row, verify its native package and signed
   transcript with `verify_queued_consent`, and reserve once through
   `reserve_prepared_attempt`. The reservation commits its incremented
   sequence and retry time before creation of a permit. The opaque `Attempt`
   retains its checkpoint-instance Arc. Add narrow read-only attempt
   accessors for founder, account and deadline bounds. Use
   `Operation { id: attempt.carrier(), generation: attempt.sequence() }`.
   Check the signed transcript deadline against the signed Offer and artifact
   bounds independently. The permit deadline is the minimum of the verified
   applicable bounds. Only the port turns that verified exact attempt into a
   socket-scoped permit. Keep its constructor private. Its author, founder,
   normalized origin, exact bytes and carrier, operation, and deadline come
   from the current observation and verified row.
4. Keep readiness, start and every flush poll subject to the existing route,
   cancellation and deadline fences. Add a once-only `BeforeFirstFlush` hook
   to the existing staged `SplitSink` path, after in-memory `start_send` and
   before the first potentially exposing `poll_flush`. An existing caller
   that supplies no hook keeps its current behavior. On the installed path
   run one read-only exact SQL row/stage/owner check and one bounded native
   digest check at this hook. The SQL read must inspect the cursor and
   custody wall floors without advancing either, and must require current
   unsent state and exact transcript and ancestry. Drop each transaction
   and native handle before returning from the hook. Refresh physical route
   and deadline again after the hook. Do not rerun SQL transactions or hash
   the stage at each async wake. The orchestration exclusively borrows its
   SQL and stage owner through the send turn. Rerun exact SQL and native
   checks after the ACK, before attempting settlement. Preserve the first
   typed fixed local fence error in the outward result when the generic
   socket failure carries only a fixed transport category. A cut before
   first flush has no possible wire exposure with this exact sink and retires
   the socket. Set possible exposure before the first underlying flush poll.
5. Only `ControlOutcome::Accepted` with an `Acknowledgement` produced by
   the same current port can reach settlement. Make `CheckedAck` fields
   private to the port, compare the observed account, normalized origin,
   carrier, and operation to the exact attempt inside that module, and use
   the port's sampled wall. Include fresh custody and scope checks before
   calling the transactional compare-and-set. Keep the existing fixture
   `MatchingAck` path independent, and share its CAS implementation through
   a library-only private port settlement bridge. The shared standalone
   cursor source does not import the port; use the existing library fixture
   loader pattern for any method requiring a port type. No externally
   accessible constructor can mint this ACK witness. Retire the port if
   positive acknowledgement fails to settle durably.
6. A refusal before reservation leaves the queued row unchanged. Failure
   after reservation retains the incremented attempt and durable retry
   time. Explicit relay rejection is a wire exposure and leaves the row
   queued. Possible exposure and failed settlement also leave the row
   queued. A fresh owner can later reserve a new attempt for exactly the
   saved bytes, with the same carrier and a higher attempt sequence. A
   previous `Attempt` or `Acknowledgement` cannot attach to the new owner
   or socket. Keep Delivery application and changes to native membership
   out of this turn.

The installed receive policy accessor is read-only. The permit predicate at
each poll is an in-memory exact operation comparison. The blocking native
digest is fixture-only and is capped at the existing 16 MiB stage-image limit.
The before-first-flush fixture verifies zero carrier writes at the private relay.
The generic permit digest checks the supplied buffer within the port. Independent
binding comes from the exact durable signed transcript and verified custody.

## Validation

Use disposable identities, the qualified private WSS loopback relay, the real
native Prepared artifact and capped SQLCipher. Confirm actual positive
publication and cold `ack_at` readback. Use relay-side capture of the exact
inner event bytes, or compare a SHA-256 digest and length taken before parsing,
to verify the complete queued-frame byte identity across send and retry. Let
a separate relay fixture receive the first exact write and drop its ACK. Wait
for the 60-second durable retry against the real qualified clock, with at
least a three-minute loopback leaf and applicable qualification window. Cold
reopen must resend the identical carrier under attempt sequence 2 and settle
once. Keep the wall and relay fixture authentic. No need to advance a mocked
clock or shorten the production retry policy for this case.

Test pre-reserve refusal with unchanged state, post-reserve fence failure
with a durable queued retry and zero private writes, negative ACK with one
write and no settlement, and accepted ACK with durable failure and retired
port. Cover wrong account/origin, native replacement, stale attempt, foreign
or retired ACK and expiry. The Combined Offer and Delivery cursor, managed
native charging, and actual daemon remain outside this proof. Keep the WSS
and cold retry fixtures opt-in. Run strict native all-target Clippy with
`installed-checks`, standalone cursor and focused transport regressions.
Obtain Claude Auto approval for the revised design and exact integrated code.
