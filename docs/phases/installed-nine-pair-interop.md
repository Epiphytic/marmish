# Installed three-platform Marmot interoperability

Beads: `marmish-360`. Proposed design; no implementation or passing matrix is
claimed. This extends [native validation](platform-validation.md), preserving
its confinement, provenance, relay and cleanup requirements. Each increment
requires a detailed Claude design review and integrated code review before
publication. The current installer and configured macOS runtime blockers remain
independent; this design does not waive them.

## Acceptance and phases

The acceptance matrix has Linux AMD64, Linux ARM64 and macOS ARM64 on both
axes: nine distinct installed client-to-installed server pairs, including the
three same-platform pairs. All nine must pass in one run attempt. A partial
matrix, cancellation, missing evidence, startup denial or timeout fails overall
acceptance, even when the server deliberately exits zero at its lifetime limit.

| Increment | Deliverable | Exit criteria |
| --- | --- | --- |
| A Installation and rendezvous | Real install script execution on three platforms, installed identity initialization, strict three-peer public artifact schemas and workflow scheduling | Actual script invoked as a child; exact source/artifact provenance; no source frontend substitution; malformed, stale and duplicate rendezvous rejected; offline tests and Claude approval |
| B Bounded live execution | Three concurrent platform jobs, policy-bound fixture receipts, installed CLI results and bounded CI supervisors | Thirty-minute cap; exactly three distinct expected client executions asserted, without claiming admission control; delivery-safe completion and native failures reported |
| C Rollup and native evidence | Always-run strict reconciliation and readable Actions summary | All nine results match separate server-side receipts and exact output/exit expectations; cleanup proven; actual three-platform results, Claude review and dual-remote publication |

At the beginning of each increment, refine its detailed interfaces and tests in
this document using the actual installed application contracts. A design is not
an implementation claim. Do not gate this new live workflow on the unrelated
managed-runtime full-suite matrix; do require actual installation, identity and
confinement readiness for each participating platform. Existing offline/native
validation remains required independently. A run on experimental.2 validates
only that archive and its recorded installer, not master application code.
The existing macOS confined `native_probe_broker` abort is an expected first-run
failure on that release; Linux namespace policy may similarly prevent startup.
The workflow must report these honestly rather than requiring a new observation
interface before it can expose already-known runtime failures.

## Scheduling without deadlock

Add a separate master-only `workflow_dispatch` workflow with no inputs and
canonical repository/ref guards. Share `marmish-native-validation` concurrency
with `cancel-in-progress: false`, so it does not compete with native validation
for runner slots. No automatic push trigger in this increment. The guard is
workflow admission, not a security boundary against repository writers.

Reuse the configured `epiphytic-ci` group and labels from native validation:
`linux-x64` / `[self-hosted, Linux, X64, bel-ci]`, `linux-arm64` /
`[self-hosted, Linux, ARM64, laptop-ci]`, and `macos-arm64` /
`[self-hosted, macOS, ARM64, laptop-ci]`. Allow three platform jobs concurrently,
with `fail-fast: false`. Each job keeps its local server alive while its local
client connects to all three servers. Six simultaneous runner slots are not
required. The same-platform pair still uses separate identities, profiles and
processes, with communication through the explicit Marmot relays.

Identity and readiness exchange uses uploaded artifacts while jobs are running,
not job outputs or `needs` on those running jobs. All platform jobs depend only
on a short resolve job that pins installer source and one complete release
asset set before any installation begins. The rollup uses `if: always()` and
`needs` on the platform matrix after it ends. All report reads are bounded and
bound to this exact run, attempt, source SHA and release tuple.

Only trusted canonical-repository master code may run on persistent runners.
Keep existing repository/ref/event guards, pinned actions, read-only permissions
and checkout credential isolation. Dispatch is master-only with no arbitrary
executable/URL inputs. Do not inherit API tokens
into installers, MDK, installed launchers, fixtures or supervisors. Only the
rendezvous adapter needs Actions read access. No host policy changes or sudo.

