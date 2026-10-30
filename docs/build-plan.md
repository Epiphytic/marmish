# Marmish build plan

## Prototype distribution extension

The [distribution design](phases/distribution.md) adds three reviewed increments:
public Radicle/Iris source availability, complete native runtime archives with
safe user-local installation, and native release automation/publication for
Linux AMD64, Linux ARM64 and macOS ARM64. Initial installation uses the script
route; Cargo CLI publication requires a separate dependency/packaging design.
Release assets remain experimental and report existing native runtime blockers.
The installer is served from Iris by default and downloads checksummed GitHub
Release archives. Both source remotes must receive completed increments.

Marmish provides asynchronous remote command execution and resumable interactive shells over Marmot. The first release runs noninteractive commands; a subsequent release adds PTYs and tmux. A disconnected client does not stop a command. All network traffic uses explicitly configured relay URLs; there is no public relay fallback.

## Requirements and defaults

- Two applications: `marmish` client and `marmishd` server, each supervising a private MDK daemon when needed.
- Persist a generated Nostr identity, or use a supplied private identity file. Pin the remote server identity independently of relay infrastructure. Never put secrets in process arguments or logs.
- Canonical destination syntax is `marmish npub@relay`: the user component is the remote Nostr identity, with configured aliases for npubs. The server component supplies relay host/URL or explicitly configured SRV discovery address(es). `-i` selects the local client identity; `-u` selects the remote npub or alias. Unix account mapping is a separate server authorization decision. Phase 2 freezes multi-relay and SRV parsing/resolution rules before implementing the CLI.
- Allowlist client identities. Unprivileged servers map authorized clients to their own Unix user. Privileged account mapping is a later isolated component.
- Explicit relay URL/set on both sides, directly or through an alias. Apply the restriction to account setup, key packages, invitations, room traffic, discovery, retries, and fallback.
- Default DM application-message lifetime: 24 hours, configurable. Separate message retention, retransmission buffers, execution deduplication metadata, MLS protocol state, and tmux lifetime.
- Noninteractive commands have separate byte-preserving stdin/stdout/stderr, EOF, exit status, status/reattach, and explicit cancellation. Disconnection alone never cancels them.
- Direct argument-vector execution by default; shell interpretation only through an explicit shell-command mode.
- Use dedicated Marmish application rooms with a versioned purpose, authenticated peer and exact membership binding; ordinary DMs and chat history never authorize execution. New independent sessions obtain fresh MLS group/session key material through supported MDK group creation and KeyPackage APIs. Resuming an existing session preserves its MLS state and replay protections.
- Interactive sessions use the target user's shell inside tmux. A default session is scoped to server identity, client identity, and target Unix user. A new session creates a new DM room.
- Occupied interactive sessions offer takeover, read-only attachment, or a new session. One writer; multiple readers. Takeover revokes the old writer. Readers cannot input, resize, or signal.
- Attachment defaults: keepalive after 5 minutes without outbound traffic; timeout after 11 minutes without fresh authenticated inbound endpoint traffic. Ping/pong counts; duplicates and server output do not renew an endpoint lease. Session lifetime is independent.
- Configurable input/output batching with a hard maximum delay from the first buffered byte. Adaptive batching follows measurement.
- X11, forwarding, and file transfer are deferred; unsupported options fail clearly.
- Use the new Marmot multi-device specification through MDK once supported. Do not imitate multi-device by copying an MLS database between processes.

## Architecture

`marmish` and `marmishd` use a small MDK transport adapter. The adapter owns daemon lifecycle, identity/relay setup, room operations, subscriptions, and authenticated sender extraction. The command and session engines consume typed Marmish frames; they never execute ordinary chat text. Network parsing and MDK run unprivileged. Future Unix-account launching uses a narrow privileged helper.

Versioned frames carry server/client context, operation/session identity, endpoint identity, attachment generation, stream/channel, sequence, and bounded payload. Handshakes negotiate explicit protocol and capability versions; unsupported or unknown versions fail closed. State schemas have their own migration versions. Leases are keyed by endpoint, never by account identity alone. Handshakes bind fresh nonces to authenticated peers. Server-issued attachment generations fence stale writers. Delivery acknowledgements describe application acceptance, not relay publication. No exactly-once execution claim is made across a crash between durable state and process creation or PTY writes: uncertain work is reported and never automatically rerun.

