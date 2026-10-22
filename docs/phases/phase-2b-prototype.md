# Phase 2b: confined command prototype

Tracked in Beads marmish-315.15. This increment delivers actual noninteractive
commands over Marmot; interactive terminals, privileged user switching, aliases,
SRV discovery and normative multi-device support remain later phases.

## Transport boundary

Use the existing hash-pinned installed MDK daemon through bounded Unix JSON RPC.
Application payloads never enter subprocess arguments or environment variables.
The daemon runs in a private user/network/PID namespace with loopback only, no
external routes, no DNS, no ambient proxies, and no host socket directories.
Each explicitly configured wss DNS origin has one loopback listener connected
through a private Unix bridge to a host broker whose destination is fixed to
that origin. TLS/SNI/certificate checking remains end to end in MDK. Only root
URL paths are supported; this confines physical origins, not paths inside TLS.
Require bubblewrap and ip; fail closed if isolation cannot start. Bound bridge
connections, buffers and deadlines. Keep only private state writable. Bootstrap
network capabilities apply only inside the private namespace: an outer user/network
namespace enables loopback and unprivileged port 443, then a nested user/PID
namespace drops all capabilities before creating the listeners and daemon. Clean up child
processes and bridges on exit. Preserve daemon home for session resumption.

RPC uses one newline-framed request per connection, a write half-close, a single
absolute deadline, effective-UID peer checking, bounded requests/responses and
strict double-encoded response validation. Wire home/socket are /state and
/state/s while host callers connect to the mounted private home socket.
Use authenticated message-row sender, direction, kind and actual group ID;
never payload identities or display names. Scan overlapping bounded history,
deduplicate message IDs, and baseline all visible messages as stale when a
server starts or enrolls an existing group. Do not respond to baseline hellos;
clients retry fresh hello messages. Fail on a full 100-row scan if its oldest
row is absent from the seen set, except the initial stale baseline. Engine
invocation IDs, input offsets and idempotent cancel remain replay backstops
when a bounded request cache evicts an entry. Fail visibly on gaps rather
than assuming sender timestamps are reliable arrival cursors. The MDK window is
oldest-first; delayed events that land outside the latest 100 rows cannot be
detected by this API and are an explicit prototype delivery limitation.

## Command and session protocol

Dedicated two-member groups use name marmish/v1/<random session ID>, description `marmish/v1 command transport`, and kind
30100 with a Marmish version tag. Server accepts only allowlisted invitations,
verifies inviter, exact membership and group purpose, and rejects ordinary chat.
A new session creates a fresh independent MDK MLS group; resume restores MDK
state. No custom MLS cryptography. Reuse a persisted session per remote identity
and relay set by default; --new-session creates another group.

Application envelopes bind protocol version, session ID, actual group, remote
identities and random request ID. A hello nonce obtains a server boot-specific
random challenge with a 60-second monotonic lifetime. All later operations need
that challenge, preventing historical control frames from becoming live after
restart. Server-issued engine execute tokens additionally authorize launches.
Challenges bind session, group, authenticated sender, hello nonce and server boot.
Exact duplicate request IDs with identical digests return cached replies;
conflicting duplicates are rejected. Bound caches and active sessions. Durable
engine invocation deduplication remains across groups: server, authenticated
principal, target account and invocation ID. Uncertain sends never cause a new
invocation ID automatically. Before any submission send, atomically write and fsync a private 0600 journal
containing invocation ID, full original argv bytes, session and original token.
On retry query status first: existing invocations reattach without a token. On
NotFound obtain a fresh token and submit the SAME invocation ID and original
argv; if admission races produce InvalidToken, query status and reattach. Never
silently create a different invocation. Exact admitted retries with original token
remain supported. Remove secret token/argv material from the journal at terminal
completion after required retry verification. Expired challenges return an explicit error. Reconnecting obtains a new
hello; retry uses the persisted invocation rather than a new command. Per-room
semantic faults are quarantined persistently; transient RPC errors retry, and
an fsynced pending binding reconciles uncertain invitation acceptance; capacity rejects enrollment without stopping
other rooms. Bridge framing/I/O faults permanently close the bridge and fail
closed until the server restarts. Engine tokens last 120 seconds for relay latency.
The same principal may inspect/reattach its invocations across groups; distinct
MLS groups isolate transport/session context, not the principal's engine rights.
Endpoint is SHA256(actual group ID).

