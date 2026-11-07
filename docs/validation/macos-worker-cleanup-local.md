# Worker isolation and cleanup diagnostics: local validation

This corrective increment follows the [failed macOS candidate](streaming-candidate-53f94f9-failure.md)
at `53f94f99d06942a58f52fd4a76048a26ced1a99d`. Its
[approved design](../phases/macos-candidate-worker-and-cleanup.md) separates
elimination of an actual Cargo output collision from instrumentation of an
unclassified cleanup failure. Local Linux results do not establish macOS
correctness or installed release qualification.

## Static checks and scope

Root `cargo fmt --all -- --check` and staged `git diff --check` passed.
Representative target, environment, nsec, runtime database and private-key paths
are ignored. A bounded scan of the 21 initially staged source/documentation
files found no nsec, private-key header or GitHub-token signatures. These checks
do not substitute for reviewing the exact final staged files.

`node --test tools/v02-actions/macos-native-regression-policy.test.mjs`
passed 6, failed 0, skipped 0 in 55.685ms. These are the new manual workflow
policy tests, including executing
the workflow's actual jq filter on two valid and five invalid Cargo-shaped
metadata fixtures. Both engine bins must be represented: the unique normal
worker and the existing feature-gated fault frontend. Four embedded shell
blocks passed `bash -n`; PyYAML `safe_load` verified the six-step/45-minute job
shape. These outputs are retained in the execution transcript, not separately
hashed log files. No hosted job or relay
traffic was used for these checks.

`ripwire --pr-context --max-tokens=4500` completed against the working diff.
Its bounded graph and zero-call/test floors do not prove absence of dependents
or test coverage. Initial quality-delta exited 2, flagging repeated test fixture
setup and diagnostic verbosity alongside test-registration graph limits,
normalized clone matches, workflow size and historical builder churn.
Useful duplication/verbosity improvements were validated and independently
reviewed below; no blanket clean-quality verdict is claimed.

## Test execution

Linux UID1001, Rust1.97.1, single Cargo lane:

| Check | Actual result | Duration |
|---|---|---:|
| Original combined four-package `--no-run` | PASS; no binary collision warning | 23.06s |
| Original combined four-package execution | FAIL; native closed-stdin test | 665.33s |
| Engine library | 141 passed, 0 failed, 1 existing ignored | Included above |
| Engine integrations | concurrent2, encrypted3, engine14, native_storage5 passed | Included above |
| Native library | 629 passed, 1 failed, 3 ignored | 619.46s |
| Stream | 29 passed | Included above |
| Vault | 12 passed; helper3 passed | Included above |
| Prototype bridge | 9 passed | 1.46s test / 3.87s command |
| Engine all-target strict Clippy (default features) | PASS | 0.95s tool / 1.03s command |

The failed test is
`stream_runner::tests::closed_stdin_retains_delivery_cursors_and_natural_exit`:
`stdin_delivered` was13, expected0. No retry, assertion change or acceptance
waiver was used. The reviewed fixture-isolation correction is described below;
neither worker-isolation success nor other green tests makes the historical
failed run a green suite. Final source-bound results and independent result
review remain required.

The exact invocation used `/usr/bin/time -p env
CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target cargo test --locked
--no-fail-fast -p marmish-engine -p marmish-stream -p marmish-native -p marmish-vault`
from `/home/openclaw/repos/marmish-installed`. No test-thread setting was
explicit; the historical inherited RUST_TEST_THREADS value is unknown. The
native image was `target/debug/deps/marmish_native-786968b731ec437d` below that
shared target. Source freeze was coordinated, not attested with pre/post hashes.
Do not infer parallel test execution or a captured causal interleaving.

A controlled Linux pipe/fork check demonstrated successful write13 while an
inherited CLOEXEC reader remained after target stdin closure, then BrokenPipe
after its release and target exit9. Private JSON
`/tmp/marmish-closed-stdin-inherited-reader-proof-r1.json` has SHA-256
`77ed38edf4d8d3787a207366b7227581cc2986dad6be70edb7b48d48ed7eb7a5`.
Four separate exact original-test executions each passed in about1.2s; log
`/tmp/marmish-closed-stdin-isolated-four-r1.log` has SHA-256
`fe7ebf2a9833749444927e4305ca77a8022c17bfacb8ba463cde619952022c62`.
This establishes a missing fixture premise, not the actual failed run's cause
or a macOS mechanism. Recurrence in the isolated child requires product defect
investigation; no retry or weakened delivery assertion is authorized.

