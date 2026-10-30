# Self-hosted managed-runtime proof

Proposed amendment to [increment 0](managed-runtime-spike.md) and its
[completion design](managed-runtime-spike-completion.md), Beads `marmish-x6z.1`.
Initial Claude review: NOT APPROVED. Independent follow-up review by
`claude-sonnet-5-5` explicitly APPROVED this revised design for implementation,
confirming B1–B6 resolved. Its three implementation conditions are incorporated
below. See [review dispositions](../reviews/managed-runtime-self-hosted-design.md).
Design approval does not establish code or native acceptance; no gate is waived.

## Trigger, scope and runner selection

Hosted AMD64 run `41138717122` confirms that its required namespace gates
cannot run: all three namespace probes returned denial, `ip`/`sysctl` were
present, and the 445-test full suite had the same two namespace failures. Both
native-help probes exited zero; cleanup and real-home checks passed. The
read-only AppArmor restriction value was 1, which does not prove it was the
exclusive cause. Preserve that failed report and its exact source provenance.
After separate design approval, use the self-hosted execution path below; a
pass establishes acceptance on that recorded configuration, not a hosted-runner fix.
Current native jobs are queued and unassigned. Organization/group API access
returned 403 with the available token, so runner availability and a scheduling
cause have not been verified.

Reuse `.github/workflows/native-validation.yml`, its `epiphytic-ci` runner group,
pinned checkout, native bootstraps and exact matrix:

| Target | Labels |
| --- | --- |
| Linux AMD64 | `[self-hosted, Linux, X64, bel-ci]` |
| Linux ARM64 | `[self-hosted, Linux, ARM64, laptop-ci]` |
| macOS ARM64 | `[self-hosted, macOS, ARM64, laptop-ci]` |

Add a separate `managed-proof` job **inside the existing native-validation
workflow**, with no dependency on `native` or its macOS confinement gates. Use
this exact three-target matrix, `fail-fast: false`, `max-parallel: 1`, and a
60-minute job timeout. Keep `contents: read` and pinned checkout with
`persist-credentials: false`. Reuse the existing bootstrap/cache commands and
pins literally: `bootstrap_ci_python.sh`, Linux authenticated Bubblewrap
acquisition/probe, existing pinned Actions cache and `marmish_ci.py bootstrap`
with the same reviewed Rust/cache environment. Small literal bootstrap
duplication is preferable to conditionalizing every step of the native job.
No new standalone workflow or reusable-action extraction is needed.

Add `workflow_dispatch` with **no inputs**. Admit `managed-proof` only for
canonical-repository `master` push events or a manual dispatch of
`refs/heads/master`. Tighten existing `native` and `live` job admission to
canonical-repository **push only**, still on `refs/heads/master`. Thus manual
dispatch runs only managed proof; pushes retain native/live validation and also
run managed proof. No pull-request, fork, arbitrary-ref or
`pull_request_target` execution. Do not add secrets, privilege elevation, sudo,
package installation into the host, host sysctl writes or runner policy changes.

The job step order is checkout, Python bootstrap (5-minute step limit), Linux
Bubblewrap bootstrap (6 minutes), Linux Bubblewrap probe (1 minute), pinned
cache restore, Rust bootstrap (10 minutes), then harness `id: proof` (25 minutes).
Bound checkout/cache/upload/cleanup steps to 2/3/2/3 minutes respectively.
The harness itself has an approximately 22-minute total deadline below its
25-minute step limit, reserving time to terminate children and write the report.
A 60-minute outer cutoff is a failure, not evidence of acceptance; job-level
timeout may prevent even always-run report/cleanup steps.
Invoke the harness with Linux-only `--require-bwrap-helper`, and on all targets
`--ci-context "${sha}:${run_id}:${attempt}:${platform}" --report
"$RUNNER_TEMP/managed-proof/managed-runtime-report.json"`. Never pass
`--failure-log` in CI. Require the run-owned report directory to be fresh; the
harness creates the report itself using `O_EXCL`.

