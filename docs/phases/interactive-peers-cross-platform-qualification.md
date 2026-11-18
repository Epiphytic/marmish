# Interactive Terminal and Peer cross-platform qualification

## Scope and execution boundary

Keep normal CI cheap. Qualify one reviewed, immutable source commit separately
on Linux x64 (`bel-ci`), Linux arm64 and macOS arm64 (`laptop-ci`). Use the
`epiphytic-ci` self-hosted group only. The two laptop labels share a physical
slot. Treat that configuration from [the existing runner plan](self-hosted-ci.md)
as documented capacity until the administrator checks live settings.

Use a dedicated, nonpublishing workflow, `.github/workflows/interactive-qualification.yml`.
A maintainer pushes the full reviewed commit, including the exact workflow
file with an independent review, to a dedicated protected
`refs/heads/trusted/interactive-qualification` branch in `Epiphytic/marmish`.
Initial registration uses a literal push branch filter and a canonical
repository, event and exact ref job condition. The protected branch itself
contains the workflow, so the initial push works before workflow registration
on the default branch. Before any candidate subprocess, pinned checkout with
`persist-credentials: false` checks out full `github.sha`; a step verifies
local HEAD and `github.sha` match, asserts consistency with push `after`,
and rejects deletion, force pushes and non-full SHAs. The operator compares
the reviewed full SHA with GitHub run metadata and report provenance before
accepting results. Candidate jobs use `permissions: {contents: read}`;
no OIDC or signing permissions, runtime secrets or downstream deploy job.
The workflow can also accept a manual dispatch for the same protected branch
after the default branch has the new workflow. If that optional trigger is
implemented, require an expected full SHA matching the dispatch snapshot and
HEAD. Existing-run reruns retain their original commit. [GitHub workflow events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch)

