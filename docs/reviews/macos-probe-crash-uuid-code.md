# Crash UUID/path attribution code review

Independent Claude Code Sonnet, auto mode, offline tests allowed.

## Initial review

**CHANGES NEEDED.** There is one narrow blocker, in how evidence is exported. The parsing, matching, caps and refactor all check out.

I read the diff, the unchanged `read_context`, `wait_context` and `extract_frames`, both docs and the original attribution. I ran the offline tests: 22 pass. I made no edits and ran no git, CI or live commands.

## Blocker

**Stale `path_shape` leaks across candidates.**
- **Cause:** `wait_context` does `result.update(context)` in its loop (`tools/marmish_macos_crash_diagnostic.py`, around line 298). Before this change only a `matched` context carried extra keys, and the function returned immediately on a match. A `candidate_path_rejected` context now carries `path_shape`.
- **Reproduction:** I used two offline candidates. `mp-…_a.ips` has the right PID but the wrong path. `sandbox-exec-b.ips` has the wrong PID. The result was `category: candidate_pid_rejected` with the `path_shape` from the first candidate.
- **Effect:** The category and the shape come from different reports. The rerun exists to establish the OS path representation from evidence, so this is misleading evidence.
- **Fix:** Drop `path_shape` before each `update`, for example `result.pop('path_shape', None)` at the top of the per-candidate loop. Then add a test with two candidates, one path-rejected and one PID-rejected, asserting that no stale `path_shape` survives.
- **Related, existing:** The same overwriting also hides an earlier path-rejected category. Keeping that one is a design choice, but it should be deliberate.

## Verified OK

- **Mach-O parsing (`parse_macho_uuid`, `read_owned_uuid`):**
  - The read is capped at 4096 bytes with `O_NOFOLLOW`, regular-file and current-UID checks, and `O_NONBLOCK`, and the fd is closed in `finally`.
  - Magic, CPU type only (subtype is ignored) and command count (1 to 128) are validated.
  - Every command needs length ≥ 8, 8-byte alignment and to sit inside `sizeofcmds`, so zero-length commands cannot loop.
  - Walking the commands must end exactly at `end`.
  - There must be exactly one `LC_UUID`, of length 24 and non-nil.
  - `struct.error` cannot occur because the bounds are checked first, so the `(OSError, ValueError)` handlers are enough.
- **Fatal before children:** UUID failure in `collect` sets `probe_uuid_invalid` and returns before `run_child`. The test confirms `run_child` is never called and the exception text does not reach the report.
- **Matching:**
  - The PID check still comes first and uses an exact `int`. The candidate must also be new against the `before` snapshot.
  - Exact and sandbox-exec paths are unchanged. The alias path is absolute, has no dot or empty components, has no `*`, and is resolved strictly and read-only. The resolver's exceptions are swallowed.
  - Redaction accepts only the four literal shapes ending in the unique basename, and also requires `procName` and `uuid_image_matches`.
  - `uuid_image_matches` requires exactly one `usedImages` entry with that name. If `path` is present it must end in `/basename`. `arch` must equal the parsed architecture, and the `uuid` must be a bounded string. UUIDs are compared as objects, so case does not matter.
- **No raw path or UUID export:** `path_shape` is only an enum, capped ints and booleans. `attribution` is a fixed label. Tests assert that neither the path nor the UUID appears in the shape or the context.
- **Refactor compatibility:** `extract_named_fields`, `extract_termination` and `extract_asi` reproduce the old `extract` exactly. That covers key names and order, the 6-item `asi` cap, the `code` and `details` handling, and non-dict inputs. The workflow YAML does not validate report keys, and nothing else calls `extract`.

## Non-blocking

- The old strict assertion `read() == {'category': …}` became `['category']` for path rejections. It is justified, but a separate assertion that `path_shape` is present on `candidate_path_rejected` would restore the coverage.
- There is no test for a non-regular probe (FIFO or directory) in `read_owned_uuid`. The code handles it.
- A ≥4096-byte file is covered only through `parse_macho_uuid`, because `os.read` truncates. This is acceptable.
- `uuid.UUID(str)` accepts non-canonical forms such as braces or no hyphens. This is harmless, since equality with the owned UUID is still required.
- The prior native report is consistent with this design. The code infers no cause, and the doc calls redaction or aliasing a hypothesis.

Fix the blocker, add the test, and I'd approve.

## Disposition

Fixed stale evidence by replacing the candidate result atomically, including
directory failures and candidate limit. Added two-report regression (path rejection
followed by PID rejection); no stale path_shape survives. Combined tests:63 run,
57 passed,6 native-only skips. Independent re-review approved the integrated fix.

## Independent re-review

Claude Code Sonnet, `--permission-mode auto`, verdict **APPROVED**.

**APPROVED.** I found no remaining blockers.

- **Atomic replace:** `wait_context` (`tools/marmish_macos_crash_diagnostic.py:296-300`) now builds a fresh `result = {'candidate_count': len(seen), **context}` for each candidate. No `path_shape` or `facts` from an earlier candidate carries into a later one, and a match returns immediately.
- **Evidence reset:** A directory open failure (line 288) replaces `result` outright. The `candidate_limit` path (line 295) returns a bare category and count.
- **Regression test:** `test_rejected_candidates_never_mix_shape_and_category` has two candidates, a path reject followed by a PID reject. It expects exactly `{'category': 'candidate_pid_rejected', 'candidate_count': 2}`, so a stale `path_shape` would fail it.
- **Offline tests:** The suite ran 23 tests and all passed.

**Non-blocking nit:** If a transient directory failure is followed by a successful open with no new candidates, the earlier failure category stays. It is not stale evidence, and it is arguably the more informative category than `context_timed_out`.
