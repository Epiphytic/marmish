# First installed Peer heartbeat

Base `6fe45ed`. This detailed phase sequences the already approved
[installed owner](peer-installed.md) and [status lifecycle](peer-heartbeat-activation.md)
into two product increments. It uses the verified private WSS and SQLCipher
paths. Existing Command, Terminal and public profile behavior retain their
normal entry points. Peer and status remain separate explicit opt-ins.

## A. Installed accepted owner

**Admission to startup.** Use the existing parsed `CommandConfig`, daemon profile
lease, production identity and system clocks. After role and enabled checks,
mount the reviewed exact-path quota dispatcher before the first SQLite open in
that process. Extend its charge registry for the actual founder and retained
recipient native account images, including their sidecars and charged cleanup.
Keep checked no-follow paths, exact slot and inode, stock SQLite delegation
and ordinary readiness independent. Register only the two existing
provisional filenames, exact recipient stage files already allocated by
custody, and an owner-allocated `founder-<local generation>.db` in the same
checked Peer directory, with each exact main and journal/forbidden-sidecar
name reserved. Never derive a filename from a remote principal. Bound accepted
bindings to 64 across directions, a 16 MiB main per retained native account
and 128 MiB across native mains/journals and other charged artifacts,
including provisional and quarantined scopes. Reserve by durable identity
before open; count each inode once; guard growth and adjoin exact prior
cleanup charges. Qualify unchanged native SQLite callback and poll isolation
with the real retained-account call graph, not the provisional analogue.
If SQLite was already opened or the process mount cannot be safely proved,
Peer reports `quota_unavailable`; stock paths continue. The dispatcher remains
process-lifetime. Stop/fence Peer on disable with its files charged;
a restart with Peer off skips registration. Start one
managed Peer owner with serialized mutations and separate directional scopes
for Both. Reuse the existing Pending store and local same-UID request/reply
machinery, with exact decisions and bounded inventory. Retain Manual, explicit
Automatic, decline, expiry, fail-closed cleanup and exact grant/block fencing.
No production constructor accepts a fixture clock, signer or relay.

**One production route.** Extend `PeerRelayOwner` and the typed
`PeerRelayConnector` within the owner boundary to open an exact direct socket
with the existing TLS policy. Extract the currently test-mounted Group socket
operations as scoped `REQ`/history/EOSE/live and exact publication on that
socket. Keep caps, NIP-42, event signature, current subscription incarnation,
route, sender and bounded history admission checks. Test relay fingerprints are
an oracle only. Bootstrap `QualifiedOrigin` from checked profile, typed
qualification and exact held route. `CurrentMetadata` alone cannot construct
it. Implement the owner-only bounded qualify command with disposable registered
founder and recipient plus unauthorized third identity. Require exact TLS
trust, challenge/response NIP-42, scoped recipient and Group reads and writes,
complete EOSE and reconnect history, exact-byte lookup, and refusal of
unregistered, wrong-route, over-size and wider filters. Refuse probe
timeouts and missing behavior. An owner-checked explicit attestation file
names this exact origin/pin, enforcement source and separately enumerated
retention, ingress (size/rate/count) and access-isolation guarantees matching
the numerical versioned relay contract. The local operator verifies and
affirmatively attests to those enforcement records; a self-entered hash or
mark on the qualification record cannot substitute. A passing probe and
separate attestations produce an atomic local operator-policy record; probes
do not prove unobserved retention or production behavior indefinitely.
Match the attested endpoint, TLS trust, suite/bounds, build/MDK, target and
unexpired trusted time. Reopen/recheck exact owner file and trusted time
before each connection and authoritative send. Expiry mid-run fences egress.
Verify TLS on each connection. A changed or stale
qualification, unsupported route, invalid time or unqualified artifact leaves
Peer unavailable. No inherited Command relay, default Peer relay, discovery,
Tor routing or third-party qualification traffic as part of this design task.