Then run `if: always()` report upload with artifact name
`managed-selfhosted-${run_id}-${attempt}-${platform}`, uploading only that one
sanitized JSON file, `if-no-files-found: warn`, and no `continue-on-error`.
Last run `if: always()` Linux helper cleanup using the existing native cleanup
shell with `steps.proof.outcome` substituted for `steps.offline.outcome` and the
new job’s own helper state. Each job owns scratch state and outcomes; never
borrow another job’s helper. Failed bootstrap/proof/cleanup remains failure;
missing reports, queued/unassigned jobs, replaced pending dispatches and forced
cancellations are absent evidence. The workflow concurrency group may replace a
pending manual run; do not call that validation. Docs-only push path filters
still apply, making dispatch necessary to test such revisions.

Add a SIGTERM handler raising `Refused('interrupted')`; handle
`KeyboardInterrupt` in `main()` as `failure='interrupted'`. Both unwind through
owned-process-group kill/reap and private-state cleanup, then attempt the
exclusive sanitized report write. Test interruption during a live child and
report emission. Runner SIGKILL/job teardown can prevent all of this; do not
promise cleanup/report delivery in that case. No blanket runner environment
inheritance is added to compensate. Native/macOS job outcomes are independent.

The repository/ref/event guards are admission checks in mutable workflow code,
**not** a security boundary against repository writers. All three job guards
must literally include `github.repository == 'Epiphytic/marmish'`; native/live
also explicitly require `github.event_name == 'push'`. Restriction of the
`epiphytic-ci` runner group to approved workflows/refs is an **unverified
operator precondition** because the policy API returned 403. Record or obtain
that policy evidence before claiming runner isolation; do not infer it from the
labels or guards. No additional secrets or host-policy changes are proposed.

## Authenticated helper and strict environments

On Linux retain the existing `marmish_ci_bwrap.py bootstrap --state ...` and
`probe` sequence. It authenticates Ubuntu archive metadata using the fixed
root-owned keyring, checks reviewed package floors/digests/ELF architecture,
extracts only a private non-setuid helper and records provenance. Preserve its
five-minute acquisition deadline, supported Ubuntu suites/architectures and
ownership-checked cleanup. An unsupported distro, unavailable trusted metadata,
missing tools or failed nested probe is a blocker, never grounds to use another
helper or weaken the test. macOS does not acquire or receive this helper pair.

Introduce a distinct `test_supervisor` environment for the full tools suite
only. The Linux diagnostic ladder and offline probes use the validated absolute
helper in argv with their unchanged strict probe environments; they receive no
helper environment pair. Construct `test_supervisor` from the existing strict test environment
plus **only** the result of `marmish_ci_bwrap.environment()`. That existing API
validates the complete `MARMISH_BWRAP`/`MARMISH_BWRAP_SHA256` pair through
`marmish_transport_sandbox.validated_bwrap()` before returning it. Reject
half-pairs and invalid selections before spawning tests. Do not copy arbitrary
`MARMISH_*`, loader, Python, uv, API-token, Actions-service or runner variables.
The uv installs, installed launchers and hostile-environment controls retain
their existing exact environment allowlists; the helper pair is not added to
those application environments.

Add Linux-only CLI enforcement: with `--require-bwrap-helper`, an empty
`marmish_ci_bwrap.environment()` result fails with
`Refused('bwrap_helper_required')`. The workflow always supplies this flag on
Linux; without it the hosted default system-selection policy remains available.
On Darwin reject the flag and any variable whose name begins `MARMISH_BWRAP`,
rather than ignoring injected helper configuration. Report `bwrap_source` as
`helper` or `system` on Linux; omit this Linux-only field on Darwin.

Add harness `bwrap_executable()` calling `validated_bwrap()`. Translate its
`SandboxError` into `Refused('bwrap_selection:' + category)` with only the known
fixed categories `invalid_bwrap_selection`, `unsafe_bwrap_path`,
`unsafe_bwrap_executable`, `bwrap_hash_mismatch`; an unknown category becomes a
fixed `bwrap_selection:unknown` refusal, never raw exception text. Use this
adapter around initial environment validation too. Both `network_prefix()` and
`linux_namespace_preflight()` use this adapter and the validated absolute
executable. A selection refusal must not become `network_namespace_unavailable`;
preflight records the fixed refusal and preserves report production. The same
validation also applies to system selection in hosted mode: missing/unsafe
`/usr/bin/bwrap` now produces an explicit selection refusal, a deliberate
failure-category tightening which needs regression coverage. No PATH search or
fallback after invalid selection. Preserve all namespace/mount/UID/capability
assertions, the existing authenticated probe and the diagnostic ladder.

