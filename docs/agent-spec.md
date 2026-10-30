# Marmish agent specification

This is the implementation and design handoff for agents working in this repo.
The validated prototype baseline is commit `a6700b4`. Subsequent changes must
update this specification when they change behavior. Use Beads for live task
status and dependencies, [AGENTS.md](../AGENTS.md) for the build workflow,
[the build plan](build-plan.md) for phases and [the README](../README.md) for
concrete commands.

## Purpose and intended product

Marmish carries asynchronous remote commands and resumable shells over the
Marmot protocol, using MDK for Nostr identity, MLS groups and encrypted messages.
It has a server application that supervises an MDK daemon and authorizes remote
identities, and a client CLI intended to feel familiar to SSH users.

The remote Nostr identity is the command-line user, not the relay. Canonical
syntax is `marmish npub@relay`; aliases for npubs and explicitly configured
relay/SRV discovery are intended. `-u` selects the remote identity. The current
`-i` selects a persistent local identity directory; importing a supplied Nostr
private key remains intended work. Both endpoints require an explicit relay set.
There must be no default public White Noise relay or silent public fallback.
Public relays have only been used as explicitly selected test infrastructure.

Noninteractive commands come first. Interactive shells will use the target
user's shell inside tmux, with native detach/resume. An occupied session must
offer takeover, simultaneous read-only attachment, or a new independent session.
Only one attachment may write; takeover must fence the old writer. Endpoint
defaults are a five-minute keepalive and an eleven-minute timeout without fresh
authenticated inbound endpoint traffic. These leases must not kill the session
or command. Configurable keystroke batching is intended, with a hard maximum
latency; adaptive batching should follow measurement. X11, port forwarding and
file transfer are deferred beyond the initial release.

An unprivileged server maps all authorized identities to its own Unix account.
Configured Nostr-to-Unix-user mapping is intended for a later audited privilege
boundary; network and MDK processing must remain unprivileged. Normative
multi-device support must use the new Marmot specification once integrated into
the selected MDK version. Do not copy MLS databases between devices as a
substitute.

## Implemented and tested baseline

The validated prototype baseline is Linux-only, single-device and noninteractive.
A native macOS implementation is under validation; its presence does not establish
native runtime support until the runner checks below pass. Its
entry points are `tools/marmish.py` and `tools/marmishd.py`; the Rust
`marmish-engine-bridge` connects the frontend to `marmish-engine` and detached
`marmish-worker` processes. The server refuses effective UID 0 and account
switching. Authorized commands run as its local Unix user in configured cwd.

Implemented behavior includes generated persistent identities, explicit relay
URLs, a client allowlist, dedicated two-member Marmish rooms, default room reuse,
`--new-session`, direct byte-preserving argv execution, explicit opt-in shell
execution, separate binary stdin/stdout/stderr, EOF, exit code/signal, detach,
status, wait, retry and cancellation. Commands survive either frontend exiting;
workers persist output and outcome for reconnection. Service managers must
preserve these workers across frontend restarts.

A full real-relay run passed all twelve assertions in 168.471 seconds using
fresh disposable server/client identities and an unapproved identity. It covered
literal special characters and non-UTF-8 argv, binary streams, exit status,
cancellation, duplicate/conflicting submissions, negative frames and invitations,
both-frontends-stopped worker completion, restored MLS/output, previous-boot
control replay rejection, fresh groups, configurable retention, and actual CLI
processes. Full offline validation passed 82 Python and 96 Rust tests, workspace
clippy and formatting. These are baseline results, not an evergreen assertion
that future edits pass.

Evidence: [live report](validation/phase-2b-e2e.json),
[validation and limits](validation/phase-2b-prototype.md),
[phase design](phases/phase-2b-prototype.md),
[approved design review](reviews/phase-2b-plan-final.json),
[completed-code approval](reviews/phase-2b-code-final.json), and
[follow-up approval](reviews/phase-2b-code-followup.json).

## Design decisions to preserve

### Transport and authority

Use the explicitly hash-pinned installed `wnd` daemon through bounded private
Unix JSON RPC. Application payloads never enter OS process arguments or
environment variables. The observed daemon hash is recorded in the README;
exact binary-to-source provenance is not established. The sibling MDK checkout
was inspected as evidence, not treated as a clean dependency to modify silently.

The daemon runs in private user/network/PID namespaces. An outer namespace
enables loopback and unprivileged port 443; the inner environment drops all
capabilities before creating listeners and starting MDK. Fixed host brokers
connect those listeners only to explicitly configured relay origins. MDK keeps
end-to-end TLS/SNI verification. No external routes, ambient proxies, host socket
directories or external DNS are available inside the daemon namespace. Fail
closed when the isolation cannot be established. This enforces physical origins,
not URL paths inside TLS. Current URLs are root-path `wss://` DNS origins.

