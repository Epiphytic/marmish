# Private installed Peer PostJoin convergence

## Outcome

A fresh Linux AMD64 qualification candidate runs the real Manual recipient
daemon and the existing same-UID approval CLI. The approved enrollment reaches
a durable, cold-readable PostJoin confirmation. Both independent native devices
are stable at the same rotated epoch. The local custody row remains Manual
`Activating` and retains its assessed stage charge, approval time and
authoritative transcript. Acceptance, Active, room use, Peer status, automatic
approval and decline/expiry effects are later phases. Ordinary Command and
Terminal retain their existing routes, permissions and behavior. The shipped
default, public relays and other platforms gain no Peer capability.

Start at 7f7d7bc with upstream MDK `e4421e86a6c73f336b0154ee09c13826d8fafff5`.
Reuse the staged exact checks in `peer_postjoin_verify.rs`, publication ordering
in `peer_activation_checkpoint.rs`, private WSS history and retry fixture, and
founder issuance in `peer-installed-founder-turn.md`. The existing PostJoin
proof reads both native stores in one process. Separate installed participants
can establish local native evidence independently and exchange authenticated
observations over the enrollment's private Control route. No recipient code
opens a founder database or trusts a fixture observer as a live credential.
The previous Cold Acceptance Receipt proof remains evidence for a separate phase.

## Durable owner and schedule

Use a distinct private qualification intent and fresh disposable
`ManualPostJoinV4` cursor/custody layout. This preserves the unrelated stock and
previous v3 launch paths. V4 refuses existing v3, v2 or legacy stores through
the existing read-only marker, ownership and topology preflight before a
mutating open. V3 likewise refuses v4. Preserve immutable markers and the two
authoritative wall floors. No migration, auxiliary cursor or app database.
Verify the executable/layout identity and real ordinary and Peer route
qualifications at launch as in the existing installed fixture. The new route
aperture is confined to this private qualification and has its own closed
Group scope. A previous Control-only qualification grants no Group access.

Extract an Activating-only checkpoint read from the checks shared by
`peer_installed_approval_checkpoint.rs` and current installed Delivery custody.
Return opaque owned context containing the Manual decision, exact selection,
charge, stage inode, accepted native baseline and verified raw transcript.
Require the current account, lease, generation, role, Unknown admission,
exact bound origin, distinct founder and recipient, both floors, selected
group and active proof window. Only the existing same-UID approval CAS produces
the decision. PostJoin reads it and never synthesizes or reconsumes approval.

A single `ManualService` owner, the current `RecoveredOwner`, `ReceiverQuota`,
readiness, stop, forwarding verifier and checked physical port own each turn.
Keep one stage, one enrollment, one Group history request and one candidate.
Dispatch only after the normal recovered Control handling and with a finite
turn per stage. Reuse anchored time, bounded backoff and independent ordinary
forwarding. Stale route, clock, proof, lost custody or exhausted budget end the
Peer turn fail closed. Global forwarding loss follows existing process-fatal
handling. Expiry fences further effects and leaves custody charged for the
separate reviewed terminal lifecycle. Neither a mere request nor a token owned
by a different turn can advance storage. No SQL transaction, physical file
mutex, quota guard or native handle crosses a network wait.

## Exact founder assertions

Add two private inner classes to `peer_control_wire.rs`, using the existing
kind 7435 envelope, exact `#p`, NIP-44 signed seal, checked route/origin and
canonical bounded v2 parsing. Use dedicated inner kinds 24868 and 24869 with
closed `postjoin_baseline` and `postjoin_converged` bodies, compatibility limited
to the v4 proof intent. Current v1/v2 Offer, Consent and Delivery bytes stay
unchanged. Only the already verified founder may authenticate the inner
observation. Outer relay encryption identity, group routing hint, NIP-42
transport authentication and knowledge of a public pin carry no founder
authority alone. Verify exact inner/outer IDs, signatures at existing envelope
layers, audience, canonical field encoding and time before interpreting either
body. Keep native evidence out of relay-visible tags and diagnostics.

Both bodies bind the two distinct member keys, existing verified room
coordinates, Offer ID and signed Offer/Consent/Delivery revisions, exact
Welcome transport ID, canonical WSS origin, exact transport route and bounded
native group ID. Require matching current native membership and creator,
exact two-leaf roster, applicable signed deadlines and bounded current turn.
Local profile/generation/decision time are validated by the cursor and never
compared to values chosen by the other participant.

