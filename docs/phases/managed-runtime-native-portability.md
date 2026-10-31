# Native managed-suite fixture portability

Detailed test-only amendment for Beads `marmish-x6z.1.2`. Independent Claude design review by actual `claude-opus-5-5` APPROVED
test-only implementation with conditions C1–C6 incorporated below. See
[review record](../reviews/managed-runtime-native-portability-design.md). Producing this design is not implementation authority.

## Actual evidence and bounded investigation

[Hosted run 41147817435](../validation/managed-runtime-native-41147817435.md)
shows macOS exited normally: 504 tests, 32 skips, exit 1 and eight failure
entries, including class setup. Cleanup completed and monitored home was
unchanged. The earlier interpreter shutdown hang is absent in this run; the
managed matrix still failed. No actual exception messages or compiler output
were exported, so the report does not establish the causes below by itself.

Read-only source inspection and disposable Linux probes establish these fixture
contradictions, without proving exact native exception reasons:

- `ZombieGroupProofTests` always supplies a synthetic 40-byte `proc_bsdinfo`.
  The included production header requires 136 bytes when `__APPLE__` is defined.
  A bounded compile of the unchanged fixture with only `-D__APPLE__` added
  failed with the fixed `unsupported BSD info ABI` assertion. No native ABI
  behavior was measured by that Linux compiler probe.
- Four `FinalReportTests` expect the Linux main branch, but actual Darwin
  returns pending/exit 2 before the mocked context is entered. Replacing only
  `validation.sys` with a Darwin platform stub reproduced four assertion
  failures on Linux. Once Linux admission is simulated, `scan_owned_stderr`
  must also remain an explicit mocked boundary in report-only tests: its real
  `/proc/self/fd/2` operation is Linux-specific and has separate focused tests.
- Two sandbox hash/home tests call `start()` and encounter validated Bubblewrap
  and Linux-tool prerequisites before their target hash/private-path checks.
  Injecting missing Bubblewrap reproduced both assertion failures. Their
  unresolved `Path(temp)` roots can additionally contain Darwin's `/var` alias;
  `_private` inspects only the final component, so canonicalization is hygiene,
  not an established cause of these failures.
- The Bubblewrap mode test assumes chmod preserves requested set-ID bits.
  Darwin may clear a bit that the caller cannot set; that remains a hypothesis,
  not a measured reason for this native failure. Existing fixture roots already
  use resolve(), so changing their root spelling alone does not explain it.

No production defect has been established. Test assumptions need isolation;
no platform admission, native ABI guard or confinement gate may be weakened.

## Ownership and interfaces

Only these source files may change after design approval:
`tools/test_marmish_lifetime.py`, `tools/test_marmish_native_validation.py`, and
`tools/test_marmish_transport_sandbox.py`. This phase and its review record are
the only owned documentation. No production headers/modules, managed harness,
helper, workflow, artifact pins, timeout, Linux namespace skips or permissions
change. Existing real macOS confinement and Linux namespace tests stay required.

For `ZombieGroupProofTests`, use the actual SDK `libproc.h` and `sys/proc.h` on
Darwin. Generate the existing synthetic headers only on non-Darwin systems;
keep their stated role as fake syscall data, not native ABI certification.
Retain all 50 fake-syscall modes, strict compiler warnings and the production
136-byte static assertion. Do not undefine `__APPLE__`, remove the assertion,
replace production headers, or skip this class on Darwin. Add a portable fixture
selection check proving Darwin selects SDK headers and Linux selects stubs.
Compiler and executable subprocesses must be bounded, checked, private and
cleaned by existing class cleanup; add a finite compilation timeout if absent.

For final-report tests, patch module-local `validation.sys` with a Linux platform
stub only within the fixtures that intentionally exercise Linux report lifecycle.
Mock `NativeLinuxContext`, fake e2e and `scan_owned_stderr` explicitly; no ambient
keychain, real `/proc` scan or real helper subprocess may run. Preserve secret
report rejection, teardown ordering, retained-state failure and exactly one
post-teardown report. Use a fully mocked context for setup-signal injection too,
rather than constructing a real provider context. Check both SIGINT/SIGTERM
handlers and environment restoration. Keep the existing private-file scanner
offset/secret test exercising the actual scanner with its current syscall
fixtures; do not replace the scanner everywhere with a passing stub.

Add a portable simulated-Darwin test and an actual-platform admission test with
no platform patch in the latter: actual Darwin keeps its refusal, while actual
Linux is shown to reach the fully mocked context rather than the pending branch. Valid arguments must retain production's
pending/exit 2 `dedicated_account_required` report; context construction, scan,
e2e, allocation and subprocess spawning must never occur, and handlers and
environment remain unchanged. Do not claim native-provider acceptance on Darwin.