After the small test-only fixture/caller extractions, all four cleanup cases
passed (0 failed, 0 ignored; 1.16s tests,10.28s compilation). Strict native
all-target Clippy with installed-checks passed16.57s; engine all-target Clippy
with fault-injection passed1.93s, verifying the initial reviewer finding's
narrow source correction. Closed-stdin source was still unchanged for these
checks. Logs `/tmp/marmish-worker-cleanup-`:

| Log | SHA-256 |
|---|---|
| `four-quality-r1.log` | `f25e1808f7d63e2e403dcbdab942bd4b6de6969b1568cc1e989311497e9dc8e8` |
| `native-clippy-r1.log` | `c96490746665623f83aed89c2056329b318e49b906368181175a7343fd64ab1d` |
| `engine-fault-clippy-r1.log` | `bebe58f2c2804587233caf65d7de46ec63f8753aff671b2171d9b841061f0598` |

The implemented D selfexec fixture passed its first exact test (1.22s) and four
fixed independent repetitions (1.210/1.213/1.211/1.246s). The final runner cohort
passed17, failed0, ignored1 existing probe (615 filtered) in6.18s. Final native
installed-feature strict Clippy passed8.18s; scoped formatting/diff checks passed.
Root compared the1692-byte original closed-stdin body from its first wait through
all delivery/retry/EOF/natural-exit assertions against HEAD: byte-identical.
Only supervision, child fixture ownership and recursion dispatch were added.
Final runner source SHA-256 is
`f8edaf4315811ec3907c7192fe9823a2a0acf513e20826e5f61a7c7c9dd1b5ea`.

The mandatory combined rerun completed with Cargo exit0 in693.03s, using the
same Cargo flags and workload,
`RUST_TEST_THREADS` explicitly unset (default libtest; CPU/affinity32). Before-run
all-crates Rust/manifest map SHA-256:
`1e8bdc556b810f9ead8627a58b10f5d9aabda114786506de33868c81d28660c3`.
Actual native test image path remains
`target/debug/deps/marmish_native-786968b731ec437d` under the shared target;
before-run SHA-256:
`2718fd4dbdaa3c3629e78d6140823f931b9e726d4a977824b6b94436c05fffb4`.
The final log `/tmp/marmish-worker-cleanup-combined-final-r1.log` has SHA-256
`3e9f5e06c208fdb393c7355c017300ee785f4a13c48491eb8b0b0289a614a1dd`.
Top-level results (nested selfexec summaries are not added again): engine141
passed/1 existing ignored (27.99s), engine integrations2+3+14+5 passed, native630
passed/3 existing ignored (632.59s), native worker1 passed, process-limits1
passed, stream29 passed, vault12 and helper3 passed; native vault1 existing
ignored. No target failed. Root independently compared the actual before/after JSON:
all source-file hashes, source-map hash and actual native test-binary hash match.
Before JSON SHA-256 `f405863f6dcc293a76aaeac9b4b9286c6b6d876dee2cda21b5b0064b6821e619`;
after JSON SHA-256 `ea32f38ee2ee427a7bc4049904a68f4b5a535549a01db6fc352236f6369495b0`.
Independent final result review approved this corrective source increment;
see [the actual review record](../reviews/macos-candidate-worker-and-cleanup.md).
This is not macOS evidence or release qualification.

Final quality-delta still exited2 (eight major measured gates). Setup extraction
reduced repeated test bodies to82/83 normalized tokens; their remaining waits,
drop actions and different terminal assertions are intentionally explicit test
premises, not candidates for a parameterized framework. Lifecycle::finish is
102 lines versus90, with the extra branch/diagnostics compiled only for tests;
production cleanup logic is unchanged. The guard/status clone match has no
shared semantic role. Other flags are test registration graph limits, a new
method growing a test class, the bounded linear workflow and existing builder
churn. These are reviewed dispositions, not a clean tool verdict. Initial
real setup/caller duplication was reduced without deleting coverage.

Private log SHA-256 (names prefixed `/tmp/marmish-worker-rename-`):

