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

The current prototype is Linux-only, single-device and noninteractive. Its
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

### Storage, retention and bounds

Use SQLCipher for engine state and retained stdout/stderr. The colocated private
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
