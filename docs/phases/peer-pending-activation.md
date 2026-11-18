# Join Peer invitations before application approval

This amendment revises [Peer activation](peer-live.md) and
[Offer receipt and Consent](peer-invitation-integration.md). Preserve
authenticated Offers, exact-origin cursors, signed flight bindings,
encrypted stage custody and durable transport retry fences. The latest
reviewed upstream MDK resolution is
`0ad3c61088e1acac58e0fcf1a9f1142bae5d997a`.

The durable state follows offered → provisioning → pending → activating
→ active, with declined and expired terminal decisions. A separate
durable provisioning permission replaces the earlier requirements to
wait for manual approval before generating the package and to reserve
using `approved_work`. Provisioning is bound to a specific generation
and admission class. Native Consent means permission to use that package
for one provisional join. Application acceptance requires a distinct
signed receipt.

## Native join and application approval

Check current client role, explicit qualified relay and local policy
before admitting an authenticated Offer. `off` and a configured block
deny provisioning. `manual` and `auto` allow bounded provisioning.
An email or other NIP-05 identifier only resolves to the full npub shown
for local verification. Its text grants no authority.

The serialized owner binds a private stage to the exact profile, direction,
canonical client and server, origin, Offer revision and local generation.
It reserves one bounded signed KeyPackage. The same origin carries
immutable Consent and Delivery. Consent relay OK acknowledges transport.
Only the exact authenticated Delivery's Welcome can enter the bound
isolated store through public `AccountDeviceRuntime::ingest_delivery`.

After native join and package consumption, use the staged proof's
authenticated `GroupJoined` receipt and native inspection APIs to check
exact Welcome ID, inviter, profile, two-leaf roster, local capability,
purpose, sole initial administrator and route. Match the consumed
package reference, stored native package and registry Delivery binding.
Missing authenticated inviter or provenance is a mismatch. Persist
`pending` only after those checks. A failed comparison fences the
revision before retirement. A crash after native join reconciles the
exact stored receipt without consuming another package or joining again.

Native join can persist a PostJoin own-leaf rotation, a post-join history
fetch and a due replacement KeyPackage. Public ingestion can publish
incidental effects through its adapter. Put a default-deny physical
adapter in front of every native publisher, scheduler and fetcher.
Provisioning and pending have an empty native egress allowlist, including
on reopen. The signed Offer exchange and authenticated Welcome use
separately bounded capabilities. Never run native general drain,
`run_due_maintenance` or `resume_outbound_fanouts` in these states.
`pause_maintenance` can still publish a prepared KeyPackage and cannot
enforce this boundary.

Pending rooms receive no native group ingest. The qualified relay must
retain the bounded exact-room events needed for later recovery. Keep
application dispatch, peer acknowledgements, status, command authority,
permission views and profile publication closed. Pending rooms cannot
appear as accepted servers or supply a route to Command or Terminal.
The local list may display the verified immutable summary with full
npub, exact invitation ID and expiry. The remote inviter can observe
provisioning traffic.

Only the local profile owner's UID through typed IPC can approve or
decline the exact pending ID and generation. Approval checks local policy
and expiry and durably enters activating. Auto mode uses the same
transition when a new validated pending state is first created. Switching
to auto applies to subsequently validated invitations. After durable
approval, resume the exact room's typed post-join history and PostJoin
own-leaf rotation. The physical gate starts empty on every reopen.
Only derive a permit from checked typed effects for this room,
generation and operation. Retained unrelated effects receive no permit.
Withhold all ordinary replacement KeyPackages, including prepared ones.
Its maintenance obligation stays isolated from the qualified relay.
Any future provisioned package uses a signed Offer generation.

Buffer recovered application events inside the isolated stage during
activation. Check local policy, stable current native authority and
expiry again before the durable active transition. Authority changes,
incomplete required history or an unsupported native effect keep the
application route closed. Only active can project buffered events or
create the signed acceptance receipt. An exact durable outbox resumes
the receipt after restart. The inviter deduces application acceptance
only from that verified receipt in the bound room. Delivery OK, native
membership, SelfRemove and other commits give no such authority.

## Decline and expiration

Persist declined for the exact generation and shut every application
route before preparing leave. Revoke every active capability and
supersede that candidate's PostJoin/history work for egress. Permanently
deny PostJoin rotation, general history fetch, KeyPackages, Welcome,
application payload, status and acceptance. Keep full-account drain
and all-purpose app leave disabled.

Only after the decline fence, permit bounded authenticated
`TransportEnvelope::GroupMessage` ingress for this exact room and route.
This allows the engine to advance across current membership epochs.
Commits and applications can differ only after native decryption.
The isolated engine may advance ratchets and stage native app events,
which a permanent drop-only event sink prevents from projecting,
triggering a receipt or entering status. Cleanup removes staged data
after the deadline. Verify current native authority after membership
changes and before every leave prepare or re-drive. In particular, a
remote promotion of the local leaf while pending becomes visible after
the fence. An admin leaf, unstable epoch or unqualified route keeps
leave unconfirmed. A valid authoritative removal needs no publication.

