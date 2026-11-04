# Native background daemon code review, final round

Actual Claude Code `--permission-mode auto`, source frozen.

Session: `a4bc8c96-bc8f-45b9-b286-ecca47bbb8b4`. Model: `claude-sonnet-5-5`.

**Reviewer model:** Sonnet 5.5 (`claude-sonnet-5-5`). I made no edits, no git changes and no Cargo runs, and I did not test live. I ran `rustfmt --check` on `daemon_launch.rs` and `diagnostics.rs`, which is clean, and read the source, docs and `/tmp` logs.

## Verdict: APPROVED (source)

I found no blocking code defects in the frozen source against the approved design (design-r2 APPROVED). Approval is conditional on the final evidence below, because I can't produce or verify it myself.

## Checked and sound

- **fd reservation:** `spawn_child` dups the child socket to `F_DUPFD_CLOEXEC` ≥3 before `Command` sets up fd 0/1/2, so closed stdio can't overwrite it.
  - `inherit_channel` also handles `source == 3`, since `dup2(fd,fd)` doesn't clear CLOEXEC.
  - Rust's own stdio setup does not clobber a log or `/dev/null` descriptor that lands on fd 0–2.
- **Corrected closed-stdio fixture:**
  - It makes its `private_root` and `append_log` calls and backs up stdio before closing fd 0/1/2.
  - Nested `append_log`, `spawn_child` and `await_startup` run while stdio is closed, with no panic path. Stdio is restored only after the handshake completes.
  - The outer test then requires R/A readiness within 10s, so a nested failure surfaces as EOF and fails the test. The fixture is meaningful.
- **Child entry:** the marker is removed, then the socket type, `AF_UNIX`, peer uid and ppid are validated. CLOEXEC is set, descriptors above 3 are closed, and this all happens before Tokio.
- **Logs:** the opener is `O_APPEND|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK` with mode 0600, then checks nlink, uid and mode, then clears `O_NONBLOCK`. The profile lease is dropped before spawn. The test covers permissive, hardlink, symlink, directory and FIFO files.
- **Finite errors:** frames are printable ASCII, at most 256 bytes, with an invalid-frame fallback, and the `DaemonStartup` arms are added everywhere. Printing is non-panicking, and retry notices are rate-limited to one per 30s.
- **Parent ack, loss and cleanup:**
  - R is sent only after activation and the first snapshot. Parent loss before R or A makes `ready()` error, so the child does its joined cleanup.
  - Only the owned child is signalled: SIGTERM, then a 45s grace, then kill. An error frame waits for a normal exit.
  - If the waiter-thread spawn fails, the child is torn down. Worst-case launch is 285s, under the 300s client deadline.
- **Client race:** the autostart loser's `profile_in_use` is tolerated from both the parent lease and the child F frame. The string match is tied to a test that asserts the exact text, and `command_socket` polls to the shared deadline. `--no-autostart` and explicit sockets still never launch.
- **Foreground fixtures:** all three native supervisors pass `--foreground`. The CLI rejects `--foreground` outside daemon operation. `spawn_service` has no remaining references.

## Gates and findings

- **Final evidence is not yet recorded (blocks the commit, not the code).**
  - **Suite:** the running suite binary was built at 11:16:14, before the 11:18:38 fixture fix, so its `closed_stdio…FAILED` line is the old ordering bug.
  - **Clippy:** the `/tmp` clippy log is the old failing `ineffective_open_options` run.
  - **Live JSON:** the live PASS JSON (11:10) predates the final source edits.
  - **Doc:** `docs/validation/native-background-daemon.md` ends with "recorded below when complete" and nothing follows.
  - **Before commit:** record fmt, `clippy --all-targets -D warnings` (the project's pre-commit rule), the corrected sequential full suite, and the live repeat. State the feature set: the doc says 370 tests, while the current run is 471 with `--all-features`.
- **Nonblocking, `close_unrelated_fds`:** it makes one `close()` per fd up to `RLIMIT_NOFILE`, which can add roughly 40–85s per child start when the soft limit is near 1e9. `close_range` or `/proc/self/fd` would fix it.
- **Nonblocking, EOF race:** after EOF the parent calls `try_wait` immediately, but the kernel closes the sockets before the child becomes waitable. The message can read "unavailable or timed out" instead of "exited before readiness (exit N)". A short bounded wait on EOF would fix it.
- **macOS:** `validate_peer` is not compiled or run there. Don't publish or tag until the macOS CI result is recorded. The docs correctly claim no macOS PASS.

## Disposition

The source is approved, with evidence gates completed before commit. Strict all-target/all-feature Clippy passed in `/tmp/marmish-nqm-final-clippy.log`; the reviewer inspected the older failing log. Final sequential suite/live results are recorded in the linked validation report. The stopped intermediate review session has no claimed verdict.

No binary release or tag is authorized by this increment. Source publication is necessary to run the existing GitHub platform workflow; publishing reviewed source for CI is distinct from publishing a qualified binary release. macOS results are recorded separately and remain a release gate. The descriptor-close loop and finite EOF diagnostic race remain nonblocking findings.
