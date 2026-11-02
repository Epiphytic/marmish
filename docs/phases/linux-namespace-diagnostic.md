# Linux keyless namespace stage diagnostic

Installed ARM run `41397651278`, source `9a694963c253071900532d3d1fc9f5489d82d878`,
failed identity initialization with `namespace_start_failed`, exit125, complete
cleanup and no timeout/output limit. That transport category also wraps
preparation I/O errors. No cause is inferred from the category.

Existing native run `41396336463`, source
`52f1640d26627e8d315a9663e168b7ea3dfd32d6`, already failed the keyless prerequisite:
ARM managed-proof job `125119000035` acquired authenticated Bubblewrap and at
2026-11-02 05:59:22 UTC reported `userns_denied_private_bwrap`; ownership-checked
cleanup passed. Native ARM job `125119000210` also failed its prerequisite.
This precedes the installed run started around 06:04 UTC. Therefore repeating
only the existing probe is redundant. Its category maps every nonzero exit;
it does not establish a denied namespace operation or a kernel/AppArmor cause.

The initial Claude design review required this evidence, precise stage limits
and exact runner group. This amended design resolves those findings by adding
a keyless cumulative-stage discriminator instead of repeating the bare probe.

## Scope and interface

Extend only `tools/marmish_ci_bwrap.py` and its tests, add the manual workflow
`.github/workflows/linux-namespace-diagnostic.yml` and offline workflow tests,
and record this design and actual reviews. No production transport changes.
The parent-owned installed workflow is outside scope.

The existing `run()` keeps its signature and success bytes. On nonzero exit,
it raises a new RuntimeError subclass whose string remains exactly
`bwrap_tool_failed`; the only extra attribute is validated finite returncode
(exact int, not bool, between -255 and255, otherwise null). Existing bootstrap
and probe handlers keep their current behavior. No child output is attached to
the exception or published. Timeout/output/storage/cleanup failures retain their
existing fixed categories and do not masquerade as a nonzero exit.

A new CLI operation `diagnostic-probe` validates the same selected absolute
Bubblewrap binary and uses the existing private temporary root and bounded
`run()` with `build_environment()`. It executes these cumulative commands in
order, stopping after the first failure:

1. `unshare --user --map-root-user --net /usr/bin/true`.
2. The same prefix, one existing Bubblewrap argument sequence, then true.
3. The same prefix, two existing Bubblewrap argument sequences, then true.

`unshare` is the absolute `/usr/bin/unshare`. Each Bubblewrap sequence is exactly
`SELECTED --unshare-user --unshare-net --unshare-pid --ro-bind / / --proc /proc --dev /dev`.
The final command equals the existing probe. No shell, new package source,
host configuration, sudo, fallback binary or policy change is introduced.

Each attempt has the existing30s/output/storage bounds. The report contains
only schema1, kind `linux-namespace-diagnostic`, ordered stage entries named
`unshare`, `one_bwrap`, `two_bwrap`, outcome pass/fail, fixed reason,
returncode/null and derived signal/null (-64 through-1 only). Known fixed
helper failure categories are allowlisted; arbitrary exception strings become
`internal_failure`. No stdout/stderr, argv, environment, path, PID, hostname,
keys or exception text is emitted. A failed stage produces nonzero CLI status;
cleanup runs regardless. A setup error produces a fixed failed report with no
stage attempts; it cannot count as a namespace failure. The implementation
must not catch BaseException or convert cancellation into a successful report.

The first failed stage identifies the earliest failing tested command
configuration. Passing unshare alone and failing one_bwrap narrows the fault
to launching Bubblewrap in that outer namespace, but does not prove a specific
syscall or host policy. Failed unshare alone narrows it further. Returncode
alone never proves permission denial. If all pass, pursue a separately reviewed
installed-stage diagnostic; no result here counts as Marmot E2E success.

## Workflow and trust boundary

