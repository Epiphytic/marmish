# Managed test-progress diagnostics: initial code review

Reviewer: independent Claude, actual model **`claude-opus-5-5`**, recorded in
the review wrapper's `modelUsage`. Initial verdict: **APPROVED, no blocking
findings**. The reviewer inspected actual harness/tests against baseline
`3b3ef8c`, reported matching scope hashes `771c8b54…bc97de` (harness) and
`e9f197ab…d48872` (tests), and confirmed those hashes unchanged after review.
The reviewer made no edits or commits. This is approval of that reviewed code
increment, not future edits or native acceptance.

The review confirmed the approved capture, parser, schema, invocation and
interruption contracts: None versus empty holder, bounded pread without shared
seek, cleanup guard and completed flag, bounded whole-buffer parser, null
unobserved fields and atomic diagnostic update, narrow parser guard, fixed
verbose invocation with unchanged production timeout/environment, refusal
passthrough, fixed deep-provenance refusal and existing signal restoration.

Six useful findings were delegated for incorporation before the proof and
follow-up integrated review. Their disposition here is **planned/in progress**,
not a claim that fixes or post-fix tests have completed:

| Finding | Follow-up refinement |
| --- | --- |
| 1. 200 ms fixture deadlines can expire before Python prints starts on a slow runner | Three-second startup allowance or explicit readiness synchronization in synthetic tests only. |
| 2. Grandchild test did not combine timeout with failure capture | Add real timeout plus holder plus writing-grandchild regression, asserting original refusal, capture bound and cleanup. |
| 3. Traceback frames retain a holder after del of caller reference | Set holder.data=None before deleting it, clearing bytes through all holder references. |
| 4. Stray OK without Ran could falsely report zero skipped | Accept final status/skip facts only after the last anchored Ran summary; otherwise keep skipped null. |
| 5. Command/environment assertion checked too little | Assert full fixed suite argv and exact expected strict supervisor environment. |
| 6. Report-window unit test assumed supported non-root host | Mock root/target admission only inside that fixture so the intended reporting path is tested portably. |

## Validation actually run by the reviewer

Environment: Linux, Python 3.12.3.

- ProgressDiagnosticTests: 17 tests passed in 1.27 seconds.
- Whole focused test file: 79 tests passed in 2.86 seconds.
- Real unittest verbose fixture with docstring, partial stdout, skip and
  subtests parsed three starts, last test_sub, completed count 3 and skipped 1.
- Dense 2 MiB valid-start parser probe: 61,680 matches in 0.14 seconds;
  long-identifier input took 0.03 seconds.
- Confirmed diagnostic limitations: identifier-ending partial stdout can leave
  a stale hint, and the initial stray-OK behavior motivated finding 4.

The reviewer did **not** rerun the full 495-test suite or symlinked-path run;
those were author-reported evidence, not independently repeated here. No
network, keychain, CI, native target or macOS execution was performed. No
ripwire quality run was performed; the reviewer referenced the author's
reported 28 non-gating findings. Reading macOS-compatible APIs is not evidence
of a native pass. The review does not supply the required three native managed
proof passes or resolve the separate configured macOS confinement SIGABRT.

The [design](../phases/managed-runtime-test-progress.md) now records the six
refinements. Final integrated review and post-refinement validation are pending;
no acceptance gate, production timeout, privilege or raw-output boundary changes.

## Follow-up integrated review

Actual reviewer model **`claude-opus-5-5`** explicitly **APPROVED for publication
as diagnostic harness code**, with no blockers and two nonblocking test-coverage
findings. Scope: the harness/test diff against `3b3ef8c`, plus design/review
documentation. Reported matching hashes before and after review were
`fbe6d7a7…a8a` (harness) and `750a7eee…381` (tests). The reviewer made no edits or
commits. This approval applies to the reviewed source, not subsequent test edits,
native acceptance or a production managed installer.

All six initial refinements were confirmed implemented: three-second synthetic
startup deadlines; combined real timeout/holder/writing-grandchild plus success
case; clearing holder.data in finally including retained mock references;
status-derived skipped only after the last Ran summary; full argv/exact env
assertion; and a portable report-window fixture that mocks the first root check
and target lookup while preserving later real ownership checks. Its home
snapshot is mocked, avoiding real HOME reads or changes in that unit test.

The reviewer reconfirmed the capture/parser/report/privacy/interruption
contracts, unchanged 900-second child timeout, 1320-second total deadline,
2 MiB output ceiling, discovery and acceptance. No regression was found. macOS
portability was assessed by reading APIs and guards, not by macOS execution.

Follow-up validation actually performed on Linux with Python 3.12.3:

