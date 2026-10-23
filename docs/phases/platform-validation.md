# Native platform and Marmot interoperability validation

Beads: `marmish-b33.3`, increment C. This is a design, not a claim of passing
native or distributed tests. It depends on [increment B](macos-platform.md) and
preserves increment A's additive command restrictions. Claude design approval
precedes implementation; integrated code and actual results need another review.

## Trusted workflow and scheduling

Trigger only on pushes to `master`, with no manual dispatch, pull-request
trigger or inputs. Add a job-level `refs/heads/master` guard. Ignore docs-only
changes (`docs/**`, `README.md`, `AGENTS.md`, `.beads/**`) so publishing evidence does not
repeat public-relay tests. The user explicitly authorized these supplied
runners, native CI testing and phase pushes; authorization persists. Repository
master-write authority therefore grants trusted code execution on these
persistent runners. Do not assume branch protection or verified runner-group
workflow restrictions, and do not change organization/branch policy.

Set `contents: read`; add `actions: read` only to live jobs needing public
artifact downloads. No write, OIDC or administration permissions. Pin actions
to reviewed commit IDs and checkout with `persist-credentials: false`. Provide
`GITHUB_TOKEN` only to public-artifact API steps; do not export it to build
tools, coordinators, MDK or fixtures. Action runtime credentials stay within
the upload/download actions. Never dump environments or token-bearing URLs.

Use one repository-wide concurrency group with `cancel-in-progress: false`.
GitHub retains one pending run; newer pending pushes can supersede older ones.
A skipped/superseded run supplies no acceptance evidence.

Run native offline checks first, using a `max-parallel: 1` matrix:

* `[self-hosted, Linux, X64, bel-ci]`.
* `[self-hosted, Linux, ARM64, laptop-ci]`.
* `[self-hosted, macOS, ARM64, laptop-ci]`.

After all three pass, start independent macOS ARM64 and Bel Linux X64 live jobs
concurrently. Each owns a server and a client. Do not schedule Linux ARM64 work
during that pair: both laptop platforms share one FIFO slot. Do not require a
second simultaneous Bel job. Labels and runner group `epiphytic-ci` are verified
in the [three-platform pilot][pilot]; current eligibility/availability still
needs actual jobs, since this credential cannot list organization runners.

Cold native build jobs have a 90-minute timeout. Live jobs consume verified
native build artifacts from this same run/attempt and have a 90-minute job
timeout. After the runner start hook releases execution, enforce a 70-minute
continuous-clock overall budget and these stage ceilings: preparation 300
seconds, full native E2E 900 seconds, pairing 1,200 seconds, distributed tests
1,200 seconds and cleanup 120 seconds. Pairing includes the documented
900-second laptop gate delay plus 300 seconds of startup/network jitter. Every
stage is also constrained by remaining overall time; retain the cleanup budget.
Peer absence before readiness is a runner/pairing prerequisite failure, not a
product protocol failure. These C budgets and artifact names are normative;
increment B references them rather than maintaining conflicting values.

## Native prerequisites and build provenance

Require Python 3.11+, Rust 1.97.1, native C/C++ tools, make and Perl for the same
vendored OpenSSL/SQLCipher configuration on all platforms. macOS requires Xcode
and increment B's deployment-target flags. Record actual tool versions; pilot
versions do not establish current versions or macOS Python compatibility.

Assume no general sudo or Homebrew ownership. Prefer verified preinstalled
tools. Missing Rust/Python may be installed under a job-owned directory using
digest-pinned bootstrap artifacts and controlled download origins. Verify the
exact Rust release through a pinned manifest or standalone archive digest;
pinning rustup alone does not pin toolchain downloads. Do not change services,
system packages, sysctls or sandbox policy. Missing privileged prerequisites
fail explicitly. Actually exercise Linux nested user/network/PID namespaces
and bubblewrap; working rootless Docker alone is insufficient evidence.

Linux builds clean pinned **unpatched** MDK with a verified source/build manifest
and executable digest, retaining its namespace/fixed-origin broker boundary.
macOS requires increment B's maintained transport patch, build manifest,
executable digest and mandatory configuration/nonce acknowledgement. Never
require Mac SOCKS configuration or acknowledgement from the Linux daemon. Both
speak the same Marmish protocol; moving Linux to the patched broker is outside
scope. Build from clean checkouts, never a sibling working tree.

Use `actions/cache` only for nonsecret build dependencies/artifacts, keyed by
OS, architecture, Rust, Cargo.lock and MDK source/patch/build-input digests.
Never cache runtime state. Native jobs publish executables and provenance
manifests as same-run/attempt build artifacts; live jobs download and revalidate
hashes before execution, without rebuilding MDK. A cache miss may do a locked
dependency fetch before offline checks, never substitute an unpinned binary.