This route is a proposed scoped exception to the existing trusted master-only
rule. It leaves that rule in force for all other workflows. A canonical
repository push and literal branch filter limit what triggers this workflow.
The trust decision is who can publish executable source to the selected ref
or send other jobs to the persistent pool. Prefer a runner allowlist of
`Epiphytic/marmish/.github/workflows/interactive-qualification.yml@refs/heads/trusted/interactive-qualification`
and enforce ref creation, update and deletion by approved maintainers with
no force pushes. This is one way to prove the required boundary. An
equivalent existing repo/org policy is acceptable when a repo or pool
administrator verifies that only reviewed source enters the candidate
route and external PRs cannot reach the pool. Keep all existing trusted
entries and do not loosen runner access. The candidate route does not
allow `pull_request`, `pull_request_target`, `workflow_run` or arbitrary-ref
`repository_dispatch` events. [Runner group access](https://docs.github.com/en/enterprise-cloud@latest/actions/how-tos/manage-runners/self-hosted-runners/manage-access) · [Ruleset restrictions](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)

Record who verified admission, when, and which effective policy permits
the candidate SHA and excludes untrusted workflow jobs. The read-only
GitHub check found no candidate ref, an empty repository rulesets response,
HTTP 403 for organizational runner policy and `all_external_contributors`
for fork approval. Do not approve external PR runs for this pool. The
fork approval setting is part of existing admission policy. It cannot by
itself establish the effective runner-group boundary or authorize an
exception to the master-only rule. Confirm admission before the first push
to this ref because that push starts the workflow. Authorized operators can
run the same-SHA private local harness on hosts they already administer
while the GitHub route is unconfirmed. It runs the checks below with
the same provenance and cleanup requirements without scheduling workflow
jobs or provisioning hosts. Report it as a local run. [Fork approval settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository)

The existing GitHub-registered “Installed three-platform Marmot
interoperability” and “Installed streaming and transfer qualification”
runs are master-only. They use published or separately produced payloads,
GitHub-hosted coordination or execution, and prior cohort assumptions. The
former waits on three concurrently running server jobs. Reuse vetted
packaging, launcher checks and reporting logic where compatible. Do not
dispatch them to prove this branch.

## Runner budget and regular CI

Keep `interactive-core.yml` limited to a few exact deterministic suites
for Terminal handshake/framing, takeover accounting and Peer authenticated
wire, authorization and persistence. Retain representative existing core
regressions. Resolve selectors against the integrated tree and fail if a
selector matches zero. No ignored/live, installer, benchmark, cross-platform
matrix, or external PR jobs in this core workflow. Validate dispatch/push
conditions and workflow-policy mutations offline before enabling the new
route. Include wrong ref, fork repo, an added `pull_request` trigger,
missing `persist-credentials: false` and `max-parallel` above one.

Qualification uses a single three-target matrix with `max-parallel: 1`,
`fail-fast: false` and workflow concurrency group
`marmish-interactive-qualification` with `cancel-in-progress: false`.
The group differs from the core and legacy groups. Before the push or
dispatch, the operator checks queued/in-progress jobs in this repository
and any other visible pool consumers with GitHub's CLI/API, and coordinates
a quiet window with the pool operator if visibility is incomplete. Do not
queue overlapping qualification or benchmark runs. Workflow concurrency
does not reserve the host across other workflow groups or repositories.
Start a five-minute admission budget for the first runnable job and again
when each previous serial job ends. If a runnable job does not start,
cancel the run and report that platform, classifying visible contention
separately from unverified runner availability. Keep the host's existing
900-second admission safety limit. The operator's shorter qualification
budget does not change it.

No hosted fallback, privileged namespace setup, host provisioning or
Keychain changes. First probe the resident tmux and report its exact version
and resolved absolute executable path trust result. Require tmux 3.3 or
newer and the backend's ownership/permission checks for the executable and
its ancestors. Missing or incompatible tmux blocks that target. Check native
architecture, pinned tools and disk in the same preflight. Use a fresh per-attempt workspace,
private prefix and Cargo target directory with no shared PR or core build
cache. A separate administrator-provisioned source download cache may be
used with Cargo lock verification. Bound the job to 90 minutes and the
runtime suite to 20 minutes initially. Use a two-worker Cargo build and
serial native tests. Clean up on success, timeout, signal and failure.
Preserve the runner's execution ID in descendants without logging it and
prove owned process groups and sockets close. [GitHub workflow concurrency](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#concurrency)

## Required private baseline on each target

1. From the same locked SHA, build release CLI, daemon and worker. Record
   pinned toolchain, target and hashes. Use existing native release package
   builder and sign macOS test binaries ad hoc before hashing, with the
   configured deployment target. A test-only renderer substitutes exactly
   the version token and the three manifest digest tokens in the unchanged
   native installer template. Validate version and lowercase hex digests,
   assert no unresolved markers, use the real payload manifest SHA-256 for
   the installed target and explicit fixture-only placeholder digests for the
   untested targets. This mode only accepts a single local target directory
   and does not exercise the release renderer's three-target provenance
   check. Run the rendered installer with `--asset-dir`
   through a controlling PTY in a fresh private prefix and file-backed
   identity state. Exercise quickstart accept/default paths and fresh
   client/server profiles. Check manifest/digests, installed launcher targets
   and `release-info` commit/target/version. Alter an input file without
   changing its size to require a payload digest refusal, then prove that
   installed files, launchers and profile state are unchanged. The published
   installer and already installed binaries are outside this test.
2. Start the exact installed daemon with `--foreground` in a disposable
   profile, validate typed readiness over native local control, exercise
   startup failure and stop/reap the daemon and its worker. Exercise actual
   installed CLI terminal PTY and termios restoration where the native
   fixture supplies a socket, and identify any test-owned daemon substitute
   in the report. Compare bytes and resize via actual PTY observations.
3. Run the direct private loopback WSS native runtime and selection fixtures
   using same-SHA source and the exact installed worker where the fixture
   supports it. Report the actual components for every case. Authenticate
   command output before, during and after Terminal traffic. Check a
   writer's binary-safe input/output, resize and named resume, one observer
   observing with input/resize refused, observer limit, takeover retirement
   and old-link close, old/forged ticket refusal and absence of later retired
   input, persistence of the shell and joined cleanup. Check busy or refused
   admission with terminal restoration. Use bounded, targeted tests and
   report expected and executed case counts. An internal harness may inject
   generated loopback TLS trust into test-only Runtime and carrier paths.
   Production TLS uses built-in public roots and does not accept that
   injection. Do not modify production trust or routing for qualification.
4. Run separate Peer cases for invitation, manual pending/approval with no
   access before approval, opt-in acceptance, client listing, redacted
   status visibility, configured heartbeat/last seen and peer-specific
   command grants. Use disposable identities. Test metadata identifier
   resolution and canonical npub verification, including an ambiguous
   lookup and mismatched approval. Record whether each assertion traversed
   the same-SHA test fixture or actual installed processes. If a case covers
   state only or is not mounted in the production branch, record its
   scope and leave the production-path requirement open.

## Optional installed-to-installed transport gate

A stock installed CLI and daemon can be tested end to end when a separately
approved private direct WSS endpoint is already available to all runners.
Require hostname and certificate chain compatible with production roots,
the NIP-42/relay behavior the integrated candidate needs, isolated disposable
test identities and resettable test scope. Use an approved endpoint rather
than an arbitrary URL from a workflow input, with Tor off. Keep service
TLS private keys off runners. There is no known approved private endpoint
with these properties in the existing checked workflows. Historical
published-command and transfer runs used explicit White Noise relays and
confirm prior permission/use for those tested workloads. They do not
establish private scope, current service capabilities or a budget for a
new Terminal and Peer matrix. Do not assume those relays for this gate or
stand up a new service with this change.

At serial capacity, the baseline proves same-platform client/server
coverage independently on all three targets. A cross-host 3×3 matrix
requires an approved private WSS endpoint and a safe harness that keeps
server jobs alive without retaining a shared runner slot. The existing
three-way simultaneous artifact barrier cannot do this. A possible
separate harness places one short-lived server at a time on an approved
fixture host outside the runner pool, using the same-SHA platform build
and an independently verified launch and cleanup budget. Then serial
clients from the three targets run against it. Repeat for each server
target. If those host slots are unavailable, report the cross-host matrix
as unrun. Exchanging build artifacts or connecting the two laptop OS
jobs serially cannot establish live interoperability. No long-lived
detached server may outlive a runner job to simulate the matrix.

## Evidence and approval

Emit bounded sanitized JSON per target, including source/toolchain/target,
checksums, test counts/results, elapsed time and exact transport and
component coverage. Report baseline, stock private WSS, and cross-host
matrix independently as pass, fail, blocked or unrun. Missing targets
and unfulfilled baseline case counts fail the qualification. A green
baseline alone gives no stock-WSS or cross-host claim. Each report binds to its exact source SHA. Record whether all required
selection fixtures are present at that SHA. A different commit requires a
new run, even when earlier or later evidence passes. Upload only allowlisted
reports using a pinned artifact action. Exclude disposable npubs, keys, profiles, databases,
terminal transcripts, host details and raw logs. Record exact job IDs and
run attempt. A Linux runner-native build does not establish the published
glibc floor, ad hoc signing does not establish notarization or Keychain
behavior, and local installer assets do not establish the public mirror
or HTTPS fetch. No source tags or GitHub releases are created here.

Benchmarks are manually invoked in the same-SHA local harness after the
functional run, outside the regular or qualification workflow. Any future
separate manual benchmark workflow requires the same trusted ref and
runner admission controls and an exclusive pool window. Use the same source
SHA and pinned, reported environment. Obtain at least 20 samples for p50/p95
first echo and resume, distinguish cold and warm startup and measure fixed
payload transfer rates. Record sample counts, transport, runner and
timeouts, and label quantiles from these small samples indicative. Keep
local and external-relay results separate and avoid thresholds inferred
from a single shared-runner result.