Python frontend feeds authenticated requests to a bounded private Rust stdio
bridge exposing the existing SQLCipher engine. Bridge operations cover token,
submit, status, input generation/write/EOF, output cursors, cancel and maintenance.
No shell interpolation: argv is encoded byte-for-byte, shell mode is explicit.
Commands run as the local non-root user in operator-selected cwd. Existing
detached workers keep running when either frontend disconnects. Read stdout and
stderr independently using byte cursors; propagate exit code/signal accurately.
Stdin uses engine generation and offsets with bounded freshness and credit.

Frames are at most 32 KiB, argv aggregate at most 16 KiB, binary stream chunks
4096 bytes encoded as base64, RPC response at most 4 MiB, history scans at most
100 rows and seen-ID caches at most 2048 per group. Fail explicitly on saturation.
Use stop-and-wait requests for this prototype, bounded overall client deadlines,
and bounded polling. No interactive batching is needed yet.

## User surface and validation

Provide runnable client/server entry points, explicit relay URLs, generated
private identity state, allowlisted client identity, and npub@relay destination
semantics. No default public relay. Provide command submission, detach,
status/reattach/wait and cancellation. Private generated keys stay in ignored
state directories. Initial generated-identity support is documented separately
from later supplied-key/alias/SRV features.

Three agents own disjoint components: confinement, MDK RPC, Rust bridge. The
coordinator owns protocol/client/server and live validation. Claude reviews this
plan before implementation and completed code in auto mode, may run tests, and
useful findings are fixed before commit/push.

Offline tests cover framing limits, peer checks, namespace approved-origin and
decoy-origin behavior, malformed frames, wrong sender/group/kind, stale challenge,
request conflicts and byte-exact bridge behavior. A bounded live harness uses
fresh disposable server/client identities with explicitly selected White Noise
relays. It runs actual commands, checks spaces/Unicode/quotes/newlines/dollar/
backticks/leading dashes as literal arguments, binary stdin/stdout/stderr and
nonzero exit status, disconnect then reattach, exact retry without duplicate
side effects, fresh session isolation and replay rejection. Capture only public
IDs, counts, checksums, timings and test outcomes; remove disposable secrets and
processes. Run existing Rust/Python regressions, Claude review and secret/ignore
audit, then commit and push this increment with clear validation evidence.

This prototype does not certify normative MDK multi-device support, production
URL policy, or verified source provenance of the pinned installed daemon.

## Review dispositions and ordered gates

Claude's initial review is recorded in `docs/reviews/phase-2b-plan.json`.
The retry journal/status-first algorithm, startup baseline, precise saturation
criterion and cross-session principal rights above incorporate its blockers.
Invitations expose group.profile.name and description before acceptance; check
these and the allowlisted welcomer, then verify exact membership after accept.
Golden RPC vectors come from the pinned installed wn capture and are tested.
The canonical destination relay must already belong to the configured origin set.
Scheduled expiry records MDK unread/clock/scan deferrals without promising erasure.
Live stdin/output are bounded to 64 KiB per test; total live harness <=900 seconds
and each operation has an absolute deadline. Implementation proceeds through
three ordered validation gates: confinement/RPC offline; bridge/protocol offline;
then real live commands. Each gate records results; final code review covers all.

The namespace bootstrap must demonstrably bind port 443 on this host. The tested
fallback is outer `unshare -Urn`, bring loopback up, set netns-only
`ip_unprivileged_port_start=0`, start fixed listeners, then nest a bwrap user/PID
namespace with no daemon capabilities over the outer network namespace. No host
network/sysctl is modified. An equivalent single-bootstrap namespace is acceptable
only with an exact port-443 test and capability dropping before daemon launch.
Each origin has its own 127.0.0.N address and generated hosts entry. Stub tests
exercise approved/decoy origins without MDK; live startup verifies optional MDK
QUIC/blossom/analytics paths can fail without widening egress. Read-only CA mounts
preserve TLS validation. Namespace teardown contains daemon descendants, replacing
the older synthetic smoke's escaped-daemon-descendant limitation; command workers
remain outside and intentionally survive disconnect.