Every native job runs Python checks, Rust tests/clippy/formatting, native engine,
storage/process and credential tests, and actual confinement probes from B.
Unsupported cases are explicit. macOS AMD64 remains unvalidated without a
native runner. Each live job also runs the full native single-host E2E before
pairing, including server restart and detached-worker recovery.

## Two-artifact public rendezvous

Create separate local disposable server/client profiles for distributed tests,
with fresh generated keys and private 0700/0600 state. Keep canonical paths short
under a private `/tmp` root resolved to `/private/tmp` on macOS; check the native
Unix socket limit, including full filenames. No private state leaves its host.

Publish `interop-<run>-<attempt>-<role>-ready` after local profiles and fixtures
exist. Here ready means ready to pair, not server readiness: the server's
allowlist needs the peer client npub. Strict versioned public JSON contains
run, attempt, commit SHA, role, fresh public nonce, server/client npubs, exact
relay URLs, fixture digest/short canonical path, platform and expiry metadata.
Public repository logs/artifacts are world-readable; avoid runner-home paths.
Never publish keys, databases, MLS state, journals or launch acknowledgements.

Poll the authenticated Actions API for the peer's exact artifact in the same
run/attempt. Do not use job outputs, which resolve after job exit and would
deadlock servers. Bound polling/backoff with the stage's continuous deadline.
Reject duplicate, expired, wrong-role or malformed artifacts and mismatches in
run/attempt/SHA/protocol/relay selection. Validate keys, nonce and fixture path.
Peer wall time is diagnostic and cannot extend local deadlines. Downloads are
at most 64 KiB compressed, with exactly one expected JSON entry of at most
8 KiB uncompressed, no ZIP64 and bounded reads; do not perform general extraction.
Verify the API artifact digest when provided. API authorization/schema failures
fail explicitly without logging token-bearing redirects.

Start the local server admitting only the peer client, wait for actual
authenticated readiness, then publish
`interop-<run>-<attempt>-<role>-paired`. This record binds both roles, both ready
nonces, both identity pairs, SHA and run/attempt, plus actual local readiness.
Wait for and validate the peer's paired record before any test commands. The
nonce pair prevents accidentally mixing stale/crossed records; it is public
correlation data, not authentication replacing Marmot. Both artifact barriers
share the one 1,200-second pairing budget. Keep server ownership across the
upload/download steps through a private local coordinator/control channel,
with explicit failure and always-run cleanup; never detached unowned processes.

Acceptance requires every job passing in one complete attempt for the reviewed
SHA. Partial failed-job reruns are not evidence: retry the whole workflow so
both peers and all gates have fresh same-attempt records. Records from prior
attempts cannot satisfy either barrier.

## Native fixtures and actual execution

Build a small repository-owned native fixture binary for each OS, hash-verify
it, and install copies in a prebuilt canonical directory tree within the short
private root. Its fixed subcommands provide argv-byte encoding, binary stdin/
stdout/stderr, bounded exit/sleep, exclusively created markers and a fixed
execution counter. No shell, general interpreter, arbitrary file path or
arbitrary program execution subcommand. Arguments still carry each explicitly
implemented capability; fixture allowlisting is not a sandbox claim. Do not
create filenames from non-UTF-8 bytes, which APFS may reject. Remote fixture
metadata chooses only validated fixture paths, never shell text or input code.

Use explicitly selected `wss://relay.eu.whitenoise.chat/` and
`wss://relay.us.whitenoise.chat/`. No SRV-dependent selection, default relay or
fallback is introduced. Retain dedicated Marmish rooms, fresh independent MLS
groups and existing authenticated sender/room/replay checks.

Both Mac-client/Linux-server and Linux-client/Mac-server must establish:

* Actual CLI execution with exact stdout, stderr, binary stdin/EOF, NUL output,
  exit status, empty arguments, spaces, quotes, dollar/command-substitution
  literals, backslashes, newlines, Unicode and non-UTF-8 Unix argument bytes.
* Additive defaults/per-key allow layers, cumulative repeated-key entries and
  deny precedence, with no per-key relaxation. Use the prebuilt fixture tree
  for slash-sensitive `*` versus `**`; check canonical-path/shell rejection and
  no denial side effects. An unlisted client npub never executes a command.
* Duplicate execution once, conflicting submission rejection, status-first
  retry, continuing detached commands and exact result retrieval after reopening
  the client profile. Native single-host runs also cover server/frontend restart,
  previous-boot replay, cancellation and both-frontends-stopped completion.
* Fresh groups, saved MLS resumption, same-principal ownership across groups,
  decoy-relay rejection and the native confinement negative probes.

Use bounded fixed fixtures and runtimes. A fixed `relay_unavailable` category
remains a failure, not a flaky pass. Publication success or mocks never count
as command execution. Inspect failures before any rerun; retain their evidence.

After its client assertions pass, the coordinator submits the marker fixture
to the peer. Its exclusive marker content binds run/attempt/SHA, direction and
both nonces, in a fresh private per-run cwd. Validate the complete content,
not existence alone. Each server remains available until own tests pass and
the peer marker validates, followed by a 30-second service grace period within
the execution deadline so the peer can retrieve its final marker result.
Missing, conflicting or malformed completion markers fail the run.