**Authoritative acceptance.** Mount the narrow reusable parts of the existing
leased daemon worker, reviewed Pending/Activating custody, sealed
PostJoin/receipt verification and SQL transitions. Implement the actual
founder operator invitation turn using the current execution client grant.
Persist the authenticated locally authored Offer, exact Consent, native
Welcome and room/binding before accepting the receipt. Scope founder rows by
direction, pair and local selection/generation and cap them within the prior
installed bounds. The test-only singleton founder row and mutable fixture
constructor do not establish authority. Reuse exact challenged signed
publication, fenced uncertain recovery and native evidence from M5; the
founder requires durable receipt acceptance and matching current native
peer leaf/epoch. The recipient requires durable Active, scoped native readback
and local acceptance receipt publication proof. Both sides retain group,
app-room, exact route, local generation and current authorization revision.
Use signed control body `v=2` on Offer, Consent and Delivery for the closed
heartbeat-capable version. Consent and Delivery preserve exact prior
revisions; the v2 native receipt binds the completed v2 offer revision
and exact existing receipt coordinates. Prepared Consent reconstruction,
including its DeliveryOffer carrier/checkpoint format, binds the same version
and refuses cross-version wrapping. Both sides persist negotiation and
derive the same 32-byte heartbeat app room from SHA-256 of the ASCII domain
`marmish/peer/status-room/v1` followed by NUL and the concatenation of the
canonical 32-byte founder, 32-byte recipient, signed 16-byte room ID and
signed 16-byte Offer ID. For raw repeated inputs 01, 02, 03 and 04 in that
order the expected digest is
`0e31574ef3903ee44ad3e33be97e5b84467005a600d741292175c547ef94dc78`.
Existing v1 rooms remain permanently status-ineligible. Upgrading requires a new authenticated
enrollment; claimed receipt fields cannot upgrade a room.

| Current boundary | Production extraction |
| --- | --- |
| `peer_invitation.rs`, raw Peer owner/group, `peer_relay_qualification.rs` | Keep bounded shared verification. Restrict production `QualifiedOrigin` to `PeerRelayConnector`; never widen arbitrary `RelayConnector` or promote fixture witnesses. |
| `daemon_peer`, `peer_daemon_worker.rs`, `peer_pending_ipc.rs` | Promote the real role/lease/IPC turn and system-clock scheduler. Keep alternate bootstrap, injected clock and transport test-only. |
| `prepared_consent`, `provisional`, `retirement`, `accept_receipt_*`, `postjoin_verify`, `publication_socket` | Extract exact verified transitions, UID and descriptor checks, bounded typed network adapter, verified publication/cleanup and native proof. Keep raw test progression, in-process founder replay, socket fixture and all fault controls private. |
| `peer_quota_vfs`, `peer_quota_dispatch.rs` | Preserve independently reviewed stock delegation and guards. Replace the test slot/fake charge owner with the actual charge registry above and qualify its production call boundary. |
| `peer_status_wire`, planner, sampler, `peer_status_state`, `peer_heartbeat_send_fences` | Mount in B only. Reuse checked codec/planning/sampling and exact SQL transaction logic. Replace plain copyable planner `Accepted` and fixture SQL authority with the sealed live owner witness. |

Production extractions require exact review and real mounted tests. Code
approval of fixture behavior alone does not cover these mounts. A exposes
`marmish peer enable|disable`, `peer relay show|qualify`, exact
`peer accept|decline`, `peer block|unblock`, `marmish servers list|show`,
and `marmishd clients invite|list|show|revoke` with existing verification
and grant policy. Defer identifier rotation, explicit terminal forget and
accepted-room removal outside A; onboarding keeps the existing verified
setup grant flow. B adds `peer_status` and the reviewed `marmishd status
config` surface, keeping the current bare daemon status command.

A can ship with dormant production wiring and typed unavailable results on
all unqualified targets. An actual qualified A deployment can enroll and
inspect peers without publishing status. This phase does not enable
accepted-room removal: revoke/disable closes authority, preserves charged
state and follows existing accepted-room removal limits. Qualification of a
deployed relay and eligibility of the production platform are operational
requirements. They do not prevent writing or locally exercising the owner,
exact acceptance and direct connector. A checked loopback fixture may
exercise these paths without granting eligibility to distributed artifacts.

## B. Periodic status over A

**Capability.** Keep `AcceptedStatus` non-cloneable and transaction-scoped;
only A's live lease owner constructs it after checking the authenticated
acceptance record, negotiated capability, current role, pair, local
generation, authorization and configuration revision, current grant/block,
exact native group/leaf/epoch, current qualified socket/subscription and
private route. Read both scope and passage of time immediately before new
work and before each physical send. Changes in any fact invalidate existing
turns. Until the daemon has reviewed reload support, recheck the held
`config.json` fd/inode and exact security digest following
`ProfileFiles::recheck` before each authoritative send. Changes close Peer
until restart; current Command reload behavior stays independent. Enrollment
and receipt clocks apply until acceptance, then fresh
qualified scope and bounded work deadlines apply; an original flight expiry
is never extended or repurposed to reset status. Read A's common exact
versioned app-room identifier; never use the fixture derivation.

