# Self-hosted CI migration

Beads: `marmish-5z3`. The user's correction supersedes the previous optimization's
choice of GitHub-hosted VMs. Keep its measurements as historical evidence; do not
relabel them as self-hosted results. No release, version bump, public installer
change or publisher enablement belongs to this work.

## Capacity and boundaries

The Epiphytic runner documentation provides Linux X64 `bel-ci`, and Linux ARM64
and macOS ARM64 `laptop-ci`. Both laptop guests share one FIFO execution slot;
Bel has one independent slot. Twenty ready listeners are not twenty execution
slots. Admission-hook waits have a 900-second bound. Workers have no host sudo.
Disks persist, while job descendants must be proven stopped before admission is
released. Do not bypass admission by leaving a daemon behind.

Runner configuration was inspected at upstream commit
`3b3f12eb11b33888f9f4f0b5f32c6788dc105595`:
[runner policy](https://github.com/Epiphytic/.github/blob/3b3f12eb11b33888f9f4f0b5f32c6788dc105595/scripts/runners/README.md).
The official Docker registry's Ubuntu24.04 multiarch manifest was retrieved on
2026-11-13 as
`sha256:2198e4c65076e9cb0b3d695219df667c0e3b0072da64ca3c2648aca17dcecaf5`;
the probe pins that index rather than a moving tag.

The org runner-inventory API returns HTTP 403 with the current credential.
Configuration is evidence of the intended pool, not proof of live availability.
Use exact native labels, trusted master-only workflows, no persisted checkout
credentials and no GitHub-hosted fallback. Foreign pull-request code must never
execute on the trusted pool. Preserve `EPIPHYTIC_EXECUTION_ID` in child processes.

## Phase A: prove repository eligibility and installed prerequisites

Add a manual, nonpublishing `self-hosted-preflight.yml`. Restrict its single job
to the canonical repository, master ref and workflow_dispatch. Serialize a
three-platform matrix with max-parallel=1 and fail-fast=false, using the exact
self-hosted OS/architecture/pool labels and `epiphytic-ci` group. Serialize whole
runs too. Timeout each job after ten minutes; do not enqueue competing builds.
Use the pinned checkout action with persist-credentials=false.

Run a small POSIX-shell probe that verifies the actual OS/architecture, nonroot
UID, expected runner identity markers, Python >=3.11, Node >=20, Git, curl, C
compiler, make, Perl and archive tools. Report bounded tool versions and glibc
or macOS/SDK versions. On Linux, require a rootless Docker engine and prove an
owned `--rm` container starts, using only a digest-pinned Ubuntu24.04 image
after verifying its registry manifest. Record provider prerequisite availability
without claiming provider functionality or installing host packages. On macOS,
verify Xcode selection, codesign/security and available SDK. Never log env dumps,
host paths, usernames, runner tokens, private configs or keychain contents.

No Cargo builds or live relay servers in this phase. Its acceptance is all three
native jobs actually run, no GitHub-hosted VM, no privileged host operation and
sanitized capability evidence. Add meaningful workflow-policy mutation tests
for native labels, dispatch/ref/repository gate, sequential admission, pinned
checkout and no credentials. Test the probe's failure behavior where practical.
Missing prerequisites are capability facts (`<tool>=missing`), not a successful
build prerequisite check. Phase A succeeds only as an inventory when all three
jobs execute natively and sanitized reports are available; policy/identity
failures (including root, wrong OS/architecture or non-self-hosted execution)
remain hard failures. Version output uses a numeric allowlist, never raw tool
output. The identity markers are `runner.environment == self-hosted` and presence
of `EPIPHYTIC_EXECUTION_ID`, without printing its value.

Use only an already running rootless Docker engine, never start a daemon.
Assign a unique run/attempt/platform container name, use `--rm`, an internal
short timeout and an always-run cleanup step that removes only that name and
proves absence. Retained downloaded image layers are an explicit disk remainder.
Before dispatch, check for other queued/active pool work visible to this token.
For each matrix job, measure the five-minute availability budget from when its
preceding serial job finishes (first job: dispatch). Cancel the whole run if an
eligible job has no runner by then, and report the unavailable platform; job
timeouts exclude queue time. Never substitute pools. Obtain actual Claude design approval before code,
and integrated auto-mode review before commit/push and dispatch.

## Phase B: migrate native qualification without changing its guarantees

After Phase A evidence, review a detailed implementation amendment specifying
the measured prerequisites and exact job DAG. Preserve production/tool feature
isolation, immutable source stamps, fourteen-job/fifteen-artifact/ten-report
candidate contracts (or explicitly version their reviewed replacements), all
tests, strict Clippy, signing, ABI validation, installed setup and real private
keychain-provider checks. Avoid more than one queued long laptop job because
the admission timeout is shorter than a build. Account for other workflows.

Bel's newer Ubuntu host cannot supply the declared glibc2.39 build baseline.
Use digest-pinned Ubuntu24.04 rootless build containers with reviewed
prerequisites and nonroot test execution; retain actual binary dependency/ABI
inspection. No host sudo, permission-test skips or invented ABI compatibility.
Separate persistent workspaces/feature lanes, bound Cargo CPU and memory load,
and re-evaluate cache/SDK invalidation against actual self-hosted evidence.
Move trusted coordinator checks too; disable automatic obsolete v0.1 workloads
or make them explicit manual reference checks instead of routing untrusted PRs
onto persistent trusted workers. Inventory remaining hosted jobs and make any
exception explicit. The disabled publisher must remain disabled.

The current macOS installed default-auto provider harness also uses `sudo dscl`
to create/delete an isolated OS account and `sudo launchctl asuser` to run it.
The laptop worker cannot perform that provisioning. A reviewed replacement must
preserve isolation and real default-auto custody semantics, or use separately
provisioned narrow infrastructure. The existing selected-disposable-keychain
unit test is narrower evidence and cannot silently replace that acceptance gate.

## Phase C: live installed interoperability and measurements

The current streaming harness has three all-participant barriers: identities,
server readiness and client completion. It cannot run on two execution slots.
Even a pairwise redesign leaves macOS↔Linux ARM64 blocked because those two
native endpoints need overlapping execution. Detached processes and sequential
store-and-forward are not substitutes for duplex stream/rsync proof.

Keep the full nine-pair gate visibly pending until independent native capacity
or a separately reviewed runner-supported service architecture is available.
An optional pairwise harness could validate seven unique cells on existing
capacity, but must not claim all nine. A clarification about independent ARM64
capacity is pending. Do not consume GitHub-hosted VMs while awaiting it.

After actual self-hosted qualification, publish queue/build/test/package/install
times, peak execution slots, cache bytes and observed memory/CPU limits; compare
equivalent workloads, retaining failures and sample counts. Review evidence with
Claude, push reviewed increments to both source mirrors and record replication
limitations honestly. No phase completes on unexecuted or skipped validation.
