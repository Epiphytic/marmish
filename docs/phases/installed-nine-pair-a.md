# Installed interoperability: phase A detailed design

Beads: `marmish-bxr`. Independent Claude design approval is recorded in
[the final design review](../reviews/installed-nine-pair-a-design-final.json).
The design alone does not establish successful native installation. The approved parent design is
[installed-nine-pair-interop.md](installed-nine-pair-interop.md). Phase A supplies
the installation, private-state and three-peer rendezvous interfaces consumed
by phases B and C. Production CLI behavior, confinement and published archives
remain unchanged. Root coordinates the independent Claude design review before
implementation and the integrated Claude code review before publication.

## Deliverable and boundaries

Run the actual checked-out installer as a subprocess on Linux AMD64, Linux ARM64
and macOS ARM64; verify the installed bytes against one resolved release; create
two independent disposable identities through the installed launcher; publish
strict public identity records; and require all three identity records before
starting any live server. Native initialization failures fail this prerequisite.
Offline fixtures exercise this path without claiming native success. Phase B
adds the server, fixed-role command fixtures and client operations; phase C adds
the nine-pair acceptance rollup and published performance results.

New files have disjoint implementation ownership:

| Module | Responsibility |
| --- | --- |
| `tools/marmish_installed_release.py` | Resolve release assets and raw manifests; verify installed ownership and payload |
| `tools/marmish_installed_state.py` | Owned private state, scrubbed child environment, bounded processes, clocks and installed identity initialization |
| `tools/marmish_installed_rendezvous.py` | Strict three-role public schemas, artifact naming and bounded Actions reads |
| `tools/marmish_installed_ci.py` | Thin phase A CLI orchestration, installation report and lifecycle integration |
| `.github/workflows/installed-interop.yml` | Resolve and three concurrent platform jobs; extend with phases B/C rather than create another overlapping live workflow |
| matching `tools/test_marmish_installed_*.py` | Focused offline tests for each module |

Keep the existing two-role `marmish_rendezvous`/`marmish_interop` interfaces intact.
Do not turn their `linux` role into an ambiguous architecture. New role constants
are exactly `linux-x64`, `linux-arm64`, `macos-arm64`, mapping respectively to
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`aarch64-apple-darwin`. Explicit relays remain the approved EU and US Whitenoise
URLs, in that order.

## Existing contracts and reuse

The checked-out `scripts/install.sh` is a shell wrapper around embedded Python.
It accepts `--debug --version VERSION --prefix PREFIX`; explicit prefixes put
launchers in `PREFIX/bin`. It downloads the tagged experimental release,
`SHA256SUMS` and the native archive. Its download limits are 512 MiB compressed,
1536 MiB expanded, 4096 archive members and 2 MiB per manifest. Its owned
installation is `PREFIX/versions/marmish-VERSION-TARGET`, with `PREFIX/current`,
`PREFIX/.marmish-install.json` and exact generated launcher bytes. The ownership
marker has fields `format`, `version`, `target`, `launcher_dir`.

`marmish_release_acceptance.installer()` exposes the embedded validators without
running its entrypoint. Reuse `release_metadata`, `release_assets`, `download`,
`verify_checksum`, `inflate`, `extract_ustar`, `validate_manifest`,
`read_installation`, `launcher_bytes` and `digest_file` where applicable.
These helpers may download/parse release inputs and **verify** an installation.
Never call `install_archive`, `install_locked` or `publish_installation` as the
acceptance installation. Existing `marmish_publish.verify_archive` demonstrates
strict archive parsing; avoid a new permissive `tarfile.extractall` path.
Cross-target resolution uses `marmish_installed.validate_record` for schema-only
validation and the `verify_archive` pattern for payload modes/digests after
strict `inflate`/`extract_ustar`. Do not call the embedded `validate_manifest`
while resolving foreign targets: it invokes `validate_prerequisites`, which
loads host libraries and checks host glibc. Full prerequisite validation is
appropriate only for matching-platform installed verification/actual execution.

The runtime manifest has exactly `format`, `version`, `target`, `source_commit`,
`files`, `prerequisites`, `experimental`, `limitations`; each file record is
`{sha256, mode}`. `files` excludes `release.json`. Source SHA is a separate
application-release SHA, not the harness checkout SHA.

