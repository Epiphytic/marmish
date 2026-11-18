# Exact native PostJoin publication after Peer approval

Prove the native boundary between durable `Activating` and a future accepted
room using opt-in tests, disposable SQLCipher stages and upstream MDK master
as resolved at `9a29d1e46f11bc8e421046d5a1e20b52b59795df`. Reuse the bound
join and decision in the [custody milestone](peer-pending-join-custody.md).
The checkpoint stays `Activating`. There are no changes to installed daemon
startup, transport policy, acceptance or heartbeat publication.

## Establish one native authority and capture

- Create a real two-account stage, authenticate the bound Delivery and reach
  durable `Activating` through an exact application decision. Use the existing
  strict custody verifier and image digest before driving native maintenance.
  Record the account, group, roster, local leaf index and hash, epoch, route,
  stage device and inode, activation generation and Welcome identity.
- Use the in-process founder as the test oracle. Apply any commit-bearing
  history for the exact group before EOSE via native `ingest_delivery`, with
  Group plane, exact endpoint and subscription ID. Mark subscription installed
  and EOSE for that group. Require the founder and recipient epochs to match
  at permit construction. This demonstrates the controlled fixture boundary.
  The subsequent WSS proof must establish whether its exact-room history
  fetch supplied adequate coverage. Relay EOSE by itself cannot establish it.
- Share one controlled wall source between MDK and the custody owner, bounded
  by the activation deadline and the readback window. Assert exactly one
  eligible PostJoin obligation bound to the recorded baseline leaf. Drive
  shortened due maintenance through a capture-only adapter, refusing the
  package publisher and scheduler. Native authors its own typed update.
- Before the first native drive, verify that no attempted target is present.
  Record at most one bounded candidate and cap observation counters. Return
  `TransportAdapterError::PublishEndpoints` with `RetryableUnavailable` and
  `pre_send: true`. Use the same classification for a deliberately refused
  attempt. This keeps the exposure bit clear and the original fanout pending
  with normal retry backoff. Afterward read back the retryable status, one
  attempt, pre-send classification, clear exposure and PendingPublication
  obligation. Classify any intentionally simulated unacknowledged
  send as `PossiblyExposed` after its recorded result. This phase may use the
  pre-send refusal for the exercised retry. `read_back` returns `NoAnswer`,
  since `NotFound` can abandon an unexposed fanout. `commit_size_refusals`
  reports no fixture-specific sizing obstruction for the qualified endpoint.
  Refuse generic `publish`, subscription, fetch, package and scheduler egress.
  An offered second or unplanned native commit ends the test stage with zero
  forwarding of that commitment. The adapter never grants authority from
  the request it observes.

## Verify, reopen and retry the exact fanout

- After capture, use a dedicated verifier. Bind the original device/inode,
  account, signed route, group, generation, roster, leaf index and baseline
  epoch/leaf hash to the durable application decision. Before confirmation the
  stable MLS epoch and local-leaf ownership must still agree. Native can have changed
  the KeyPackage lifecycle and image digest. Sample the native authority,
  maintenance and fanout rows in one `with_group_authority_snapshot`, and read
  `session().own_leaf_hash` at that cut. The earlier strict image digest and
  `slot_only` lifecycle checks apply only before native maintenance starts.
- Require exactly one `GroupEvolutionSemantic::SelfUpdate { trigger: PostJoin,
  obligation_id }` with the expected obligation, source epoch and local leaf,
  next target epoch, no removals, a signed ID and a pending reference. Match
  durable `OutboundFanout`, pending state and request. Verify the signed event
  and the qualified route. Bind the scoped permit to the exact operation,
  signed bytes, signed ID, pending reference, obligation, generation, deadline
  and endpoint. MDK narrows the forwarded attempt to one qualified endpoint
  with one required acknowledgement. Compare that exact per-attempt request.
  A retry can use the identical previously attempted candidate under the
  same durable authority. Refuse expiry, route or roster movement and
  superseded, confirmed or mismatched work. Scope each permit to one turn.
- Close after capture and reopen the encrypted stage with a fresh closed
  adapter. A blind broad redrive must yield no egress. Rebuild the scoped
  permit only after the dedicated verifier passes. Exercise a typed pre-send
  refusal and repeat with the same frozen bytes and signed ID. Deliver those
  bytes through the test sink to the founder's native runtime before returning
  success. Require durable confirmation, the same new stable epoch at both
  runtimes, an evolved recipient leaf hash, the expected roster and route,
  and the matching completed PostJoin obligation after at most one extra due-maintenance sweep.
  Use a post-confirm verifier that expects the new epoch and evolved leaf.
  A transport ACK alone is insufficient. The native stage is the only durable
  source of capture identity in this proof.
- Demonstrate the closed gate before approval and on different-inode stage
  copies, replacement package and unplanned native work refusal, and bound
  capture and expiry. Use one table-driven permit matcher test for wrong
  operation, endpoint, generation, obligation, expiry and bytes. Exact replay
  and no second semantic update apply to a single unrolled stage lineage.
  Restoring older encrypted contents in the same inode is beyond this proof.
  The WSS activation cut must persist the exact capture record in the custody
  database before first physical egress. A resumed WSS permit must match that
  durable record.

Run the complete proof explicitly. Include M2 custody, available M3 daemon and
ordinary command regressions and strict lint as applicable. Keep installed
acceptance closed until private WSS history, durable capture, recovery receipt,
active projection and startup policy have separate evidence.
