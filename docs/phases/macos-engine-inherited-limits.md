# macOS inherited engine limit diagnostic

Actual Claude auto design review approved this bounded diagnostic (session
`99fc3557-fb6b-49c2-9eee-eec22d0fd727`, 70.379 seconds, eight turns). Its useful
plumbing and interpretation simplifications are incorporated below. The
coordinator authorized implementation after that approval. This authorizes no
product correction or qualification waiver; actual code review must precede
manual dispatch.

## Question and evidence boundary

Original installed stream-matrix run 42395261837 has three macOS server records
persistently Preparing, with validated Hello, admitted/cached Open and an
unanswered first input Exchange. All six Linux-target cells completed. Inspect
remained responsive. `stream_runtime::prepare_streams` can remain Preparing when
`CommandEngine::try_prepare_live` is unavailable; an engine-open failure is
currently reduced to `OpenFailed` without its originating error. These records
do not establish that engine opening failed.

The original execution chain is GitHub local JavaScript action (`action.yml`,
`using: node24`) → `supervise.mjs::supervise` → installed-check Rust driver →
`installed_matrix::Daemon::start` → daemon. The successful cold multiprincipal
diagnostic invokes its Rust driver directly from a bash workflow step. Both use
the same daemon launcher and canonical `/private/tmp` workspace convention.
Neither driver path explicitly normalizes resource limits. Environment clearing
in `installed_process::command` does not reset inherited resource limits.

