# Selected corrective copy qualification review

The test-only corrective runner is approved for the bounded scope below. This
is source and offline-test evidence; no corrective relay run, installed-platform
result, release publication, or complete qualification gate is claimed here.
Beads: `marmish-8ka.5`.

## Reviewed scope and contract

The review covered these three source files on `27aafb0` plus the uncommitted
increment that accompanies this record:

- `tools/marmish_copy_corrective_runtime.py` (new selected-case entry point).
- `tools/test_marmish_copy_corrective_runtime.py` (new offline tests).
- `tools/marmish_stream_runtime.py` (optional keyword-only parser configuration,
  argument vector, and prevalidated-main-options hooks).

The existing `tools/marmish_copy_runtime.py` remains unchanged: its default
12-case suite and 600-second default operation timeout are preserved.

The selectable cases are `put_raw`, `get_raw`, `put_zlib`, `get_zlib`,
`put_existing_destination`, and `get_existing_destination`. Execution uses the
original validators and canonical order, with no hidden preparatory transfers.
A GET whose PUT is omitted uses an explicitly reported local fixture seed; the
negative GET also uses the locally created main payload, matching the original
same-host fixture. This does not demonstrate independent remote source creation.

A bounded regular-file baseline must contain exactly one fulfilled result for
each selected case. The runner checks the executable hash and version, platform,
architecture, relay origin/path-redaction context, payload length, and applicable
codec. A baseline source revision, when present, must match the supplied revision.
Old refusal rows bind payload length through the baseline report's top-level
field; they are labeled `expected_refusal`, not successful copies. The relay path
itself cannot be verified from the redacted baseline and is reported accordingly.
An overall failed baseline can supply individually valid fulfilled rows, with its
failed overall status retained explicitly. Timed-out or missing result rows are
ineligible; this tool cannot manufacture a completion duration for them.

Before creating private state or starting a child, the runner requires an
operation timeout of at least `ceil(4 * max(selected baseline seconds))`. Each
fulfilled rerun records its deadline and observed headroom. It fails if that
ratio is below 1.5. Rerun elapsed time includes inherited local validation, making
the headroom check conservative relative to the original baseline measurement.
The report lists selected, completed and omitted cases, with
`selected_only: true` and `full_gate_claimed: false`. Baseline/refusal checks,
per-case success, cleanup and the shared report writer all still determine the
actual overall result.

## Actual independent reviews

All four reviews used Claude Code with `--permission-mode auto`, model
`claude-sonnet-5-5`. They were independent source reviews; reviewers could run
hermetic Python checks, but no native executable, Cargo, relay, network,
workflow dispatch or publication was authorized.

| Review | Actual session | Duration | Turns | Verdict |
| --- | --- | ---: | ---: | --- |
| Initial design | `4e8782cb-6a68-4d15-b398-c89bc42ab5e9` | 75.716 s | 8 | APPROVE |
| Expected-refusal design amendment | `743a089f-701b-47e7-b910-16e55162ebca` | 44.427 s | 5 | APPROVE |
| Integrated code | `f7491089-b053-4c63-b114-92ed9fd6fd54` | 118.391 s | 11 | APPROVE with useful followups |
| Followup code and tests | `c6c7cdaf-d42b-471f-8cc2-435abb4632fa` | 26.863 s | 5 | APPROVE |

Useful design findings were incorporated: strict bool/number handling; bounded
nonblocking regular-file reads; duplicate-key, nonfinite and recursive-JSON
refusal; explicit source/context limitations; source oracles for local GET seeds;
and inode/mtime/content preservation for existing-destination refusals.

The first code review prompted a hash recheck before `--version`, actual
managed-CLI/options wiring coverage, matching and mismatched version tests,
explicit per-row refusal labels, fulfilled-row-only wording, and category-specific
negative assertions. The followup verified those changes and found no blocker.
Hashing and executing a pathname remain separate operations; this trusted local
test tool does not promise an atomic executable-identity security boundary.
The reviewer attempted one additional recursion probe, which automatic review
rejected; it was not retried. The final authored deep-nesting regression ran
successfully in the normal hermetic suite.

Raw machine-local review files are
`/tmp/marmish-copy-corrective-{design,code}-r{1,2}.json`; they are not committed
artifacts. This record contains the sanitized verdicts, scope and dispositions.

## Executed checks and limits

The author and final reviewer independently ran:

```sh
PYTHONPATH=tools python3 -m unittest -v \
  test_marmish_copy_corrective_runtime \
  test_marmish_stream_runtime test_marmish_rsync_runtime
```

All **34 tests passed**; the author's final run took 0.061 seconds. Tests cover
argument refusal before state/process creation, strict baseline provenance,
NaN/infinite/deep-nested and duplicate-key JSON, FIFO refusal without blocking,
selection and omission rules, actual six-case ordering, unchanged default
12-case behavior, preserved metadata/content, wrong codec and insufficient
headroom failures, binary/version changes, and real managed report/private-state
cleanup with product execution stubbed. The initial reviewer additionally ran
the earlier corrective test module 60 times without a failure. Python compilation
and `git diff --check` passed. No live-corrective result is implied.

`ripwire . --quality-delta` returned exit 2; this is **not a clean quality gate**.
The reported shared-parser/main short-horizon churn is real and limited to the
compatibility hooks. A small duplicate-key rejection helper is deliberately
self-contained rather than importing a larger unrelated qualification entry
point. Dynamic parser callbacks and unittest discovery produced dead-code
false positives, and a nested fixture function was counted again as its own
clone. New class/test verbosity and the forwarding wrapper's parameter count
were consciously retained: validation and case orchestration remain bounded,
and changing these during qualification would add interface churn. No broad
refactor or measurement waiver is claimed.