Authority comes from the authenticated MDK row's actual group, sender, direction,
kind 30100 and Marmish version tag. Payload-claimed identities or display names
never grant execution. Validate purpose and exact two-member membership. Rooms
use name `marmish/v1/<session-id>` and description
`marmish/v1 command transport`; ordinary DMs never authorize commands.

### Sessions, freshness and recovery

Reuse a saved conversation per remote identity and canonical relay set by
default. Each explicit new session creates a fresh independent MDK MLS group;
resumption preserves existing MLS state. This does not certify one-use
KeyPackages or normative multi-device behavior. Fresh keys alone do not prevent
application replay.

Frames bind protocol, session, actual group, server/client identities and random
request ID. A fresh hello obtains a boot-specific challenge valid for sixty
seconds and bound to group/session/authenticated peer. Engine launch tokens last
120 seconds in this prototype. Exact request duplicates are cached; conflicting
content is rejected. Startup history is baselined as stale. Old valid control
frames must not become new commands after restart.

Durable invocation identity is scoped to server, authorized principal, target
Unix account and invocation ID, across groups. The same authenticated principal
can inspect its jobs from a new session. Endpoint identity is derived from the
actual group. Separate MLS groups isolate transport context; they do not create
separate engine ownership rights for the same principal.

Before first submission, fsync a private journal containing the original argv,
invocation, session and token. Retry queries status first. If absent, obtain a
fresh token and submit the same invocation ID and original command; reconcile
an admission race by querying status. Never silently allocate a new invocation.
Existing reattachments must not replay stdin. A newly admitted retry may send
stdin, but interrupted stdin is not a durable client-side replay stream.

Persist launch state before process creation. Report uncertain launch or worker
loss explicitly and never automatically rerun uncertain work. Output cursors,
input generations/offsets and ordered EOF provide byte and retry boundaries;
expired, truncated or uncertain output is an explicit gap. A client transport
failure does not cancel a command or prove it never started.

Persist invitation intent before acceptance, then reconcile MDK confirmation
state after a timeout or restart; acceptance is not assumed idempotent.
Transient RPC failures retry. Semantic room faults quarantine only that room,
persistently. Bridge framing/I/O faults close the bridge and fail closed until
restart rather than reading mismatched replies.

### Additive command restrictions

The command-policy increment adds optional server `--command-policy` JSON.
`--allow` remains the authentication allowlist. Defaults and every per-key entry
are cumulative: each present allow list must match and any deny match wins.
An absent allow list is unconstrained; an empty one denies all. Entry ordering
never grants an override. Patterns are anchored executable-path byte matches:
`*` excludes slash, `**` includes slash, slash is literal, and backslash escapes
only star or backslash. See [the approved design](phases/command-policy.md).

Applicable restrictions require already canonical absolute argv[0], checked at
admission and launch; they refuse explicit shell strings. This is executable
selection, not argument filtering, interpreter confinement or inode pinning.
The engine and transactional Store enforce policy before consuming a launch
token or creating a job. Denial is the fixed `command_denied` category.
Existing accepted retries precede current policy checks. Each job persists its
canonical-path requirement; tightening policy does not revoke accepted jobs.
Validation: [14-assertion real-relay report](validation/command-policy-e2e.json)
and [checks and limits](validation/command-policy.md); 109 Rust and 87 Python
tests passed for this increment.

Config JSON version 2 prevents older binaries silently ignoring restrictions;
new binaries accept version 1, with absent policy and job markers defaulted.
The encrypted database schema version is unchanged.

### Native platform increment under validation

The native implementation separates Linux and Darwin process identities, clocks,
descriptor enumeration, launch descriptor closure and filesystem durability.
Darwin uses boot UUID plus process start identity, suspend-inclusive time, native
Unix peer credentials, APFS exclusive publication and descriptor-relative storage.
The Python transport factory selects Linux namespace confinement or native macOS
Seatbelt confinement with an explicitly restricted SOCKS broker. Missing native
primitives or provenance fail closed. No public relay defaults are introduced.

Clean daemon builds pin upstream MDK source, lockfile and native executable hashes;
macOS applies the maintained restricted-transport patch. Build provenance is
verified before execution. See [the native design](phases/macos-platform.md).
The local `cleanup_ready` bridge operation consumes the engine and closes
SQLCipher before acknowledging readiness; an opaque guard holds frontend and job
locks through owned test-state deletion. It is absent from the remote protocol.
Failure to prove quiescence preserves state and reports incomplete cleanup.