For the two sandbox hash/home tests, resolve only the temporary root before
constructing controlled child paths; preserve the deliberately symlinked final
home component. Mock only preceding platform prerequisites (`validated_bwrap`
and fixed tool lookup) within those two tests. Assert `Popen` is never called,
target exact hash/private-path refusals, close each owned daemon with immediate
addCleanup/finally ownership, and retain checks that failure leaves no process.
Add portable regressions covering the same target validations with explicitly
missing ambient helper/tools: the fixture-supplied prerequisites isolate the
intended checks, while independent real namespace tests still fail without
Linux confinement. Never resolve an attacker-supplied home before testing it.

Split Bubblewrap rejection coverage by contract. Use real filesystem operations
for non-executable/group-writable files, untrusted ancestors, final/intermediate
symlinks and FIFO rejection. Keep actual mode assertions for ordinary chmod
cases. Exercise setuid/setgid rejection deterministically via a narrow fstat
fixture: preserve directory/owner metadata, modify only selected regular binary
mode bits, and assert the exact unsafe-executable refusal before access/hash.
Also retain mandatory real special-bit rejection on Linux, asserting chmod
preserved the bits; off Linux retain that real check when observed stat shows
the filesystem preserved the requested bits; absent bits cannot truthfully be called accepted
unsafe bits. Deterministic synthetic coverage always runs, so this is not a skip
or reduced predicate coverage. Simulate cleared special bits in a portable
regression to prove the fixture still checks both rejection predicates.

## Validation, reporting and stop conditions

Before source edits, obtain independent Claude design approval and record all
conditions. Run affected modules normally and through a symlinked repository,
with bounded process ownership and no concurrent HOME-sensitive proof. Keep
Linux namespace tests real and mandatory; missing prerequisites remain failures.
Run the full tools suite. Use temporary-copy mutations to check that removing
set-ID rejection, accepting a home symlink, bypassing hash rejection or emitting
reports before teardown is detected; bound any mutation process and clean it.

Root coordinates the final managed proof and native Linux AMD64, Linux ARM64 and
macOS ARM64 reruns, unchanged acceptance gates and budgets. Record actual counts,
skips, exit status, cleanup/home checks and failure entries. Local simulated
platform/compile results are not native evidence. If a native failure persists,
stop the acceptance claim: use a separately bounded, reviewed native diagnostic
that emits only fixed stage/cause categories, booleans and requested/observed
permission bits, never raw stderr, private paths, command lines or environment.
Do not add such a diagnostic to the production harness under this test-only scope.
The separate configured macOS sandbox SIGABRT and hosted Linux namespace denial
remain unresolved independent gates. Obtain integrated Claude code review before
publication; no phase completion follows solely from passing fixture tests.

## Required independent review conditions

C1: Assert scanner called exactly once with context.secret_values before exit,
returned key merged into the passing report, and absence on secret/earlier e2e
failures. The review literally says “it must never be called on the secret-report
or failure paths.” A later teardown failure necessarily follows the earlier
scan: the same condition requires scanning before context exit. Preserve this
lifecycle order and record that nuance explicitly; do not claim no scan on a
failure only discovered later. Add drop-scan-merge and early-report mutations.

C2: Use actual-platform branching rather than a new Darwin-only skipped test;
no new Linux skips. C3: Linux real chmod preservation remains mandatory; synthetic
fstat uses a real os.stat_result changing only selected executable mode, exact
unsafe-executable refusal, and assertions access/hash are not reached. C4: sys
stub delegates every attribute except platform; explicit context booleans, both
installed/restored signal handlers, exact failure category and environment.
C5: Separate unmocked-prerequisite order regressions assert unsafe_bwrap_path or
missing_confinement_tool precedes hash/private checks; tool lookup fixture accepts
only unshare/ip/sysctl. C6: Preserve include order, add no synthetic headers on
Darwin, compiler timeout is failure, and header-selection unit checks never
substitute for actual SDK compilation or native matrix acceptance.

## Local implementation checkpoint

The three owned test modules now implement the reviewed boundaries. Only test
source changed: Darwin header selection uses the SDK; report-only fixtures mock
Linux admission and scanning explicitly; hash/home fixtures isolate prerequisite
ordering; and set-ID rejection has deterministic metadata coverage alongside
mandatory real Linux chmod checks. No production or confinement gates changed.

Local Linux checks passed: affected modules **68 tests, zero skips** both normally
and through a symlinked repository path; full tools discovery **512 tests,
8 skips**, exit 0 in 21.445 seconds. The actual Linux namespace class ran.
`git diff --check` passed. An initial symlink-wrapper invocation had a missing
`sys` import before any test ran; the corrected bounded invocation passed.
Five disposable-copy mutations were detected by the focused regressions: removing
set-ID rejection, accepting a symlinked home, bypassing the daemon hash check,
emitting an early report, and dropping the scanner-result merge. Each mutated
child exited 1, with a 20-second subprocess bound; temporary copies were removed.
No mutation touched integrated production source, and no packaging proof ran.