Report jobs not assigned within five minutes to the user; this is a
notification threshold, not a GitHub assignment SLA or a running process that
can observe itself before scheduling. The external workflow driver records that
notification. With the current default concurrency queue, a pending run can be superseded by
a newer run entering the group. That is cancelled, never green; re-dispatch it.
Only the running run is protected by `cancel-in-progress: false`, per
[GitHub's concurrency contract](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency).
Use a 75-minute job timeout, 15 minutes for install/preparation, ten minutes
for all installed/identity-ready records, a 30-minute hard server window
including drain, and eight minutes reserved for cleanup/report. Start a server
only after all three installed/identity-ready records exist. Slow startup still
consumes its own server window. No server waits indefinitely for a late runner.
Keep `caffeinate` on macOS and existing suspend/drift detection on both
`laptop-ci` jobs; do not assume their common label proves they share a host or
can run concurrently. Concurrent capacity must be established by actual runner
assignment; missing capacity fails the barrier, not confinement.

## Real installation is part of the test

Installer bytes are `scripts/install.sh` at the run's `GITHUB_SHA`. The resolve
job records that blob's SHA256; each platform checks those exact bytes and invokes
`sh install.sh --debug --version VERSION --prefix PRIVATE_PREFIX` as a real
process. Use a fresh owned 0700 root and
private scratch HOME, excluding actual user installations and credentials.
Download actual public release assets through the installer's production
network/verification path. Do not import its embedded Python, call
`install_archive` directly, preinstall the application or mock downloads for the
acceptance run. Such fixtures remain useful unit tests only.

For the initial release lane, freeze one explicitly named published experimental
release, its three target assets/checksums, application source SHA, installer
source SHA and interpreter selection. The resolve job separately downloads all
three archives and checksum manifest,
verifies the archive checksums and records their embedded `release.json` bytes.
The script independently downloads/verifies its assets; its temporary archive
is deleted, so do not claim direct measurement of that deleted download. After
installation, compare installed `release.json` byte-for-byte with the resolved
manifest, hash every payload file against it, and verify installer ownership
metadata and launcher bytes. This binds the installed payload rather than
pretending the current script exports an archive digest. Record interpreter
version and a sanitized path classification; do not publish runner-home paths. Distinguish installer source from
application source: current master against an older archive does not validate
new application code. A moving `latest` is not permitted mid-matrix.

A future prepublication candidate lane may serve authenticated same-run exact
assets through a separately reviewed installer source interface. The current
script accepts release versions, not arbitrary local candidate archives; do not
pretend a helper-level archive install exercises the script's download path.
Publishing a new release is a separate authorized action, not an implicit test
side effect. Test default Iris resolution in a separate bounded check against a
pinned revision; an unpinned `head` download must not replace reviewed script
bytes in the nine-pair run.

After installation, invoke absolute `PRIVATE_PREFIX/bin/marmish` and
`PRIVATE_PREFIX/bin/marmishd` for identity creation and every live operation.
Use their bundled defaults, with no source-tree imports, injected application
PYTHONPATH or helper overrides except the existing authenticated Linux Bubblewrap
selection required by confinement. Prove the selected launcher/installed manifest
and archive provenance; `--doctor` is diagnostic, not runtime acceptance.
The current installer still requires Python; a uv-managed installer is future
work and must satisfy the same contract once implemented.

Installation, identity setup and authorization must be real and noninteractive
in this lane. Keep the separate PTY quickstart tests; they do not prove this live
matrix. Disposable file-backed identities can validate transport independently
of GUI keychain availability. Label that storage mode explicitly; retain native
keychain acceptance separately, and never claim the file-mode lane validates
default keychain onboarding. The intended managed quickstart and Both-mode setup
remain separate increments.

## Public identity and readiness exchange

Prepare fresh independent server and client keys on each platform. Upload only
strict versioned public records: run/attempt/source/release digests, platform,
server and client public identities, explicit relay set, random run nonce and
expiry. Keep secrets, MLS/SQLCipher state, authorization tokens and journals local.
Use the existing bounded authenticated artifact reader and redirect policy,
extending its two-role schema deliberately rather than overloading linux/macos.
The phase-A schema must explicitly replace the old single `/f/main` fixture
path with the three canonical role-specific fixture paths and digests.
Reject duplicate artifacts, wrong attempt/source/platform, bad fields and expiry.

Each server waits for all three public client identities and authorizes exactly
those keys, with a narrow fixture executable policy and no shell command mode.
Each platform then starts its installed server and advertises actual readiness
only after the daemon's native/confinement startup succeeds. Bound server
readiness to five minutes after the launch barrier; missing readiness fails the
run and leaves the remaining server window for delivery and drain. Clients wait for
all three actual-readiness records. An npub existing alone is not readiness.

Use an explicit small relay set from reviewed CI configuration (the existing
explicit EU/US White Noise test URLs may be retained). No public relay defaults
are added to the application, and relay publication alone is never success.

## Fixture provenance and policy-derived server evidence

Build the native test fixture from the harness source SHA during preparation,
using the existing pinned native build tooling. It is test code, separate from
the installed application. Stage three fixed-role variants in owned 0700
`/tmp/mci-.../f/<client-platform>/main` paths (canonical `/private/tmp` on macOS),
outside PRIVATE_PREFIX; advertise each SHA256 and harness source separately.
Commands run as the server user outside the MDK sandbox, so fixture execution
requires command-policy permission, not expanded MDK filesystem/network access.

Use defaults `allow_exec` containing exactly these three executable paths, then
a single per-key `allow_exec` containing only that principal's fixture path.
Allow restrictions are additive intersections: default `allow_exec: []` or a
default deny-all would deny every client and cannot be overridden. Unknown keys
remain outside the server allowlist. Disable shell mode. The trusted fixture
has a compile-time role and fixed pair-specific receipt location; client argv
cannot choose its role, output path or executable. It writes a small O_EXCL
receipt containing the fixed role, challenge, invocation label, argv/stdin
digests and bounded outcome. It cannot spawn commands or write arbitrary paths.
The role-to-principal mapping is established by the enforced server policy,
not by the client-provided label. Validate policy cross-execution denial with
side-effect-free checks before counting live executions; such rejected attempts
do not count toward the three. Preserve unauthorized-principal coverage.

The supervisor reads the server's existing local MDK RPC socket using the
same ownership/native-peer checks as `running_rpc` in `marmish_interop.py`.
Observe verified group membership and authenticated incoming request/outgoing
terminal-response rows; decode with current protocol validation, binding actual
peer, group, invocation and terminal outcome to the fixture receipt. Request
labels alone do not prove invocation identity. Never read encrypted databases
directly or expose MDK RPC credentials to fixtures. Local observation has the
server user's authority and is not an independent external attestation.
Increment B must verify actual history row fields/terminal responses and test
wrong-principal, fabricated-label and duplicate-receipt refusal before code
approval. If this existing interface cannot prove those bindings, stop and
review a production diagnostic/candidate-release increment; do not manufacture
an authenticated result or silently patch the installed archive.

## Bounded commands, receipts and shutdown

Prefer a CI-owned bounded supervisor first, leaving public server CLI semantics
unchanged. The user permits either this or CLI flags. If exposed later,
`marmishd --max-commands 3 --max-uptime 30m` must have a separately reviewed
admission/draining contract. Time uses the existing suspend-inclusive clock;
the hard lifetime begins at server launch and includes startup. A normal
count/uptime limit may return zero, but its fixed-schema reason and counts must
be recorded; the rollup decides whether the test passed.

Each client sends one fixed bounded fixture command per server. Include a fresh
pair-specific challenge, the fixed platform labels and a literal byte edge-case
payload in its argv/stdin. Validate exact stdout, stderr and terminal exit status
at the client, including spaces, quotes, dollar signs, backticks, newline,
Unicode, empty arguments and binary stdin where supported by the installed CLI.
Build argv arrays without shell interpolation. Use bounded fixtures that exit
within seconds and cannot run arbitrary client-selected programs.

On the server, count only unique authenticated invocations with verified
terminal evidence and fixture receipts. Reconnects, denied requests, frames,
retries and duplicate IDs do not increment the count. Require exactly the three
expected client principals, not merely a total of three commands. Keep private
state and unbounded command logs out of public artifacts.

The published-release baseline uses supervisor lifetime and exact-count
assertions, not daemon admission control: today's installed server has no
`--max-commands` flag. Clients issue exactly their one test command to each
server; any fourth execution is a failure. The supervisor stops the frontend
when all three terminal client-receipt artifacts arrive or the 30-minute hard
cap expires. A normal SIGTERM shutdown returns zero; the rollup still fails
missing/failed/timed-out pairs. Do not claim this prevents arbitrary extra
admissions. Production admission/drain flags are a future separately reviewed
runtime change requiring a published matching archive or candidate lane.

A single client profile owns one transport. Run its three commands serially in
fixed platform order, using `--timeout 300` and a bounded external process
watchdog. The total command budget is at most 18 minutes including startup/
termination margins; reserve the remainder for startup and artifact delivery.
Every client always publishes one bounded terminal artifact covering all three
pairs: received, failed category or timed out. Servers stop after all three
artifacts, including failure artifacts, rather than idling until the cap.

Keep result/status delivery available while clients retrieve terminal output.
Client receipt publication occurs only after actual CLI byte/status validation;
server success requires all three received results match its local evidence.
No fourth acknowledgment command is needed. Artifacts coordinate shutdown only;
actual results must have arrived over Marmot before receipt publication. A
missing artifact at the hard cap fails acceptance. Publish receipts before
waiting for the local server to stop, preventing a cyclic shutdown wait.

Generic daemon exit must continue to preserve detached commands. CI commands
are tightly bounded; at timeout the supervisor requests authenticated
cancellation where available and verifies worker termination before deleting
state. Never delete uncertain live engine state or indiscriminately kill users'
processes. Cancellation/SIGKILL can prevent cleanup and reporting; missing proof
is failure, with owned uncertain state retained under the existing policy.

## Rollup and validation

Require exactly three distinct platform installation records, three server
reports and three client reports. Cross-check the exact expected set of nine
ordered pairs and all identity, nonce, invocation, source, asset and result
bindings. Reject duplicate/missing pairs even if counts total nine. Require
client byte/status checks, server authenticated completion and bounded fixture
receipt agreement, successful cleanup, and no unexpected extra execution.
Show a 3-by-3 Actions summary with success/failure/missing per cell and concise
installation/startup/timeout categories. Preserve partial sanitized evidence on
failure; never convert cancellation or timeout into a green matrix.

Offline tests exercise schema/type/size bounds, duplicate/stale artifacts,
two-role compatibility, late runner, failed install/confinement, retry and
counting races, receipt spoofing/wrong principal, final-output delivery delay,
deadline/suspend/cancellation behavior, cyclic-wait prevention, and exact-nine
rollup rejection. Real native tests execute the actual script and installed
frontends on each architecture. Single-host simulations and mocked installers
do not satisfy the nine-pair acceptance criterion.

The installer uses unauthenticated GitHub release downloads; shared egress can
hit rate limits. Keep bounded retries and sanitized debug HTTP categories, never
inject Actions tokens into the installer. Live frontends run without `--debug`
when stderr is compared byte-for-byte. Rollup fetches exact-named artifacts;
rendezvous must reject truncated artifact lists or paginate within fixed bounds.
After proven process/worker cleanup, run actual installer uninstall and remove
the owned private root: uninstall intentionally retains version directories.
