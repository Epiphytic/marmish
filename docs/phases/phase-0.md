# Phase 0: compatibility evidence and Rust foundations

## Purpose and boundaries

Establish a reproducible MDK compatibility contract and a small Rust workspace before implementing remote execution. This phase implements configuration validation, bounded transport framing, a documented CLI adapter/probe, and offline tests. It does not execute remote commands, spawn shells, implement job persistence, implement production room orchestration, or claim a working network release.

Follow the review and Beads workflow in [the build plan](../build-plan.md). Obtain Claude's detailed-plan review before implementation. Assign implementation through Beads and disjoint agent ownership; record review evidence and unresolved dependency gates there. This document describes design and acceptance, not a second task tracker.

## Baseline evidence

The read-only source baseline is `/home/openclaw/repos/mdk-fork` at `37c248a11239e108afc9bd4ee62b8f60542012e3`. Do not modify, build into, clean, checkout, or install from that unrelated working directory. Record its revision as the inspected baseline, not as proof that an arbitrary installed binary matches it. The checkout has staged modifications: record a diff stat and SHA-256 and reverify cited evidence from committed blobs using `git show REV:path`. Distinguish committed-source findings from working-tree observations. If later source verification needs a clean build, use a separately approved isolated source location and target directory.

Relevant baseline sources:

- `crates/cli/README.md`: installed `wn`/`wnd` commands, account setup, groups, application events, subscriptions, and retention.
- `crates/cli/src/daemon/runtime_host.rs`: explicit account setup discovery/default relay configuration.
- `crates/cli/src/daemon/protocol.rs`: CLI-shaped private daemon requests and public response types.
- `crates/cli/src/daemon/mod.rs`: 1 MiB request ceiling, 64 subscriptions, private socket lifecycle.
- `crates/marmot-app/src/directory/member_key_packages.rs` and `relay_plane/safety.rs`: discovered peer relay routes and host-safety checks, which are not an operator allowlist.
- `docs/explorations-of-multi-device.md`: exploratory, non-normative multi-device design; production enrollment support is unverified.

## Workspace and ownership

Use a minimal Rust workspace with a committed lockfile and explicit edition/MSRV selected from the installed toolchain and dependency requirements. Keep dependencies small; do not import the entire MDK workspace merely to model its CLI. Production client/server binaries belong to later phases.

| Owner | Files | Responsibility |
| --- | --- | --- |
| Agent A | `crates/marmish-core/**` | Relay/config validation, retention values, byte/frame limits, typed probe configuration, and core tests |
| Agent B | `crates/marmish-mdk/**` | Explicit-executable CLI adapter, offline fixtures/fake executable, probe example, and adapter tests |
| Integrator | Root `Cargo.toml`, `Cargo.lock`, `.gitignore`, `README.md`, `docs/**` | Agreed workspace scaffold, dependency integration, compatibility report, verification, and Claude review dispositions |

Freeze the shared interface before parallel edits. `marmish-mdk` depends on `marmish-core`; Agent B does not alter core files. Root manifest changes go through the integrator.

## Core contract

Define an explicit nonempty `RelaySet` whose entries parse as `ws` or `wss` URLs. Normalize through the URL parser, including default ports and slash forms, and deduplicate canonical values while preserving deterministic order. Reject credentials, fragments, query strings, and non-root paths for this initial contract; document this conservative compatibility restriction. Do not silently turn `http` into `ws`. Configuration cannot obtain relays from defaults, environment discovery, peer claims, or public fallbacks. Aliases may resolve only to explicitly supplied configuration.

Host-safety and relay authorization are separate checks. Development loopback requires an explicit setting in configuration only; phase 0 never exports `WN_ALLOW_LOOPBACK_RELAYS` or starts a relay connection; the adapter must surface MDK's private/link-local address restriction. Normalization is not DNS pinning or network enforcement. Do not claim an allowlist based on normalized startup arguments alone.