Only workflow_dispatch, project Epiphytic/marmish and refs/heads/master are
allowed. Exact runner target is group epiphytic-ci and labels
[self-hosted, Linux, ARM64, laptop-ci]. Workflow and job permissions are exactly
contents:read; no explicit token or secret expressions. Pinned checkout
11d5960a326750d5838078e36cf38b85af677262 has id checkout and
persist-credentials:false. Python bootstrap is bounded5min, authenticated
Bubblewrap bootstrap6min, discriminator2min, cleanup3min, totaljob18min.
The state path is $RUNNER_TEMP/namespace-diagnostic-bwrap-state.

Cleanup always runs. If checkout failed it refuses to invoke unchecked source,
reports cleanup_retained_unverified when a state path exists (including
symlink), otherwise cleanup_not_initialized, and fails. Otherwise invoke the
existing ownership-checked cleanup. No continue-on-error hides stage failure.
Distinct concurrency group marmish-linux-namespace-diagnostic has cancellation
disabled; coordinator scheduling must prevent simultaneous native workloads
on the same host. No live dispatch occurs during implementation/review.

## Limits relative to installed startup

This uses runner build_environment(), not installed private HOME/TMPDIR/XDG.
Installed startup uses PATH /usr/bin:/usr/sbin:/bin, system Python --outer,
namespace-local loopback setup and unprivileged-port sysctl; Bubblewrap drops
capabilities, clears environment and binds selected runtime files rather than
read-only host root. It also executes inner self-test and daemon readiness.
None of those stages is exercised here. Installed stderr remains DEVNULL.
The diagnostic must not claim equivalence to those production stages.

## Validation and review

Offline tests cover exact command construction/order, stop-first-failure,
finite returncode/signal validation, no raw output/exception disclosure,
existing run/probe compatibility, fixed category handling, and all workflow
security/runner/bounds/cleanup contracts. Use mocked subprocess tests only;
no local native process scans or live CI. Actual independent Claude design
approval precedes code; actual Claude auto-mode code review and affected
checks precede parent-coordinated publication and dispatch.

## Followup review dispositions

B1: diagnostic setup calls refuse_api_tokens() and requires both explicit
MARMISH_BWRAP and MARMISH_BWRAP_SHA256 inputs before validation. Missing either
produces selection_missing and no run calls. No default binary is accepted.
B2: new report categories are plain data constants only, not new require() or
SandboxError literals. The subclass uses existing bwrap_tool_failed. Include
frozen vocabulary test_marmish_release among affected checks; no vocabulary
module changes are needed.
B3: exact stage reasons are pass, nonzero_exit, bwrap_tool_timeout,
bwrap_tool_output_oversized, bwrap_metadata_oversized, launch_failed (OSError),
and internal_failure (other Exception, including TimeoutExpired). Setup categories
are selection_missing, selection_invalid, setup_failed; ordinary success has
category complete and stage failure category stage_failed. No exception message
is copied. Call checked_selection() under redirected in-memory stdout, suppressing
its bounded bwrap_validation line; the diagnostic operation publishes exactly
one JSON line on stdout and returns its own status directly from main, avoiding
the existing generic error handler. Positive codes (including128+n) never
indicate signals. KeyboardInterrupt/SystemExit are not caught.

Tests also require fixed exception str/args/repr and unchanged existing probe
contract, injected launch/timeout/selection exceptions carrying secret sentinels,
no subprocess invocation on absent selection, no uploaded artifacts, shell
syntax of every workflow run block, and no continue-on-error/token expressions.

Code-review N2 refinement: an exception after any stage attempts (for example
private temp-directory removal failure) produces category cleanup_failed,
retaining finite stage evidence and nonzero status; setup_failed has no
stage attempts. N1 moves tests before unittest.main; N3 adds token-refusal test.

Final local validation: focused helper/workflow/managed-workflow suite passed
38 tests and73subtests; py_compile and scoped git diff --check passed. Actual
Claude initial and final code reviews approved. The combined release suite
previously had55passes79subtests and one import-closure gate failure from
concurrent bootstrap-owned imports in marmish_ci.py; its owner must repair and
rerun the gate before integration. No native diagnostic has run for this change.