Command execution follows `accepted → launching → running → exited`, with terminal `unknown-launch` or `lost` states when recovery cannot establish the outcome. Persist and sync `launching` before spawning. Record process identity including start time; never trust a reused PID. A per-job worker owns the child and persists status/output independently of a client or frontend connection; worker loss may make the outcome unavailable. Fault-inject every persistence/spawn boundary. Define a narrow same-user launcher interface in phase 1 that can later be implemented by the privileged helper. State directories are 0700, identity/state files 0600. Marmish state and per-stream output spools now use keyed SQLCipher, with protected master-key publication, per-database derived keys and offline detached-worker recovery. The separately reviewed [storage increment](phases/sqlcipher-storage.md) records recovery rules and encryption limits. The unprivileged frontend refuses effective UID 0 until the privilege phase. Environment, cwd and umask follow server policy; shell-string execution requires server permission as well as explicit client selection.

The durable deduplication key is (server identity, authorized principal, target Unix account, invocation ID), preserved across groups and sessions; retrying an invocation in a fresh group cannot run it again. Freshness tokens and transport sequence numbers are additionally bound to the authenticated group and session. Persist the request digest and state before launch; a repeated ID with different content is rejected. Output has bounded disk storage and explicit gaps when expired or truncated. Input has ordered, deduplicated delivery and a finite freshness window; reconnecting never blindly replays old keystrokes. Resource quotas cover processes, groups, frame sizes, pending bytes, output storage, and request frequency. Completion remains queryable after output expiry.

Delayed first delivery is not permission to launch indefinitely. Execution requires a server-issued, principal-bound token/epoch with a short server-clock deadline (proposed default 60 seconds); the client may request a longer bounded window explicitly. A started job outlives this token. Durable tombstones cover every interval in which a token can authorize execution, plus bounded in-flight delivery and skew margins. Expired tokens remain invalid after tombstone cleanup, restart, or clock anomalies; on uncertain clock/epoch recovery invalidate outstanding tokens. Never evict live replay protection to accept more work: refuse admission at quota. Output expiry never removes invocation identity/digest/outcome records early. Tests exercise replay after output deletion and after restart.

For noninteractive work, stdout/stderr always drain into a bounded disk spool independently of connected readers. The default at the cap is an explicit truncation/gap record while continuing to drain; a slow or absent reader cannot deadlock a job. Network catch-up uses per-stream credits, bounded frames and cursors. Stdin has a bounded ordered window; EOF applies only after all preceding input bytes. The phase 1 plan specifies disconnected stdin behavior and overload responses. Interactive batching never bypasses stream flow control.

Noninteractive jobs share a dedicated Marmish principal/account default room, multiplexed by invocation ID. Persist the room's versioned application purpose and binding to the server, client, target account and session; an authenticated sender in a general DM is insufficient authorization. A newly created independent session uses a newly created MLS group and supported MDK KeyPackage/invitation operations, with fresh group/session key material; the detailed plan must verify the selected MDK API and its KeyPackage freshness, consumption and reuse guarantees rather than assume them. Bind every application session identifier to its actual authenticated group, and enforce sequence, invocation-digest/deduplication and freshness checks within that binding. Fresh keys alone do not prevent application replay; rekeying or key rotation never resubmits or reruns commands. Resumption restores the existing session's persisted MLS state and durable application replay state; creating an independent group is a separate explicit operation. Every new interactive session gets its own room. Only validated Marmish payloads in an authorized room from an authenticated principal are considered. Ordinary chats and historical bootstrap records cannot execute commands. Early releases support one enrolled device per account; any unsupported multi-device state fails closed where MDK exposes enough information, otherwise enrollment remains gated. A room's authenticated members can potentially read its plaintext application content; addressing an output frame to an endpoint is not cryptographic isolation from other room devices. Phase 4 must document and test that visibility and device authorization explicitly.

## Phase workflow

Beads is the source of truth for task state, dependencies, review findings, and durable decisions. Repository documents hold the design and test evidence, not competing task lists.