Use a typed retention duration with default 86,400 seconds. Distinguish explicit disabled retention from an absent value; bound numeric conversion and reject overflow/invalid durations. No cleanup path in this phase deletes MLS state. Define independently bounded encoded payload/frame and NDJSON line sizes, with a conservative application frame limit well below MDK's 1 MiB outer request limit. Reject over-limit data before allocation or decoding expands it without bounds. Preserve arbitrary stream bytes in the future envelope using an explicit encoding; the phase may implement that bounded envelope without assigning execution semantics.

Avoid fixing a full command/session wire protocol before the phase 1 review. A versioned probe envelope is sufficient to establish opaque application-content round trips; no received text or event can trigger a process launch.

## MDK CLI adapter/probe contract

Use the documented `wn` command surface first. Do not reverse engineer or depend directly on private CLI-shaped daemon RPC. Every invocation uses an explicitly configured executable path, private state path, bounded subprocess duration/output, structured argument arrays, and sanitized diagnostics. Do not invoke a shell to construct commands. Use `env_clear()` with no inherited environment for the only real call, `--version`; set cwd explicitly, null stdin, bounded concurrent stdout/stderr drains, and kill/reap the child process group on timeout. Ambient account, WN_*, HOME/XDG_*, proxy and analytics variables are never inherited. Fixture tests assert the exact cleared environment under poisoned ambient settings. Never pass a private key in arguments or capture it in logs. Phase 0 does not implement identity import, secret storage, or any runner accepting secret stdin; these belong to the reviewed phase 2 plan.

The production `RealWn` type exposes only `version()`, with no generic runner. Future typed command plans can be constructed and inspected but cannot be executed in a production build: every network-capable operation fails closed before spawn. A generic fixture runner exists only in `cfg(test)`; it canonicalizes fixture executables under test-owned temporary directories. A typed command classifier recognizes only version as offline. No public boolean or feature grants production egress. The probe supports offline validation by default. The only real installed-binary probe permitted in phase 0 is `wn --version`; no daemon start, account, room, message, or help-driven runtime operation is permitted. The observed installed binary reports `0.13.0`, which does not establish provenance. The following are future interface documentation and offline fixture command shapes, not authorized real phase-0 operations:

```sh
wn --home STATE --json daemon start \
  --discovery-relays RELAY --default-account-relays RELAY
wn --home STATE --json create-identity
# Import secrets only through piped stdin, never argv:
wn --home STATE --json login --secret-store file --nsec-stdin
wn --home STATE --account HOST --json groups create marmish CLIENT --retention 1d
wn --home STATE --account CLIENT --json groups invites
wn --home STATE --account CLIENT --json groups accept GROUP
wn --home STATE --account HOST --json messages send-event GROUP APP_KIND CONTENT
wn --home STATE --account HOST --json messages subscribe GROUP --kind APP_KIND
wn --home STATE --account HOST --json messages list GROUP --kind APP_KIND --limit 20
wn --home STATE --account HOST --json messages sweep-expired
```

Relay list flags at this baseline use comma delimiters; verify from committed argument definitions and reject commas in configured URLs so one value cannot inject additional relays. `messages send-event` CONTENT is argv: it can expose sensitive terminal data locally and hits OS argument limits. This is a phase 2 interface gap requiring a stdin/IPC API or an explicitly reviewed safe solution. A production transport using sensitive payload argv is not approved. File secret-store mode is needed for headless operation; its permissions remain a phase 2 validation gate. `APP_KIND` is a test-selected nonreserved application kind, not an invented interoperable Marmish assignment. Validate actual JSON responses before claiming a concrete authenticated-sender schema. Bound stderr, subprocess failure text, and NDJSON records. Terminate and reap timed-out probe children. Keep stdout/stderr separate.

NDJSON reads use a bounded reader, never unbounded `read_line`. Record canonical MLS `group_id` separately from the Nostr routing identifier. Relay acceptance/publication is not an application acknowledgement. NDJSON subscriptions have a bounded initial snapshot (up to 200 at this baseline) and bounded replay/deduplication; they are not a durable cursor stream. The compatibility report must describe paginated catch-up, resubscription gaps, and application-level deduplication required by later phases. Phase 0 only tests parsing/command contracts, not a claim of lossless replay.

