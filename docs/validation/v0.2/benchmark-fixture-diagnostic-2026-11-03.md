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