`marmish --init -i PROFILE --key-storage files --relay URL ...` prints exactly
one JSON object with `account` as a 64-character lowercase hexadecimal public
key. The default native dependency paths come from the installed `release.json`.
Do not pass `--wnd`, `--bridge`, `--worker`, `PYTHONPATH`, source launchers or
replacement runtime files. The existing authenticated Linux Bubblewrap
selection is the sole allowed confinement-tool override.

Reuse `marmish_platform.continuous_time()` for suspend-inclusive deadlines and
`process_identity(pid)` for local UID/boot/process-creation identity. Reuse
`marmish_rendezvous.read_zip` unchanged for small public artifact validation;
HTTP exchanges use the new adapter-only `request_bounded` worker and retain the
existing redirect/token policy. Its existing HTTP helpers, two-role validator,
unpaged listing and expiry checks must not be reused unchanged.

## Release resolution and immutable pin

The resolve job has no API token in its Python process. Freeze the initial
application version as the explicit checked-in constant
`0.1.0-experimental.2`; there is no workflow input or moving `latest`. The
checked-out installer SHA256 must be computed from exact bytes before loading
the verification helper. The release pin is public, produced once per run
attempt and consumed by all three platform jobs.
Resolve runs on GitHub-hosted `ubuntu-24.04`; it performs host-independent
archive inspection, not runtime acceptance of foreign targets. An offline test
explicitly resolves a Darwin fixture while running on Linux.

`marmish_installed_release` exposes:

```python
resolve_release(repo: Path, output: Path, ctx: dict, *, version: str) -> dict
validate_pin(record: dict, manifests: dict[str, bytes], ctx: dict) -> dict
load_pin(directory: Path, ctx: dict) -> dict
verify_installed(prefix: Path, pin: dict, manifest_raw: bytes,
                 role: str) -> dict
```

Only the resolve CLI supplies `version`, from the checked-in constant. Functions
take explicit dependencies/paths to allow offline tests; this is not a public
arbitrary-URL installation interface. `resolve_release` downloads the release
metadata once, requires the exact tag and all three unique uploaded archive
assets plus one unique `SHA256SUMS`, and downloads/validates each archive. It
uses strict extraction in an owned temporary directory, one target at a time,
checks its payload and keeps the raw `release.json` bytes. All three manifests
must have the same application `source_commit`, version and experimental flag;
targets must match their role. Fail on missing or duplicate assets, invalid
checksum lines, mismatched source SHAs or malformed archives. No archive is
published as an Actions artifact. Bound the entire resolve step to 15 minutes;
use existing per-download byte/time limits and at most two retries for transient
network/429/5xx errors within the remaining deadline. No retries for validation
errors. Rate-limit failure is a recorded failure, not a credential fallback.
Classify transient resolve failures from `InstallError.__cause__` HTTP status
(429/5xx) or network timeout/URLError, never message text. Each retry gets a
fresh download directory because the installer downloader uses exclusive file
creation. The installer subprocess itself gets one attempt in this increment;
its bounded failure diagnostics explain rate limiting rather than retry a
possibly partially owned prefix. A future installer retry policy needs an
explicit pristine-prefix check.

The resolve artifact contains `record.json` and exactly three raw manifest
sidecars named `<role>.release.json`. No installer copy is necessary: every
platform has the identical guarded checkout. Bound `record.json` to 32 KiB and
each sidecar to the installer's 2 MiB limit. A completed resolve artifact can use
the pinned `download-artifact` action; it does not use the 64 KiB rendezvous ZIP
reader. Reject extra files/symlinks when loading it. Pin JSON uses canonical
encoding: UTF-8, sorted keys, compact separators, no NaN, one terminal newline.
`pin_sha256` is the hash of the exact canonical `record.json` bytes.

The pin has exactly this shape (`HEX40`/`HEX64` denote lowercase hex strings):

```json
{
  "format": 1,
  "kind": "release-pin",
  "repository": "Epiphytic/marmish",
  "run": "DIGITS", "attempt": "DIGITS", "sha": "HEX40",
  "installer_sha256": "HEX64",
  "version": "0.1.0-experimental.2",
  "tag": "v0.1.0-experimental.2",
  "source_commit": "HEX40",
  "checksums_sha256": "HEX64",
  "relays": ["wss://relay.eu.whitenoise.chat/", "wss://relay.us.whitenoise.chat/"],
  "targets": {
    "linux-x64": {
      "target": "x86_64-unknown-linux-gnu",
      "archive_name": "marmish-VERSION-TARGET.tar.gz",
      "archive_sha256": "HEX64",
      "manifest_name": "linux-x64.release.json",
      "manifest_sha256": "HEX64"
    },
    "linux-arm64": "same exact field set with this role's constants",
    "macos-arm64": "same exact field set with this role's constants"
  }
}
```