[Official Node v24 source](https://github.com/nodejs/node/blob/v24.x/src/node.cc)
contains a POSIX initialization branch guarded by `kNoAdjustResourceLimits` that
attempts to raise the soft open-file limit: it uses a finite hard limit directly,
or searches up to 2^20 when the hard limit is infinite. This establishes a source
mechanism, not the actual GitHub bundled Node version, effective runner limits,
or a Marmish failure cause. The macOS engine launcher rejects an inherited
RLIMIT_NOFILE above its supported ceiling of 65,536. This experiment measures
whether the launch chains cross that ceiling; it does not start the engine.

## Smallest implementation and ownership

Add only a manual workflow `.github/workflows/macos-engine-inherited-limits.yml`,
a diagnostic-only local action under `tools/macos-engine-inherited-limits/`
(`action.yml`, a tiny `run.mjs` entry point and `probe.mjs` parser/runner module), and focused Node policy/parser
tests there. No changes to the existing supervisor, product Rust, installed
harness, dependencies, candidate provenance, release automation, or acceptance
criteria. No Cargo, installer, Marmish binary, relay, identity, keychain, or
network request from the diagnostic probe.

The workflow has only `workflow_dispatch`, no user inputs, repository permission
`contents: read`, one job on the currently used hosted `macos-15` ARM64 runner,
and `timeout-minutes: 5`. It checks the expected repository, Darwin/arm64 host and
nonroot execution with finite failures, then uses the existing pinned checkout
SHA with `persist-credentials: false`. It does not request tokens, secrets,
write permissions, dependency installation, or artifacts containing raw logs.
No matrix, reusable workflow, scheduled trigger, or automatic retry is needed.

Run two measurements in the same job, without adjusting either limit:

1. A `shell: bash` baseline invokes `/bin/sh` directly with the fixed measurement
   program. Its measurement process chain does not pass through Node (the
   separate checkout action is not its parent).
2. A `uses: ./tools/macos-engine-inherited-limits` step starts an action declared
   `runs.using: node24`. Its entry point uses `node:child_process.execFileSync` with the
   inheritance-relevant Node24-parent seam: fixed executable `/bin/sh`, fixed
   `-c` program and an explicit inherited child environment. Output is captured
   rather than inherited; synchronous exec also differs from the original spawn.
   No `shell: true`, detached
   process, pre-exec wrapper, limit-changing command, or installed-check binary.
   This reproduces the relevant Node24-parent → native child inheritance seam;
   the omitted Rust intermediates do not change limits in the inspected source.

Both execute the identical fixed `/bin/sh -c` program: read `ulimit -Sn` and
`ulimit -Hn`, check success, and accept only decimal digits or `unlimited` before
printing the fixed `soft=...` and `hard=...` lines. The shell baseline appends
these two lines directly to `$GITHUB_OUTPUT`. Workflow `with:` expressions pass
them as action inputs, never interpolate them into shell source. The Node24
action captures the same two lines and validates them. There are no measurement
files, cleanup step, or separate final validator.

The action uses a tiny unconditional entry point and one dependency-free module
exporting a strict parser and runner,
with `execFileSync('/bin/sh', ['-c', FIXED_PROGRAM], ...)`, timeout 5,000 ms,
maxBuffer 4,096 bytes, explicit inherited environment and
`stdio: ['ignore', 'pipe', 'pipe']`. Capturing finite output differs from the
original inherited-stdio supervisor, as does synchronous execution: only the
Node24-parent → native-child resource-limit inheritance seam is matched. No
pre-exec wrapper, limit adjustment or arbitrary child arguments are allowed.
Spawn, signal, timeout, overflow and nonzero exit are fixed-category failures;
never print raw stderr, error objects, command arguments, paths or environment.

## Closed observations and comparison

Each parsed observation contains exactly `schema=1`,
`kind=macos-inherited-nofile`, `chain=shell|node24`, and `soft`/`hard` strings.
Normalize only `unlimited` to `infinity`. Otherwise accept canonical nonnegative
decimal strings (`0` or nonzero digit followed by digits), at most 20 digits.
Unsupported spellings, empty values, duplicate keys, additional output,
noncanonical decimals or soft > hard fail. Explicit infinity ordering and BigInt
comparisons preserve exact limits without Number rounding. Limits are never
changed.

The action emits exactly one closed successful JSON summary:
`schema=1`, `kind=macos-inherited-nofile-comparison`, `qualification=false`,
`ceiling=65536`, `observations` containing exactly shell then node24,
`soft_changed`, `shell_exceeds_ceiling`, `node24_exceeds_ceiling`, and
`provenance`. The four provenance strings are `macos_version` from
`sw_vers -productVersion` in the shell step, `image_version` from `ImageVersion`,
`image_os` from `ImageOS`, and `node_version` from `process.version`. Require
finite closed-character strings (version digits/dots, image ASCII alphanumerics,
dots/underscores/hyphens, and exact v24 numeric semantic version), never an
arbitrary environment dump. Missing/malformed provenance is an explicit failure.
The action uses environment references for inputs, without shell interpolation.
Normal GitHub run/job/step metadata supplies source/run provenance. No optional
sysctl probes are needed.

A numeric difference is an observation, not a failed job. Missing baseline
outputs, malformed child output or any measurement failure prevent successful
summary emission. Default step dependencies ensure a failed baseline cannot
silently skip into a successful comparison. A sanitized evidence document may
later copy actual observations, exact run/attempt/source and limitations.

## Validation and actual reviews

Before code, obtain an actual independent Claude Code review in
`--permission-mode auto` of this design and the cited existing launch contracts;
incorporate useful findings and repeat if blocking findings remain. Record the
review metadata/verdict without inventing approval. The implementation is a
separate bounded increment after approval.

Focused tests must cover decimal and infinity normalization, exact field/type
validation, unknown fields, missing baseline values/duplicate child keys, malformed and
oversize values, soft greater than hard, and exact 65,536/65,537 comparisons.
Test actual action exec arguments against the original supervisor inheritance shape,
signal/nonzero/spawn/timeout failure handling, and that any failure prevents a
successful summary. Static workflow tests require manual-only trigger, one
`macos-15` job, five-minute bound, read-only permissions, pinned checkout, local
`node24` action, identical fixed shell program, no external dependencies/secrets,
and no limit-changing command or product execution. Local Node tests validate
parsing and ownership; they cannot establish macOS inherited limits.

Obtain actual scoped Claude auto code review of all new workflow/action/tests,
including failure paths. Run affected Node tests, workflow parsing/policy checks,
format/diff checks and inspect the exact intended diff. Record code approval and
publish the reviewed increment through the coordinator before a single manual
run. No candidate or installed matrix rerun is part of this phase.

## Exit criteria and interpretation

The diagnostic phase exits after one actual manually dispatched macOS run has
completed with both observations and the validated closed comparison, or an
explicit accurately recorded failure. Capture the exact workflow source SHA,
run ID/attempt, job conclusion, measurements and review/test evidence. A failed
probe is not evidence that the limits are equal or that the engine failed.

If shell soft <=65,536 and Node24 soft >65,536, the result supports an inherited
bootstrap incompatibility hypothesis for the original launcher chain. If Node24 soft is at or below
65,536, this sample does not support the inherited-limit hypothesis and the
engine-open error remains unknown. If shell soft exceeds the
ceiling, it contradicts the passing historical cold run under this hypothesis;
runner-image drift or another confounder remains possible. Record the single
sample image provenance rather than treating it as the historical runner state.
Other combinations must be reported literally. The probe checks only the soft-limit gate, not an already-open descriptor at or
above 65,536 or other bootstrap rejection paths. None proves the original engine
failure or validates streaming on macOS; no product fix has been exercised.

Only a separate contingent phase may propose surfacing the actual engine-open
error/readiness or measuring limits inside the real daemon, with its own scoped
design and code review. Do not alter engine caps, silently lower limits, change
launch behavior, relax qualification, or rerun expensive workloads based solely
on this experiment.