The [native validation workflow](../.github/workflows/native-validation.yml)
uses the supplied Linux AMD64, Linux ARM64 and macOS ARM64 runners. It requires
native isolation probes, offline tests, fresh pinned daemon builds, native real
Marmot command tests and commands in both Linux/macOS directions in one run
attempt at the same SHA. See [the validation design](phases/platform-validation.md).
Only trusted master pushes execute this workflow. macOS AMD64 runtime support
remains unvalidated without a corresponding native runner.

Native prerequisite failures now report fixed sanitized operation stages and
bounded errno/subprocess status, preserving the first failure through cleanup.
The macOS unconfined shim handoff runs separately before the broad spike; it
provides no Seatbelt proof. Missing checkout never invokes stale cleanup scripts,
and offline failure/cancellation still retains its private prefix. See the
[diagnostic correction](phases/native-diagnostics.md) for offline checks and
remaining native gates.

[Native run 40978430684](validation/native-run-40978430684.json), at `f1f3616`,
passed Linux AMD64 validation, including all three required patched-daemon
bootstrap tests and the metadata permission regression check. Linux ARM64
refused its nested namespace prerequisite (`userns_denied_private_bwrap`).
macOS ARM64 passed disposable explicit-Keychain provider acceptance; its
independent unconfined shim received the exact 72-byte frame, then exited 70
during lifetime cleanup. That status does not identify which cleanup operation
failed. The broader Seatbelt spike, full native frontend acceptance and
cross-platform live jobs remain pending; these provider results do not close them.

[Native run 39982966090](validation/native-run-39982966090.json), at commit
`fccfddc`, passed the Linux AMD64 namespace prerequisite, 131 Rust workspace
tests, clippy/formatting, Python checks (172 total, four Darwin-only skips),
pinned daemon build, executable upload and owned Bubblewrap cleanup. Linux
ARM64 and macOS ARM64 jobs remain queued without assigned runners. Their native
checks and both live cross-platform directions remain pending; cross-compilation
and Linux mocks are not substitutes for them.