The two textual placeholders in this example are explanatory, never valid
schema values. No URLs from artifacts are used to select executable inputs;
asset names are derived from fixed version/target constants and production
installer host validation applies to release metadata URLs.

`verify_installed` performs no writes. Require owned regular files and safe
ancestors, check the marker/current link and both launcher bytes using existing
ownership helpers, compare installed `release.json` byte for byte with the
sidecar, and validate every payload file digest/mode plus the exact payload set.
Accept exactly this tree, using owned-UID `lstat` type/mode checks throughout:

```text
ROOT/install/                                      directory 0700
  .marmish-install.json                            regular 0600, nlink=1
  .marmish-install.lock                            regular 0600, nlink=1, empty
  current -> versions/marmish-VERSION-TARGET        sole intentional symlink
  bin/                                            directory 0700
    marmish                                       regular 0755, exact launcher_bytes
    marmishd                                      regular 0755, exact launcher_bytes
  versions/                                       directory 0700
    marmish-VERSION-TARGET/                        directory 0755
      release.json                                regular 0644, raw sidecar bytes
      [exact payload and implied directories]      file manifest modes; dirs 0755
```

All regular files have one link. The marker has exactly
`{format: 1, version, target, launcher_dir: str(PREFIX/'bin')}`; the owned selector
link has exactly the relative target shown. Exactly one version directory and
exactly the directories implied by payload files are allowed. No other prefix,
bin or version entries are allowed, including leftover `.marmish-stage-*`,
`.marmish-current-*` and `.marmish-write-*` entries. The bin directory is 0700
because `owned_directory` first creates it under the fresh explicit prefix.
Reject every other symlink and altered launcher. Test each layout rule against
a real-helper-installed offline fixture. The exact installed
root and launcher paths are returned privately; the public installation tuple
contains only `target`, `version`, `source_commit`, `archive_sha256`,
`manifest_sha256`, `installer_sha256`. The archive digest is the resolve job's
verified archive digest; do not claim to hash the installer's deleted download.
The independently downloaded installed bytes are proven against the pin by raw
manifest comparison and full payload verification. A release changed between
resolve and installation must fail.

## Private state, actual subprocess and installed identities

`marmish_installed_state` exposes these shared interfaces:

```python
create_state(state_file: Path, ctx: dict, pin_sha256: str, role: str) -> dict
load_state(state_file: Path) -> dict
child_environment(state: dict, *, runtime: bool) -> dict[str, str]
clock_sample() -> dict
elapsed(start: dict, end: dict) -> dict
run_bounded(argv: list[str], state: dict, *, timeout: float,
            output_limit: int, runtime: bool, stdin: bytes = b'') -> dict
initialize_identity(state: dict, name: str, *, timeout: float) -> str
cleanup_state(state: dict, *, runtime_cleanup) -> dict
```

`create_state` refuses a preexisting state file; creates owned mode-0700
`/tmp/mci-<random 6..16 URL-safe characters>` (canonical `/private/tmp/...` on
macOS); atomically writes a 0600 state pointer and 0600 private `state.json`.
Root ownership, canonical path and inode/UID are revalidated at every load and
before cleanup; symlinks and an arbitrary pointer into another directory fail.
Open private state/root entries with `O_NOFOLLOW`, verifying the opened file or
directory's identity rather than relying only on a preceding pathname check.
Private state contains format/context/role/pin digest, root identity, prefix,
scratch HOME/TMP paths, public output path, start clock, preparation deadline,
barrier deadline and tracked subprocess identities. Server/client profiles are
`ROOT/server`, `ROOT/client`; prefix is `ROOT/install`; subprocess cwd is the
owned scratch HOME. A private configuration may hold public records and local
process metadata; public artifacts never contain this configuration.
Keep the state configuration immutable after preparation. Concurrent phase B
server/client processes write separate per-child ownership/metadata files under
an owned private registry (or use an explicit locked registry), never replace
the entire shared configuration to append a process. Atomic disjoint lifecycle
reports avoid losing another process's cleanup identity.

The orchestrator hashes the actual installer bytes against the pin, copies those
exact bytes to owned 0600 `ROOT/install.sh`, and loads any embedded validation
helper from those same hashed bytes. It then calls:

```text
/bin/sh ROOT/install.sh --debug --version VERSION --prefix ROOT/install
```

The verified copy is byte-identical to the checkout installer recorded in the
pin; recheck its digest after use. This is a real subprocess with the normal
network path. Every child uses a new
process session, `close_fds=True`, no shell interpolation and an explicit scrubbed
environment. The bounded process wrapper drains stdout and stderr concurrently,
counts each stream independently, and terminates the owned child group on
timeout/output excess. The combined capture bound is 2 MiB for installation,
64 KiB for identity initialization, and is explicit for phase B byte tests.
`output_limit` is the combined stdout-plus-stderr byte ceiling; per-stream
counters support diagnostics without allowing each stream a second full cap.
All waits use the suspend-inclusive remaining deadline; polling interval is at
most one second. It records PID identity before signaling and verifies exit
after graceful interruption then bounded TERM/KILL escalation. Unknown process identity is cleanup
incomplete, never permission to signal an unrelated PID.
Observe leader exit with `waitid(WNOWAIT)` as in `mdk_smoke.Processes`; preserve
the unreaped leader through the final owned process-group signal, including
successful child exit. Only then reap and prove process-group absence within a
bounded grace period. Leader exit or closed pipes alone do not prove descendant
cleanup; ambiguous group identity/absence fails closed.
On timeout/output excess, runtime CLI children first receive SIGINT and up to
five seconds for Python's `KeyboardInterrupt` to unwind `Profile.__exit__` and
native transport cleanup. SIGTERM alone does not run the client's Python
finally blocks. Non-runtime installer children first receive TERM. Then send
TERM followed by bounded KILL to remaining owned members; server supervision
may use its existing graceful SIGTERM handler. These grace periods consume the
explicit termination margin and never extend the server lifetime.
Keep the leader unreaped throughout. A previously reaped leader never authorizes
another group signal; enumerate/prove absence or retain incomplete state.

Return shape is `{returncode, timed_out, output_limited, stdout, stderr,
timing, cleanup}`; `stdout`/`stderr` are private bytes, not JSON artifact fields.
Failure categories are finite reviewed constants, not exception text or child
output. Child diagnostics remain in private state during the job. Public failure
reports may include only bounded recognized installer/runtime categories,
signals and phase names. Do not upload unrestricted installer debug logs,
profiles, home paths, argv, environment or exception text.

`child_environment` is an allowlist: fixed system directories plus the selected
Python directory in PATH, private HOME/TMPDIR/XDG config/data/cache/runtime
directories, `LANG=C`, execution marker and optionally `RUNNER_TRACKING_ID`.
Use the same Python selection for installer and installed shell launchers;
record its version through `python3 -I -B` in this exact environment. Do not copy
ambient PATH, proxy variables, `PYTHON*`, Git/GitHub credentials, Actions runtime
tokens, SSH agents, native keychain references, cargo registry credentials or
unreviewed `MARMISH_*` overrides. For runtime only, permit the existing
authenticated Bubblewrap selection returned by its validator. Bootstrap tools
may use their separately reviewed environments; those do not become this child
environment. The harness never needs GitHub credentials except the isolated
rendezvous adapter step and pinned artifact action steps.

`initialize_identity` uses `runtime=True`, forwarding only verified
`MARMISH_BWRAP`/`MARMISH_BWRAP_SHA256` values from
`marmish_ci_bwrap.environment()` on Linux. It accepts only `server`/`client`, invokes absolute
`ROOT/install/bin/marmish --init -i ROOT/NAME --key-storage files` with both
explicit `--relay` arguments and bundled defaults, and requires successful exit
plus exactly `{account: HEX64}` from strict JSON stdout. Reject duplicate
identity values and refuse existing profiles. No debug flag on byte-sensitive
live commands. File storage is explicit disposable CI scope, not a native
keychain acceptance claim. A macOS native probe abort or Linux namespace denial
is recorded at identity initialization; it is not bypassed with source code,
unconfined processes, alternate MDK or a different release.

## Clock and timing API