**Send and receive.** Mount the reviewed sampler, codec, planner and sealed
single-writer lane. Status defaults off; an opted-in room schedules an
initial sample and an hourly default with the approved bounds and jitter.
The shortest first status rollout is username, uptime and load, with
`share_permissions = false` explicitly configured. The approved enabled
config defaults still stand; a scope-limited artifact must reject unavailable
permission, disk and IP settings clearly rather than silently discard
intent. A changed first rollout boundary needs design approval. Apply the
approved per-key narrowing and current grant checks even in this scope.
This first profile is an implementation increment only. Complete the
approved encrypted allowed-command views and configured disk/address
projection and the remaining requested operator controls before the final
PR is ready for approval. Additional fields do not upgrade existing room
authority.

Reserve and commit exact sequence, owner revision, app event id, canonical
payload digest and bounded turn before invoking MDK. Confirm the exact current
persisted native fanout at the earliest adapter callback before socket
publication. Preserve the approved one-second total native action budget,
including network and ACK. Prove the first callback can perform the actual
bounded write and return its native result within that budget, on a
production adapter. Existing first callback evidence observes provenance
then publishes only on a 30-second retry and cannot satisfy this gate.
Atomic permit consumption precedes socket I/O. A lost ACK or timeout
closes the gate and marks the turn spent/uncertain; block all physical
redrive and show bounded or terminal disposition, with no retry loop, before
enablement. A TLS acknowledgment alone cannot create another permit.
Scope receiver admission before native ingest, enforce existing
record/byte caps and the proven 16 pending native applications per stage
(including after restart). Bind the native pending outcome to its exact
authenticated source and sender. Commit terminal source disposition and
durable projection before exact native ACK, then verify exact absence with
the bounded pending scan. Missing bound/absence proof keeps receive
typed-unavailable. The SQL transaction advances monotonic
sequence and records the first qualified local receipt. Replay cannot refresh
it. Surface canonical founder, accepted state and actual last verified
heartbeat separately in same-UID `marmish servers list|show`; abandoned or
preexisting observations start cached at best. No public event/metadata
or diagnostic projection and no status-derived command authority. Keep only
the newest completed heartbeat for seven days and retain durable replay
floors. ACK native pending payloads after SQL commit. Retire exact terminal
accepted send fanouts through the supported native acknowledgment. Residual
native history stays charged inside A's hard bounds. Without supported exact
native pruning, exhaust the room into typed `quota_unavailable`.

**Restart and invalidation.** Stop the owner and fence active turns on
disable, local security config changes, clock rollback, route loss, grant
loss, block, generation or native-authority change. Preserve sealed spent
records and advance authority revision before new status. On restart,
recheck exact SQL, accepted group, current role, policy, route, native
authority and a current qualified connection. A clean status reopen
requires either matching durable no-handoff/finalized state and positive checks of native absence, or an exact terminal accepted
native fanout matched to a committed terminal SQL row and its frozen
destination. The latter runs only native local acknowledgment
(`acknowledge_published_app_messages`) and then verifies absence without
network redrive. With the gate physically closed, the owner checks both
fanouts and queued intents. There is no reviewed public cancellation for
unresolved status work. Persist room-level quarantine and typed
`status_work_unresolved` or `quota_unavailable` for the local show projection. No broad redrive, raw SQL native deletion, old
permit, clock/freshness reconstruction or silent recovery. This fail-closed
room behavior does not block authoring the production phase. Deployment
requires validated bounded recovery or explicit acceptance of quarantine
until separately authorized repair; ordinary operations still run.

## A's first patch. Relay CLI and production transport boundary

The public CLI recognizes only exact `peer relay show` or
`peer relay qualify --attestation <file>` as local, with existing
profile/debug global options before `peer`. Any other `peer` input
including explicit `peer -- <argv>` follows the existing remote hostname
grammar. Preserve both existing `peer -- echo rsh` and
`peer rsync --server -s` regressions and test an ordinary remote command
with host `peer`. Apply the same exact local grammar/explicit remote
escape rule before adding later `peer` and `servers` controls.
Route local qualification before ordinary relay selection. Open only
the existing selected profile with no-create
`ProfileLease::acquire_ready` for either credential backend and checked
config. Initially require the daemon stopped for the
exclusive lease; report a typed busy result. `show` makes no network
call, creates no state and prints only origin, abbreviated trust
fingerprint, bounded times and typed qualification. Schema 1 records
retain metadata compatibility for readback but always show unqualified;
only freshly issued schema 2 can report qualified.