## Version identity and live integration gate

Record dated source URL/revision and exact source line evidence, `wn --version` output, and executable hashes for any real binaries actually probed. A version string alone cannot establish source provenance. An approved runtime pin must associate both binaries with a reproducible source build or verified artifact and required capabilities. Reject unknown/mismatched runtime identities for any gated live path; offline fixtures explicitly identify themselves as fixtures. An arbitrary `--expect-version` flag cannot bless a binary or enable networking. If provenance cannot be established here, record the runtime pin as pending and leave live execution disabled.

The inspected MDK revision lacks a demonstrated strict configured-relay allowlist. Therefore the normal adapter/probe must fail closed before spawning a network-capable MDK process when strict egress capability is absent. A user-supplied `--relay` or boolean acknowledgement does not satisfy this gate. Do not weaken the requirement to permit all public hosts or assume newly generated identities eliminate discovered routes.

The compatibility report compares native MDK allowlist, network namespace/nftables with IP-literal relays, and seccomp/connect-filter containment, including DNS/rebinding, TLS identity, redirects, proxy and subprocess behavior. Select native MDK allowlisting as preferred, unverified; absence blocks live work. OS containment is optional defence in depth, not a substitute for exact URL identity enforcement. A future real local integration experiment requires proven isolation that constrains both address destinations and requested relay identities, or an independently verified MDK allowlist implementation. Test setup must verify that enforcement before starting MDK, prohibit public traffic, and leave the default gate closed. Phase 0 provides reproducible proposed commands and marks real networking unverified; it does not run this experiment. Do not modify MDK to remove the blocker during this phase.

The decisive live test publishes peer directory/group routing information naming an out-of-set relay and proves there is no attempted connection outside the configured set, including discovery, imported identities, invitations, retries, and fallback. Merely observing successful traffic to a local relay is insufficient evidence. A failed or unrun test gates phase 2; it does not block independent phase 1 command-engine work.

## Retention and device gates

Founding room configuration can use `--retention 1d`; group retention changes apply by authenticated epoch rather than rewriting old message policy. `wnd` does not schedule expiry sweeps, so later orchestration must schedule `messages sweep-expired` and inspect deferred/failure results. Fixtures should cover successful pruning and deferred statuses. Document that local expiry is not guaranteed deletion from relays or other devices, and unread/clock-skew handling can defer cleanup.

Record multi-device enrollment, authenticated device attribution, history synchronization, and revocation as unverified capabilities. Do not infer support from a stable KeyPackage slot or a simulator scenario. No database copying, same-key multi-writer workaround, or private MLS state reuse. Phase 4 remains blocked until a specific supported normative specification and MDK API are verified.

## Verification and phase acceptance

Required automated checks: `cargo fmt --all -- --check`, `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`, `cargo test --offline --locked --workspace`, and `cargo build --offline --locked --workspace --all-targets`. Use the existing Cargo cache where possible. If dependencies are absent, fetching public build dependencies is within the user-authorized implementation scope; record the explicit `cargo fetch` command, then run tests/build offline. Dependency fetching is distinct from forbidden public-relay traffic. Optionally run tests with `unshare -rn` and record platform denial if unavailable. Tests must remain offline and must never discover or start an installed `wn` implicitly.

Core tests cover equivalent URL normalization/deduplication, empty/malformed/unapproved relay inputs, retention default/disabled/overflow cases, byte round trips, and frame limits. Adapter tests use a test-owned fake `wn` executable with explicit fixture output to exercise argv/environment construction, successful synthetic JSON/NDJSON decoding, malformed/overlong/truncated records, nonzero exits, timeout cleanup, exact environment clearing, PATH-empty operation, and strict-live-gate rejection before process spawning (marker/counter seam). No real installed executable is discovered implicitly. Tests use clearly synthetic data; no private key literals are included. Test-support executables cannot be installed as production transports. The fake is only a subprocess contract fixture: it performs no Marmot cryptography, networking, identity authentication, or transport and is never selectable as a production backend.