These are local fixture results, not native macOS or ARM64 acceptance. Root owns
quality assessment, integrated Claude code review, managed proof and native
reruns. The C1 teardown-after-scan nuance remains explicit for code review.

Root's quality-delta check against Git HEAD exited 2: 34 findings, comprising
17 gating regressions, 2 minor rows and 15 new-symbol rows (19 existing symbols
worsened overall). This is not a clean quality result. Material costs include
two short test-body clone groups, report fixture parameter count 5 to 7,
`invoke` growth from 30 to 63 lines, report class growth 81 to 138, and sandbox
class growth. The added report switches keep e2e failure and platform admission
explicit; scanner order/merge, signal/environment restoration and pre-teardown
output assertions account for fixture growth. These checks are required by the
review, but the larger fixture and short assertion clones are real maintenance
costs for integrated review to assess; extracting a helper solely to lower a
metric would not by itself improve them. Other rows flag recent churn and
new unittest-discovered classes/methods or mock callbacks, exercised by the
passing runs above rather than statically referenced calls. No suppression,
baseline reset or cosmetic refactor was applied. Root's PR-context check exited
0; it does not override these quality findings or replace code review.

## Final local refinement checkpoint

Following integrated review, the C fixture includes actual/synthetic
`sys/proc.h` before fake syscall macros, preventing later SDK declarations from
being renamed. Production headers and their ABI guard remain unchanged. A fixed
single-method unittest case/result regression proves simulated Linux silently
cleared set-ID bits fail exactly twice, with no errors/skips; the existing
Darwin-cleared regression passes and still exercises deterministic predicates.
Controlled hash/home unit fixtures use `/tmp` with a canonical temporary root
because socket path length is checked before the targeted private/hash checks;
this prevents unrelated Darwin temporary-root length from masking those checks.
This is fixture isolation, not resolving an attacker-supplied home.

Final local checks: **69 tests, zero skips** normally and through a symlink;
full tools **513 tests, 8 skips**, exit 0 in 21.507 seconds. All five disposable
mutations listed above were detected again, exit 1 each under the same bound.
`git diff --check` passed. No tests or proof remain active. Earlier quality counts
refer to the initial reviewed checkpoint; no new quality scan or suppression
was applied by this agent. Final integrated review and native acceptance remain
root-coordinated, and this record grants no implementation authority.

Final source SHA-256 values:

- `tools/test_marmish_lifetime.py`:
  `0792ebc31a107c221a358086a6fe5b04abb1afca20975d3cbd67a15338d148e3`
- `tools/test_marmish_native_validation.py`:
  `8145fef064a5c29715d5f205164234516c0862b649b1c323d8e5087e30b2af8d`
- `tools/test_marmish_transport_sandbox.py`:
  `fe44db01f8dfc41becc94752e643053ebbf34441befb350dd0400d30c4383fbd`

Final root quality recheck after the useful code refinements: exit 2, 37 rows,
17 gating, 2 minor and 18 new-symbol findings; PR-context exit 0. The earlier
34-row result remains historical, not the final count. No suppression or
baseline change was used; the same real fixture-growth/duplication costs apply.

## Measured native follow-up

After independent final code approval, source e5a636d1026978b5c539c898795e554d32118eac
was published to GitHub and Radicle and verified through Iris. Actual local
[Trial19](../validation/managed-runtime-selfhosted-trial19.md) passed 513 tools
tests with 8 skips and all ten executable cases, enforced Linux offline
isolation, complete cleanup and unchanged monitored home metadata.
[Hosted run 41150264034](../validation/managed-runtime-native-41150264034.md)
passed all three installer jobs and the actual macOS managed proof: 513 tests,
32 skips, all ten assertions, offline retention reached, cleanup complete and
unchanged monitored home metadata. Darwin records uv-offline behavior without
independently enforced OS network denial. The native suite counts, unchanged skip count and source skip conditions
support the inference that the SDK fixture compiled and passed. The report
does not list individual skipped tests. The eight prior failure entries and summary hang are
absent; this establishes the bounded fixture-repair outcome, not every inferred
cause of those earlier failures.

Both hosted Linux suites ran 513 tests with 8 skips and failed the same two
mandatory namespace cases; their later offline-retention gates were not reached.
Overall native matrix acceptance remains failed. Suitable self-hosted results
are still required for parent phase0; the production uv installer and unified
service are not implemented, and configured macOS confinement SIGABRT remains
independently unresolved. No gate was relaxed to obtain the macOS packaging pass.