## Suspend, ownership and cleanup

Use B's continuous local clock for deadlines. Hold an owned
`caffeinate -i -s` on macOS during the finite live window, and verify its actual
availability/effect in the native spike. Continuous/runtime-monotonic divergence
over five seconds fails as suspended. Tart host suspension can freeze guest
clocks together: also record wall/continuous divergence diagnostically, never
as a deadline extension or proof against suspension. Actual bidirectional
completion and the enclosing Actions timeout remain required.

Preserve validated host-only `EPIPHYTIC_EXECUTION_ID` through supervisors,
helpers and detached workers. Never source it from remote messages. Commands
retain their fixed environment; explicitly account for/reap their owned groups.
Ordinary disconnect must leave accepted commands running. Only final cleanup
cancels remaining owned invocations.

Coordinators handle exceptions/signals; workflows have `always()` cleanup.
Release bounded waiters, cancel owned jobs, verify worker/command termination,
then stop owned MDK/brokers and remove only private test state. Use native run
manifests and boot/PID/start/executable identities for interrupted recovery;
never broad process kills, stale PID signals or deleting live-worker state.
Do not disable runner descendant tracking. Unverified cleanup fails the job
with sanitized diagnostics; never claim success or erase recovery evidence.

## Publication and acceptance evidence

First obtain integrated Claude auto-mode code approval and local checks, then
push the reviewed implementation commit to `master` to run CI. Verify the run's
`head_sha` equals that reviewed SHA. Review actual results, fix substantive
findings with affected gates/review and a new implementation push if required.
After complete same-attempt success, publish sanitized validation documentation
in a separate docs-only evidence commit; its paths do not start another live run.

Upload only bounded public rendezvous records and sanitized result JSON with
short retention. Results identify run/attempt/SHA, direction, platform/toolchain,
MDK provenance/digests, relay set, assertion names, elapsed time and cleanup
outcome. Omit arbitrary stderr/output, credentials, private paths and environment.
Print a compact Actions summary and retain reviewed results/run links under
`docs/validation/`. All native jobs, full local E2E on both OSes, both distributed
directions, negative cases and verified cleanup must pass for the reviewed SHA
in one attempt. Update README/spec with actual support and limits, audit staged
files and `.gitignore`, commit/push evidence and record it in Beads before close.

[pilot]: https://github.com/Epiphytic/.github/actions/runs/38104521926

## Review refinements and bootstrap provenance

The code review requires the distributed completion marker **after** every
client assertion, including the actual CLI process. Each ready record also
carries a distinct generated public `intruder` identity. Each supervised server
uses authenticated local MDK RPC to observe the peer intruder's dedicated
Marmish invitation, exact two-member roster and pending/unaccepted state.
A client timeout alone supplies no allowlist acceptance evidence. Both local
server observation and peer completion are required for overall success.

Python requires 3.11+ **and** `tarfile.data_filter`; older 3.11 patch releases
without the backport fail the capability check. On macOS ARM64 the job installs
a user-local standalone CPython 3.13.17. The author verified these pins online
on 2026-10-22 using the publisher's release API, SHA256SUMS and independently
downloaded archive. The three sources agreed on
`9c985cbf327425873469896ce748c7134b6deca8edb81e93c589ad2fc5aff445`:

* [Publisher release 20261021](https://github.com/astral-sh/python-build-standalone/releases/tag/20261021).
* [Publisher SHA256SUMS](https://github.com/astral-sh/python-build-standalone/releases/download/20261021/SHA256SUMS).
* [The pinned macOS ARM64 install-only archive](https://github.com/astral-sh/python-build-standalone/releases/download/20261021/cpython-3.13.17%2B20261021-aarch64-apple-darwin-install_only.tar.gz).

The downloaded Rust 1.97.1 channel manifest was independently hashed that day:
`03569b1886ceb5c05276b50c8431ab111de944cd6140fe1fa7d821dd8e0f29cf`.
The manifest's target-specific archive digests pin native payloads. Bootstrap
installs into an owned staging directory, verifies required components and a
complete file-hash inventory, then publishes the prefix. Incomplete or changed
managed cache prefixes are reinstalled; arbitrary preexisting directories are
never removed. Cache keys include bootstrap sources and rust-toolchain.toml.

Build bundle metadata binds run, attempt, SHA, platform, architecture and tool
versions. Artifact names plus GitHub's same-run trusted artifact store establish
provenance; the enclosed digest inventory detects corruption, not malicious
substitution of an entire consistently forged trusted artifact. Runtime/private
state is never cached. `EPIPHYTIC_EXECUTION_ID` and `RUNNER_TRACKING_ID` remain
in owned supervisors; the supplied runner uses the former for host cleanup even
when a daemon boundary intentionally clears other environment variables.
