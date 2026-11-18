# Terminal bootstrap after restart

The private signed relay retained the previous writer's Probe and Confirm while
the server Terminal runtime restarted. Before any new local Start, the fresh server
sent a Challenge to the old client alias. Its in-memory replay ledger starts
empty. The old Confirm cannot answer a new Challenge or authorize a terminal,
but the stale Probe takes the only in-flight slot from the new client.

## Admission and retries

`Readiness::observe` defines the live boundary. The exact inbox EOSE must make
`Acknowledged` become `Ready` on the pinned origin and connection epoch after
its own signed AUTH OK. Earlier EOSEs before the exact OK and foreign inbox
EOSEs do not open it. Later EOSEs cannot reclassify buffered deliveries. No
classification is needed for unverified `Lane::open`, which cannot use this
initial-Probe reader. Keep the same exact readiness and NIP-42 checks.

In the verified lane, `Ingress::read` transfers buffered events to `Lane.staged`
only when that readiness transition happens. Incoming events after the
transition use the live receive channel. Add `Lane::discard_stored_alias_inbox`
for the server's bootstrap lane only and call it once in `serve_inner` before
its first `next`. It fails on an unverified lane and discards only staged events
matching the exact inbox ID and AccountInbox plane. This discards those events
before native placement or acknowledgment, so a post-EOSE redispatch may still
be examined. Buffered old Confirm cannot answer a Challenge that has yet to be
created. Bootstrap client, live Confirm, enrollment Welcome and group readers
keep their existing behavior. Existing 256-event and 1 MiB staging bounds apply.

EOSE is a relay availability hint. A relay can replay a still-fresh Probe after
EOSE. That still consumes at most one of the existing eight replay charges and
can only elicit a new Challenge if all authenticated binding and freshness
checks pass. A Confirm from another transcript still grants no effects.

Keep the exact original client Probe nonce, body, timestamp and canonical rumor
ID. `Live::last` already owns the original Probe body until Challenge advances
it. Store one optional original wall second for the client Probe and clear it
on advance, expiry or retirement. There is no extra retained nonce, rumor
plaintext or serialized buffer. Add `Exchange::rewrap_probe_bound(&BoundSigning,
Time)`, requiring exact bound signer, unexpired Exchange, client role and current
`Body::Probe`. Use `Codec::wrap` with the canonical original body and timestamp.
That reconstructs the same `UnsignedEvent`, recomputes its canonical ID, uses a
fresh seal ciphertext and outer signing key, and returns both ID and frame.
Compare the reconstructed ID to the original `Prepared.id` before returning the
new frame for network I/O. The sender does not decrypt its outbound envelope.
`start_client` and existing exact `Exchange::retry` retain their established
contract. Only the scheduled `Bootstrap::retry_probe` uses the new method.

Keep the 2, 5, 10 and 18 second cadence and the existing 30-second absolute
client deadline. Lose the attempt if the server is not ready at the final retry
and no qualifying Probe arrives afterward. Connection rolls, remote delay or
relay suppression still can cause a no-Challenge timeout. Do not fall back to an
old frame on rewrap or publish failure. Stop after an authenticated Challenge
and keep all socket, deadline, EOF, grant and Command fences. The signed age and
binding, native room admission and retention limits are unchanged.

## Verification

Extend exact readiness tests for early and wrong-inbox EOSE, exact authenticated
transition and later duplicate EOSE. Verify the server eligibility seam removes
stored candidates, leaves acknowledged IDs untouched and retains post-ready
messages. Test independently decoded inner equality and distinct valid outer
IDs, obtaining the same replay charge across outer IDs. Cover rejecting wrong
signer, corrupted canonical ID, elapsed cutoff, revoked authority and any
authenticated Challenge. At the 18-second retry the original inner age remains
eligible. An old Confirm delivered live after a new Challenge remains invalid.

On the private signed relay restart the server Terminal runtime while the original
Probe and Confirm remain stored. Require no startup Challenge for the old alias
and warm Default into the same pane. In a separate controlled test, withhold
exact authenticated EOSE, then queue the first Probe to the server before the
held EOSE. Verify early EOSE before AUTH in the deterministic readiness test.
Use duplicate-ID suppression
and verify no live redispatch of the original event. Require a fresh retry
outer ID before the first server Challenge and successful attach within the
existing absolute deadline. Retain existing denied AUTH, EOF, concurrency,
capacity and shell-exit proofs. Record ready and retry latency separately.