- Baseline identifies exactly one accepted founder SelfUpdate signed event
  by outer ID and SHA-256 of exact bytes, the resulting stable source pin
  (ordinal and authenticator), both expected leaf identities and indices, and
  the recipient's original leaf hash. The joined recipient's authentic
  Welcome pin is the preceding epoch. An opaque founder observation is minted
  after the founder proves prior Welcome, the exact durable native
  SelfUpdate and stable scope under its own current grant. Reactivating a
  stale founder journal or having only a matching membership list confers
  no permission to issue an assertion.
- Converged references the exact Baseline rumor ID and resulting source, the
  exact recipient PostJoin outer ID and signed-byte digest, and the successor
  stable target pin and both leaves including the changed recipient leaf
  hash. Construct it only from exclusive native ingest of those exact signed
  event bytes at the pinned source, scoped to that group and route, followed
  by native applied outcome, exact processed ID, stable successor and durable
  checked readback of the canonical roster. A processed-ID lookup alone is
  account-wide. `IngestOutcome::Processed` also covers other inputs, so require
  the captured before/after scope and the exact expected recipient-only
  rotation. No buffered, ignored, app, recovery or unrelated effect may
  satisfy the proof.

The v4 qualifier uses an independent real founder owner/identity/lease and
the reviewed founder issuance fixture, with scoped current command grant
checks and exact retained Offer/Consent/Delivery. The issuer is private
qualification code. It signs no free-form truth supplied by the operator or
recipient test, and does not ship as an installed founder service. Retain the
exact authoritative proof envelope bounded by the existing codec cap and its
inner revision in the encrypted owner journal, then re-open and re-bind on
cold read. An authenticated repeat of the already retained inner revision
confers no new effect. Authenticated conflicts or mismatched scope stop
progress. Reject untrusted, malformed and unrelated live input without new
authority or an empty-scan busy loop. A converged observation can be retained
while the recipient still has an unconfirmed native candidate at source
after loss of its physical acknowledgement.

## Checked physical and metered native work

Use `peer_raw_socket_owner::Owner::open_checked` and checked NIP-42 provenance.
Extract a small bounded Group parser from `peer_raw_socket_group.rs` so the live
candidate never imports `terminal_relay_fixture::GroupFingerprint` or its
transcript. Bind the checked private connection, incarnation, current account,
single kind 445 `#h` route and expected group, finite frames/bytes/deadline and
exact correlation. One history request ends at matching EOSE. Feed only the
exact checked signed SelfUpdate named by Baseline once, and confirm its
native transition from Welcome to the asserted source. Require the raw
history event ID and signed-byte digest, no changed route, contradictory
duplicate, truncation, or missing matching update. Unrelated frames yield no
native authority. EOSE and MDK history bookkeeping alone never prove content
or convergence. Reuse `start_post_join_history_fetch`, `finish_post_join_history_fetch`
and `mark_post_join_eose` only through a verified native history turn.

Use the existing measured original/copy model in `peer_receiver_inventory.rs`
and `peer_quota_dispatch.rs`. Before any effects reserve the exact existing
charge and evidence through the same cursor. Each scoped original open checks
owner, key, original inode, clean sidecars, cap, sole group and live window;
readback runs on separately measured copies and rechecks source inode/size
after closing and unlinking them. Use `NativeRouteWindow::manual_window` and
`ForwardingVerifier` checks at native/transaction boundaries. Keep existing
native deadlines and one-poll rule. No storage-handle crossing of any await
and no broad adapter or transport permission.

The public MDK runtime exposes `run_due_maintenance` and
`resume_outbound_fanouts` but no public setter for an arbitrary
`TransportPublishReport`. Qualify the split before mounting the actor. An
immediately-ready, no-network, deny-by-default adapter captures one exact
`TransportPublishRequest` with a non-accepting report. Closed authenticated
readback checks the exact single own-leaf obligation, native pending state
reference, semantic evolution, frozen fanout and normalized signed request,
including sole endpoint and required acknowledgement count. Independently
prove the cold reopened resume asks for that same request and the one-use
adapter can return a typed positive report only from a checked physical ACK
for that current attempt. Do physical WSS publication between the closed
native turns. An unrelated adapter call, second request, pending poll,
unexpected durable state, elapsed deadline or unsupported locked API fails
the qualification. Respect native retry delay; do not hand-edit native rows
or swap clocks to force live readiness. A single checked readback may need
an additional individually fenced native sweep already seen in the isolated
proof. A successful test must demonstrate exact stable confirmation under this
split, with no cache-held native state or database handle across transport.
If pinned MDK cannot expose and settle the exact continuation under these
constraints, stop before installed mounting and report the native interface
gap. Do not add a fork or weaken the quota.

