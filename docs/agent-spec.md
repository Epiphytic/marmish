# Marmish native v0.2 agent specification

The user explicitly authorized
publishing native 0.2.0 before full platform qualification, with macOS testing
still in progress and fixes planned for v0.2.1. This is not a claim that all
acceptance gates passed. Source and distribution facts must match actual publication evidence.

**Validation boundary:** baseline source
`347cf799ae5baa9e577ded1a00566c7a6b99b933` passed product unit verification,
native builds and ABI inspection on Linux AMD64, Linux ARM64 and macOS ARM64;
macOS signing passed. The macOS installed-checks feature suite passed 426 tests
and failed two benchmark child-process fixtures. The cause remains unproven.
All-platform installed/full end-to-end, lifecycle and actual default-auto provider
qualification remain pending. Descriptions below document implemented contracts,
not a claim that each contract has final hosted validation on every target.


Baseline run [41655210565](https://github.com/Epiphytic/marmish/actions/runs/41655210565)
failed overall; its Linux jobs were cancelled after those product steps passed.

## Scope and navigation

Marmish executes explicitly authorized remote commands through authenticated,
encrypted Marmot/MLS rooms. The native Rust CLI, daemon and dedicated worker
replace the historical Python application for v0.2. Python sources remain a
legacy reference; Python, uv, Node, Cargo, a browser and WASM are not required
by installed product operation. Some repository qualification tools use Python
or Node outside the product environment.

The architectural plan is [v0.2-plan.md](v0.2-plan.md). Detailed contracts live in
[Phase 2](phases/v0.2-phase2.md), its
[interfaces](phases/v0.2-phase2-interfaces.md),
[retention/ACK amendment](phases/v0.2-phase2-retention-and-ack.md),
[server limits](phases/v0.2-phase2-server-limits.md), and
[native installation](phases/v0.2-phase3-release-entrypoints.md).
Beads tracks work and review dispositions; this document describes behavior.

The supported release targets are Linux AMD64, Linux ARM64 and macOS ARM64.
Current manifests declare glibc 2.39 for GNU/Linux and macOS 15.0 for Darwin.
Other operating systems, architectures and older runtime platforms are not
covered by those assets. Target availability is separate from full qualification. Darwin ad-hoc signing is not Developer ID signing or
notarization; the actual manifest records the signing state.

Direct relays are the intended v0.2 command route; full installed qualification
is still pending for this initial release. Working Tor command
transport is deferred to v0.3. Embedded Arti and its foundation evidence remain
experimental command capabilities, with no direct-network fallback after a
Tor failure. No PTY, tmux, interactive shell, Unix-login mapping, root execution
service or general process sandbox is promised by this release.

## Native 0.2.1 launcher upgrades

The development native installer automatically replaces recognized v0.1
`0.1.0-experimental.N` launchers using the strict legacy marker, owned selector
and exact regular wrapper bytes. It never executes an existing launcher.
Receipt-owned native updates retain their existing behavior. Foreign files,
symlinks and ambiguous ownership refuse without replacement. The old runtime,
marker, private state and same-directory launcher backups remain; this is not
profile migration or automatic disk cleanup. Receipt generation determines
rollback versus roll-forward after interruption, with recovery on the next
installer invocation. Native uninstall removes native links, not the retained
legacy backups. See the [reviewed design](phases/v0.2.1-native-legacy-upgrade.md).
The published 0.2.0 bootstrap does not gain this behavior until updated binary
assets and a matching bootstrap are published. Hosted migration tests on all
three targets are still pending; local tests are not a platform qualification.

## Identity, ownership and local trust

One named profile owns one Nostr identity, encrypted native state and one
serialized MDK account owner. The same identity can be client, server or both.
Separate named profiles have separate ownership leases and state. The local
CLI talks to that owner; it does not concurrently open its MLS database.

Private profiles, sockets, configuration and key material are owned by the
current Unix UID. Socket authentication uses SO_PEERCRED on Linux and
getpeereid on macOS, alongside an owner-only socket directory. Local IPC is a
same-UID interface, not a new remote authentication mechanism. Root operation
is refused. Installation and identity state use separate directory trees.

`-i NAME` selects the local named identity. `-u ACCOUNT` selects a remote public
Nostr identity or configured alias; it never selects a Unix account or imports
a private key. Public destinations require the npub spelling; nsec and secret
material are not destination syntax. Configuration accepts its reviewed public
key forms without claiming that parsing alone authenticates a peer.

Roles control capability, not identity creation. Client mode can initiate
commands to trusted remotes; server mode can accept execution from granted
clients; both permits each independently. A default both profile with no
execution grants cannot accept commands. No engine is constructed when neither
current nor retained historical principals require one.

`trusted_remotes` authorizes outbound command-room relationships.
`execution_clients` authorizes inbound command execution. Setup's `--trust` and
`--grant` change these different sets; neither implies the other. Updates require
explicit setup update semantics. Removing authority invalidates current cached
responses/publications as required; it does not silently cancel a running job.
A fresh unauthenticated message, ordinary DM, relay connection or relay receipt
cannot grant command execution.

## Keys and storage

`--key-storage auto|keychain|file` selects native custody. Auto may choose file
storage for a genuinely unavailable provider when creating a fresh identity.
A configured keychain profile does not downgrade, regenerate keys or fall back
to plaintext/file custody after locked, denied or erroneous provider access.
Explicit keychain mode fails when the provider cannot satisfy its contract.

macOS uses Keychain and Linux uses Secret Service through its real provider
session; Linux hosts without an accessible provider can explicitly use file
custody. Keychain-backed profiles retain references, not serialized secret
copies. Named identities use distinct entries, including independent Nostr and
SQLCipher material. Both product frontends embed the private vault-helper
entrypoint; no separately installed helper executable is required.

Secret-bearing engine, journal and MDK storage uses SQLCipher. One process-wide
initialization barrier precedes all linked SQLite consumers, including Arti's
cache. This addresses the reviewed parallel initialization race without
serializing the test suite or weakening encryption. Private files, ownership,
link/type checks and canonical path requirements remain enforced.

Current format constants are profile 2, journal 6, engine 1, config 1 and commands
1. Command tables share the journal's persisted compatibility gate; commands=1
is not an independent database version check. Journal opening recognizes the
implemented older native formats 0, 3, 4 and 5 before checked initialization and
stamping 6. This does not promise migration of historical Python profiles.
Newer unsupported formats fail preflight before ordinary owner opening.

Read-only preflight can cause the pinned SQLite stack to create/open bounded
WAL/SHM sidecars; it does not promise a byte-for-byte unchanged directory.
Main encrypted database preservation and the exact permitted sidecar inventory
are separately specified in the
[version lifecycle design](phases/v0.2-installed-version-lifecycle.md).
Never delete databases, change format markers or bypass checks to recover access.

## Relay selection, destinations and routing

Relays are explicit; there is no public default. A named profile freezes a
canonical relay set and route policy. Selected command destinations must equal
that exact set, not merely be a subset. CLI options cannot broaden or replace a
live owner's connector policy. Aliases bind an exact remote npub and exact relay
set, not a hostname-based authorization rule.

Commands require the separator before argv, for example:

```
marmish -i client npub1...@relay.example -- /canonical/program literal-argument
marmish -i client -u npub1... --relay wss://relay.example/ -- /canonical/program
marmish -i client ALIAS -- /canonical/program
```

Bare hosts normalize to secure WebSocket URLs. Full URLs, IPv4 and bracketed
IPv6 use the native parser. Conflicting inline, alias, --relay, --srv and -u
sources fail rather than choosing one by precedence. Relay endpoints never
choose or replace the authenticated remote Nostr identity.

Explicit `srv:_SERVICE._tcp.DOMAIN` or repeated `--srv NAME` opts into bounded
system-recursive DNS discovery in the CLI before IPC. The raw-query resolver
accepts only matching SRV answers; it does not follow response-directed CNAME
or additional-record target lookups. No search domains, public resolver default,
implicit service guessing or periodic refresh is used. Resolution happens once
under its original five-second budget, then its snapshot is discarded after
selection. The resulting canonical relay set must still equal the profile set.
See [destinations](phases/v0.2-phase2-destinations.md) and the overriding
[raw-query correction](phases/v0.2-phase2-srv-raw-query.md).

SRV is rejected before DNS for always-Tor mode and onion selectors. In auto mode,
ordinary explicit SRV selection permits direct DNS as documented; it does not
authorize onion name leakage. `.onion` names are never sent to system DNS.
Tor auto routes ordinary relays directly and onion relays through Arti; always
uses Arti for every relay, and off refuses onion endpoints. Route failure never
changes the selected route. Mixed auto sets intentionally permit direct traffic.

Relay publication confirms relay acceptance, not peer decryption, command
admission, output consumption or process completion. Native source ACKs and
frozen outgoing-event obligations are tracked independently of application TTL.

## Command authentication and sessions

Native `marmish-command/1` intentionally breaks legacy Python wire compatibility.
Closed canonical request/response schemas bind protocol purpose, direction,
client, server, session, operation and request identity. Actual MLS group bytes
are authenticated out-of-band, not trusted from payload fields or confused
with a transport identifier. Command rooms and foundation-test rooms have
separate purpose records and quotas; cross-purpose reuse is rejected.

The server checks current room authority and inbound grants before admission.
Engine endpoint/principal ownership derives from the sealed authenticated
context. Peer-supplied fields cannot supply a token, UID, arbitrary engine root
or execution authority. Command replay uses timestamp-bound IDs, a durable
seven-day replay floor and immutable invocation digests, not fresh-key intuition.

Ordinary commands reuse a separate default command room. `--new-session` on a
new submission requests an independent room and leaves default reuse unchanged.
Handle operations, setup and administration reject that flag. The CLI freezes
one preparation token across bounded retries; the daemon chooses the actual
independent session and authenticates the room.

An atomic encrypted-journal creation fence binds token, peer, exact normalized
relay-set digest and session before one native create attempt. An interrupted
attempt without an accepted room returns command_room_uncertain; retry does not
create again or adopt an unproven orphan. A successful fresh room consumes an
active slot. There are 64 active command rooms, 4096 lifetime room metadata rows
and 4096 permanent preparation markers per profile, with no user room-retirement
operation. Total metadata exhaustion refuses and terminally forgets the new
unaccepted native group; it does not erase old obligations. Existing default
reuse remains possible at capacity. See [new sessions](phases/v0.2-new-session.md).

## Execution policy and process behavior

A native command carries direct byte argv and one immutable finite stdin value.
The server accepts the whole canonical argv/stdin digest durably before engine
side effects. Argv is not shell-parsed. Shell requests are denied in native v0.2;
authorized interpreters can nevertheless execute arbitrary code as the local
Unix user. Execution policy is not a sandbox or resource-isolation promise.

Executable paths must be absolute and canonical. A symlink spelling such as
/bin on a platform where it aliases /usr/bin may be refused even when the
canonical target is permitted. Filesystem canonical inspection occurs on the
blocking engine owner, before token issuance, independently of pure policy
matching. Cwd is fixed by config or the passwd home, with a minimal fixed
execution environment and no shell, tilde or environment expansion.

Default and every matching per-key restriction accumulate. Allows intersect,
denies union, omitted allow adds no constraint, and an empty allow denies all.
Repeated matching key entries add restrictions. Slash-aware globs match argv0:
`*` excludes slash and `**` includes it. An effective /** baseline for omitted
allow does not override explicit layers, grants or canonical-path checks.
Configuration reload requires restart; no hot-reload authorization is promised.

Engine calls run on a bounded blocking owner thread, not in MDK convergence.
The actor registers operations durably, projects completion facts/outbox updates
and acknowledges commits before advancing recovery barriers. An ambiguous
Submit is reconciled by status before any possible retry; an engine-known job
that later disappears is never redispatched as a new command.

Stdin is at most 16 KiB, immutable and local EOF-delivered only after actual
input progress reaches its length. There is no network streaming stdin or PTY.
With a TTY, default stdin is empty; --stdin explicitly reads finite input, while
--stdin-none forces empty. Non-TTY input defaults to reading through EOF under
the finite bound. Withholding EOF does not make interpreter side effects atomic.
An early exit with unconsumed nonempty input produces an input-failed diagnostic;
a zero-input command need not receive fabricated EOF to have a valid outcome.

## Handles, output and failure reporting

Default submission waits for authenticated admission, then copies stdout and
stderr bytes independently. --detach returns after durable server acceptance,
prints the private canonical handle on stderr and leaves stdout empty. A normal
successful attached command need not print a handle. `commands`, `command-status`,
`wait`, `output` and `cancel` operate through bounded same-UID IPC and explicit
selectors, including --handle and --handle-file.

A handle binds profile, remote, session, invocation and original request SHA.
It is not remote authority. Its whole-request SHA differs from the group-bound
Submit digest used by the engine ledger and Finish. Read-only client invocation
metadata exposes that latter digest separately; no raw MLS group is exposed.

Output polling uses one outstanding combined bounded Poll and durable stdout/
stderr cursors. CLI writes bytes before committing local response consumption.
A crash in that interval may repeat bytes after restart; terminal output writes
are not exactly once. Unread responses are not age-evicted simply to free quota.
Explicit gaps, invalid chunks and uncertain outcomes are reported, not hidden.

Known remote exit codes pass through; signal N maps to min(255,128+N).
Local/transport/policy/input/output failures use 255; unknown launch/lost or
ambiguous admission uses 254. Remote 254 and 255 remain valid remote outcomes;
finite stderr diagnostics distinguish them. Timeout preserves the handle and
reports pending rather than claiming remote termination. Ctrl-C detaches; only
explicit cancel requests cancellation. Cancellation itself is not proof of exit.

Ordinary client disconnect, daemon Stop and uptime exit do not cancel admitted
workers. Restart resumes ledger/native reconciliation and output retrieval.
Accepted-but-undispatched work may remain for recovery; Lost/UnknownLaunch is
never converted to successful completion or safely rerunnable work by guesswork.

## Limits, retention and automatic server exit

Defaults are four active jobs per principal, 32 globally, 1024 retained
invocations per principal and 10000 globally. Config may lower these within its
validated bounds. Uncertain/unreconciled jobs retain active credit. Retained
rows, pending requests, publications, rooms and native obligations have distinct
finite limits; overload is explicit busy/quota backpressure, not dropped work.

The final encoded command envelope is always at most 32768 bytes, including
argv/stdin/metadata. Local IPC frames are at most 64 KiB. Output uses combined
bounded pages; the default retained stream cap is 8 MiB each and aggregate
engine spool cap is 512 MiB. OS pipes continue draining when storage caps cause
gaps. A full output hash is unavailable when retention/gaps prevent proving it.

Payload retention defaults to one day; the command replay horizon is seven days.
Engine tombstone retention is fixed at two days. Expiring payload does not expire
running/uncertain ownership, immutable replay facts or protected native bytes.
Retirement requires the appropriate consumed-slot, source, publication and
post-ACK native-absence proofs. Invalidations protect the affected command groups;
no fabricated ACK or unfiltered-obligation deletion may satisfy cleanup.

`--max-commands N` counts unique engine admissions, not retries, refused requests
or provisional ledger reservations. Durable credits prevent overshoot. Reaching
N closes new admission, while existing responses continue serving. Automatic
exit additionally requires the authenticated client's durable Finish assertion
for every counted admission and complete local/native drain predicates.

Finish binds the original group Submit digest, authoritative terminal fingerprint
and exact final gap-aware cursors. It cannot launch work, feed input, cancel or
supply execution authority. Last-output consumption and frozen Finish creation
are atomic. Busy fingerprints permit bounded fresh-Poll/Finish retries, at most
four Finish attempts per invocation; replay reuses bytes and does not recount.
Finish is a peer consumption assertion, not proof that a person read or fsynced
output, and its response is not inferred from relay ACKs.

`--max-uptime` accepts a positive duration through 30 minutes. If max-commands is
set without an uptime, 30 minutes supplies the escape bound. The suspend-inclusive
monotonic budget begins before identity/config/network startup and is checked
inside admission/dispatch as well as the owner loop. Process restart does not
reset durable admission counters; qualification restarts also preserve the
original overall workload deadline. Revocation, missing Finish or offline native
obligations may force uptime exit rather than count exit. Neither exit cancels
workers or proves their absence. See [server limits](phases/v0.2-phase2-server-limits.md).

## Installation, updates and operational cleanup

The native bootstrap verifies a release-pinned manifest and all three product
binaries plus licenses before calling Rust install. It uses fixed asset names,
exact hashes/sizes/provenance and bounded HTTPS downloads; no archive extraction,
manifest shell evaluation, unpinned latest lookup or legacy-version fallback.
Native setup is offline and uses installed product entrypoints for identity,
role, trust and grant configuration. Installation does not initialize identities.

Versioned installations use owned receipts and checked filesystem transactions;
selecting a retained version reverifies it. Uninstall removes owned launchers
while preserving identity/config/database state and installed version contents.
Rollback selection does not imply older binaries can read newer state. Unsupported
formats refuse before profile opening; no compatibility marker is rewritten to
force acceptance. Runtime requires the installed sibling marmish-worker.

The release bootstrap is intended to be served byte-for-byte from Radicle/Iris
and GitHub, with GitHub the independent script backup. Both routes may still
need GitHub-hosted release assets. Availability of one mirror does not prove the
other; source replication, HTTPS delivery and installed behavior need separate
evidence. A native-bootstrap cutover and matching URL digests are required before
the default script can be described as installing v0.2. Six-case HTTPS installer validation passed separately in run 41667214179 attempt 1; it is not inferred from source publication.

Ordinary shutdown preserves jobs and durable native work. Hidden same-UID fixture
cleanup is a separate irreversible draining lifecycle: close admission, refresh
all ledger rows under an allocated fence, commit terminal/presence facts, consume
the actual engine into its cleanup guard, retain the live owner/profile lease and
original inode receipt through protected removal. Unknown, revoked or incomplete
proofs fail cleanup. No-engine clients use honest engine-absence/empty-server-ledger
and retained-directory proofs, never a fabricated engine guard or quiesced flag.
No caller-supplied PID or filesystem path authorizes process-tree deletion.

## Initial release and evidence status

The user authorized the initial native 0.2.0 release with a prominent macOS
full-testing-in-progress caveat; corrections are planned for v0.2.1. This
changes publication timing, not the truth of pending acceptance results.

Baseline candidate [41655210565](https://github.com/Epiphytic/marmish/actions/runs/41655210565),
attempt 1, used source `347cf799ae5baa9e577ded1a00566c7a6b99b933`.
Product verification, builds and ABI inspection passed on all three targets;
macOS ad-hoc signing passed. The macOS isolated installed-tool suite reported
426 passed and two failed local benchmark child-process fixtures. No actual
cause is proven, and no claim of a corrected fixture or full macOS success is
made. The uncommitted diagnostic work is excluded from initial release source.

Release source `73ed2b41b64ecf614d245668e3ab3b2e5b753895` changes only release-workflow
and initial-release documentation against baseline `347cf799ae5baa9e577ded1a00566c7a6b99b933`.
Runtime, build, dependency and scripts trees are unchanged. Asset-build run
41664164076 attempt 1 completed successfully. Keep actual manifests and build outcomes bound to their source. Do not
relabel baseline tests as new-source tests or claim identical executable bytes:
compiled metadata and build provenance differ.

Earlier evidence remains useful within its recorded scope: foundation acceptance
[hosted-41577006224](validation/v0.2/hosted-41577006224.md),
[public CLI regression](validation/v0.2/public-cli-admission-regression.md),
[internal direct recovery](validation/v0.2/local-command-recovery-direct-2026-11-03.md),
and the [lifecycle component review](reviews/v0.2-version-lifecycle-integrated-code-r3.md).
Component review is not hosted release qualification.

The following full qualification remains pending unless a new independently
bound result is explicitly recorded:

- Installed matrix: nine directed platform pairs and exact bytes/exits.
- Local installed cohorts: count/uptime exit, policy, recovery and new-session.
- Benchmark: cold plus three warm samples on each target, with real latency and
  goodput. No final-platform values or percentiles may be invented.
- Version lifecycle: actual A/B/A loading, live-worker preservation, C refusal,
  state/sidecar proofs and owned keychain lifecycle.
- Providers: actual default-auto Linux and macOS native-provider qualification.

HTTPS installer validation passed within its tested scope for both script sources across all three
targets in run 41667214179 attempt 1; the pending list above does not include it.

Actual release manifests bind source and immutable asset hashes. Record source
mirror and installer-byte verification as their own evidence; do not infer
installed qualification from publication.


Every qualification report binds its own source, run/attempt, target,
manifests/tooling and original deadlines. Partial results retain their failure
and cleanup facts. Synthetic lifecycle variants are explicitly synthetic.
Process exit or relay publication cannot substitute for exact-byte execution,
authenticated origin, engine admission, terminal validity and owned cleanup.

## Initial asset verification

Asset-build run [41664164076](https://github.com/Epiphytic/marmish/actions/runs/41664164076),
attempt 1, successfully built, inspected and packaged all three targets and
rendered the native bootstrap from this release source. All 15 product assets
were individually checked against their manifests. This caveated packaging run
skipped product test reruns, installed-test tooling, provider qualification and
quickstart jobs; its success is not full qualification. The exact rendered shell
also installed Linux AMD64 from the artifact asset directory: release metadata
matched source/version/formats `[2,6,1,1,1]`, and all four installed product files
matched their expected hashes and modes. Public release and both installer URL cutovers have been verified. The local installed native protocol check passed, as described below.

The installed Linux AMD64 direct-relay command check passed (exit 0), using the
installed production owner, engine and worker with disposable identities. Total
elapsed time was 19444 ms. Its running-command owner-restart/recovery case took
6295 ms and verified exact recovered output, execution count one, identical-submit
attachment and remote conflict refusal. Literal argv completed in 1590 ms;
16 KiB binary stdin/output in 2778 ms (5896 output bytes/s); stderr plus exit 7
in 1466 ms. Joined engine/MDK stops, engine quiescence, runtime cleanup and state
cleanup all passed, with no retained state. This is one local installed native
protocol check, not the public-CLI platform matrix, an OS restart test, or full
benchmark/provider/lifecycle qualification.

The [initial v0.2.0 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.0)
is public with all 15 product assets; server sizes and digests match the verified
manifests. GitHub and Radicle tags both identify the release source. Bootstrap-only
commit `7e3e74411bed8657e8b8f5b599b36c2b702fe5bd` changes only `scripts/install.sh`.
Both public installer URLs were independently fetched and matched the rendered
bootstrap SHA256 `46b4f369792f4f4ea36726c24414c5cca2ef977fec61712d821a1b81f985367a`.
The two-source/three-platform HTTPS run [41667214179](https://github.com/Epiphytic/marmish/actions/runs/41667214179), attempt 1, passed its resolver, all six install/setup cells and rollup. This validates both script sources on all three targets within that test scope; it does not establish full command-matrix, provider or version-lifecycle qualification.

A separate Linux AMD64 install downloaded the public release assets without an
asset-directory override and passed (exit 0), selecting active generation 2
with the same version, source and format tuple. See the
[initial release evidence](validation/v0.2/initial-release.md) for exact reports and boundaries.

The installed lifecycle checker derives synthetic A/C versions from the frozen
stable `0.2.N` candidate and matching native lock entry, rather than pinning
`0.2.0`. See the [patch-version amendment](phases/v0.2-lifecycle-patch-versions.md).
This changes qualification tooling only; hosted lifecycle qualification remains
pending, and public release pins are unchanged.

The canonical credential-helper correction (`marmish-far`) makes managed and ordinary sibling `marmish`, `marmishd` and worker entrypoints select validated same-version `marmish` for provider operations. Exact downloaded standalone CLI behavior is preserved. This fixes an inconsistent helper identity; reported macOS Keychain denial causation and full daemon startup remain pending actual pre/post-fix macOS qualification. No ACL or file-fallback policy changes. See [design](phases/v0.2-canonical-credential-helper.md).

### macOS provider supervisor correction (offline reviewed, native pending)

Candidate 41698628787 macOS stopped in offline supervisor tests; real Keychain qualification was skipped, so it supplies no provider or helper-custody verdict. The bounded correction canonicalizes lifecycle fixture paths and handles an initial owned-child group EPERM only by exact-child reap plus a fresh ESRCH proof; unknown or denied cleanup stays red. Linux provider23/HTTPS24 offline checks and independent [code review](reviews/v0.2.1-macos-provider-supervisor-code-r1.md) pass. Native macOS offline tests and actual disposable Keychain qualification remain pending. See [sanitized evidence](validation/v0.2.1/macos-provider-supervisor.md) and [approved design](phases/v0.2.1-macos-provider-supervisor.md).