- Focused file normally: 79 passed in 11.56 seconds.
- Focused file through symlinked repository path: 79 passed in 11.48 seconds.
- ProgressDiagnosticTests repeated three times: 17 passed each, about 10 seconds.
- No test processes were present at review start; focused runs left no owned
  managed scratch directories behind. Other agents' review-wrapper files were
  distinguished from runtime directories.
- Actual nesting-depth and helper-provenance probes: 30,000 nested JSON levels
  (60,001 bytes, within the 64 KiB cap) raise RecursionError and produce the
  fixed refusal with context suppressed.

Two useful **test-only** gaps are being corrected before a narrow final review:

1. The existing 2,000-level JSON fixture parses on Python 3.12 and fails through
   a type check, so it does not prove the RecursionError catch. Use 30,000 levels
   and prove the intended failure path rather than merely the same category.
2. The exact-env test's identity supervisor stub cannot detect bypassing the
   returned environment. Make that stub add a sentinel and assert its returned
   environment is the one used by run. No production environment change follows.

The harness remains the approved unchanged source while these fixture edits
are made. Final narrow test-only review is pending. The reviewer did not run the
full tools suite, packaging/home-snapshot proof, Python 3.13.13, network, CI,
keychain, macOS or native targets. Author-reported proof runs were not repeated.
All three native passes, required Linux namespace tests and the separately
unresolved configured macOS SIGABRT remain outstanding and unwaived.

## Final narrow test-only approval

Actual reviewer **`claude-opus-5-5`** explicitly **APPROVED publication of the
test-only follow-up as diagnostic code**, with no remaining code findings.
The reviewed harness remained the previously approved unchanged `fbe6d7a7…`
source; final test SHA256 is
`f4b3b25391e631115a88555091d38b026acc4df29acc404a8700b081ac64c332`.
The reviewer confirmed hashes unchanged before/after its review, made no edits
or commits, and confirmed neither correction weakens tests, environment or gates.

The provenance fixture now first proves plain json.loads on its 60,001-byte,
30,000-level input raises RecursionError, then requires the exact fixed refusal.
The supervisor stub returns a copied environment plus a fixture-only sentinel;
the exact-env assertion proves run receives that returned environment. The
sentinel exists only in tests; real supervisor behavior is unchanged.

Reviewer-run checks on Linux/Python 3.12.3:

- Both focused tests passed in 0.003 seconds. An initial isolated-import attempt
  failed because `-I` excluded the module directory; the successful invocation
  omitted `-I`. That invocation detail is not managed-interpreter validation.
- Mutation in a temporary copy removing the RecursionError catch made the deep
  provenance test fail with uncaught RecursionError.
- Mutation in a temporary copy discarding test_supervisor's returned environment
  made the exact-env assertion fail on the missing sentinel. Temporary mutation
  copies were deleted; repository files stayed unchanged.

The narrow reviewer did not rerun the full focused file, symlinked path, full
tools suite, packaging proof, managed Python 3.13.13, network, CI, keychain, HOME,
macOS or native targets, and did not repeat author runs. No other test processes
were running at review start. Interpreter recursion-limit variation remains a
fixture limitation: if a future interpreter accepts 30,000 levels, the explicit
precondition fails loudly rather than silently bypassing the intended catch.
The subsequent actual Trial17 is pending; it is not evidence in this approval.
All three native proof passes and required Linux namespace tests remain required;
the separate configured macOS SIGABRT remains unresolved and unwaived.

## Final evidence and publication approval

Independent Claude Code in `--permission-mode auto`, actual model
`claude-opus-5-5`: **APPROVED for evidence and publication**, no blocking findings.
The reviewer confirmed the final approved source hashes remained unchanged,
the Trial17 report was byte-identical to its original with SHA256
`0b2be9f56437a75248e8db50716cfc46ae85c373a85f7eab2feec3e1bf6ccfa5`,
and all companion source hashes matched. It verified 495 tests, 7 skips, exit 0,
495 observed starts, all ten assertions, enforced offline namespace, complete
cleanup, unchanged monitored home metadata and no unmet executable cases.

It independently compared each hosted run 41141651561 aggregate report with its
downloaded original, checked sizes/checksums and job metadata, and confirmed
the companion accurately preserves all three failed managed outcomes alongside
three passing installer jobs. The old hosted source `3b3ef8c` does not validate
the new native progress diagnostics; its 900-second timeout is established by
that source, not a report field. Local Trial17 does not waive native failures.

The reviewer checked specification accuracy and publication privacy using local
file/JSON/checksum operations only. It ran no tests, proof, network, CI, keychain
or native execution and made no edits or commits. Its requested follow-up was
to replace the specification's pending-evidence wording with this approval;
that wording and a cosmetic extra blank line were corrected. Native acceptance,
the separate configured macOS SIGABRT and production managed installation remain
open. Earlier pending statements above describe their respective review times.