The user's additional requirement is actual latency and speed results published
in the repository. Phase A supplies common measurement/provenance fields;
phase B defines timed workloads and phase C publishes measured results.
`clock_sample()` privately returns `{continuous_ns, monotonic_ns, wall_ns}` as
nonnegative integers, taking continuous time from `continuous_time()`. Capture
samples immediately around the actual installer subprocess, installed
verification, each identity initialization and the barrier. `elapsed` returns
`{elapsed_ns, active_ns, suspended_ns, clock_valid}` where elapsed uses the
continuous delta, active the monotonic delta, and suspended is
`max(0, elapsed-active)`. Negative/regressing deltas or `suspended_ns > 5e9`
mean `clock_valid=false`; do not silently replace measurements with wall-clock
differences. Wall time is used only for public expiry and run correlation.
Check the known suspend-excluding `time.get_clock_info('monotonic').implementation`
on each supported host (Linux CLOCK_MONOTONIC, macOS mach_absolute_time), with
tests; refuse unknown clock semantics instead of assuming them.

Public timings contain durations only, never absolute host boot clock samples.
The exact `timing` shape is `{unit: "ns", clock: "continuous+monotonic",
stages: {STAGE: {elapsed_ns, active_ns, suspended_ns, clock_valid}}}` with fixed
stage names `install`, `verify`, `identity_server`,
`identity_client`, `barrier`; completed stages only. Each integer is bounded by
the 75-minute job allowance plus a small shutdown margin. Missing measurements
are missing, never zero or inferred successful work. Python bootstrap precedes
the harness state, so phase A omits bootstrap duration. These setup durations are not command RTT
or throughput. Never subtract clocks on different hosts; future pair latency
must be measured from one client's local samples. No speed claim is made by
phase A.

## Three-peer public records and adapter

`marmish_installed_rendezvous` exposes:

```python
artifact_name(ctx: dict, role: str, stage: str) -> str
canonical_bytes(record: dict) -> bytes
validate_identity(record: dict, ctx: dict, pin: dict, role: str,
                  *, now: int) -> dict
validate_server_ready(record: dict, ctx: dict, pin: dict, role: str,
                      identities: dict[str, dict], *, now: int) -> dict
wait_records(ctx: dict, pin: dict, stage: str, roles: tuple[str, ...],
             *, deadline: float, output: Path) -> dict[str, dict]
```

Only the adapter entrypoint reads `GITHUB_TOKEN`. It never starts installer,
runtime, compiler or fixture subprocesses. It writes validated public records
to an owned 0700 peer directory and exits; later runtime steps have no token.
`wait_records` accepts the fixed three-role tuple, not artifact-controlled
roles, URLs or output paths. `stage` is initially `identity-ready` or
`server-ready`; B/C will add strict terminal-result validators. Names are
`installed-interop-RUN-ATTEMPT-ROLE-STAGE`; exactly one artifact is allowed for
each name. Retried uploads with duplicate names fail rather than select newest.

Identity-ready has exactly these fields:

```text
format=1, kind="identity-ready", run, attempt, sha,
pin_sha256, role, target, nonce=HEX64,
server=HEX64, client=HEX64, relays,
installation={target,version,source_commit,archive_sha256,manifest_sha256,installer_sha256},
interpreter={implementation="cpython",version=[major,minor,patch],path_class},
timing, published_at, expires_at
```

The context is the current canonical repository run/attempt/harness SHA. Every
installation field must equal the pin entry; `target` matches role. Public
`server`/`client` values must differ, and the all-role barrier requires all six
public identities and all three nonces to be distinct. `interpreter.path_class`
is a reviewed enum (`system`, `homebrew`, `ci-bootstrap`); no private interpreter
path. CPython version must meet the installer's prerequisites. `published_at`
and `expires_at` are integer epoch seconds; require `published_at <= now+60`,
`now < expires_at`, and `0 < expires_at-published_at <= 4200`. Freshness checks
repeat when crossing a barrier or launching a server, not just at download.
Reject booleans as integers, extra keys, JSON duplicates, NaN/Infinity, wrong
field types and records over 8192 bytes. A record is ready only after full
installation verification and both identity initializations have passed.

Phase B server-ready has all identity-ready fields with `kind="server-ready"`
plus exactly `ready=true`, `identity_ready_sha256`, `fixtures` and
`server_started_at`. `identity_ready_sha256` is an exact three-role map to hashes
of the canonical accepted identity-ready records. The identity and installation
fields must match the sender's identity-ready record. `fixtures` is an exact
three-role map to `{path, sha256}`: an owned fixed binary per client role at
`ROOT/f/ROLE/main`, with ROOT matching the private root grammar above. macOS
paths use canonical `/private/tmp`. This public path is allowed solely as the
command-policy fixture target; it is not a private profile path. Phase B builds
the variants from the harness source SHA and supplies these descriptors. Ready
publication waits for actual installed daemon readiness and unchanged clock
validity. No fixture/ready placeholders are uploaded by phase A.
All three fixture paths must share exactly one sending server ROOT, each with
the corresponding role segment; mixing syntactically valid roots fails.

