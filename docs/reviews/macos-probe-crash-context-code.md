# Owned probe crash-context code review

Independent Claude Code Sonnet, permission mode auto; offline tests permitted.

**APPROVED.** I found no blockers against `docs/phases/macos-probe-crash-context.md`. This is a code and offline-test approval, not runtime acceptance. Native evidence is still expected from the dispatch.

I ran the two offline suites together (`PYTHONPATH=tools python3 -m unittest test_marmish_macos_crash_diagnostic test_marmish_macos_probe_diagnostic`): 29 tests, all pass. I made no edits, ran no git writes or CI, and touched no live identities or keychain.

**Checked against the design**
- **Prior driver:** `tools/marmish_macos_probe_diagnostic.py` is unchanged against HEAD. The new driver imports its helpers instead of copying them, and the helper behavior is unchanged.
- **Production reuse:**
  - The fixture comes from `base.make_fixture`, which uses the unchanged `PROFILE` and `DarwinConfinedDaemon._command`.
  - The probe is built with the same compile, sign and verify recipe in an owned canonical `/private/tmp/marmish-crash-*` directory.
  - The probe name is `mp-` plus 8 hex characters from the random fixture name, 11 characters in total, with a test on the length.
- **Runtime gates and children:**
  - It enforces the Python 3.11+, Darwin and non-root gates.
  - The control read runs first, then the confined read, sequentially.
  - `Popen` uses fixed `PATH`, `close_fds`, DEVNULL stdin and stdout, and a private stderr capture. Stderr is redacted and capped.
  - The child gets `wait(timeout=5)`, and the `finally` block kills and reaps it. The child PID is used for matching and is popped from the report.
- **Report attribution:**
  - It looks for `.ips` files only after a SIGABRT.
  - It takes a snapshot immediately before the confined run and accepts only new files that match the probe prefix or `sandbox-exec-`.
  - The reports directory is opened with `O_DIRECTORY|O_NOFOLLOW` and checked for UID ownership. It is never created, and it is re-checked on each poll.
  - Candidates are opened relative to the directory fd with `O_NOFOLLOW|O_NONBLOCK`. The code requires a regular file owned by the current user and reads at most 2 MiB plus one byte.
  - Parsing requires the two-object form, an exact integer PID equal to the `Popen` PID (bools rejected), and `procPath` equal to the probe path or `/usr/bin/sandbox-exec`.
  - The report records only the fixed `probe` or `sandbox_exec` category, never the path.
  - More than eight candidates gives `candidate_limit`, and polling stops after 20 seconds.
- **Extraction:**
  - Extraction is allowlist-only, covering exception, termination, `asi` for the six named libraries (at most 6 values) and faulting-thread frames (at most 24).
  - Frames carry only a symbol and an image basename.
  - Each string is redacted before it is capped, and anything over 64 KiB becomes `<oversized>`.
  - Everything else (`byProc`, `coalitionName`, `environment`, UUIDs, addresses, metadata) is dropped. The tests assert this.
- **Report:** It is written exclusive with mode 0600 through `base.write_report`, inside the fixture's `with` block so it exists before cleanup. It is finite, and the output path is rejected if it falls inside the fixture. The report is written even when the confined child aborts, and the exit status stays nonzero.
- **Workflow:**
  - The diff is 29 added lines and no deletions. Permissions, timeout, concurrency and pins are untouched.
  - The prep step (`id: crash_report`) and run step (`id: crash_diagnostic`) both use `if: always()` and `umask 077`, write to a separate private directory, and run after the first diagnostic.
  - The upload uses the distinct name `macos-owned-probe-crash-context`, `if: always()` and `if-no-files-found: error`, and uploads only that one report.
- **Scope:** The new driver is referenced only by the workflow and its own tests, so it is not in production diagnostics or release file lists.

**Non-blocking suggestions**
1. `candidates()` and `open_reports()` don't catch `OSError` from `os.listdir(fd)` or `os.fstat`. This is unlikely on a hosted runner.
   - If the failure happens in the pre-run `snapshot`, the confined run never executes.
   - If it happens in `wait_context`, `diagnose` still writes a report, but it says `fixture_failed` and the case statuses are lost.
   - Catching `OSError` there and returning a fixed `reports_dir_unavailable` category would keep the evidence.
2. Already-seen rejected candidates are re-read on every poll. That is bounded at 8 files × 2 MiB × about 21 polls, so it is only an efficiency note.
3. If the `python_missing` fallback runs after the prep step failed, `REPORT_PATH` is empty and the `> ""` redirect fails. The first diagnostic step has the same behavior, so I'd leave it.

## Disposition

Approved integrated code retained for native diagnosis. Nonblocking directory-I/O
resilience and bounded rejected-candidate rereads do not affect the healthy
hosted fixture; failures remain non-successful. No production permissions changed.
