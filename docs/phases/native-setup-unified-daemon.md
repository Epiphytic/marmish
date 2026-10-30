# Managed installation and one service per identity

Proposed high-level plan for Beads `marmish-x6z`; this is design input, not an
implementation approval or a claim that these interfaces already exist. Obtain
Claude's overall-plan review, then a detailed reviewed design at each increment.
The macOS startup investigation remains independent and retains its native gates.

## What exists

Python is part of the application runtime, not only the installer:

- `scripts/install.sh` embeds Python for bounded HTTPS downloads, release/schema
  validation, archive/path checks, immutable installation, ownership markers,
  uninstall and terminal quickstart. Replacing it with `curl | tar` would discard
  important behavior rather than remove a redundant language dependency.
- `tools/marmish_release.py:launcher` generates shell launchers executing
  `python3 -I -B` and importing `marmish_app.client_main` or `server_main`.
- `tools/marmish_app.py:Profile` owns key sessions, confined MDK startup, pinned
  daemon RPC and persisted room bindings. Both frontends currently open their own
  Profile. `Client._exchange` and `Server.tick` are synchronous polling loops.
  Starting both frontends against one identity directory is not a supported
  shared-daemon implementation; the transport and key locks must remain enforced.
- Rust already supplies the durable command engine, worker, engine bridge,
  protocol/domain types and native keychain helper. The Rust `marmish-mdk`
  adapter explicitly supports offline observation/version only; it is not a
  replacement for the Python live transport. Cargo workspace installation alone
  cannot install today's full application.
- Native archives include MDK and platform helpers, SQLCipher-backed engine
  binaries and provenance. macOS uses native system frameworks and ad-hoc
  signatures. Python, native dependencies and sandbox startup are separate gates.
  The reproduced abort also affects Apple `/usr/bin/true` under the profile, so
  replacing the installer or statically linking our helper is not a demonstrated
  fix for that failure.

## Recommended direction

Use uv as an installation-time manager for an explicitly tested CPython release
and an isolated Marmish environment. Keep the working Python application and
Rust helpers while extracting a unified service. A complete Rust application
port is a separate optional initiative, not necessary to avoid relying on an
arbitrary system Python or to share a transport.

