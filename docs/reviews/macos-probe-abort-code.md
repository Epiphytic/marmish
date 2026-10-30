# macOS probe diagnostic code review

Independent Claude Code, Sonnet, permission mode auto.
Scope: integrated driver, tests, workflow and specification; offline tests permitted.

**APPROVED**

I found no concrete blockers. The driver, its tests and the workflow match the approved `docs/phases/macos-probe-abort.md`. Native reproduction validity is unproven until the workflow is dispatched, so I have not inferred runtime acceptance.

**What I ran:** the 13 new tests pass, offline and mocked. The existing `test_marmish_native_spike` and `test_marmish_interop` suites also pass (30 tests, 1 skipped). `git diff` shows no change to `marmish_transport_macos.py`, `tools/native/` or any production file. The only references to the driver are its own test and the new workflow, so it is outside `DARWIN_NATIVE` and the release file lists.

**Checked against the doc**
- **Recipe:** `build_probe` runs exactly `clang -Wall -Wextra -Werror -O2 -mmacosx-version-min=15.0 SOURCE -o EXEC`, then `codesign --force --sign -`, then `codesign --verify --strict`. A test pins these argv lists, and the flags are recorded in the report. Build failures become fixed categories (`*_failed`, `*_timeout`, `*_unavailable`) with a numeric status and no raw text.
- **Command construction:** the driver uses a construction-only `DarwinConfinedDaemon.__new__` fixture with `_directory`, `home`, `socket` and a stub `_broker.address`, and never calls `start` or `_prepare`. Confined commands come from the production `_command`, and tests assert they equal its output. `HOME` and the control directory are siblings, so the profile, probe and CONFIG/ACK paths sit outside `HOME`. Profile bytes are asserted equal to the imported `PROFILE` before hashing.
- **Cases:** the five cases run in the specified order and continue after failures. A confined probe case is marked `control_invalid` when its matching control fails. `system_true_confined` has no control, as the doc allows. TCP targets the same owned loopback listener port in both the control and confined cases.
- **Subprocess options:** `stdin`/`stdout` are DEVNULL, `env` is exactly `{'PATH': '/usr/bin:/bin'}`, `close_fds` is true and the timeout is 5 seconds. `subprocess.run` kills and reaps a timed-out child.
- **Report capture:**
  - Stderr goes to a private temp file and at most 4097 bytes are read.
  - Output is decoded with replacement and control characters are stripped except newline and tab.
  - Redaction runs before the cut to 4096 bytes, with partial-token removal at the capture boundary and a truncation flag.
  - Tests cover the partial boundary and the byte bound.
  - The report is written with `O_EXCL|O_NOFOLLOW` and mode 0600, outside the fixture and before cleanup. It is written on signal, timeout and build failure.
  - A confined abort leaves `ok` false and the exit status 1, so the original abort is not relabeled as success.
- **Cleanup:** the listener closes via `with`, the fixture is a `TemporaryDirectory`, and the tests assert the report exists before the fixture is removed.
- **Workflow:**
  - It triggers on `workflow_dispatch` only, on hosted `macos-15`, with `contents: read`, a 10-minute timeout, `persist-credentials: false` and no caches or secrets.
  - Both action SHAs match the pins in the existing workflows.
  - The artifact upload runs under `if: always()`, uploads only the report path, and errors if the file is missing.
  - A missing python3 is reported as a fixed category with exit 1.

**Non-blocking notes**
1. The report key is `schema` in the driver but `format` in the doc and in the workflow's `python_missing` fallback. Align them before consumers parse the report.
2. The profile equality check in `make_fixture` uses a bare `assert`, which `python -O` would strip. Nothing runs it with `-O`, so a plain `raise` would be sturdier.
3. The partial-token redaction can miss a non-ASCII token cut mid-codepoint, because the trailing replacement character defeats `endswith`. Native paths are ASCII, so this is theoretical.
4. The `report_outside_fixture_required` check compares an unresolved `--output` with the resolved fixture root. A `/tmp` alias path could bypass it. The workflow uses `$RUNNER_TEMP`, so this isn't reachable today.
5. As the design review already noted, an empty stderr from the confined cases will be inconclusive. That is expected and needs a follow-up investigation, not a guess.

Acceptance still depends on dispatching the hosted run and checking that both controls pass. It should also be read together with the `/usr/bin/true` confined result.

## Disposition

Aligned missing-Python fallback schema, replaced the optimizable profile assertion
with explicit equality failure, and resolve output aliases before checking fixture
containment. Offline tests rerun: 40 run, 34 passed, 6 native-only skips.
Non-ASCII capture-boundary redaction limitation is confined to the synthetic
hosted ASCII fixtures; this driver never captures real identity/daemon stderr.