Each ZIP remains at most 65536 bytes, with exactly one unencrypted, non-ZIP64
`record.json` at most 8192 bytes; use existing strict ZIP and digest checks.
List artifacts using `per_page=100&page=N`, validate response structure and
`total_count`, collect all matching names before accepting any record, and fail
if listing cannot be proven complete. Maximum ten pages/1000 artifacts and
262144 bytes per page; exceeding the bound refuses rather than truncates. Require
exact artifact `name`, non-boolean positive integer `id`, non-boolean nonnegative
integer `size_in_bytes` within bounds, `expired=false`, and mandatory
`digest="sha256:HEX64"`. A changing count or duplicate ID across pages can reflect
concurrent uploads: restart the entire listing within the deadline, at most
three times. Fail continued instability; a consistent snapshot with two IDs for
one wanted name always fails. Reject malformed entries/digest mismatches.

Do not reuse existing `request/archive` unchanged. Add
`request_bounded(url, token, limit, deadline)` in the adapter, retaining the fixed
API origin, approved HTTPS signed-download redirect hosts and no token on
redirect. Run each HTTP exchange in a dedicated adapter-only subprocess whose
parent enforces the continuous-clock total deadline and bounded response pipe.
This prevents slow-drip headers/body from extending urllib's per-read timeouts.
The worker uses connect/read timeouts `min(15, remaining)` and incremental
`read1` chunks at most 64 KiB, checking accumulated bytes/deadline between chunks.
Pass an API credential only over a private bounded pipe, never argv/environment;
the redirected exchange receives no credential. The worker executes only this
fixed network-adapter implementation, never an installer/runtime/fixture.
On total timeout kill/reap the owned worker group with the same reserved-leader
discipline and report a finite network/deadline error. Require the digest before
passing it to existing `read_zip`. Offline slow-drip listing, header and
redirected-ZIP tests must prove the parent watchdog bounds the exchange.
Retry only bounded
transient errors, with three-second intervals within the absolute deadline.
No record cache may silently extend expiry or cross attempts.

`wait_records` writes only a complete validated map; partial progress may be
kept privately but cannot masquerade as the completed barrier. The local
identity record participates in the same validation. This prevents launching
one server while another platform is still queued or cannot install.

## Orchestration, scheduling and cleanup

The thin CLI offers `resolve`, `prepare`, `identity-ready`, `barrier`, `cleanup`
subcommands with explicit `--state`, `--pin-dir` and fixed `--role` as needed;
the adapter CLI owns `wait --stage ... --state ... --output ...`. `prepare`
creates state and executes installation/verification; `identity-ready` performs
the two installed initializations and emits a public record; `barrier` verifies
the adapter's complete map, uniqueness and expiry without a token. Phase B calls
these functions directly or extends the CLI, without duplicating state/schema
logic. Every failure emits a bounded sanitized installation-result report.

The workflow uses the approved canonical master/manual guards, pinned actions,
read-only permissions, credential-free checkout and shared noncancelling
concurrency group. Resolve completes before all three platform jobs; platform
matrix has `max-parallel: 3`, `fail-fast: false` and the approved runner group and
labels. Python bootstrap and authenticated Linux Bubblewrap preparation happen
before installation. Mac sleep assertion begins before identity initialization
and remains supervised through the live work. Runtime-confinement tests remain
mandatory; doctor is static information only.
Grant `actions: read` on platform jobs for the adapter; resolve retains only
`contents: read`. Track caffeinate PID/identity in its own private child record,
supervise it across workflow steps, and stop it through owned cleanup. No
untracked runner-wide sleep assertion.

Each platform step uploads an installation report on failure and, on successful
identity preparation, its one identity-ready artifact immediately while the
platform job remains running. The isolated adapter step then waits at most ten
minutes (also bounded by the private overall deadline) for all three records.
Phase A stops at the completed barrier; this must be described as prerequisite
validation, never nine-pair acceptance. Phase B extends the same jobs after the
barrier. Reserve cleanup/report time within the 75-minute job timeout; use
`if: always()` cleanup and public-artifact upload. A queued job cannot notify
about itself: root's external driver handles the five-minute assignment check.

