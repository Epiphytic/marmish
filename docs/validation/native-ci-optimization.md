# Native CI optimization: measurements and capacity

Measured on 2026-11-12–13 for Beads `marmish-poz`. The corrected split passed all 14 qualification jobs and all nine installed
streaming pairs on three platforms at source `9131926f85ce2cacc04805027871bdfe92dafc61`.
Its final observation took 38m10s versus 68m13s baseline, with unchanged test
counts. Hosted image variants broke the original simultaneous-hit and 7 GB
storage criteria; independently reviewed reassessment removed tooling caches
and retained strict product keys. The failed storage target remains explicit.
Results below are observations, not projected or statistically guaranteed
runtimes.

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

At the initial audit, Marmish's cache usage and inventory APIs reported
**zero caches and zero stored bytes**. GitHub's documented default cache budget is **10 GB per repository**;
an account-specific override was not verified. The measured cache sizes and restore/save costs below qualify the final policy;
the initial audit alone did not establish a warm-build improvement.
The initial six platform/lane caches shared that budget; the corrected policy
retains only three product cache writers. Retaining full build trees without
measurement risks eviction churn. [Cache limits](https://docs.github.com/en/actions/reference/limits)

The implemented split is bounded to six build jobs/two macOS per ordinary
candidate run, while serializing ordinary native candidate runs. Caveated runs
retain their separate existing concurrency group; these limits do not cap
other organization workflows. This matches the observed overall peak but sustains compiler
load longer; it leaves three macOS slots under the documented default for
other organization work. Measured candidate peaks remained six jobs/two macOS
jobs; the audit does not reserve organization-wide capacity.

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

## Intermediate download-only policy: cold qualification

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

The documentation-only warm source is
`fe9a2e4d707e1754562bca34db9718601a97f996`. Runtime code, workflows,
source-digest inputs, Cargo manifests and lockfiles are unchanged. The next chronological observations test exact warm hits, fresh embedded
source stamps and nine directed installed streaming cells; the reassessment
below supersedes the original simultaneous-hit criterion.

## Warm qualification and hosted image rollover

[Candidate 43345491416, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/43345491416/attempts/1)
passed all 14 jobs in **36m03s** (2,163 seconds), with 10,585 aggregate job
seconds and peak six jobs/two macOS jobs. The full read-only inspector passed
15 artifacts and 10 closed reports, bound to the new source above. Packaging
probed every product and tool's embedded metadata before hashing; inspection
verified those new source stamps and their immutable product/tool/bootstrap
bindings. Cache reuse did not substitute for any qualification gate.

Five lanes had exact hits. Cold ARM product used image `20261101.172.1`, while
cold installed tooling used `20261111.182.1`; the initial warm run assigned
both lanes `20261101.172.1`. The ARM tools cache correctly missed on that
different image. Its new 305,847,595-byte download archive increased retained
storage to **seven entries/4,993,467,856 bytes**. All six original IDs, keys
and sizes remained unchanged. AMD64 used image `20261101.360.1` and macOS
`20261104.0514.1` in both runs. This is not a single controlled matched-image
timing pair. The independently reviewed image-rollover amendment requires a
repeat with six exact hits and individual key/image/save bindings; it does
not weaken invalidation or delete correct misses.

The initial warm ARM test count appeared two lower because parallel child
harnesses interleaved their `test result:` prefixes. Complete numeric result
tuples were still present. Independent inspection proved the same multiset of
32 product suites, 843 passing executions, five ignored and 2,325 filtered out
in baseline, cold and warm logs. Both affected subprocess helpers also had
explicit parent success lines. The private parser now counts complete numeric
tuples, preserves unknown per-tuple timing/status rather than inventing it,
and sums duration observations separately. Five adversarial parsing cases
passed. Re-extraction confirms unchanged platform totals on every run.

| Observation | Whole candidate | Aggregate job seconds | Peak jobs/macOS |
|---|---:|---:|---:|
| Original baseline, 43174030584/1 | 68m13s | 11,552 | 6 / 2 |
| Initial full-target cache trial, 43332758455/1 | 53m24s | 13,504 | 6 / 2 |
| Intermediate-policy cold, 43340461715/1 | 36m43s | 12,145 | 6 / 2 |
| Initial warm with one image miss, 43345491416/1 | 36m03s | 10,585 | 6 / 2 |
| Second warm with product image miss, 43345491416/2 | 40m18s | 11,659 | 6 / 2 |
| Final bounded repeat, AMD image miss, 43345491416/3 | 52m13s | 11,701 | 6 / 2 |

For these first attempts, whole-candidate time means run creation to the last
job's completion, rather than the later metadata update timestamp. Aggregate
time sums job start-to-completion, excluding time before runner assignment.
Cold elapsed time was 46% lower than baseline with 5% more aggregate job time;
initial warm elapsed was 47% lower with 8% less aggregate job time. The
40-second cold/warm difference is small: installed tooling caches downloads
only, and tests still execute in every lane. Individual hosted workloads vary.
The first split trial's AMD64 installed harness took 2,023.37 seconds versus
1,182.45 baseline, with every test passing; broad test/compile slowdown was
observed, but its underlying CPU/storage cause was not established.


## Lane measurements

Step elapsed seconds include Cargo wrapper overhead. Product lanes run the
production feature set; isolated tooling lanes run the installed-test feature
set. Both execute tests, strict Clippy and release builds on every run.

| Platform / lane | Cold tests | Cold Clippy | Cold release | Cold whole job | Initial warm tests | Warm Clippy | Warm release | Warm whole job |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| AMD64 product | 1,458 | 56 | 382 | 1,939 | 1,679 | 16 | 78 | 1,824 |
| ARM64 product | 1,359 | 63 | 420 | 1,884 | 1,123 | 16 | 83 | 1,265 |
| macOS product | 1,216 | 97 | 694 | 2,112 | 911 | 28 | 161 | 1,194 |
| AMD64 tooling | 1,455 | 59 | 446 | 1,983 | 1,526 | 68 | 425 | 2,043 |
| ARM64 tooling | 1,433 | 79 | 472 | 2,006 | 1,397 | 73 | 448 | 1,938 |
| macOS tooling | 1,150 | 92 | 711 | 2,000 | 1,123 | 128 | 770 | 2,057 |

These compare intermediate-policy cold attempt 43340461715/1 with initial warm
43345491416/1, which includes the documented ARM tooling image miss.
Product dependency reuse substantially reduced Clippy/release time in this
observation. Under this intermediate policy tooling recompiled from cached
downloaded sources; it set the initial warm critical path. The final correction
removes those tooling download caches.
Cold cache restore steps took 1–3 seconds; warm product restores took
24 seconds (AMD64), 18 seconds (ARM64) and 41 seconds (macOS).
Warm tooling restores took 6 seconds (AMD64), 1 second (ARM64 miss) and
7 seconds (macOS). Cold post-job cache cleanup/save took 13/15/49 seconds
for products and 6/8/17 seconds for tooling, in AMD64/ARM64/macOS order.
Warm hit post-job steps took at most one second; the missed ARM save took six.


## Second warm observation

[Candidate 43345491416, attempt 2](https://github.com/Epiphytic/marmish/actions/runs/43345491416/attempts/2)
passed all 14 jobs and complete 15-artifact/10-report inspection in
**40m18s**, measured from its own attempt start `2026-11-13T02:12:00Z`
to last job completion `02:52:18Z`. The original run creation timestamp is
not a rerun start. All platform test counts remain identical to baseline.

Five lanes again hit exactly; ARM product now used image `20261111.182.1`,
correctly missing its original old-image product cache. Its new archive
(cache ID `10452806916`, 1,290,150,377 bytes) increased storage to
**eight entries/6,283,618,233 bytes**. All original six cache IDs, full keys
and sizes remained unchanged. No entries were removed. Each actual restore
was joined to an observed saving run/attempt/job and retained cache ID.

| Platform / lane | Tests, seconds | Clippy | Release | Whole job |
|---|---:|---:|---:|---:|
| ARM64 / product | 1,417 | 65 | 443 | 1,997 |
| AMD64 / product | 2,014 | 19 | 100 | 2,190 |
| ARM64 / installed-tools | 1,360 | 68 | 429 | 1,880 |
| AMD64 / installed-tools | 1,469 | 63 | 403 | 1,958 |
| macOS / product | 855 | 22 | 121 | 1,085 |
| macOS / installed-tools | 1,282 | 131 | 875 | 2,329 |

This is 41% less elapsed time than baseline, but slower than the first warm
observation. The macOS tooling lane took 38m49s and set the critical path;
AMD64 product tests also took longer. Successful qualification and exact
cache hits do not eliminate hosted workload variation. The final bounded
repeat uses the same source, unchanged cache keys and all retained variants.
These original simultaneous-hit criteria are superseded by the reassessment
below; corrected-source candidate and installed-stream qualification remain pending.


## Final bounded repeat: qualification passed, original cache target failed

[Candidate 43345491416, attempt 3](https://github.com/Epiphytic/marmish/actions/runs/43345491416/attempts/3)
passed all **14 jobs**, unchanged test totals and all 18 timed Cargo stages.
Complete read-only inspection passed **15 artifacts/10 reports**, bound to
source `fe9a2e4d707e1754562bca34db9718601a97f996` and attempt 3.
Elapsed time was **52m13s**, aggregate job time **11,701 seconds**, peak
**six jobs/two macOS**. This is 23% less elapsed than baseline, with about
1% more aggregate runner time. It was not a fully warm candidate.

Five exact hits were observed. AMD64 product received Ubuntu image
`20261111.370.1` rather than `20261101.360.1`, correctly invalidating its
cache. Its tests/Clippy/release took **2,309/84/594 seconds**; Cargo reported
5m26s test compilation, 1m23s dev compilation and 9m53s release compilation.
The cold environment rebuild and longer test execution explain the critical
path; the underlying CPU/storage cause of the execution variability is not
established. ARM product took 1,173/18/85 seconds, macOS product
877/29/161, ARM tools 1,364/75/457, AMD tools 1,676/83/611 and macOS tools
935/72/604. Each stage exited zero.

The new AMD product archive, ID `10454762856`, is 1,294,566,063 bytes.
Nine retained entries total **7,578,184,296 bytes**: the original six archives
plus two ARM image variants and this AMD variant. Every original ID, full
key and size remains unchanged. **The 7 GB experiment target failed**, and
no candidate achieved six simultaneous exact hits. Automatic candidate repeats
stopped after the approved two; nothing was deleted to manufacture a fit.
The independent reassessment below defines the revised completion
path. Every lane separately has an exact-hit observation, with all 15 actual
warm restorations bound to full keys, observed saving jobs and retained IDs.


## Reviewed correction: product caches only

The independent reassessment rejected raising the allowance to make the result
fit. Installed download-only caches showed no measurable whole-job improvement:
AMD64 tooling took 1,983 seconds cold and 2,043/1,958/2,393 seconds with hits;
macOS 2,000 cold and 2,057/2,329/1,651 with hits; ARM64 2,006 cold and
1,880/1,915 with hits (1,938 on the image miss). These few varying observations
do not establish a causal speed effect.

The reviewed correction removes tooling caching and preserves all compilation,
tests and explicit `CARGO_INCREMENTAL=0`. Product cache keys/options remain
unchanged. A single new source-bound candidate and installed streaming proof
are required; actual per-product-lane exact-hit evidence replaces chasing a
simultaneously warm run. Image invalidation stays intact. The new ARM and AMD
product variants saved in attempts 2/3 remain unproven as restores until an
actual matching-key restoration is observed.

Nine entries still occupy **7,578,184,296 bytes**; removing writers does not
instantly free retained archives. Five product entries account for
6,354,145,521 bytes, and four orphaned tooling entries for 1,224,038,775 bytes.
No deletion or allowance change is part of this correction. GitHub normally
expires caches after more than seven days without access and evicts older
entries under storage pressure. The shared default is 10 GB, with custom
settings unverified. [GitHub cache retention and eviction](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#usage-limits-and-eviction-policy)

One product image per target uses about 3.77 GB. Two variants for every target
would model about 7.54 GB, or 8.76 GB while orphaned tooling entries remain.
Other workflows share the same allowance: the 7 GB target is not guaranteed,
and eviction can cause correct recompilation. The actual failed peaks remain
part of the record.

Before this correction, [installed stream 43361710386/1](https://github.com/Epiphytic/marmish/actions/runs/43361710386)
passed all five jobs and all nine directed duplex cells using exact candidate
43345491416/3 and source `fe9a2e4d707e1754562bca34db9718601a97f996`.
Read-only digest/context/rollup validation also passed. This is regression
evidence for the previous policy, not qualification of the upcoming correction.


## Final corrected-source native qualification

[Candidate 43363338143/1](https://github.com/Epiphytic/marmish/actions/runs/43363338143)
qualified source `9131926f85ce2cacc04805027871bdfe92dafc61`: **all 14 jobs
passed in 38m10s**, with **10,677 aggregate job seconds** and peak **six
jobs/two macOS**. Against the 68m13s/11,552-second baseline this observation
used 44% less elapsed time and 8% less aggregate job time. It is one hosted
observation, not a guaranteed runtime or a controlled causal benchmark.

Actual read-only inspection passed all **15 artifacts/10 closed reports**,
bound to that source and attempt. Actual bootstrap/default and explicit
quickstart plus real keychain-provider qualification passed on Linux AMD64,
Linux ARM64 and macOS ARM64. All 18 timed Cargo stages exited zero. Platform
counts match baseline: Linux each 1,762 passing executions/eight ignored,
macOS 1,789/eight ignored; these count subprocess executions, not unique tests.

| Platform / lane | Tests, seconds | Clippy | Release | Whole job |
|---|---:|---:|---:|---:|
| AMD64 / product | 1,677 | 17 | 80 | 1,840 |
| ARM64 / product | 1,146 | 17 | 84 | 1,298 |
| macOS / product | 865 | 28 | 151 | 1,127 |
| AMD64 / tooling | 1,536 | 66 | 436 | 2,053 |
| ARM64 / tooling | 1,411 | 68 | 447 | 1,941 |
| macOS / tooling | 1,229 | 119 | 830 | 2,203 |

Exactly three product cache-context lines show exact hits, and exactly three
tooling markers show `cache=disabled incremental=0`. No tooling cache action,
restore or save ran. AMD64 restored original cache `10450236189`, macOS
`10450320364`, and ARM64 restored newer-image cache `10452806916`. That ARM
variant now has actual reuse proof; the newer AMD variant `10454762856`
remains saved but unproven as a restore. Full keys and saving/restore bindings
are in the [machine-readable observations](native-ci-measurements.json).

The final repository inventory remains nine entries/**7,578,184,296 bytes**:
6,354,145,521 product bytes and 1,224,038,775 orphaned tooling bytes. This is
unchanged, above the failed original 7 GB target and below the documented
10 GB default. No entries were deleted, keys weakened or allowance raised.
Old tooling entries await ordinary expiry/eviction. Future image variants and
other workflows can cause eviction and recompilation.

A second bounded log-parser correction accounts for one apparent missing AMD
execution: its actual complete numeric summary ends `147 filtered outok`,
joined to a parent status. The missing tuple explicitly reports one pass and
147 filtered tests, exactly the observed discrepancy. Both parent tests have
explicit success lines. The private parser accepts only that tightly shaped
interleaving and leaves fragmented status/duration unknown. All 44 AMD64 numeric
suite tuples match the baseline multiset exactly;
the 32 AMD product tuples also match ARM product tuples exactly. Eight focused
positive/negative cases passed, including rejection of arbitrary suffixes and
malformed counts. Re-extraction preserves all five earlier published sample
objects and restores the exact C baseline count comparison; no test or runtime
code changed, and no CI rerun was used to hide the discrepancy.


## Final installed nine-pair proof and operation observations

[Installed stream 43367612858/1](https://github.com/Epiphytic/marmish/actions/runs/43367612858)
passed all **five jobs/nine directed duplex cells**, using exact candidate
43363338143/1 and source `9131926f85ce2cacc04805027871bdfe92dafc61`.
Read-only validation checked successful job inventory, complete directed pair
coverage, archive digests, canonical context digest, exact source/candidate
pins and rollup metrics. No artifact was executed by the evidence collector.
The workflow itself executed the installed clients/servers through the explicit
relay `wss://relay.us.whitenoise.chat/`.

| Client | Server | Payload bytes | Command wall, seconds | Payload bytes/second |
|---|---|---:|---:|---:|
| linux-amd64 | linux-amd64 | 204,800 | 58.146 | 3,522.1 |
| linux-amd64 | linux-arm64 | 204,800 | 59.090 | 3,465.9 |
| linux-amd64 | macos-arm64 | 204,800 | 55.958 | 3,659.9 |
| linux-arm64 | linux-amd64 | 204,800 | 62.049 | 3,300.6 |
| linux-arm64 | linux-arm64 | 204,800 | 63.206 | 3,240.2 |
| linux-arm64 | macos-arm64 | 204,800 | 59.341 | 3,451.2 |
| macos-arm64 | linux-amd64 | 204,800 | 62.735 | 3,264.5 |
| macos-arm64 | linux-arm64 | 204,800 | 63.575 | 3,221.4 |
| macos-arm64 | macos-arm64 | 204,800 | 61.175 | 3,347.8 |

These are one duplex sample per pair, including command/startup/transport
overhead. Wall time is not network RTT; payload throughput is not an isolated
link-capacity benchmark. There are no percentile or saturation claims. See the
[source-bound machine-readable rollup](native-ci-installed-stream.json) for
exact nanoseconds, byte counts, rates and verified archive identifiers. This
normal streaming cohort is regression evidence for this CI increment, not a
substitute for other release-qualification cohorts.

The final code and validation changes leave runtime, Cargo manifests/lockfiles,
versions and public installer pins untouched. The release publisher remains
`disabled_manually`; no release or tag was created. The final evidence commit
contains documentation/aggregate JSON only; the tested executable source is
the full commit above.

At the publication check, GitHub and Radicle refs both matched that source and
Radicle reported three synchronized seeds. Iris raw/head/exact-source and root
requests timed out or failed TLS from this host; direct Iris node connection
was also unavailable. Iris external readback remains unverified, so it must not
be reported as current. No Radicle security setting was weakened.