Before each phase, write a detailed phase plan with file/module boundaries, interface contracts, acceptance checks, dependencies, and exclusions. Review it with Claude in a fresh session. Resolve blocking findings and record dispositions. Delegate implementation to agents with disjoint file ownership and an agreed interface; integrate centrally. Review the complete phase code with Claude Code in `--permission-mode auto`, allowing local test execution and inspection. Incorporate useful changes, rerun affected checks, and request another review when substantive findings require it. Record exact reviewer identity, revision/diff, commands, results, and remaining limitations. Reviewers are independent of the author; all blocking findings and dispositions are recorded in Beads. No phase closes with unresolved blocking findings. After the completed-code review, incorporated changes and all required checks pass, audit the exact staged diff and ignore rules for secrets, disposable identity/key files, MDK homes/databases, logs and transient test output. Stage only intended source, design and sanitized evidence artifacts, commit the reviewed phase or increment, and push it to the configured project remote. The user explicitly authorized these phase-completion commits and pushes. Record the commit, remote/branch and successful push evidence in Beads. Preserve the actual repository history: earlier work that was not committed separately remains an honestly described baseline or combined commit; do not rewrite history to fabricate one commit per earlier phase.

The user has requested this iterative execution workflow. Git commits and pushes after the above gates are authorized. The user's 2026-10-29 distribution request additionally authorizes experimental binary release publication and a public Radicle mirror (Beads `marmish-ic3`); unrelated deployment and third-party messages still require their own authorization. The user also explicitly authorized bounded live transport tests on selected official White Noise public relays using disposable test identities; public infrastructure remains absent from defaults. A genuine upstream blocker pauses its dependent work while independent phases may proceed.

## Phases and exit criteria

| Phase | Deliverable | Exit criteria |
| --- | --- | --- |
| 0 Compatibility and foundations | Pin and probe MDK, determine actual daemon contract, select Rust workspace and test seams | Evidence identifies supported identity/relay/room/message/subscription/retention/device behavior; checked-in bounded offline probes and dated source evidence; choose and document an enforceable relay egress boundary; absence of that boundary gates production transport, while explicitly authorized synthetic live probes may proceed; gaps explicitly gate dependent features; workspace builds and checks run |
| 1 Asynchronous command engine | Durable invocation state, byte streams, process supervision, bounded output, deduplication and recovery, tested without network | Commands survive client loss; separate stdout/stderr and EOF; status/reattach; explicit cancellation; duplicate/conflicting/stale submissions; crash uncertainty; output limits and cleanup tested; narrow launcher boundary and effective-UID-0 refusal; launch crash boundary fault injection |
| 2 Marmot transport and first release | Begin with the separately reviewed [increment 2a smoke](phases/phase-2.md) and user-requested [SQLCipher storage follow-up](phases/sqlcipher-storage.md), then the separately reviewed [increment 2b confined command prototype](phases/phase-2b-prototype.md), supervised daemons, persisted identities, explicit relays, allowlists, rooms, noninteractive client/server CLI | Disposable server/client identities execute real bounded commands on the test machine through Marmot over explicitly selected relays; sanitized evidence identifies exact artifacts and pass/fail outcomes, including stdout/stderr/exit/status, disconnect/retry/no duplicate execution, wrong sender, literal argv and binary stream bounds; dedicated application-room/session binding and supported fresh independent MLS group creation are verified; enforced allowlist passes peer-advertised decoy-relay zero-dial test; no public fallback; retries do not rerun; retained output and exit status recover; scheduled expiry sweeps and failure diagnostics work within supported MDK/relay limits; fail closed on unsupported dependencies |
| 3 Interactive sessions | PTY/tmux, terminal restoration, resize, batching, session selection, takeover/readers, keepalive and timeout | Interactive programs work; detach/resume redraws; simultaneous attachment races and stale input are fenced; readers cannot mutate; one room per new session; timing and output overload tested |
| 4 Multi-device conformance | Integrate and certify the selected MDK implementation of the new device specification | Independent endpoint/device identities, room access, revocation, reconnect, takeover, readers, and history visibility tested; incompatible MDK versions rejected; no unsupported multi-device promise before this gate |
| 5 Privileged account mapping | Small audited launcher, identity-to-Unix-user policy, account/session setup | No network/MDK work as root; correct UID/GID/groups/home/shell; unauthorized user switching denied; revocation enforced; isolated privilege tests and Claude security review pass |
| 6 Operational release | Packaging, diagnostics, private relay deployment example, performance/retention guidance | Installation and restart recovery tested; resource limits documented and exercised; benchmark interactive/large-output workloads; secrets absent from logs; release capability matrix accurately states supported phases |

