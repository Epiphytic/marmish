# Self-hosted CI: Phase A design review

Actual Claude Code auto-mode review, model `claude-opus-5-5`, session `464193d5-8ac6-4cde-9fac-79e5a5d0bac5`, on 2026-11-13.

**Verdict: conditional APPROVE for Phase A.** The staged plan is sound, and Phase A is safe and worth doing. Before implementation, three short wording changes need to go into the design (B1–B3 below). None of them requires redesign. Once they are in, take this as approval of Phase A only. Phase B and Phase C still need their own detailed approval.

I only read files and ran local read-only checks: `docs/phases/self-hosted-ci.md`, the workflows in `.github/workflows/`, the existing workflow-policy tests (`tools/test_*_workflow.py`), `tools/bootstrap_ci_python.sh` and `docs/phases/platform-validation.md`. I found no Epiphytic runner documentation in local checkouts. The capacity and admission facts below come from the design itself and from runner configuration already used in other workflows. I did not check them against live runners (the org's runner-inventory API returns 403 with the current credential, as the doc says). Nothing was edited or dispatched, and no environment or secret data was printed.

## Overall staged plan
- The order is right: check prerequisites first, then migrate qualification with its guarantees kept, then run live interoperability, which stays pending honestly until capacity exists. Keeping the old GitHub-hosted measurements as history, with no relabelling, is correct.
- The repository already has this pattern: `installed-interop.yml:36-46` and `linux-namespace-diagnostic.yml:11-16` use the same repo/ref/dispatch guard, the `epiphytic-ci` group, the exact native labels and the pinned checkout `11d5960…` with `persist-credentials: false`. `tools/test_linux_namespace_diagnostic_workflow.py` is a ready template for the policy tests. Phase A adds no new trust surface.
- Phase A leaves all runtime tests unchanged, includes no Cargo build and no relay servers, and claims no migration completion. Its preflight results are only evidence about what is installed.

## Blocking clarifications (B1–B3)

**B1. A job that never gets a runner must be cancelled, not just reported.**
- `timeout-minutes` does not include time spent queued. A self-hosted job with no matching runner can wait about 24 hours, and might later grab the shared laptop slot without warning.
- With `max-parallel=1`, jobs 2 and 3 only queue after the job before them finishes, so "five minutes after dispatch" only applies to job 1.
- Wording to add: "if any matrix job has no runner within 5 minutes of becoming eligible, the operator cancels the run and reports that platform as unavailable; no pool substitution."

**B2. Docker container lifecycle and the admission rule.**
- Processes in a rootless Docker container are children of the user's `dockerd`, not of the job. The admission hook's check for leftover job processes cannot see them.
- Wording to add:
  - Phase A only uses a Docker engine that is already running. It never starts `dockerd-rootless` or `systemctl --user` itself.
  - The run uses a unique `--name`, `--rm` and a short timeout, so the container exits on its own.
  - An `if: always()` cleanup step runs `docker rm -f <name>` and checks that the container is gone.
  - The pulled image stays on the persistent disk, and that is recorded as a known remainder.

**B3. Define what a missing prerequisite means for acceptance.**
- At the moment, "require rootless Docker" or "Python ≥3.11" would fail a job. That collides with the acceptance line "all three native jobs actually run".
- Proposed rule: the probe finishes and reports `missing:<tool>` capability facts. Phase A is accepted when all three jobs ran natively and their evidence is sanitized. Any missing prerequisite becomes an explicit input to Phase B, not a quiet pass.
- Policy and identity failures stay hard failures: wrong OS or architecture, running as root, or `runner.environment != 'self-hosted'`.

## Strongly recommended (not blocking)
- **Assert `runner.environment == 'self-hosted'`** and make the policy test check for it. This is the cleanest evidence of "no GitHub-hosted VM". Spell out what "runner identity markers" means: only whether `EPIPHYTIC_EXECUTION_ID` is present plus that field. No runner names or hostnames.
- **Probe Linux user namespaces and bubblewrap** using only commands that change nothing, both on the host and inside the probe container (for example `unshare -Ur true`). `platform-validation.md:68-69` already says rootless Docker alone is not enough, and the current Linux lanes depend on `marmish_ci_bwrap.py`. This is the cheapest way to settle a Phase B risk early.
- **Sanitize by allowlist, not by redaction.** Extract a version number with a regex and print only that. Some outputs contain host paths or usernames:
  - `cc --version` prints `InstalledDir`.
  - `xcode-select -p` prints a path; report the Xcode check as pass/fail plus the SDK version only.
  - `docker info` and `docker context` show the socket path, which includes the username.
  - `security list-keychains` must not be run with its output visible; use `command -v security` or `codesign` instead.
- **Python and Node:** on macOS the host Python may be older than 3.11, which is why `bootstrap_ci_python.sh` exists. Report the host version and whether the bootstrap succeeds as separate facts. The runner ships its own Node for running actions, so make a host Node ≥20 check report-only unless a later phase actually needs it.
- **Concurrency group:** use a dedicated group with `cancel-in-progress: false`. Do not reuse `marmish-native-validation`: GitHub keeps only one pending run per group, so joining it could silently replace a queued qualification run. Before dispatching, check that no other `epiphytic-ci` run is queued.
- **Policy tests should cover:** no `push` or `pull_request` triggers, no `secrets.` or `sudo`, `permissions: contents: read`, `max-parallel: 1`, `fail-fast: false`, the exact label sets, and the always-run cleanup step. Test that the probe fails correctly on a faked wrong architecture, a root UID, and output containing a forbidden path or username.
- **Dispatch needs the workflow on master:** the guard means this branch must be reviewed and merged to master before any dispatch. The plan already puts review before push.

## Limitations for later phases (not approved here)
**Phase B:**
- Cargo builds exceed the 900-second admission wait, and the two laptop guests share one first-come-first-served slot. Spreading 14 jobs over two slots will be slow, and it needs an explicit job DAG, so the laptop is never asked to queue more than one long job.
- Rootless Docker build containers:
  - Capping Cargo CPU and memory needs cgroup v2 delegation, which may not be available.
  - Tests that use namespaces inside a container may need seccomp or user-namespace settings the worker cannot change.
  - The container-lifecycle rule from B2 has to hold for every build.
- The glibc 2.39 baseline through Ubuntu 24.04 containers is reasonable, but it still needs real checks of the binaries' dependencies and ABI.
- The macOS `sudo dscl` / `launchctl asuser` provider harness has no replacement yet. That is a real acceptance gap, and the doc names it correctly.
- `installer-validation.yml` and `installed-coordinator.yml` still trigger on `pull_request`. They must not land on the trusted pool as they are.

**Phase C:** blocked on capacity, as the doc says. A pairwise harness can claim at most seven unique cells, so the nine-pair gate stays visibly pending.

## Author disposition

B1–B3 incorporated before implementation: eligible-job queue budgets and cancellation; named bounded containers with always-run cleanup/absence proof; missing tools reported as inventory facts, policy/identity failures hard failures. Numeric version allowlists, exact environment marker and no host identifiers are required. Phase A uses a separate concurrency group. Namespace/bubblewrap probes are omitted: v0.2 explicitly removed those prerequisites, so they cannot become a migration gate. Python bootstrap is not executed in this inventory; report host capability only. No later-phase approval inferred.
