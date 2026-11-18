# Private WSS activation and durable acceptance receipt

Extend the native PostJoin proof through the qualified private WSS daemon
fixture, using two disposable accounts and their own encrypted state. Start from
an exact unexpired `Activating` row written by the existing manual or automatic
owner. End with one recoverable recipient acceptance publication and one durable
founder acceptance. Keep the fixture opt-in and loopback-only. Leave installed
admission, normal Command and Terminal routes, telemetry, and relay defaults
unchanged. Use upstream MDK master and record its resolved commit. Require code
approval of the native PostJoin gate before starting this phase.

## History and authority

Extend the existing Peer-only relay fixture with one explicitly registered
opaque transport group route. Each of the two fixture accounts authenticates
on its own TLS WSS connection. Admit raw Peer kind 7435 under the existing
policy. Admit encrypted group kind 445 only from those accounts with one exact
`h` route tag. The history subscription has one filter for kind 445 and that
same `h` value. Reject unauthenticated or unregistered principals, other routes,
extra filters and all other kinds. Close or reauthentication invalidates the
connection witness and any outstanding turn. Nostr routing tags select the
stream. Native verification establishes group author and message authority.

After issuing the Welcome and before the recipient's Group subscription, use
the founder's real stable native group to produce one `SendIntent::SelfUpdate
{ group_id }` under the same bounded WSS fixture. Require the exact signed
commit to be durably accepted by the relay and confirmed by the founder. Keep
its native source and target epoch evidence. Qualify that founder self-update
first with a real preceding Peer Delivery Welcome at epoch N. Record whether
native founder-side Welcome tracking is present before and after that commit,
and prove the founder has reached a stable N+1. A native refusal to create or
confirm that commit with the fixture's actual Welcome state blocks the later
steps until a different native state-bearing witness is reviewed. The relay
records exact accepted IDs at the Group subscription cut. The fixture compares
that fixed transcript with the ordered, bounded Group response through EOSE,
and verifies that the required state-bearing commit reached native ingest.
Exercise a relay that omits the commit and still emits EOSE. That case must
refuse even when the response is within its framing and size limits.

Compare both native stores before the recipient's publication permit. Require
matching canonical pre-rotation `EpochPin` ordinal and authenticator, two exact
roster accounts and leaf positions, the expected local recipient leaf and
credential, and the signed route. At subscription installation, call
`mark_post_join_subscription_installed` and retain the exact fetch ID from
`start_post_join_history_fetch`. After verified EOSE and processed deliveries,
call `finish_post_join_history_fetch` for that ID for `Fetched` and
`mark_post_join_eose` to advance maintenance. `Fetched` records a host report.
The transcript and independent founder pin supply the actual qualification.
For lost EOSE, cross the configured `eose_timeout` on the fixture clock and
require `expire_post_join_history_fetch` to yield `DeadlineExpired`. Refuse
truncation, conflicting duplicate IDs or order, overflow, or route loss.
Identical duplicate delivery cannot count as another native state effect.
The history claim is limited to the observed fixture subscription. Use one
stage and a serial owner while applying history. Keep application
effects isolated during activation and discard authority from any application
received before the relevant decision exists. Carry forward the M4 shortened
fixture maintenance timing and measure activation across the relevant grace and
quiet windows after the last authenticated state-bearing commit. The history
commit moves both accounts from N to N+1. PostJoin moves them to N+2 and the
acceptance receipt binds N+2 and its authenticator.

## Durable capture and exact WSS publication

Use a private fixture custody-schema epoch with one publication journal keyed
by the existing custody row identity and explicit publication kind (`PostJoin`
or `AcceptReceipt`). The existing row authority, checked stage witness,
foreign keys, transaction and no-overwrite rules carry across the new epoch.
Each entry also matches the exact generation, group and route. Keep one stage
and at most two journal entries for its current generation. The journal records
semantic identifiers plus the signed event ID and SHA-256 of the canonical full
signed bytes. Native fanout owns the bytes. Use the current 65,536-byte signed
request limit. Independently read both stores at each cold reopen.

For `PostJoin`, reserve the turn in the journal against the exact custody and
native obligation. An initial closed-gate maintenance drive can freeze the
candidate with zero WSS output. Capture its obligation ID, canonical source
and projected target epoch pins and recipient leaf witnesses, then read the
exact pending-reference/transport fanout association from the native store.
Persist that immutable association before granting a physical turn. Keep
opaque native handles in native storage and compare them there during reopen.
A restart before capture may recover exactly one verifiable frozen native
candidate. Ambiguous evidence or a same-inode rollback refuses publication.
A complete native rotation can be recognized without sending when independent
stable epoch, changed recipient leaf and exact completed obligation agree.

Persist the exact armed attempt before adapter egress, including the event
identity and frozen bytes digest. Permit one exact publish on the same admitted
socket witness, with the existing bounded turn deadline. Recheck the immutable
request at the adapter and the connection witness at correlated ACK. WSS loss
or lost ACK is uncertain exposure. Only the previously recorded capture and
armed attempt may authorize a subsequent exact native retry. An uncertain
result alone creates no capture or permit authority. Require the fixture to
store and deliver the first event to the other account even when it drops the
publisher ACK. Disable self-echo and recovery lookup in this fault path so
neither can confirm behind the test's back. On retry the relay verifies the
same signed bytes by event ID, deduplicates the event and returns its positive
ACK. Recognize already-completed native rotation before attempting another
send. Refuse mismatching bytes or scope, expired custody and policy changes.
The recipient and founder must independently reach the identical stable native
pin and expected roster with changed recipient leaf, and the recipient must
prove the exact completed PostJoin obligation. Relay ACK alone cannot promote.

## Active transaction and acceptance wire format

Add `Active` to the fixture custody schema. Its row is reached by one
`Activating` compare-and-set that rechecks phase, revision, generation,
deadline, canonical principals, role, registered route, enrolled room, stage
witness, confirmed native epoch and leaf. The same SQLCipher transaction
inserts the exact pending `AcceptReceipt` journal entry with fixed authored
time and expected canonical inner application event ID. The verified native
proof remains bound to the stage identity and group at the transaction boundary.
`Active` in this fixture grants this one receipt path. It adds no generic
capability, permissions, status or host telemetry authority.

Reserve inner `MarmotAppEvent` kind 30102 and require empty inner tags. Use
canonical compact JSON content of at most 1,024 bytes with fields in the order
shown below. Require integer `v` equal to 1, unsigned 64-bit integer `epoch`,
and exact lowercase hex types for the remaining fields, measured after decode.
Reject mismatched length, unknown, duplicate or missing fields, and inputs
that differ from canonical re-encoding. Use the app event's canonical `id`,
`pubkey` and fixed `created_at` from the reservation.

| Field | Decoded type |
| --- | --- |
| `v` | Integer 1 |
| `founder`, `recipient` | Distinct 32-byte Nostr public keys |
| `room_id`, `offer_id` | 16 bytes each |
| `offer_revision` | 32-byte signed Offer revision |
| `welcome_id` | 32-byte exact signed native Welcome event ID |
| `epoch` | Confirmed activation epoch ordinal |
| `epoch_identity` | 32-byte native authenticator for that epoch |

## Receipt preparation, reconciliation and founder decision

The sole recipient owner verifies native `Complete` and stable exact epoch,
then durably reserves `NativeStarted` for this receipt and calls public
`AccountDeviceRuntime::send(SendIntent::AppMessage { group_id, payload,
expected_epoch })` once under the capture-only adapter. Native prepares the
ratchet and frozen outbound fanout. Inspect `outbound_fanouts_for_group` for
exactly one expected application fanout, matching `.application_message()`
inner event ID, expected scope, native `.request()`, outer message ID and
signed bytes. Persist the immutable signed outer binding before WSS egress.
After a process cut between native freeze and capture, reopen and record the
same native fanout without another `send`. If `NativeStarted` has no matching
frozen or accepted evidence, fail closed. Use `resume_outbound_fanouts` for
receipt retry, with the exact permit, timeout and uncertainty rules above.

MDK retains completed application fanout until the caller acknowledges its
local projection. Recover exact accepted fanout or publication effects after a
restart, recheck group, inner event ID, outer message ID and full target
acceptance, and commit the recipient's `Published` journal state before
`acknowledge_published_app_messages`. Before that local commit the retained
native result can reconstruct publication evidence without retransmission.
After it, the only recovery operation is exact native cleanup. `Published`
represents relay submission, independently of founder acceptance.

On the founder, require a native `GroupEvent::MessageReceived` with the exact
registered group and signed route, current confirmed activation pin, recipient
account and sender leaf, exact prior founder-side enrollment and expected room.
Require the app author to match the native sender and verify all inner fields
against the independent founder record. Check the matching native stored
message and pending invalidation before projection, following the Command
adapter's native authority checks. Durably record founder acceptance once under
the complete semantic binding in the table above. Store exact source IDs and
native sender/epoch evidence for readback. Before clearing the pending ingress
row, commit and read back that acceptance. A restart between native ingress
and projection consumes the retained queue via
`MessageStorage::list_pending_application_events`. A restart after projection
checks the exact durable result and uses `delete_pending_application_events`
only for the matching native source. Exact duplicates have no second effect.
A moved epoch or authority, a mismatching source, or a late receipt cannot
create acceptance. Key idempotence to the semantic enrollment, with message
IDs used to verify source and retries.

## Implementation and qualification

1. Extend the existing `terminal_relay_fixture.rs` Peer mode and the
   `peer_daemon_wss_tests.rs` independent TLS connections. Drive the real
   founder history preflight and the bounded recipient subscription with fault
   oracles before adding durable publication. Reuse the scope and correlated
   ACK sequencing in `peer_prepared_consent_wss.rs::publish_consent_wss` for
   the Group turn, with the Group-specific subscription contract above.
2. Reuse `peer_pending_join_wss_tests.rs::Round` stage/cold-open patterns and
   the native `peer_postjoin_proof_tests.rs` gate and verifier. Replace that
   proof's synthetic history only in the M5 fixture. Integrate one checked
   journal and owner loop for exact PostJoin capture/retry and reconstruction.
3. Add the acceptance codec, the atomic Active/receipt reservation and the
   second publication kind. Reuse the native application provenance and
   pending-row retirement pattern in `command_adapter.rs` for the independent
   founder. Run the integrated daemon path once after both stores pass their
   focused deterministic transaction-cut tests.

Use the automatic daemon case for one full loopback WSS path through exact
native confirmation, recipient receipt publication and founder acceptance.
Reuse the manual daemon case through its existing durable approval boundary.
Test a dropped/truncated history turn and an omitted state event followed by
EOSE, foreign writers/routes and filters, expiry before permit, exact lost ACK
and deduplicated retry, reopen before and after each durable capture, tampered
capture, both receipt acknowledgement cuts and late/duplicate receipt. Check
zero egress on refusals and zero extra native sends or durable decisions. Keep
one working ordinary Command check. Record activation and acceptance latency
separately, plus reopen timings and sanitized relay event counts. Keep private
WSS out of routine CI and use fast wire/store regressions there. Decline and
other leave transitions stay with their existing fence. Review integrated code
and validate the exact resolved dependency with Claude Code Auto.