## Journal and restart

Store `Reserved → NativeStarted → Captured → Armed → Uncertain/Published` in
the one v4 encrypted cursor, preserving the existing checked publication
state machine. Reserve exact custody, founder Baseline and source pin before
native mutation. Seal candidate identity as signed-byte SHA-256 plus native
outer ID, semantic obligation, pending/evolution identity, stage and target.
Use native durable fanout as byte authority. Compare the full normalized
request whenever arming a new physical attempt. A fresh sole owner with the
same durable generation and independently verified original may reconstruct
one candidate after a cut before capture. An unmatched NativeStarted state
never authorizes new history input, another rotation or another stage.

Arm one send before opening its physical aperture. Preserve attempt number,
time, current route scope and exact candidate. A lost positive result or
interruption after Arm is potentially exposed. Only after current cold
readback and the exact authenticated Converged assertion may an Uncertain
candidate retry once, using identical bytes and signed outer ID, following
native due time and current qualification. Never recreate a candidate. Exact
checked relay ACK permits one native settlement, under a freshly verified
same attempt. It does not itself confirm local PostJoin. If closed native
state already proves stable completion at the exact target with the exact
confirmed evolution/processed ID and matching Converged assertion, record
confirmation without retransmission. Persist applicable current wall floors
and proof bindings atomically in the publication confirmation CAS. Replaying
that confirmation is read-only and contributes no effects.

The custody row remains Manual Activating before and after confirmation. No
Active transition, acceptance receipt, room descriptor, status subscription,
new heartbeat, remote command permission, deletion or quota release. Keep
selector/read projections and same-UID approval reply semantics unchanged.

## Validation and implementation boundaries

1. Qualify canonical assertions and founder native issuer, distinct v4
   attachment and exact journal/restart decisions, separately metered readback,
   and both halves of the one-poll native publication split. Use deterministic
   codec, SQLCipher, live-fence and ownership tests plus isolated real MDK.
   Reuse the old genuine PostJoin proof for comparison.
2. Qualify finite checked Group I/O under real private TLS and NIP-42. Then
   mount the single v4 actor turn only after both prerequisites pass. Keep the
   actor owner change separate from the codec, physical parser and storage,
   and build the native adapter and fixture issuer with explicit separate
   ownership. Default and existing candidate paths must retain their exact
   selection and registration behavior.

The opt-in end-to-end fixture installs source-matched private Linux AMD64
binaries, uses disposable independent identities and two separate native
stores, distinct explicit ordinary and Peer WSS, real qualifications and the
existing CLI to observe Pending and record Manual approval. Prove cold exact
Activating with durable PostJoin confirmation and equal independently
witnessed native pins, one founder update and one recipient send. For lost
ACK prove exact single optional retry, two identical recipient sends at most,
and eventual stable completion. Exercise separate closed-owner process cuts
after approval, after native freeze before capture, after Arm, and after
native confirmation before the local journal; check no duplicate native
operation or further write after cold completion. A relay withholding a
reply stays private to the test. No fault injection switch in installed
binaries.

Refuse wrong signer/recipient/transcript/origin/epoch/leaf/candidate, expired
or conflicting proof, missing history despite EOSE, stale incarnation, copied
stage or wrong qualification layout, and an unrelated native fanout. Test
v3/v4 mutual refusal, journal/floor rollback, exact cutoff/no data loss,
ordinary commands during Peer failure, and zero activation/receipt/status
events even after restart. Report application- and relay-visible event counts
and measured timings separately for approval-to-baseline, verified history,
native capture/readback, physical publication and final confirmation. Keep
core CI to fast deterministic codec/storage checks; private transport,
restart and installed timing remain opt-in. Cite exact author and composed
Claude Auto reviews and immutable build identity before claiming support.