Add `--ci-context <sha40>:<run_id>:<attempt>:<platform>`; parse into `report['ci']`
without reading `GITHUB_*`. Require exactly four fields: lowercase hex SHA of
40 characters; positive decimal run ID of 1–20 digits; positive decimal attempt
of 1–6 digits; platform exactly `linux-x64`, `linux-arm64` or `macos-arm64`
consistent with the detected native OS/architecture. No whitespace, signs,
leading zeroes or extra fields. Missing context remains permitted for local
harness runs and is represented as absent CI evidence, not fabricated fields.
Record workflow as fixed `native-validation` and runner class from the fixed
platform-to-label matrix above, never from arbitrary runner environment text.

Add `helper_provenance(pair)` for `report['bwrap_helper']`. Revalidate selection,
read the adjacent provenance file as bounded (64 KiB), non-symlink regular
same-owner/root-owned state, and require `executable_sha256` to equal the
validated pair’s `MARMISH_BWRAP_SHA256`. Copy only: suite (`noble|resolute`),
architecture (`amd64|arm64`, matching native architecture), package version
(`[A-Za-z0-9.+:~_-]{1,128}`), `keyring_sha256`, `package_sha256`,
`executable_sha256` (each lowercase 64-hex), and version
(`bubblewrap [0-9]+(?:\.[0-9]+){1,3}`, at most 64 characters). Reject malformed
or mismatched required facts with fixed `bwrap_provenance_invalid`; do not copy
source URLs, paths, unknown JSON keys or arbitrary strings. System mode has no
helper-provenance object. Same-user provenance remains diagnostic evidence,
not a new protection against a malicious owner.

`marmish_platform.execution_marker()` already validates
`EPIPHYTIC_EXECUTION_ID`. Default to omitting it from `test_supervisor`. Add only
its validated result if inspection of the actual runner ownership/cleanup
contract demonstrates the specific tracking need, and record that decision.
Otherwise do not forward it. In either case assert no marker enters the
harness-owned uv, installed-launcher, offline or probe environments. Do not
assert its absence from every suite-created confinement payload: existing
transport intentionally forwards the marker into its own payload and tests
that contract. This increment preserves that behavior, with no blanket runner
or loader/token environment forwarding.

## Dependencies, diagnostics and cleanup

The Linux bootstrap still needs a capable existing CI Python; the macOS CI
bootstrap provisions its separately pinned Python. These are harness
prerequisites, not a claim about the future user installer. Require `curl`; on
Linux require `sha256sum`, `/usr/bin/readelf` (binutils), helper tools `unshare`,
`ip`, `sysctl` and existing authenticated-package tools/keyring. Darwin retains
its existing `shasum`, `otool` and signing prerequisites. Missing tools are a
recorded blocker; self-hosted jobs never install them with sudo. The managed proof
continues to exercise its own pinned uv/CPython and unchanged verified release
archive. Use the existing reviewed native compiler bootstrap rather than
rustup installation during the suite. The direct bootstrapped `rustc` discovery
via GITHUB_PATH must be tested on self-hosted; do not assume a rustup shim.
Record the actual distribution and CPU
architecture and require the archive's real doctor to pass. In particular a
self-hosted ARM64 glibc version below the current release's requirement blocks
acceptance; do not alter the doctor, substitute an unreviewed build or infer
compatibility from the PBS interpreter's lower dependency floor.

