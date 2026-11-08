# Native HTTPS test fixture portability correction

Approved Part 1 only by actual independent Claude review; see [review evidence](../reviews/streaming-native-https-fixture.md). The qualification chain has stopped. Existing phased implementation authorization persists. This fixes one test, not the 21–22 failures across about ten legacy modules. The separate macOS diagnostic workflow was not approved and is excluded.

## Observed failure and scope

Legacy managed run 42416166342 at source 50ad3d1 failed the full tools suite on all three platforms. Each sanitized report lists all four subcases of `test_marmish_native_https_check.HttpsInstallerContracts.test_cell_dispatch_cleanup_and_busy_lease_or_runtime_preserve_state`. The hosted full-suite traceback was captured internally but not retained in the uploaded artifacts. Therefore the exact original hosted assertions remain unobserved.

A bounded local reproduction of that exact test, with ambient `TMPDIR` set to an owned `/tmp/marmish-managed-<8>/tmp`, ran one test and failed four subcases in 0.005 seconds. The ordinary Iris and GitHub cases failed `status == 0` with the actual cell report `failure=runtime_path_length`, `stage=context`. The busy-lease and runtime-present cases failed `fetch.call_count == 2` because fetching never began. No real installer, product command, network transport, or relay was executed: the existing test mocks those boundaries. The temporary reproduction tree was cleaned.

Private traceback: `/tmp/marmish-https-long-tmpdir-repro.log`, mode 0600, SHA256 `1a09a9b1d89d778eee080362d13c64969dce5abe99750b5eba34d01201ac3f50`. The earlier default-ambient focused result is `/tmp/marmish-legacy-50ad-q8wam5nl/focused-2.log`: one test passed, covering the same four subcases.

The managed supervisor sets `TMPDIR=<managed-root>/tmp`. The test uses an ambient `TemporaryDirectory()` as mocked account `pw_dir`. The cell creates another `.mh-<8>/t` below that directory and deliberately requires `len(path.encode()) + 53 < 104` before invoking the mocked installer. Typical managed nesting yields path length 60 on Linux or 68 on macOS, exceeding the socket budget. The local reproduction establishes this fixture defect, without claiming that every original hosted failure has now been diagnosed.

## Minimal implementation boundary

Change only the outer `TemporaryDirectory` allocation in the named HTTPS test to use a short prefix and an explicit canonical platform temporary parent: resolved `/tmp` on Linux and resolved `/private/tmp` on macOS. It remains an automatically cleaned, private, uniquely owned directory. Keep the account mock pointing at that owned root, and let the real cell create its own `.mh-*/t` descendants exactly as before. Do not mock away the path guard or filesystem operations.

Leave production `marmish_native_https_check.cell` unchanged, including the socket-path budget. Preserve all four `(source, locked, runtime)` cases and every existing assertion: exit status, owned-state removal, both source fetches, absence of private paths and npub in reports, and busy state preservation. Do not add retries, skip the test, weaken expected cleanup, change production path selection, or introduce a general temporary-directory abstraction.

Validation after authorization: run the exact named test once under the default ambient environment and once under a newly owned managed-style long `TMPDIR`, in separate Python processes so tempfile caching cannot confound the result. Each must report exactly one executed test, zero failures/errors/skips, and execute the unchanged four subcases. Retain private command/status/traceback logs and hashes, clean owned temporary trees, run the relevant Python syntax/static checks and diff check, and obtain independent Claude code review before publication. These results are tooling-unit evidence, not installed product or relay qualification.


Implementation conditions from the approved review: resolve the explicit short
temporary parent with `resolve(strict=True)`, with no ambient fallback. Keep
the prefix at most eight characters; with `mh-` on macOS the nested path is
approximately 39 bytes, plus the reserved 53 bytes gives 92, below the strict
104-byte limit with eleven bytes of remaining allowed margin. State the margin
in the code comment. Local tests cover Linux only; the macOS branch requires
hosted execution and is not established by candidate Rust tests.