The scoped action is `SendIntent::Leave` on a current non-admin leaf.
The engine records a durable `LeaveRequest` together with the exact
`SendResult::Proposal` containing SelfRemove. Production does not build
with `test-support` or call `session_mut()`. The public deferred-commit
preparation method does not support Leave. Use the production
`AccountDeviceRuntime::send_host_change_with_audit_context` and its
physical adapter in an isolated leave owner. For initial preparation
and any automatic re-drive across epochs, that adapter may capture
bounded locally generated `TransportPublishRequest` values and refuses
physical publication. A capture alone grants no authority, since native
publication can include other retained work.

Only mint a permit after independently binding the captured or frozen
exact signed event ID and bytes to a locally authored, typed SelfRemove,
the durable `LeaveRequest`, current stable room epoch, local leaf,
terminal generation and qualified route. The qualification proof must
demonstrate this with current public runtime/storage interfaces, such as
`group_authority_view` and the exact sent-message record. The physical
gate enforces the permit for the exact request on re-drive, including
when native `resume_outbound_fanouts` sees other frozen work. It refuses
all unpermitted work again, including after reopen. A reopened adapter
starts with no permits. Neither candidate order nor the room envelope
authenticates the effect type. If current public data cannot prove the
exact typed association or cannot re-drive its unchanged bytes, use a
narrow upstream prepare/read API, leave unconfirmed, and keep deployed
traffic disabled until the proof passes.

Treat `EngineError::LeaveAlreadyRequested` as in progress. Recover the
stored request and re-drive the exact persisted current-epoch proposal.
A later stable epoch may produce a new checked native proposal. Record
departure only from authoritative native membership evidence, keeping
relay OK separate. The inviter may commit authenticated SelfRemove for
the known exact room and client leaf under current policy. Allow one
bounded attempt per stable epoch and stop no later than cleanup expiry.
A refusing inviter can leave the outcome unconfirmed.

Bound cleanup to 24 hours, one serialized re-drive and existing relay
backoff. Persist declined with leave unconfirmed at deadline before
retiring private state if there is no authoritative removal. Expiry
after join has the same application fence and cleanup. Expiry before
join closes the stage without native leave. Keep encrypted replay
fences through signed acceptance windows plus the existing skew margin
even when the operator hides the row. Decline fences the revision and
generation and sets a 24-hour per-inviter reprovisioning cooldown.
Permanent blocking requires an explicit local block.

## Admission and validation

Preserve the existing eight pinned/known and eight other retained Offer
slots across relays. Within the profile, one provisional reservation and
one ten-minute start slot are reserved for currently pinned or
previously locally accepted canonical inviters. The second reservation
and start slot are reserved for all other inviters. Class is derived
from durable local trust at reservation, persists with its charge and
cannot be claimed by a sender. Unknowns cannot borrow trusted capacity.
Apply one simultaneous candidate per canonical inviter across origins,
durable rate charging across restarts and one active provisioning
operation per turn. Charge leave cleanup and failed activations to the
same reservations. Keep independent aggregate ready-room and descriptor
budgets. Pending expires within 24 hours and before signed Delivery
expiry.

Reuse the existing 64 KiB envelope and 128 MiB combined native storage
admission thresholds for each bounded turn. Limit declined convergence
to one exact room, the bounded ingest and fetch cursor, and the same
stage-wide storage budget. Apply bounded per-turn work, existing rate
backoff and a hard deadline to cleanup. Measure peak DB, WAL and SHM
across transactions and reopens. SQLite can cross a sampled threshold
during a write. Deployed traffic stays disabled until native limits or
a filesystem quota provide a measured practical bound, and the explicit
relay passes recipient, writer, history and event-volume qualification.

Use actual MDK at the resolved revision and private loopback accounts.
Test crash recovery before pending commit, both approval modes, off,
incorrect inviter/roster/purpose/route, stale decisions, expiry,
policy changes and unknown Sybil pressure. Test a refusing unknown
inviter keeping its slot while another unknown cannot take the reserved
trusted stage or start slot. Include reopen and time-window expiry.

Prove zero physical native egress while provisioned or pending with a
durable PostJoin obligation and a previously prepared replacement.
Test denied retained effects across reopen and attempted re-drive,
including explicit injection of native `resume_outbound_fanouts`.
Send pending application frames and verify no projection, app ACK or
telemetry. On approval verify history recovery, checked PostJoin
rotation, delayed dispatch and one acceptance receipt after retry. On
decline verify restarts around the fence, exact persisted current-epoch
retry after `LeaveAlreadyRequested`, remote epoch changes, admin
promotion, one authoritative native departure, and bounded unconfirmed
retirement for an offline or refusing inviter. Reject every other
egress type after decline, including a retained PostJoin effect.
Verify Command progress independently. Measure native provision to
pending, decision to receipt and decision to leave. Keep relay
qualification, CLI/daemon integration and production enablement
separately evidenced.
