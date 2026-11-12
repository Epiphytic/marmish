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