Produce `docs/mdk-compatibility.md` recording exact evidence, commands/check results, supported versus unverified behavior, relay-policy blocker, payload limits, catch-up limitations, retention scheduling, runtime provenance, and multi-device gates. Include a reproducible offline probe invocation and clearly labeled optional live procedure. Mark every unrun live check unverified, not passed or complete. Do not invent JSON fixtures and describe them as captured real MDK output; label synthetic fixtures explicitly.

After integration, Claude reviews the complete diff in auto mode with access to local tests. Incorporate useful findings, rerun affected checks, and record dispositions in Beads/review evidence. Phase 0 closes only when foundation checks and reviewed evidence are complete, no blocking phase-0 findings remain, and downstream live/device blockers are explicitly preserved. Successful offline fixtures do not close the phase 2 network gate.

## Frozen interface and integration order

The integrator creates the root workspace manifest and shared dependency versions first. Implementations remain in their assigned crates. Agent B may use core only through these agreed signatures; it can author before A finishes and compile after both land.

Core public types (all errors implement `std::error::Error`, diagnostics never echo rejected raw values):

```rust
RelaySet::parse(entries: &[String], allow_loopback: bool) -> Result<RelaySet, ConfigError>
RelaySet::as_slice(&self) -> &[String] // canonical URL strings, first-seen order
Retention::default() -> Retention // seconds() == Some(86400)
Retention::from_seconds(seconds: Option<u64>) -> Result<Retention, ConfigError>
// None = Disabled; Some(0) is rejected; numeric bound chosen and documented
Retention::seconds(&self) -> Option<u64>
Limits::default() -> Limits // raw 16 KiB, encoded JSON frame 32 KiB, NDJSON line 64 KiB
ProbeEnvelope::encode(bytes: &[u8]) -> Result<Vec<u8>, FrameError>
ProbeEnvelope::decode(frame: &[u8]) -> Result<Vec<u8>, FrameError>
```

`ConfigError` distinguishes empty set, invalid URL, disallowed scheme/components/host, loopback denied, and retention out of range. `FrameError` distinguishes size limit, invalid JSON/version/encoding. Envelope bytes are canonical padded base64; check input/encoded length before decoding, enforce decoded size, reject noncanonical encodings. Limits are constants or immutable values in this phase; callers cannot enlarge them. Host canonicalization covers case, IDNA and IPv6; trailing-dot and private/literal host behavior is explicit and tested.

Adapter public types:

```rust
RealWn::new(executable: PathBuf) -> Result<RealWn, AdapterError> // explicit absolute file
RealWn::version(&self) -> Result<VersionObservation, AdapterError>
// No run(Command) on RealWn
CommandPlan::class(&self) -> CommandClass // OfflineVersion or NetworkCapable
CommandPlan::arguments(&self) -> &[OsString] // synthetic planning only
CommandPlan::require_live_egress(&self) -> Result<(), AdapterError>
// NetworkCapable always returns LiveEgressUnverified before spawn
```

`VersionObservation` contains bounded version output only, never approval/provenance. `AdapterError` distinguishes invalid executable, missing executable, timeout, output limit, invalid response, subprocess failure, and unverified egress. JSON/NDJSON parsing may be adapter-private helpers exercised under test; no invented authenticated sender schema is exposed. There is no constructible production `EgressProof` and no production network runner in phase 0, which is a stronger boundary than passing an unverified token to a generic runner.

Secret wrappers are deliberately deferred: phase 0 accepts no private key and does not execute import commands. Phase 2 must introduce a zeroizing redacted secret type before implementing import, with no serde serialization and no secret argv/log capture. Configured private state paths, where created, use 0700, refuse symlinks, and are ignored by Git. Fixtures use temporary test-owned paths and no secret values.