Keep all reviewed interruption, substitution, offline, literal-argv, hostile
ambient state, stdlib/native audit and unchanged full-tools-suite gates. Keep
the macOS exact signed restricted-startup refusal probe and manifest binding
from the completion design. This does not fix or prove configured macOS native
confinement, whose SIGABRT remains a separate open gate. Do not convert a signal,
missing namespace capability, skipped required test or caught exception into a
passing managed result. Darwin still makes no independently enforced network
denial claim. A private `/tmp` helper may itself be denied by AppArmor
when a profile permits only `/usr/bin/bwrap`; repeated denial is a valid failed
outcome, not a reason to change policy. Managed and native jobs share labels
and may overlap on a host running multiple runner instances. Such overlap or
other unrelated uv activity can trigger `real_uv_home_changed`; preserve that
fail-closed result as an expected evidence-contamination failure, not a proven
defect in the managed runtime, and never waive the snapshot gate.

Retain the harness's command deadlines, output/tree limits and owned group
termination/reaping; additionally bound acquisition, report upload and workflow
cleanup. No worker may kill other runner processes or remove shared caches.
Use private run/attempt-owned scratch state and the existing helper cleanup API,
which checks owner, state binding and device/inode before removal. On proof or
other relevant native test failure/cancellation, retain the helper under the
existing conservative cleanup policy, emit a fixed retention category and never
report cleanup success. An operator is responsible for removing retained
random `/tmp/marmish-bwrap-*` directories using verified state and ownership;
never reclaim them by elapsed time or prefix glob alone. On success clean the verified helper after all consumers
finish. If ownership cannot be proved, refuse removal and fail cleanup rather
than recursively deleting a guessed directory. Reports remain available even
when cleanup fails; cancellation may prevent upload and must be reported as
missing evidence rather than success.

Reports identify source commit, workflow/run/attempt, target and fixed runner
class; existing pinned artifact digests; helper package suite/version,
architecture, archive/keyring/executable digests; bounded read-only namespace
policy facts; exact gate outcomes; and cleanup/retention category. Verify helper
provenance against the validated selected executable, not just an arbitrary JSON
file. Do not publish hostnames, absolute private paths, environment dumps,
credentials, raw command output or failure logs. Preserve hosted failure
reports and distinguish the new execution environment in fresh report names.

## Validation and exit criteria

Before native execution add focused tests for the following contracts:

- Canonical-repository/master-only dispatch/push admission, exact group/labels,
  independent proof-job scheduling, no-input manual dispatch excluding native/live,
  explicit step bounds, and report
  collection without swallowing the proof's exit status.
- Validated helper pair reaches only `test_supervisor`; explicitly assert the pair is absent from uv, installed-
  launcher, offline and probe child environments. Require-helper mode
  rejects absent selection, hosted mode covers explicit system validation, and
  Darwin rejects helper configuration. Half-pair, bad hash, altered
  binary, unsafe permissions/symlink and disallowed ownership fail closed before
  child launch. No fallback or arbitrary loader/token forwarding is permitted.
- Offline and diagnostic command builders use the validated absolute helper;
  its digest/provenance binding and required namespace assertions remain intact.
- Strict CI context grammar and native target match; provenance hash mismatch,
  malformed version/types and oversized/symlink provenance fail without raw data.
- SIGTERM and KeyboardInterrupt kill/reap owned children and write interrupted
  reports; no marker leaks into harness-owned uv/offline/probe/launcher children.
- Missing prerequisites, low glibc, failed probe, failure, timeout, cancellation,
  missing report and cleanup ownership mismatch cannot produce acceptance.
  Valid owned success cleans up; retained failure is explicitly recorded.

Then execute all three native targets using the same reviewed managed harness,
archive pins and full suite as the hosted design, without new skips or catch-pass
paths. Require complete sanitized reports, successful mandatory gates and
verified owned-process cleanup on each. Unit/workflow validation or a single
Linux pass does not satisfy the matrix. Report queue delays and actual runner
assignment separately from test results. Finish with Claude's integrated code
review in auto mode, incorporate findings, and record evidence and remaining
platform limitations before the parent publishes the reviewed increment.

Integrate after or with the already-reviewed completion work touching
`network_prefix`, `linux_namespace_preflight` and `full_tools_acceptance`; one
integration owner reconciles these functions rather than overwriting changes.
Place workflow contract tests beside existing native workflow tests in
`test_marmish_native_spike.py` / `test_marmish_interop.py`. A shared toolchain
cache save race between jobs is benign; cache provenance verification remains
mandatory. Design approval does not establish implementation or native acceptance.