| Log | SHA-256 |
|---|---|
| `combined-no-run-r1.log` | `35ffc8acce2df33817bcdd73279981f6eec6a37b5a845bb2230573447f40f984` |
| `combined-r1.log` | `0013933774327319f807294743e0d331fc499257ee3e2703322609dacee35961` |
| `prototype-r1.log` | `1cf4eab54b20f895519a7d4acacf639d9bd8f64863dfa35a2082f2938ca95426` |
| `engine-clippy-r1.log` | `bfc32c8d80abc45e019d7449ca5bf2f3ef773d2961d88cfa3fe23a3c41385c75` |

The two debug worker files were hashed separately after combined compilation;
target metadata established unique names:

| Debug worker | SHA-256 |
|---|---|
| `marmish-engine-worker` | `c3c899c7ef5e71c50feb4f8918b95f6777173f971430cc4e28d618c8c91dcfad` |
| `marmish-worker` | `e6a72cc14b0ccae71aa4698c634dfa7dc8d4660760287e13870a84a7997cd9c4` |

Actual pinned-toolchain metadata was retained at
`/tmp/marmish-worker-rename-metadata.json`, SHA-256
`2e9fd157f9edcfb4575e27600df47fce4302ba2379a973b325b1fe8a935b8cf3`.
`cargo metadata --locked --no-deps --format-version 1` selected the repository's
Rust1.97.1 toolchain. Engine bins are exactly `marmish-engine-worker` and the
existing fault-injection-gated `marmish-test-frontend`; the unique worker keeps
its explicit source path `src/bin/marmish-worker.rs` without an implicit alias.
Native bins retain their existing names, including `marmish-worker`. Root
independently read and hashed this existing JSON; no overlapping build occurred.

The broader historical Python release suite has a preexisting failure: a
diagnostic test extracts an obsolete embedded-Python installer marker from
the current Rust installer and raises IndexError. Reproduction against the
original HEAD module/test body also failed. Beads `marmish-5j0` tracks this
separately; it is not reported as a green suite or silently repaired by weakening
its assertion. The two new legacy worker source/destination mapping checks
passed and remain in scope for integrated review.

## Hosted and release limits

The focused macOS workflow subsequently passed at source
`2ebc8b6b2489f3b63255658846b23e7fe58f855f`: run
[42406498129/1](https://github.com/Epiphytic/marmish/actions/runs/42406498129),
job128505241150. It executed the unchanged combined four-package suite and
20 repetitions of each of the four exact cleanup tests (80 named successful
executions and 80 one-test-passed summaries). The serial job took1143s under
its2700s cap, leaving1557s; cap/observed=2.362 is one measured observation,
not a guarantee. Compilation/identity checks took340s, combined suites728s,
and the80 focused executions57s. See the
[sanitized hosted evidence](macos-worker-cleanup-hosted.json) and
[actual result reviews](../reviews/macos-candidate-worker-and-cleanup.md).

Both the historical failed macOS run and this passing run set
`RUST_TEST_THREADS=1`; no parallel macOS run was performed. The unknown inherited
thread setting refers to the first failed local Linux combined run. This result
establishes neither historical causality nor the old collision's winning bytes.
Five existing ignored tests remain ignored. No relay or installer runs in this
diagnostic. Its evidence stays bound to2ebc8b6 and is not transferred to a new
source revision.

The subsequent exact-source three-platform candidate
[42408127994/1](https://github.com/Epiphytic/marmish/actions/runs/42408127994)
passed both ARM platform jobs but hit the AMD64 package-job90-minute cap while
release-compiling installed-test tools. It is unqualified and dispatched no
installed cohorts; [structured evidence](streaming-candidate-2ebc8b6-timeout.json)
preserves its results. A fresh candidate and all eight original installed
cohorts remain mandatory. No version bump, public installer pin change, tag or
release publication is part of this increment.

Final D/runner check logs (prefix `/tmp/marmish-worker-cleanup-`):

| Log | SHA-256 |
|---|---|
| `closed-isolated-final-r1.log` | `4bd62ed3cb789d957ed2ef0acc2d552a8aaee5566287cf83b660b6b8ab1ad84d` |
| `runner-final-r1.log` | `c7ce54ba1a78ae435ed0a770563aa3285097bcaf1b641e785dc34b9e0d76bace` |
| `closed-four-final-r1.log` | `c7dd0549611226b4acadbf3f0710189713b757f2fbd520c25403429f3a7b1e6a` |
| `native-clippy-final-r1.log` | `203c1e717efe85ec0dcdec9e8e5174725e0ff796d2661b1f444b5228a2c6bf02` |