Phase 1 depends on the protocol and process contracts from phase 0, but can proceed independently of unavailable MDK multi-device support. Phase 2 depends on a usable MDK message interface; phase 3 depends on phases 1 and 2. Phase 4 depends on the upstream device API and on phase 3 for interactive certification. Phase 5 depends on the phase 1 launcher contract and can follow the unprivileged first release independently of phase 4. Phase 6 packages only verified capabilities; multi-device availability is a release gate for any claim requiring it.

## MDK compatibility constraints

The inspected MDK revision accepts explicit discovery/default account relay sets but peer key-package and inbox resolution can introduce additional relay URLs. Explicit startup defaults alone do not meet Marmish's requirement. Phase 0 must choose a native MDK strict-allowlist policy or equivalent tested transport/egress enforcement; do not substitute a warning or optimistic configuration. Network isolation must account for DNS changes, WSS TLS identity, redirects/proxies and subprocess egress, not merely compare the initial URL strings. Production transport remains gated until this is proven. The user-authorized synthetic two-identity smoke in [the phase 2 plan](phases/phase-2.md) may use explicitly selected public relays without claiming strict egress enforcement; offline protocol/process work may continue. Phase 2 must inject a peer-advertised decoy outside the set and prove zero attempted out-of-set connections. Operators provide their relay infrastructure; no public endpoint is a default.

The daemon exposes explicit expiry sweeps but does not schedule them. Marmish schedules sweeps, reports failures/deferred cleanup, and verifies that deleting application content does not delete needed MLS state. Local retention is an application policy; relay-side deletion/expiry is best effort and is never represented as guaranteed erasure. Capabilities of the new device specification must be verified independently from exploratory documents.

## Decisions to validate in phase plans

- Exact pinned MDK source/binary and the stability of its daemon RPC versus persistent CLI subprocess interfaces.
- Application event encoding/capability negotiation supported by that revision.
- Supported MDK create-group/KeyPackage APIs and actual freshness, consumption/reuse and invitation guarantees for new independent sessions; persisted MLS behavior when resuming an existing room. Do not implement custom MLS key generation or infer application replay safety from fresh encryption keys.
- Availability of authenticated per-device sender information and the intended visibility of terminal output to other devices sharing a room.
- How message expiry is requested, local cleanup is enforced, and required MLS state is protected from content expiry.
- Execution worker survival across server restarts and safe treatment of the launch crash window.
- Quota defaults, disconnected stdin behavior, concrete tombstone duration satisfying the freshness invariant, and hard limits on delayed authenticated requests.
- How to validate complete authenticated room membership and device admission at the selected MDK version; unsupported enrollment cannot silently receive command privileges.

## Command policy and platform extension

The additive default/per-key command policy and native macOS request is tracked
in Beads `marmish-b33`. These increments precede interactive sessions and retain
the existing protocol, disconnect, replay and storage guarantees.

| Increment | Deliverable | Exit criteria |
| --- | --- | --- |
| A Command restrictions | [Detailed policy design](phases/command-policy.md), strict configuration and engine enforcement | Cumulative defaults/per-key limits, bounded star/double-star matching, shell/PATH refusal and canonical spelling checks at admission/launch (no inode pinning), negative and real Marmot policy tests, Claude design/code approval, commit/push |
| B Native macOS | Detailed reviewed platform design, durable engine, authenticated Unix RPC and confined MDK | Native macOS build, detached execution/recovery/SQLCipher tests, proven confinement without silent fallback, preserved Linux behavior, Claude review, commit/push |
| C Interoperability | Native CI matrix and live cross-platform command validation | Linux X64/ARM64 and macOS ARM64 native checks, macOS-to-Linux and Linux-to-macOS real Marmot execution, literal/binary/denial/retry evidence, Claude review, commit/push |