An installation-result report uses the common context/pin/role fields,
`kind="installation-result"`, `outcome` (`passed`/`failed`), fixed `stage`,
finite `error` or null, installation tuple or null, interpreter or null, timing,
and `cleanup` (`complete`/`incomplete`/`not-started`). A passed installation
result means setup passed, not the live matrix. Phase C validates reports
against this schema; failure reports cannot contain arbitrary strings.
Allow `pin_sha256=null` only for `pin_unavailable`/`pin_invalid` before validated
pin state exists. Reviewed error categories are `pin_unavailable`, `pin_invalid`,
`release_resolve_failed`, `installer_refused`, `installer_timeout`,
`installer_output_limit`, `install_verify_failed`, `identity_init_failed`,
`barrier_timeout`, `artifact_invalid`, `artifact_network_failed`,
`artifact_listing_unstable`, `clock_invalid`, `uninstall_failed`,
`cleanup_incomplete`, `internal_failure`. Optional diagnostic fields are only
finite installer/runtime stage and bounded numeric exit code/signal/HTTP status,
mapped from recognized fixed-schema debug records and process results. Any new
label requires an explicit schema/test update.

Cleanup is ownership based, never `pkill`, name matching or unverified recursive
deletion. On every exit path stop tracked owned foreground groups, run existing
native transport cleanup using verified controls/profile ownership, and prove
owned detached workers/daemons are absent before removing the private root.
macOS native lifetime/control directories can live outside the profile root;
discover and clean those through the existing authenticated control APIs rather
than assuming recursive profile deletion covers them.
Initialization creates new-session daemons outside the frontend group. On Linux
use a unique per-run `EPIPHYTIC_EXECUTION_ID`; scan owned-UID `/proc` entries using
native-validation audit patterns before/during execution and at cleanup, matching
the exact marker. Bound scans to 4096 processes and each cmdline/environ read to
64 KiB. For every match pin `process_identity`, verify executable/argv against
the installed payload, verified Python plus installed outer transport script,
system unshare and authenticated Bubblewrap paths actually used by this run,
then record its identity in the private registry. Include a post-leader-exit
scan to find newly orphaned setsid descendants. Signal verified individual
identities through Linux `pidfd_open`/`pidfd_send_signal`, with identity checked
before and after opening, to avoid PID reuse. A matched unrecognized/unreadable
process is cleanup incomplete, not permission to kill it. Require two clean
bounded scans after cleanup. Phase B extends the allowed executable set only
with its exact verified fixed-role fixtures.

On macOS use authenticated `.marmish-run.json` daemon/shim identities for
`ROOT/{server,client}/mdk`, `process_identity` checks and `cleanup_control(home)`
to stop/verify lifetime controls outside the frontend group. A launch interrupted
before a verifiable control record exists is cleanup incomplete unless the
owned process registry proves the launched processes absent. No broad PID/name
matching substitutes for proof. Test a setsid grandchild, a reaped-leader race
and timeout during initialization.
Phase B supplies its runtime-cleanup callback to `cleanup_state`; phase A still
must verify no initialization-created native process remains. Signal handlers
set shutdown intent and enter this same bounded cleanup path. Ambiguous process
identity, native cleanup refusal or interruption produces `cleanup=incomplete`
and retains private state rather than deleting evidence under live processes.
It is never uploaded. Copy only schema-validated public reports to an owned
artifact directory outside the deletion root before removal. The real install
script's uninstall retains version payloads. After process absence is proven,
rehash the private installer copy and run the actual
`/bin/sh ROOT/install.sh --uninstall --prefix ROOT/install` through `run_bounded`
with scrubbed environment and a 30-second cap. Require success and absence of
owned launchers/current/marker, then remove the entire fresh owned test root.
Skip uninstall only when no installer-owned marker exists and verified absence
of launchers/current/marker sets `checks.uninstall_complete=true`;
malformed/unsafe markers fail. Uninstall failure records
`uninstall_failed` and retains private state with cleanup incomplete.
Persistent runner user
installations, keychains and sibling job files are untouched.

Write the final public `cleanup-result` outside ROOT only after cleanup/root
removal has finished. Its exact fields are `format=1`, `kind="cleanup-result"`,
common run/attempt/sha/pin_sha256/role, `outcome` (`complete`, `incomplete`,
`not-started`), `checks`, finite `error` or null and `duration_ns`. `checks` has
exactly `frontend_absent`, `mdk_absent`, `workers_absent`, `caffeinate_absent`,
`engine_cleanup_ready`, `uninstall_complete`, `root_removed`, each true/false/null.
Phase C requires every check true for live success (verified absence of an
unused component is true); null is only unavailable proof on a failure, never
green. Earlier installation/server cleanup fields are provisional. Exactly three
final cleanup records are required by phase C even after platform failure.