`qualify` is an explicit foreground TTY operation. It generates disposable
probe keys in memory and prints the two public enrollment identities, the
unauthorized third principal and the bounded private probe route so the
operator can confirm the exact endpoint and arrange its approved test
scope under the existing operator qualification contract. The tool never
registers external users or modifies relay ACLs. It accepts no profile
signer or raw secret argument. Validate one closed, canonical owner-checked
attestation JSON object (8 KiB maximum, `schema: 1`). Its exact fields
are `schema`, `origin`, `trust` (existing typed pin expression),
`enforcement_source` (bounded operator reference), `valid_from`,
`valid_until` (Unix seconds), `control_limits` (exact numeric keys
`event_bytes: 65536`, `stored_per_recipient: 256`,
`per_timestamp_second: 3`, `accepted_per_600s: 32`,
`stored_per_writer_recipient: 32`, `retention_max_seconds: 324000`),
and `enforcement` (exact true-or-refuse fields
`control_registered_writer`, `control_exact_recipient_auth`,
`control_expiration_no_early_eviction`, `group_exact_route_acl`,
`group_authenticated_members`, `group_reconnect_history`).
The current time must be in the attestation window; validity of any
issued qualification ends at its earlier attestation/probe limit.
After displaying exact origin and pin, require the exact TTY confirmation
`qualify <canonical origin>` before any relay traffic. The operator
affirms both authorization to run the probe and the displayed
attestation. Its trust root is the local owner, with no unrecognized
signer or free-form waiver.
Start a monotonic 30-second total I/O window after confirmation. Admit
exactly four opens in order for a completed probe. The first three use
different disposable principals (writer, unauthorized outsider, and
recipient), followed by one fresh reconnect as the recipient. Hold
at most one active client socket and finish or drop it before the next
open. Enforce the total-open and reconnect budget in the probe as well
as at most 64 protocol frames and 2 MiB of combined traffic. Exercise the required probe suite above; timings and
budgets may be narrowed but cannot silently skip required cases. Negative
proof requires the exact refusal class for that operation or, for an
authenticated request that omits its scope tag, an empty EOSE after
independent exact positive history on the same socket. Report fixed local
probe-check categories without including relay response text.
Timeout, disconnect or an untrusted response is not negative evidence.
Recheck local file, trusted time and exact profile after the window.
Write no qualification on refusal or partial evidence. Use a versioned
schema 2 record, atomically promoted by the owner with both completed
evidence inputs, checked expiry and readback. `operator_evidence` is
SHA-256 of canonical attestation bytes. `probe_evidence` is SHA-256 of
the canonical closed versioned probe summary, including probe nonce,
suite, exact origin/trust digest, execution times and passed result
for each required check. Keep only the digests in the issued record;
capture no probe secret or operator enforcement source in diagnostics. A fixture can administer dynamically
registered disposable probe identities only inside its test process.

The production connector bridge accepts a direct pinned transport turn
only from `PeerRelayOwner`. Probe and qualified runtime open are separate
typed permissions. This first patch provides the former and checked
qualification readback. It neither mounts enrollment/heartbeat nor
declares platform eligibility or issues their live group authority.
Use a production-compatible private CA loopback socket for tests with
the normal binary entry point. Assert pin/route refusals, report-only
readback, rejected metadata/attestation/trust drift and expired records,
bounded no-record failure, successful local issuance and unchanged
ordinary Command relay defaults. No external relay or native runner is
needed to author or exercise this local code path.

## Implementation and qualification

1. One agent owns typed route/qualification and exact quota startup; a
   second owns production custody/acceptance and role turns. Integrate in the
   existing daemon and UID IPC owner. A then proves installed foreground
   acceptance through exact private WSS, with Manual, Automatic, a bounded
   removal/retirement case, exact local decisions and receipt/ACK crash cuts.
   Exercise TLS pin and capability refusal, absent option and ordinary
   readiness independence. No fixture clock, transport or authority mint
   in release code.
2. Implement B sender/receiver transactions against A's sealed API. Run
   actual binaries through the same installed qualification fixture. Send
   a service heartbeat and verify local inventory at the other end. Cover
   disabled/status-free pending and awaiting receipt, isolated pair,
   stale epoch/revision, grant/block/disable, changed trust, earliest
   publication callback, lost ACK/fanout quarantine and clean reopen,
   duplicate projection and old subscription rejection. Assert no public
   record or unauthorized callback. A hung Peer relay must leave Command
   ready and executing within the approved independent bound. Report
   sanitized scheduling-to-receipt latency separately from enrollment,
   and dispatch lag alongside Command.
3. Keep regular CI to deterministic core and existing smoke. Run installed
   packaging/private WSS separately on the authorized self-hosted Linux
   AMD64/ARM64 and macOS ARM64 targets. Compare native quota isolation and
   ordinary startup with the control profile. Enable only explicitly
   qualified artifact targets and independently qualified operator endpoints;
   report local loopback, artifact eligibility and deployed relay evidence
   separately. Reuse the existing Beads work on `marmish-dwt.3` and
   `marmish-dwt.4` and record Claude design and exact integrated code reviews
   before each phase closes.