The organization runner configuration at
[`Epiphytic/.github`](https://github.com/Epiphytic/.github) was inspected via
`scripts/runners/README.md` and `test-self-hosted-runners.yml`. Labels are
`[self-hosted, macOS, ARM64, laptop-ci]`, `[self-hosted, Linux, ARM64, laptop-ci]`
and `[self-hosted, Linux, X64, bel-ci]`. Laptop platforms share one execution
slot; concurrent live macOS/Linux pairs must use Bel X64. Only public rendezvous
identities and sanitized results may leave private endpoint state. macOS MDK
artifact provenance and confinement require source inspection and runner probes
before claiming support; unavailable capabilities block dependent work only.

## Reference baseline

Planning starts from the existing checkout at `/home/openclaw/repos/mdk-fork`, revision `37c248a11239e108afc9bd4ee62b8f60542012e3`. The checkout is evidence to inspect, not automatically the selected dependency. Phase 0 must record the exact chosen source and behavior. Upstream references: [MDK](https://github.com/marmot-protocol/mdk) and [Marmot specification](https://github.com/marmot-protocol/marmot).

## Native keychain storage extension

Beads `marmish-d84` tracks named native keychain storage for Nostr identities
and SQLCipher master keys. New profiles prefer a present native keychain:
macOS legacy Keychain or Linux Secret Service. Locked/denied/ambiguous states
fail closed; backend absence alone may permit auto mode to choose private files.
Existing file profiles require explicit migration and are never regenerated.
Expose `--key-storage auto|native|files`; persist the selected backend. Auto mode
announces file selection when no native backend is present; an uncertain backend
failure never selects files.
Human names identify keys; immutable random references bind actual credentials.
SQLCipher master keys remain independent per state root.

| Phase | Deliverable | Exit criteria |
| --- | --- | --- |
| K1 Native vault | Reviewed host-side provider/helper for named native credentials | Non-overwriting create, exact lookup, locked/absent distinction, bounded secret IPC, actual isolated Linux keychain tests, native macOS tests when available, Claude code approval, commit/push |
| K2 Integration | Reviewed frontend selection, stable references, private engine/MDK handoff and explicit migration | Default selection only for new profiles, no persistent plaintext keys in native mode, detached worker independence, MDK confinement maintained, migration crash recovery and wrong-key/descriptor negatives, Claude code approval, commit/push |
| K3 Native validation | Real command execution using native keychains | Disposable named identities and per-root SQLCipher keys, reconnect/restart/detach and byte-preservation, native Linux/macOS tests and bidirectional execution where prerequisites work, sanitized evidence and honest outstanding gates, Claude approval, commit/push |

At each phase start, inspect the concrete contracts and obtain Claude approval
for its detailed design before implementation. A present but inaccessible store
never triggers silent plaintext fallback. Do not expose broad D-Bus or Keychain
authority inside the confined daemon.

## Guided installer increment

The [installer quickstart design](phases/installer-quickstart.md) adds opt-in
`install.sh --quickstart` after the experimental distribution. Its boundary is
verified installation plus existing CLI setup, with portable terminal I/O,
explicit relay and peer entry, native-keychain auto selection and owned child
cleanup. Exit criteria are offline archive/PTY acceptance on Linux amd64/arm64
and macOS arm64, independent Claude design/code review, accurate setup docs and
verified GitHub/Radicle publication. This does not close outstanding native
confinement or interoperability gates.

## Managed installation and unified identity service

The [reviewed overall proposal](phases/native-setup-unified-daemon.md), tracked
in Beads `marmish-x6z`, sequences a uv-managed interpreter/native-companion
packaging spike, thin verified bootstrap and app-owned setup, a shared per-identity
service with authenticated local CLI access, durable client/server/both roles,
and native installed acceptance. The default setup selection is Both through
Enter or c/s/b; absent inbound authorization leaves serving durably disabled
until explicit enablement. The installer increment can ship independently.

This is intended work, not existing support. Detailed designs and Claude reviews
precede each increment; the current macOS sandbox startup failure remains a
separate release/runtime gate. See [the overall review](reviews/managed-install-unified-overall.md)
for the nonempty engine contract, retained per-key restrictions, authoritative
configuration/migration, policy-transition recovery, and bootstrap trust decisions.