uv documents both [managed Python installation](https://docs.astral.sh/uv/guides/install-python/)
and [isolated installed tools](https://docs.astral.sh/uv/guides/tools/). These are
capabilities to build on, not evidence that this repository is already a uv
package. The implementation must pin and validate the bootstrap/tool/interpreter
versions and sources; the user's Python 3.14.5 is not automatically a supported
release interpreter merely because it satisfies a minimum version check.

The public script should become a small POSIX shell bootstrap using curl. It
obtains the reviewed uv distribution when needed and delegates to the managed
installer/application entrypoint; it does not duplicate role selection or contain
network/identity orchestration. The existing bounded archive verifier remains
in use until equivalent verification is implemented and tested in its replacement.
Do not trade it for unvalidated extraction. An initial compatibility increment
may run that verifier under uv's managed Python before moving its implementation
into the application package. This still uses Python internally, honestly, but
requires no user-managed Python installation.

The steady-state launcher and service invoke a fixed absolute installed
interpreter, application and helper paths. They do not run `uvx`, resolve packages,
upgrade tools, or download an interpreter when the service starts. A service must
start offline with only its configured Marmot relays available. Avoid depending
on a transient uv cache or mutable generic tool environment for a running service;
retain an install-owned runtime slot and its interpreter until no installed version
uses them. Package application code and matching target-specific helpers together
or bind them by an exact release manifest; do not mix Python code from one version
with native helpers from another. Native licenses, helper hashes, code signatures,
Keychain behavior and source provenance remain part of the release gate.

## Service and local client boundary

One running `marmishd` owns one local profile, one Nostr account, one key session,
one confined MDK daemon, one MDK RPC dispatcher, and at most one engine frontend.
The same account may originate commands to peers and receive authorized requests.
Multiple named keys mean multiple independently configured service instances,
not multiple Nostr accounts selected inside one running service. Preserve the
catalog's existing one-name/account/profile binding and reject a duplicate owner.
The local CLI selects an existing profile/key/service; it does not open the MDK
home or retrieve the Nostr private key when a service owns them.

Proposed user interfaces, subject to detailed review:

- `marmish setup`: configure/init a named identity, explicit relay URLs, role and
  optional authorized inbound peer. Show the public npub and service status.
- `marmishd --identity PATH`: run the configured identity service in foreground;
  optional user-level systemd/launchd integration follows the same entrypoint.
- `marmish -i PATH npub@relay -- COMMAND ...`: connect over authenticated local
  IPC, preserving literal argv/stdin/stdout/stderr/status/detach/retry semantics.
- `marmish service status`, `marmish service enable-server`, and
  `marmish service disable-server`: explicit local configuration transactions.
  Existing `marmishd` and client options need documented compatibility/migration.

Use an owned mode0700 runtime directory and mode0600 Unix socket, with Linux
SO_PEERCRED and macOS getpeereid/process checks through the existing platform
helpers. A socket pathname alone is not authentication. Reject different UIDs,
unsafe/symlink paths and stale endpoints whose process identity cannot be verified.
The local endpoint is versioned, frame-limited, deadline-bound and backpressured;
its handshake binds profile ID, account and service generation. It exposes
Marmish operations only, never unrestricted MDK RPC, key retrieval or a raw engine
bridge. Client invocation IDs and received data retain existing durable journals.

A single service scheduler demultiplexes MDK room traffic by authenticated peer,
room, session and request direction. Client replies cannot enter the server
request dispatcher, and server requests cannot satisfy pending client exchanges.
Do not call the current blocking `Client._exchange` from the service tick loop.
Convert transport operations into bounded tasks/state machines with correlation,
retry deadlines and per-peer/group queues. Serialize profile mutations through
the owner; independent CLI attachments must not race room creation or overwrite
session bindings. Bound concurrent calls, rooms, histories and queued bytes.
A slow/disconnected CLI cannot starve inbound work or other local clients.

CLI disconnect leaves accepted commands running. Read/status/cancel/retry work
through the owner, with ownership and replay checks unchanged. Service restart
reconciles existing MDK state and durable workers rather than regenerating identity
or executing uncertain jobs again. Preserve two-member dedicated Marmish rooms,
per-session application binding, fresh independent MLS groups for new sessions,
default session reuse, exact request IDs, challenge/freshness checks, tombstones,
short configurable message retention and additive default/per-key restrictions.
Fresh MLS state alone remains insufficient to prove application replay safety.

## Roles and setup semantics

The menu is `[c] Client  [s] Server  [b] Both (default)`. Enter selects Both;
case-insensitive single-character input is sufficient. Arrow-key selection can
be added without making a terminal UI package mandatory. Blank role input and
blank authorized-npub input are separate questions. A blank npub is valid and
must never become an allow-all entry. Noninteractive setup requires explicit
options and never reads a piped script as prompt input; interactive prompts use
the controlling terminal. Restore terminal state on normal exit and signals.

Persist both requested role and explicit server enablement state in a versioned
configuration with the identity binding. The initial transaction behaves as
follows:

| Selected role | Authorized inbound peers | Effective behavior |
| --- | --- | --- |
| Client | Any | Outbound client enabled; inbound server disabled |
| Server | At least one valid npub | Inbound server enabled; outbound client disabled |
| Both or Enter | At least one valid npub | Both directions enabled |
| Server | None | Service configured, inbound disabled; no implicit outbound enable |
| Both or Enter | None | Client enabled; inbound disabled until explicitly re-enabled |

Show disabled state and the exact enable command in setup's result. Merely adding
an authorization entry later does not implicitly re-enable an inbound service
that setup or the operator disabled. `enable-server` requires a nonempty valid
allowlist and explicit local action. An empty effective authorization set always
prevents inbound admission, even if stored server enablement says true. Removing
the last peer disables server admission durably and requires explicit re-enable
when peers are added again. Configuration updates are atomic and validated by
the service, not mutations of a shared profile file by competing CLIs.

Disabling a role stops new operations in that direction. It does not silently
cancel already accepted commands or erase their journals/output. Define in the
detailed role design how existing authenticated response delivery, retained
status/cancel access and policy revocation behave during disablement; outbound
initiation must remain disabled while bounded responses/recovery are possible.
Disabling serving and revoking a peer are distinct operations. Existing engine
revocation/command policy checks remain authoritative. Re-enablement cannot
reset replay state or reactivate a revoked identity.

Native-keychain auto selection remains default when available; named identities
remain supported. No fallback to file secrets on locked/denied/corrupt native
storage. Setup resumes an interrupted initialization idempotently and never
replaces an existing account, key binding or encryption master just to change
roles. Retain current root refusal until privileged mapping has its own design.

## Proposed increments and exit criteria

| Increment | Deliverable and dependency | Required exit evidence |
| --- | --- | --- |
| 0 Contracts and packaging spike | Review this overall plan; define versioned service/config/IPC schemas and migration. Prove a pinned uv-managed interpreter can install the exact application and native companion without system Python, source builds or ambient package resolution. Select archive-plus-environment versus platform wheel layout from evidence. | Native Linux amd64/arm64 and macOS arm64 installation/launch smoke; exact interpreter and helper provenance; offline restart; written role transitions and bounded scheduler design approved by Claude. No claim that packaging fixes confinement. |
| 1 Managed bootstrap and application setup seam | Small curl/shell bootstrap plus install-owned retained managed Python slots; keep existing verifier semantics; move interactive setup entrypoint out of installer. Add app packaging/entrypoints where required. Depends on packaging spike. | No-system-Python install, malformed/traversal/oversize/archive/hash rejection, interrupted upgrade rollback, idempotence and owned uninstall; no automatic service downloads; argv/path/space tests; signatures, licenses and installed default paths validated. |
| 2 Unified identity owner and CLI IPC | Single service owns Profile/keys/MDK; bounded request demux and scheduler; CLI routes through authenticated local API. Preserve current noninteractive protocol and Rust engine. Depends on approved IPC/scheduler detailed design and increment1 entrypoint contract; may be developed separately from packaging mechanics. | Two local CLIs share one MDK/profile; simultaneous inbound/outbound commands with same Nostr identity; slow client isolation; foreign UID/stale endpoint denial; literal/binary streams, disconnect, retry/no duplicate, restart recovery, replay and quarantine tests. Real Marmot execution required. |
| 3 Persistent roles and guided setup | c/s/b menu default Both; optional inbound npub; explicit server enable/disable/status; atomic config and migration; optional user service integration. Depends on owner/control API. | Native PTY tests for Enter/c/s/b/invalid/EOF/signals; Both with no npub remains client-only through restart and later authorization edits; explicit enable succeeds only with allowed peer; disabling does not cancel detached work; old profile/keychain/room/journal migration preserves account and state. |
| 4 Native acceptance and distribution | Freeze exact sources/artifacts and perform Linux/macOS cross-platform acceptance, then publish reviewed experimental increment. | Fresh-key installed bidirectional command execution, same-identity client+server operation, all negative confinement/policy/IPC gates, native keychains, three architecture installs, offline service restart, explicit relay selection, bounded failure reporting, GitHub/Radicle/Iris verification. Report platform failures honestly; no success inferred from build or static doctor. |

At each increment, write the concrete detailed phase design, obtain Claude review
before implementation, delegate disjoint components, run acceptance, obtain
Claude auto-mode code review, incorporate findings, then commit and push both
source homes. Beads owns tasks, dependencies, review dispositions and blockers;
this document is the architectural proposal, not a parallel task queue.

A native installer or fully native application can be evaluated after these
contracts stabilize. A Rust bootstrap could reuse a tested bounded manifest/
archive verifier and call app-owned setup, but does not eliminate Python while
the service remains Python. A full native port includes live RPC, confinement,
key lifecycle, migration and scheduler behavior absent from today's Rust adapter.
A Linux musl static target could reduce the current glibc dependency, but needs
its own native-library, SQLCipher/OpenSSL, resolver and full platform E2E
validation; it is an evaluated optional distribution target, not supported by
this proposal. macOS still requires its dynamic system and Keychain frameworks. Neither is a shortcut
around the observed general sandbox startup abort.

## Overall review decisions before detailed designs

The first overall review identified four gaps. These are requirements of the
proposed increments, not implemented behavior:

1. Empty inbound authorization never starts an inbound Server or engine frontend.
   The existing Rust admission contract requires a nonempty principal set and
   stays enforced. Client with no peer is valid and touches no inbound path.
   Later allowlist changes must use a newly designed, reviewed engine-policy
   transition API under the identity owner's serialization: preserve all jobs,
   journals, output and replay records, update the persisted policy/version marker,
   and recover or fail closed after interruption. Never reinitialize the engine,
   replace its database, or edit a persisted marker to bypass policy comparison.
   Do not expose live allowlist edits until this transition is implemented and
   validated. Initial enable after no-peer setup creates the first inbound engine
   only after explicit enablement and a nonempty validated authorization set.
2. Preserve the complete restrictions catalog independently of active membership
   or server enablement. Inactive per-key restrictions are retained and validated,
   then reapplied unchanged when that principal is authorized again. Construct
   each active admission policy with only active principal entries so the existing
   subset invariant holds; never discard dormant restrictions or weaken defaults.
   Policy transition tests must demonstrate removal/re-addition preserves them.
3. A versioned, owned mode0600 service configuration is the sole role/allowlist
   authority for managed instances at startup and reload. An owner-mediated pending
   transaction/generation record coordinates config and engine policy transitions;
   inconsistent generations close admission until recovery. Detailed design must
   prove crash consistency before implementing edits. Existing CLI flags remain
   available for one compatibility release, but when managed configuration exists
   conflicting --allow/role flags are rejected and can never override durable
   disablement. Existing deployments are not silently enabled or migrated:
   explicit `marmish setup` migration imports the identity and operator-supplied
   authorization, confirms role through the new menu, and preserves all secrets,
   encryption state and journals. No-peer migration leaves inbound disabled.
4. Bootstrap uv and managed CPython versions and per-target artifact SHA256s are
   held in reviewed release data, with bounded HTTPS downloads and strict hash
   verification. No mutable latest interpreter/tool resolution, ambient UV_*/PIP_*
   overrides, package index configuration, or user uv config may alter selection.
   Run bootstrap in an allowlisted environment with configuration discovery
   disabled. Increment0 must prove exact CPython artifact provenance and digest
   enforcement, select a concrete tested patch release (not merely >=3.11), and
   refuse installation if the chosen uv interface cannot enforce this. Services
   never run uv or download anything at startup.

Increment1 can ship independently of the unified daemon and retains existing
frontend behavior, including invocation-only authorization. Its detailed managed
bootstrap design uses install-owned retained runtime slots rather than modifying
immutable release trees. Increment3 setup replaces foreground-only authorization
with the durable managed configuration; documentation and migration tests must
distinguish those modes. Root refusal, debug/doctor
semantics and explicit relay selection remain covered. Role disablement response/
status/cancel semantics and the bounded shared scheduler are mandatory detailed
phase design items, not implicit choices delegated to implementation.

The engine-policy transition API and config/engine generation recovery protocol
are explicit increment2 deliverables and prerequisites of increment3 role edits.
Re-enabling an existing engine uses that transition; it never uses first-engine
initialization. Increment0 must choose and prove the bootstrap trust anchor for
uv/CPython/release data; if uv cannot enforce a pinned interpreter digest, evaluate
shipping the verified interpreter in the release archive rather than silently
weakening verification or resolving an arbitrary interpreter.
