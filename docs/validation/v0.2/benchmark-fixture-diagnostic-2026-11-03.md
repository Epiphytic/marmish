# macOS installed benchmark fixture failure: diagnostic preparation

Candidate [41655210565](https://github.com/Epiphytic/marmish/actions/runs/41655210565),
attempt 1, built source `347cf799ae5baa9e577ded1a00566c7a6b99b933`.
macOS job `125984173554` passed native product verification, signing, dependency
inspection and packaging, then failed the isolated installed-checks feature
suite: **426 passed, 2 failed, 191.10 seconds**, reported at
2026-11-03 15:09:10 UTC. The failing assertions were:

- `cli_child_deadline_and_cancellation_are_owned_and_bounded`: copied printf
  child did not report a successful exit (line 1682).
- `detached_submit_and_wait_preserve_exact_handle_bytes_and_one_deadline`:
  outer Result was Err where a retained handle was expected (line 1608).

Those original assertions omitted child status, stderr, outer Error and loop
case. The evidence does not identify a production error or prove either a
macOS executable-cache issue or an insufficient fixture deadline.

The reviewed diagnostic increment preserves all original byte, handle, marker,
deadline and cancellation assertions. It adds bounded test-only failure context,
a narrowly source-bound macOS workflow, and build-only utility probes. It does
not change product execution or attempt a speculative correction.

Local Linux validation: ten benchmark tests passed in 0.56 seconds; strict
installed-checks feature all-target Clippy passed in 4.56 seconds; workspace
format check, diff check, Python syntax and actionlint 1.7.12 passed. Utility
probes completed with the expected Linux bytes/statuses for original, renamed,
overwritten and recreated printf, fractional sleep, binary shell printf and
rewritten shell scripts. These are diagnostic checks, not macOS acceptance.

Literal audit of every `fs::copy`/`std::fs::copy` in the feature-gated
`installed_*.rs` files found system-utility copying only at the two benchmark
fixture lines. Other matrix shell fixtures use fractional sleep. Official
Apple sleep source supports fractional seconds; this rules out assuming that
syntax is universally unsupported, without proving behavior on this runner.

Design review r1 requested concrete replay/provenance/probe rules; r2 approved
the corrected design. See the linked
[design](../../phases/v0.2-benchmark-fixture-diagnostic.md) and reviews.
Actual macOS diagnostic execution, any root-cause correction and a new whole
candidate remain pending. No release qualification is claimed.

## Resumed testing

User resumed macOS testing after caveated initialrelease. Diagnostic parent is
7c8dcc6f2773d1ffab9db47bb3b454acdb8f422e; actual design review approved the
updated provenance and finite diagnostics. Workflow independently verifies
crates/Cargo/tools/toolchain equivalence from347 to that parent. No actual
macOS diagnostic has run yet; no root cause or acceptance is claimed.

Resumed author checks (Linux uid1001): actionlint1.7.7 PASS; Rust benchmark
filter10/10 PASS0.21s; strict feature all-target Clippy PASS12.28s; targeted
rustfmt2024 and diffcheck PASS; Python utility probe PASS (3844byte JSONL).
Actual workflow Bash extraction tests passed13 provenance scenarios and2
probe-status scenarios (both exact tests execute on probe success and failure).
Commands: `cargo test --offline --locked -p marmish-native --features
installed-checks --lib installed_check::benchmark::tests -- --nocapture`;
`cargo clippy --offline --locked -p marmish-native --features installed-checks
--all-targets -- -D warnings`, sharing existing target directory. Full logs
remain outside repository. These are local diagnostic checks, not Mac proof.

Bounded ripwire quality-delta at7c8dcc6+dirty reports diagnostic-only test
complexity10→22 and length76→114 for failure replay; copied-printf context
adds6lines. These are not production complexity changes. Change-check parsed
the one tracked changed Rust file as53whole-file symbols and broad name-based
dependents; it does not model the seven untracked workflow/docs/probe files
or cfg(test) execution boundaries. Its reach counts are not a production
impact claim. Actual diff and focused diagnostic tests provide the scope proof.

Final actual Claude code confirmation APPROVED session75f5b786-63de-48da-998b-976c8e6da9e2. Final workflow blob d8b43f08d53cc92070f8107e92da4ab52d28c774, SHA25646b3f851b03118482deccf18b9a56e8a84f5d9adc3c55160cb4fee0bf2aedf82. Author final shell checks13guard+5status cases PASS, including missing/empty/nonnumeric status; both exacttests always executed. Final actionlint/diffcheck PASS. No macOS run or diagnosis yet.

## Actual diagnostic41670576377

[Run41670576377](https://github.com/Epiphytic/marmish/actions/runs/41670576377),
attempt1/source49d1314d8b1720d3554bf5945542bfc95f1e616b, started
2026-11-03 16:05:34UTC (5seconds after creation) and failed16:11:26UTC.
Source guard and utility-probe steps passed. Probe exit status was0; original,
fresh copied, overwritten previously executed, and recreated printf all returned
exact binary:ok bytes. Fractional sleep and rewritten shell probes also passed.
These results do not establish the Rust test cause or product qualification.

First Rust test compiled in5m24s, then failed after0.57s: detached loop case3
returned outer Timeout where a retained handle was expected. Its failure-only
submit replay with a fresh3s deadline exited0 and returned the synthetic handle.
The original case3 deadline remains80ms shared across submit+wait, including
its30ms submit sleep. Startup-budget exhaustion is plausible; dyld/AMFI or any
other particular cause is unproven. No fixture timeout has been changed.

The second Rust test **did not run**. GitHub's Bash command includes -e; the
step's set -uo pipefail retained errexit, so cargo exit101 aborted before status
capture. Artifact13248721575 contains only five files: utility JSONL/stderr,
probe status, source record, first-test log; no outcomes.txt or second-test log.
The earlier local claim that both tests always execute was insufficient: its
fake cargo returned0 and did not exercise inherited-errexit with a failed test.
Exact extracted-step reproduction under bash -e -o pipefail with cargo exit101
confirmed one call and no outcomes.txt. The follow-up amendment adds the missing
failure-path regression and captures status in explicit if/else.

Artifact ZIP SHA256102e57592a88cf3eb78a19816eb5d93310c9bc0c4ebe8595b7d35ca29144ea7b.
Raw logs/artifact remain outside the repository under
/tmp/marmish-y5r-macos-41670576377. No production correction or acceptance is
claimed; both exact tests still need an actual macOS run with reliable capture.

Follow-up author validation: extracted actual YAML under bash -e -o pipefail passed14 provenance cases,5 probe-status cases (zero/nonzero/empty/nonnumeric/missing), and5 cargo/marker cases (first/second/both/neither failure101 and missing single-test marker). Both calls, exact statuses, both logs and aggregate result asserted. Exact old49d1314 YAML mutation fails the both-calls assertion (one call, exit101, nooutcomes), confirming the regression detects the bug. Local harness is uncommitted /tmp/marmish-y5r-errexit-guards.py. Actionlint1.7.7 and diffcheck PASS. No Cargo rerun: Rust/probe/production files unchanged. Scoped ripwire workflow quality-delta exit0: one minor verbosity81→87, gating0; change-check reports one changed workflow and no indexed tests, so the extracted Bash checks provide behavioral evidence.

Completed-code review found one doc-only trailing blank line; corrected and full diffcheck PASS. Reviewer independently reproduced old-step onecall/101/nooutcomes versus newstep twocalls/aggregate1/exactstatuses, plus wrong if-not misfix rejection. Final local provenance cases15 include independently wrong fixed-baseline parent and original-baseline tree versus frozen-current-tree checks. Test step runs inherited -e; provenance guard explicitly enables -Eeuo itself. Actionlint again PASS; workflow source unchanged during these refinements.

## Follow-up macOS diagnostic 41673137820

Source9d285236fdf35f4c6fdd80fd028cf061dc41d5b5; created16:20:13UTC,
started16:20:18UTC (5s), failed16:24:47UTC. Provenance, toolchain and all
utility probes passed. Reliable capture retained both exact Rust test statuses101.
First test compiled4m05s and ran0.40s: case3 outer Timeout before retained
handle; fresh3s submit replay exited0. Second test compiled1.34s and ran0.04s:
copied printf returned unix_wait_status(9), empty stdout/stderr. That is SIGKILL,
not its2s deadline. No particular platform mechanism is established.

Artifact13248573605 contains seven files, including both logs and outcomes.txt.
ZIP11400bytes, SHA256803df7e1f15e058e6ec45aefd1f94ebcb6ac4bbdc7319dc5ad8a2c507f351552.
Raw log/artifact remain outside repo under /tmp/marmish-y5r-macos-41673137820.
The approved next correction changes only the test fixtures; a fresh full
candidate must establish whether it resolves actual macOS execution.


## Fixture correction local validation

Actual design and completed code APPROVED; sessions and verbatim results are
appended to existing design-r2/code-r1. Source changes are confined to the two
benchmark test fixtures. No production deadline, cancellation, executable
validation or diagnostic workflow changes.

Using CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target, uid1001:

- cargo test --offline --locked -p marmish-native --features installed-checks --lib installed_check::benchmark::tests -- --nocapture:10/10PASS,2.14s tests,24.35s compile.
- cargo clippy --offline --locked -p marmish-native --features installed-checks --all-targets -- -D warnings:PASS.
- rustfmt --edition2024 --check crates/marmish-native/src/installed_benchmark.rs and git diff --check:PASS.

Logs remain /tmp/marmish-y5r-fixture-focused.log and
/tmp/marmish-y5r-fixture-clippy.log. Scoped ripwire quality-delta used a disposable
git snapshot containing exact HEAD/current benchmark file because direct file
invocation lacks git baseline context. Exit0/gating0; two minor verbosity rows
(test76→83lines; tests module579→582). Added lines retain and verify the old/new
inode proof. Edit-check reports unchanged test signature. Single-file snapshot
has no cross-module call graph, so it does not establish repository blast radius;
exact diff inspection confirms every Rust hunk is within tests. No suppression or
quality-baseline acknowledgment was added. Mac success is not claimed locally;
the next immutable full candidate must execute the corrected fixtures there.
