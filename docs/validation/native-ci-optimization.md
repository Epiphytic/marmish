# Native CI optimization: baseline and capacity

Measured on 2026-11-12 for Beads `marmish-poz`. Optimization validation is
**pending**: the observations below describe the existing workflow, not a
demonstrated improvement. No projected runtime is presented as a result.

## Existing successful baseline

The successful [candidate run 43174030584, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/43174030584)
qualified source `05bec8b2b4c4ea0a161a1adbc494b0d180a92ba8` in **68m 13s** overall.
The following measurements sum Cargo's reported compile/Clippy durations and
test-harness execution durations from that run. These are different kinds of
work; neither sum is an individual job's elapsed duration. Setup, packaging,
uploads and other wrapper overhead are excluded from these sums.

| Platform | Compile and Clippy total | Test execution total |
|---|---:|---:|
| Linux AMD64 | 1,242.70 s (20m 42.70s) | 2,361.92 s (39m 21.92s) |
| Linux ARM64 | 1,474.47 s (24m 34.47s) | 2,164.22 s (36m 4.22s) |
| macOS ARM64 | 2,450 s (40m 50s) | 1,481.56 s (24m 41.56s) |

The existing compiler phase used three platform jobs concurrently, including
one macOS job. Later quickstart and provider validation actually overlapped
**six jobs, including two macOS jobs**, at `2026-11-12T08:23:32Z`. This establishes
observed usable concurrency for that run, not the organization's maximum.

## Runner capacity and cache budget

The organization API reports **GitHub Team**, and Marmish is public. Completed
job metadata identifies runner group `GitHub Actions`; the workflow's
`ubuntu-24.04`, `ubuntu-24.04-arm` and `macos-15` labels therefore use standard
GitHub-hosted runners. GitHub documents a Team default of **60 concurrent
standard hosted jobs, at most five macOS jobs**; the macOS ceiling is shared
with larger hosted runners. These are documented defaults, not a verified
custom Epiphytic limit. Organization runner, runner-group and hosted-runner
inventory APIs returned HTTP 403, so this audit cannot establish custom limits
or live organization-wide runner inventory. [GitHub Actions limits](https://docs.github.com/en/actions/reference/limits)

For public repositories, these Linux runners provide four CPUs and 16 GB RAM
per VM, while the ARM64 macOS runner provides three CPUs and 7 GB RAM. Separate
jobs receive separate VMs; simultaneous independent Cargo builds inside one
macOS VM would contend for its smaller memory allocation. [Hosted runner specifications](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)

Epiphytic's separate self-hosted configuration, inspected at commit
`3b3f12eb11b33888f9f4f0b5f32c6788dc105595`, documents **two execution slots**:
laptop macOS and Linux ARM64 guests share one slot, and Bel Linux AMD64 has one
independent slot. The example's 20 ready listeners are not 20 concurrent jobs.
This is configuration evidence, not confirmation of live availability or
Marmish access, and it does not describe the hosted VMs used by this baseline.
[Runner policy](https://github.com/Epiphytic/.github/blob/3b3f12eb11b33888f9f4f0b5f32c6788dc105595/scripts/runners/README.md),
[laptop configuration](https://github.com/Epiphytic/.github/blob/3b3f12eb11b33888f9f4f0b5f32c6788dc105595/scripts/runners/config.laptop.example.json),
[Bel configuration](https://github.com/Epiphytic/.github/blob/3b3f12eb11b33888f9f4f0b5f32c6788dc105595/scripts/runners/config.bel-container.example.json).

A read-only snapshot at `2026-11-12T23:26:44Z` found no in-progress Actions runs
across 55 accessible organization repositories. It excludes inaccessible
repositories and queued-only runs, and is not a capacity reservation.

Marmish's cache usage and inventory APIs reported **zero caches and zero stored
bytes**. GitHub's documented default cache budget is **10 GB per repository**;
an account-specific override was not verified. Retained cache sizes and actual
restore/save time need measurement before claiming a warm-build improvement.
Six independent platform/lane caches must share that budget, so retaining full
build trees without measurement risks eviction churn. [Cache limits](https://docs.github.com/en/actions/reference/limits)

The capacity audit supports retaining three compiler jobs/one macOS initially,
or bounding a split to six jobs/two macOS while serializing native candidate
workflows. The latter matches the observed overall peak but sustains compiler
load longer; it leaves three macOS slots under the documented default for
other organization work. Cold and warm optimization results remain pending.

Existing legacy `native-validation.yml` and `release.yml` cache writers were
also inspected. Both pin `actions/cache` to
`0057852bfaa89a56745cba8c7296529d2fc39830` and retain whole target/toolchain
paths under separate `native-…` and `hosted-distribution-…` namespaces. They
cannot accidentally restore the new unique prefix, but they compete for the
same repository storage allowance if dispatched. No entries from either existed
in the audited zero-cache snapshot; they were neither deleted nor expanded.

## Initial split candidate and cache fallback

[Candidate 43332758455, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/43332758455)
built source `c57f8a9f1d2121e2dfb68bdfcf72337c38516fff`. All 14 jobs passed in
**53m24s**, versus 68m13s baseline (22% less elapsed time). Peak concurrency was
six jobs including two macOS jobs, with build queues of 3–5 seconds. Aggregate
job time was 13,504 seconds. This is one observation, not a statistical benchmark.
All target test totals matched the baseline: Linux platforms each executed
1,762 passing tests with eight ignored, and macOS 1,789 with eight ignored.
These totals include subprocess executions, not unique test names.

The Linux AMD64 installed-tools critical lane took 3,124 seconds, including
2,362 seconds for its test command; harness execution alone was 2,023.37 seconds.
Its Clippy and release build took 94 and 629 seconds. No test, compiler or
packaging failure occurred; this run's longer test execution explains the
critical path. Supported APIs did not expose the active job log until completion.

Six retained archives totaled **7,538,607,536 bytes**, above the planned 7 GB
budget. The approved fallback changes installed-tools to download-only caches
and advances the cache epoch to v2. The initial run demonstrates passing cold
qualification, but is not acceptance of the final storage policy. A fresh
cold/warm pair with the smaller policy remains required. Only the six retired
v1 entries owned by this task may be removed; unrelated caches stay intact.

## Final-policy cold qualification

[Candidate 43340461715, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/43340461715)
qualified source `559415fec078c41d6057159d8e485fe9a0930e70` using the v2 cache
policy. All **14 jobs passed in 36m43s** (2,203 seconds), with 12,145 aggregate
job seconds, peak six jobs/two macOS jobs and unchanged target test totals.
All six cache contexts recorded an exact miss and `CARGO_INCREMENTAL=0`.
The read-only candidate inspector passed the complete **15-artifact/10-report**
inventory, including installed default/explicit quickstart and actual provider
qualification on all three platforms.

Six retained v2 archives total **4,687,620,261 bytes**, below the 7 GB budget.
Product archives are 1,294,728,875 bytes (AMD64), 1,289,434,789 bytes (ARM64)
and 1,185,265,417 bytes (macOS); installed download-only archives are
306,153,586, 305,909,795 and 306,127,799 bytes respectively. Exactly the six
task-owned retired v1 entries were removed after their IDs and sizes were
recorded; no unrelated entries were deleted.

The source following this documentation-only update is the intended warm
candidate. Runtime code, workflows, source-digest inputs, Cargo manifests and
lockfiles are unchanged. Exact warm hits, fresh embedded source stamps and the
nine directed installed streaming cells remain required before acceptance.
