# Self-hosted CI: Phase A code review

Actual Claude Code auto-mode review, model claude-opus-5-5, session 464193d5-8ac6-4cde-9fac-79e5a5d0bac5, on 2026-11-13.

**Verdict: APPROVE for Phase A, with no blockers.** The code can be published and the manual inventory dispatched once it is on master. Phase B and Phase C are not reviewed or approved here.

This was an actual Claude Code review on model `claude-opus-5-5`. I read the code and ran local checks only. I made no edits, did not dispatch anything, ran no real Docker or relay services, and printed no credentials.

**Scope reviewed:** `.github/workflows/self-hosted-preflight.yml`, `scripts/self-hosted-preflight.sh`, `tests/ci/test_self_hosted_preflight.py`, the CI direction paragraph added to `docs/agent-spec.md`, `docs/phases/self-hosted-ci.md` (the amended Phase A text) and `docs/reviews/self-hosted-ci-design.md`.

## Tests I ran
| Check | Result |
|---|---|
| `python3 -m unittest discover -s tests/ci -v` | 9/9 pass. This covers the 22 policy mutations (20 replacements plus 2 added triggers), the 6 identity refusals, version sanitization, and the stubbed Docker ownership and cleanup cases. |
| `tools/test_marmish_release_automation_workflow` | 14/14 pass |
| `sh -n` on the script | pass |
| `git diff --cached --check` | pass |
| actionlint / shellcheck | **Not run:** neither tool is installed here, so I could not reproduce your actionlint pass. |

## The key risks you listed
- **Name collision:** an existing container with the same name causes a hard failure before the cleanup marker is written, so cleanup prints `not_attempted` and never touches it (`:91-96`, and the test checks Docker is never called). OK.
- **Proving cleanup worked:** cleanup removes only that exact name, then lists containers again. If the engine can't answer, it fails with `cleanup_uncertain`. It prints `absent` only when the engine confirms nothing is left. Unknown is never reported as absent. OK.
- **Step outputs when the probe fails:** `docker_ready=true` is written before `pull` and `run`. Every path after that ends with exit 0, so a failure or timeout during pull or run still triggers cleanup. OK; one residual is noted in N4.
- **Rootless only, no root:** the check needs the exact `"name=rootless"` security option; rootful Docker is a hard failure. The container runs with `--user 65534`, `--cap-drop ALL`, `no-new-privileges`, `--network none`, `--read-only`, `--rm` and an internal `timeout 20`. The job refuses UID 0. The script contains no sudo. OK.
- **No raw output in logs:**
  - Every subprocess has its output discarded, or reduced to a version number by a numeric-only pattern with a 64-character cap.
  - The test with hostile version strings checks that every output line has the form `key=value` using only safe characters.
  - `EPIPHYTIC_EXECUTION_ID` is only checked for presence and is never printed.
  - `runner.environment` and the matrix values reach the script through `env:`, so they are never pasted into the script text.
  - OK.
- The amended design items are all in the code: the job is limited to this repository, master, and manual dispatch; checkout is pinned with no saved credentials; matrix jobs run one at a time and don't cancel each other; the concurrency group is dedicated; the job times out at 10 minutes; cleanup always runs. The five-minute queue rule and the check for other pool work before dispatch are steps for the operator, so they are not in the code.
- Nothing touches runtime tests, Cargo, versions, the installer or release gates. The `agent-spec.md` paragraph says the work is only a prerequisite inventory and claims no migration.
- I accept the author's reason for leaving out the bubblewrap probe (v0.2 removed that prerequisite).

## Findings (none block)
1. **Name race (low).** If a container with the same name appears between the check and `docker run`, the run fails on the name clash. Cleanup would then remove that container. Only a process using the same user's Docker engine could cause this, and the name is unique per run, attempt and platform. Optional fix: add an owner `--label` to the container and have cleanup filter on that label.
2. **Policy validator gap (low).** It does not check how many jobs there are or how many `runs-on:` lines. A second job using `ubuntu-latest` or `macos-14` would pass, because only `ubuntu-24.04` and `macos-15` are forbidden. Suggest requiring exactly one job and one `runs-on:`.
3. **Ambiguous Docker result (low).** `docker_rootless=missing` means either "no rootless engine" or "the engine can't be reached from the runner environment", for example a socket or context that isn't set. Read the dispatch results with that in mind.
4. **Probe killed before outputs are processed (low).** If the runner does not keep step outputs from a probe that was killed by its timeout, cleanup reports `not_attempted`. That is still honest, not a false `absent`, and the container is limited by `--rm` and `timeout 20`. Treat `not_attempted` after a probe timeout as "unknown" in the evidence.
5. **Wording mismatch (cosmetic).** The design says `missing:<tool>`, but the code prints `<tool>=missing`. Align one with the other.
6. **Image digest not checked by me.** Without network access I couldn't confirm that digest `2198e4c6…` is the multi-architecture image index. The ARM64 container check (`uname -m` against `aarch64`) will catch a wrong digest on the first dispatch.

