# Legacy managed fixture dependency preparation

This phase repairs dependency preparation in the optional Python reference test
harness. It adds no Python, uv, Cargo, namespace or broker dependency to installed
Marmish. Legacy protocol compatibility and this reference suite are not native
v0.3.1 release gates.

A local managed-Python diagnostic reached `NativeFixture` class setup, where
`cargo build --locked --offline` failed because the fresh environment lacked
`serde_json`. That diagnostic did not retain the compiler version. Its process
controller cleanup remains unknown. Hosted class-setup failures and the separate
Darwin Watchdog failure retain unknown causes; the local observation does not
establish their causes.

The approved design is the third revision reviewed by Claude Opus 5.5, session
`add923e8-ec64-4de9-b134-a4814bc65f24`. Claude independently approved the completed code in session
`4474280d-b9be-4079-891c-14ff587aa719`. All 27 focused mocked tests passed
on host Python and again on managed Python 3.13.13. A real Linux AMD64 matched
control and all six fixture tests passed; see the
[validation record](../validation/legacy-offline-fixture-cache.md). No namespace
probe, complete tools suite or hosted platform result is established by those
focused checks.

## Implemented boundary

Changes are confined to `tools/marmish_managed_runtime_spike.py`, its existing
test module, and the legacy managed-packaging job in
`.github/workflows/installer-validation.yml`.

Compiler selection recognizes symlink, hard-linked and byte-identical copied
rustup proxies. A recognized proxy takes one explicit
`rustup which --toolchain 1.97.1 rustc` lookup; lookup failure cannot fall back to
the default toolchain. The lookup disables auto-install and does not install a
missing local toolchain. Direct compiler selection remains supported, subject
to admission of executable `cargo`, `rustc` and `rustdoc` siblings, exact version
1.97.1, native host, file identities and binary hashes. These are test-only host
tool prerequisites, not a general detector for hostile binaries. The hosted
legacy job separately provisions Rust 1.97.1 through a pinned action with a
five-minute step limit.

`prepare_full_tools_fixture_cache` creates a fresh private Cargo home and fetch
target, then makes one bounded `cargo fetch --locked` invocation for the existing
fixture manifest. It imports no ambient registry cache, credentials, wrappers or
proxy settings. Source, manifest, lockfile, compiler and Cargo configuration
checks bind preparation to the subsequent offline builds. The fixed fixture
lock permits its existing 21 checksummed crates.io dependencies and local root.
There is no retry, alternate registry or online fallback inside a failed test.

The namespace preflight retains its original environment before supervisor and
cache setup. Only the unittest environment receives the prepared `CARGO_HOME`;
installed application and offline-runtime environments do not. Bare Cargo,
rustc and rustdoc resolution through the actual suite PATH must match the bound
tools. Checks after preparation and after the suite detect observed input or
configuration changes. Publicly writable ancestors remain a limitation: these
checks do not prevent transient modification by another local actor or provide
a filesystem sandbox.

Fixture builds retain `--locked --offline`, their separate targets, original
120-second timeout and existing assertions. Preparation has a 120-second fetch
timeout, two-MiB output bound, 512-MiB cache threshold and 50,000-entry threshold.
Size and entry checks are polling limits rather than instantaneous quotas. The
900-second suite, 1320-second overall spike and 30-minute hosted job limits are
unchanged; their worst cases are not guaranteed to fit together.

## Reporting and validation limits

A `fixture_dependencies` PASS means dependency fetch and admission succeeded;
it does not mean the fixture, suite, installed runtime or process cleanup passed.
Reports separate preparation failures from suite failures and retain post-suite
verification failures without replacing an existing suite failure. Private
compiler provenance and bounded failure output use exclusive private files;
raw tool output and compiler paths are excluded from public reports.

The existing executor attempts process-group termination and reaps its leader,
but does not verify group absence for this fetch. Reports explicitly retain
`group_absence_unverified`. Spawn or cleanup errors remain failures with
uncertain cleanup; no process ownership, signal or reap policy changes here.

The completed local matched control used recorded Rust 1.97.1 tools and managed
Python: an empty-cache offline class-setup failure,
followed by the same selected test using the cache prepared through the actual
new function. Historical compiler provenance cannot be reconstructed from a
successful control. A successful selected test must exercise its real compiled
fixture and receipt assertions, with no skips. All six fixture tests also passed locally. The
original full tools suite remains a subsequent validation boundary; unrelated
failures remain failures. Linux AMD64, Linux ARM64 and macOS need their own
source-bound evidence before this phase claims platform validation.

## Compiler admission diagnostics follow-up

Beads `marmish-d93.5`. Hosted macOS run `42632233855` at `6a4072c` failed
compiler admission before fetch or the full tools suite. The rejecting command
and predicate remain unknown despite successful toolchain provisioning. Claude
independently approved diagnostic design V2 in session
`51cd7d08-650d-4df1-8527-35ba2e3ce7f6`, record SHA256
`27e004284661255cefe55575030918a4d33102f0416d3ffa672445cae5702712`.

The two existing Python files add an optional current/last probe record to
the existing dependency report: fixed tool and execution/validation stage, exit
code, byte count, expected version/host and exact-match booleans. No raw output,
paths or exception messages enter public reports. Commands, ordering, timeout,
version grammar and admission predicates remain unchanged. Unknown host refuses
before execution. Rejected returned/partial output uses only the existing
explicitly requested private failure log; no additional writer or artifact.
Clear the capture before every command and after all successful version checks
so later provenance/fetch errors cannot inherit stale compiler output.

Mock acceptance covers each tool, host/version mismatch, nonzero/oversize output,
executor failure/partial capture, absent or occupied private log, adversarial raw
bytes, and successful clearing before later failures. Reinspect the integrated
namespace phase before edits. Independent completed-code review precedes source
publication; hosted results must establish the rejecting predicate before any
loader, version or timeout correction. This is diagnostic work, not a cause or
platform qualification claim.
