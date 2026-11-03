# Owned relay readiness diagnostic validation

Source parent: `bc75231530eb1fc4ef8db60d93c3528d5e9e71bd`.
Beads: `marmish-7nw.2`.

The [approved design](../../phases/v0.2-phase1-owner-diagnostics.md) addresses
missing owner evidence in [run 41562508066](hosted-foundation.md). It adds no
transport retry, timeout or readiness relaxation. The prior failure remains
unexplained; all three direct reports passed but no onion endpoint was published.

An agent implemented the fixture, shared jq projection and inert tests. Root
integrated workflow validation and independently reran
`bash scripts/test-v02-owner-diagnostics.sh`: **PASS**. The test runs real fixture
cleanup with owned fake child executables; it exercises all six stages, every
allowed pair and cross-pair rejection, malformed/oversized/private input,
consecutive duplicate suppression, the 32-event cap, extractor pipeline failure,
allocation failure, source binding and private-state removal. No live relay,
keychain or Cargo operations run in this suite.

Bash syntax, actionlint and `git diff --check` pass. Shellcheck is unavailable.
Scoped `ripwire scripts --quality-delta` exits 0 with no gating regression;
it reports minor complexity increases of one in cleanup and serve, nine more
serve lines and similarity between two new test helpers. No suppression or
baseline was added. This diagnostic-only change does not require rerunning
unchanged native Rust tests; a fresh full hosted matrix is still required.

The [actual Claude completed-code review, round 1](../../reviews/v0.2-native-owner-diagnostics-code-r1.md)
approved the integrated code without blockers and independently passed the inert
suite in 14.8 s plus extra filter probes. Two useful findings were incorporated:
validation now slurps extractor output and requires exactly one JSON value, and
the duplicate-flood test interleaves invalid lines. A successful extractor that
prints two JSON values is tested through real fixture cleanup and yields `[]`.
The agent reran the focused suite, Bash syntax and diff checks successfully;
root reran actionlint successfully. The [actual round-2 re-review](../../reviews/v0.2-native-owner-diagnostics-code-r2.md)
**APPROVED** without blockers, independently passing the final inert suite in
17.8 s, Bash syntax, actionlint, diff checks and single-value guard probes.
This approves the code, not hosted onion transport.