The first native Linux runner stopped before tests because Bubblewrap was absent.
The reviewed CI refinement acquires a private executable from authenticated Ubuntu
package metadata, checks reviewed version floors and hashes, and passes its
absolute path through the sanitized frontends. The supervisor validates that path
before entering namespaces. This changes no host policy: a kernel or AppArmor
denial remains a hard failure, and successful acquisition alone proves neither
confinement nor native support. See [the refinement design](phases/platform-validation.md#runner-refinement-rootless-linux-bubblewrap-prerequisite)
and [the first-run evidence](validation/native-run-39904454063.json).

A second runner attempt authenticated and ran Bubblewrap but rejected its
service-managed ancestor path before namespaces; see [that evidence](validation/native-run-39907438883.json).
The reviewed placement refinement uses a fixed trusted Linux `/tmp` parent,
without relaxing the production validator. Prefixes are retained after uncertain cleanup, and no operator lifecycle
cleanup mechanism is currently verified. A successful native acceptance run must
complete its own cleanup; no host configuration is changed by Marmish. The local fresh-key
[full real-relay retry](validation/rootless-bwrap-e2e-retry.json) passed all
14 checks and completed cleanup. An earlier invitation-stage timeout is retained
in [the failed-run record](validation/rootless-bwrap-e2e.json); the successful
retry does not establish its cause or prove macOS interoperability. The
[final placement E2E run](validation/bwrap-placement-e2e.json) also passed all
14 checks in 211.453 seconds with complete owned cleanup; see the
[increment validation](validation/bwrap-placement-increment.json) and
[final code review](reviews/bwrap-placement-code-final.json).
The next native attempt identified a group-writable `bin` directory under the
runner's permissive umask. Its creation now explicitly requests mode `0700`,
with a real-selector bootstrap regression and
[actual signed acquisition/probe evidence under umask002](validation/bwrap-umask-local.json).
Native platform acceptance still requires the workflow results.


### Storage, retention and bounds

Use SQLCipher for engine state and retained stdout/stderr. In file-storage mode, a colocated private
master key permits unattended detached-worker recovery; obtaining both the key
and data defeats this at-rest protection. Frontend session metadata and retry
journals are private 0600 JSON, not encrypted databases. Outstanding journals
contain command arguments and authorization tokens; completed waits or terminal
status/cancel observations scrub them logically. This is not forensic erasure.
The [storage design](phases/sqlcipher-storage.md) governs key publication,
recovery and rejection of older plaintext roots.

Conversation application messages default to one day of retention, configurable
for new groups (currently 1–604800 seconds). This is separate from output,
invocation replay records, MLS state and future tmux lifetime. Relay deletion is
best effort; unread/deferred local messages and backups may outlive the policy.
Schedule local expiry maintenance and distinguish deferral from erasure.

Current bounds include 32 KiB application frames, 4096-byte stream chunks,
256 KiB RPC requests, 4 MiB RPC responses, 32 active/pending rooms and 256
quarantined rooms. Quarantine exhaustion blocks new enrollment until operator
intervention. Engine output defaults to 8 MiB per stream and a one-day lifetime;
replay records last at least 48 hours. Preserve independent replay protection
when deleting output.

MDK returns the newest 100 matching history rows oldest first. The seen-ID cache
is bounded at 2048 per group. Detect missing window overlap as a history gap;
do not use sender timestamps as arrival cursors. Delayed events that fall
entirely outside that window cannot be detected by the current API. Stop-and-wait
requests and eight-second retransmission recover missing replies within bounded
client deadlines; they do not promise reliable unlimited history or high
throughput.

## Remaining scope and limits

The working prototype does not close the production release gate. Strict broader
URL policy, verified daemon build provenance, packaging and operational hardening
remain distinct from the tested namespace origin boundary. Broker DNS results
are not filtered to public-only addresses; operators select trusted origins.
Commands themselves run outside the daemon sandbox with the server user's
authority. This is not a command sandbox.

PTY/tmux, attachment leases and takeover/readers, adaptive batching, identity
aliases, SRV discovery, supplied-key import, multi-device conformance, privileged
account mapping, X11, forwarding and file transfer are intended capabilities,
not implemented claims. Preserve the phase dependencies in the build plan and
use Beads to record the specific next work and blockers. Do not interpret an
unavailable future MDK capability as evidence that the existing command
prototype or explicit-relay tests cannot run.

### Native named key storage extension

Beads `marmish-d84` tracks native macOS Keychain and Linux Secret Service storage
for Nostr private keys and per-root SQLCipher masters. The user requested multiple
named keys and native storage by default when present. The reviewed phasing is
[the keychain extension](build-plan.md#native-keychain-storage-extension).

K1 adds the host-side `marmish-vault` library and bounded private-stdio
`marmish-keychain` helper, with exact immutable references, display names,
non-overwriting creation, readback and metadata-only listing. It distinguishes
absent, uninitialized, locked/denied, missing and ambiguous results. See
[the K1 contract](phases/keychain-vault.md) and [validation](validation/keychain-vault.md).
Linux actual-provider tests run an isolated owned GNOME Keyring. Disposable
explicit-keychain provider acceptance also passed on native macOS ARM; default
frontend and confined-daemon runtime execution remain separate pending gates.
K2 integrates native selection and multiple named Nostr keys. For a new profile,
`--key-storage auto` selects the native backend when present; only an absent
backend selects files with an announcement. Locked, denied, uninitialized or
broken services fail closed. Existing file profiles remain files until explicit
`--migrate-keys-to-native`; persisted native profiles never fall back. A durable
nonsecret files-mode marker preserves fresh-profile retries. Native helper
installation is required to establish availability in auto mode.

`--key-name` selects a catalog entry bound exclusively to one local profile
path/device/inode and random profile ID. Names cannot copy or adopt another
profile's MLS state. The default catalog is platform-specific, configurable with
`--key-catalog`; it contains Nostr references only. Each server engine has an
independent native SQLCipher reference and strict public marker. Fresh references
are journaled before creation and read back before profile publication.

The host helper obtains keys through bounded private stdio. A source-guarded,
manifest-v2 patched MDK consumes its Nostr bootstrap via a private inherited
stdin pipe, verifies the expected public identity, and uses an immutable
in-memory secret store. It cannot discover the host keychain from confinement.
The engine receives a provided master over private bridge stdin; detached workers
receive a fixed frame bound to job, root device/inode and native reference, then
replace fd0 with `/dev/null`. Workers already running can finish while the
keychain is locked; starting a new frontend still requires keychain access.

Migration fences the profile/catalog/MDK home/engine and workers, authenticates
existing encrypted state, creates native references, publishes both bindings,
then removes legacy secret files. An active worker refuses migration. Committed
cleanup is explicitly retryable; no identity or master is silently regenerated.
Migration currently requires an initialized encrypted database and every retained
job to be `Exited`; `Lost`/`UnknownLaunch` records refuse it. A fresh pending
reference whose creation never happened fails closed on retry, with no automatic
reset. Keep the journal; these are explicit recovery limits.
Python private RPC copies are not a full memory-erasure guarantee. Authorized
commands run as the local user and can access that user's services.

See [the K2 contract](phases/keychain-integration.md) and
[approved Linux integration review](reviews/keychain-k2-code-followup.json).
K2 is committed as `ed4271c`. Real native-provider Marmot validation is in progress; macOS runtime remains a
separate pending gate. Do not interpret implemented paths as a live acceptance
claim until linked validation evidence is published.