## Acceptance and required tests

Offline tests must demonstrate the concrete boundaries, not mirror internals:

- A fake child shell records that the actual installer path/arguments were
  executed; no acceptance path calls `install_archive`. A local fixture server
  or mocked release downloader is unit-test scope only, clearly labelled.
- Strict archive/manifest extraction, duplicate/missing assets and checksums,
  source/target mismatch, malformed/oversized sidecars, bad payload hashes/modes,
  installer drift, changed release between resolve and install, ownership marker
  mismatch, selector escape, altered launcher and unexpected installed files.
- Child token/env leak sentinel rejection, unrelated HOME isolation, argv paths
  containing spaces, stdout/stderr bounds, exact timeout behavior, PID identity
  change refusal and cleanup after partial installation/initialization.
- Public-key output strictness, duplicate local and cross-role keys/nonces,
  native initialization failure, no source fallback and no public private-state
  or raw-child-log fields.
- Three-role schema completeness, extra/mistyped/duplicate keys, wrong
  run/attempt/source/pin/role, stale/future records, retry artifact duplicates,
  duplicate pages/IDs, pagination overflow/truncation, corrupt ZIP/digest,
  rejected redirect and explicit proof the API token is absent on redirect.
- A late or failed third platform never completes the barrier; all three
  installed identities must exist before phase B can start any server.
- Fake-clock suspend/drift, deadline expiry during listing/download/process
  drain, signal cleanup, retained uncertain private state and sanitized reports.
  Durations come from measured samples and never cross-host timestamp
  subtraction; absent stages are absent, not fabricated zero measurements.

Run the affected installer, release and rendezvous regression tests alongside
new module tests, workflow static validation and meaningful subprocess tests.
The live dispatch records exact release/installer/source provenance and honest
native prerequisite results on all three platforms. Reviewed and tested phase A
infrastructure may close with an independently tracked native runtime blocker;
its report must distinguish infrastructure completion from failed native
initialization and the still-incomplete nine-pair acceptance. Root records the
blocker and fixes it under the reviewed production-change scope.
No native success, command latency or full interoperability claim is inferred
from a passing mock, relay publication, script exit or static doctor result.

## Integrated code review refinements

The first independent [Claude code review](../reviews/installed-nine-pair-a-code-initial.json)
identified implementation gaps against this design. The corrected implementation
captures same-UID unreadable process identities before spawning any work and
excludes only unchanged boot/start/PID tuples predating that capture. Newly
unreadable or replaced identities remain a failure. Linux inner namespace
processes are authenticated through the exact owned confinement config, fixed
profile home/relay set/execution marker, registered sandbox/daemon digests and
inode-equal `/config.json`, `/sandbox.py` and `/wnd` bindings observed through
identity-fenced proc magic links. Fixed ip/sysctl argument vectors require an
authenticated confinement parent. Verified digest caching includes device,
inode, size, mtime and ctime; observed orphan identities are retained privately.

macOS runtime records retain interruption/return-code and observed daemon/shim
identity evidence. An interrupted launch before its control record exists
remains incomplete; previously observed identities must prove absent after
authenticated recovery. One registered caffeinate starts before identity
initialization and survives stages until owned final cleanup. Pipe readers expose
first/last nonempty output observations relative to the continuous start clock.

The isolated artifact adapter writes a closed, sanitized `adapter-result` status
next to its output. The barrier runs after successful local identity setup even
if the adapter failed, propagates a finite failure category, and verifies that
its own downloaded identity-ready record exactly matches what it published.
Missing adapter evidence cannot leave a passed installation report. The verified
private installer copy is rehashed immediately after installation as well as
before uninstall. A complete new workflow attempt is required; rerunning only
failed jobs cannot reuse the prior attempt's release pin or identities.

Operational limits from the final code review: a newly appearing unreadable
same-UID process makes cleanup fail closed; avoid interactive logins and unrelated
service restarts during validation. macOS caffeinate should run on AC power. Its
identity-checked shutdown has the platform's small check-to-signal race because
macOS has no pidfd interface. Linux reports caffeinate absence vacuously because
it never starts that macOS-only process. The first actual confined native launch
still has to validate the sandbox-tree authentication against real processes.