The 9 tests passing and these results prove nothing about live runners. They only become migration evidence after the real dispatch, under the five-minute cancellation rule.

## Author disposition

N2 incorporated: require exactly the inventory job and one runner selection, with two additional hosted-job mutations. N5 aligned capability notation. N1 remains a low-risk same-user race with a unique run/attempt/platform name; no other workflow is authorized to create that name. Phase B must use stronger container ownership for long-lived build containers. N3 is interpreted only as missing capability. N4 means a timed-out probe cannot qualify cleanup from a missing output marker; retain that uncertainty and the failed job, never report successful absence. Docker image digest was verified against the official registry index by the author before implementation. Author actionlint used the private installed binary; reviewer correctly did not claim to run it.


## Final follow-up review

**Final verdict: APPROVE for Phase A, with no blockers.** You can push to master and run the manual inventory dispatch. This was an actual Claude Code review on model `claude-opus-5-5`. I only read the code and ran local tests: no edits, no dispatch, no real Docker.

## What I checked
- **Scope:** 7 files in total: 6 with staged or unstaged changes, plus the new untracked `docs/reviews/self-hosted-ci-code.md`.
  - `.beads/issues.jsonl` is also untracked. It is the passive Beads export, so keep it out of this commit.
- **Workflow and probe:** both files are staged with no unstaged changes. They are the same as what I approved last time.
- **N2, fixed (`tests/ci/test_self_hosted_preflight.py`):**
  - The validator now requires `inventory` to be the only job and exactly one `runs-on:` line at job level.
  - Two new mutation cases append a hosted job (`ubuntu-latest`, `macos-14`), and both are rejected.
  - The test still runs the checked-in workflow, so the checks apply to the real file.
- **N5, fixed:** the design now says `<tool>=missing`, which matches what the probe prints.
- **`docs/reviews/self-hosted-ci-code.md`:** it reproduces my review faithfully. The author's notes on N1, N3, N4 and N6 are reasonable and claim nothing beyond the evidence:
  - N1: Phase B must use stronger container ownership.
  - N4: a probe that times out keeps both the failed job and the uncertainty.
  - N6: the author checked the image digest against the registry.

## Tests (run again now)
| Check | Result |
|---|---|
| `python3 -m unittest discover -s tests/ci` | 9/9 pass (now 24 mutation cases) |
| `tools/test_marmish_release_automation_workflow` | 14/14 pass |
| `git diff --check` and `git diff --cached --check` | pass |
| actionlint / shellcheck | Not run: neither is installed here. That is the author's evidence, not mine. |

## Non-blocking notes
1. **Inline job could slip past the validator (very low).** A second job written on one line, such as `  other: {runs-on: ubuntu-latest, ...}`, would not match the `^  name:$` pattern or the indented `runs-on:` check. Workflows are reviewed by hand, and a hosted job would also fail at the probe's `runner.environment` check, so this is acceptable. If you want to close it, also reject any 2-space-indented key under `jobs:` other than `inventory:`.
2. **Mutation count in the review record (cosmetic).** The record still says "22 policy mutations"; it is 24 now. Optionally add a line to the author disposition.

## On the dispatch
- The pool check you described is a snapshot: nothing is in progress, 28 runs are queued, but none has an unfinished job eligible for the pool. That only says what the token can see right now. It does not reserve capacity.
- Apply the five-minute cancellation rule to each matrix job when it becomes eligible.
- All nine pairs stay pending until capacity exists. Nothing here counts as migration evidence until the real dispatch produces sanitized reports from all three native jobs.

## Post-dispatch evidence review

Actual Claude Code auto-mode follow-up, same session and `claude-opus-5-5`,
2026-11-13: **APPROVE** `docs/validation/self-hosted-ci-preflight.md` as an
accurate report, with no blocking factual issues. The reviewer read the private
sanitized AMD64 capability report and independently queried run/job metadata
for run 43384760030. It confirmed incomplete Phase A acceptance, ARM64 cancelled
before checkout, macOS unexecuted, and no cleanup or migration claim inferred
from cancellation. All reported AMD64 capability values matched the sanitized
report. The reviewer did not dispatch, cancel, edit, or fetch raw logs.

The log endpoint's HTTP 404 responses, dual-source push, Iris timeout, author
actionlint result and pool snapshot remain author evidence; the reviewer did
not independently reproduce those facts. Approval of the report does not
approve Phase B or complete Phase A.
