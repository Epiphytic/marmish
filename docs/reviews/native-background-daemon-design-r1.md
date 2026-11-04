# Native background daemon design review r1

Reviewer: Claude Code, actual model `claude-sonnet-5-5`, auto permission mode.
Session: `f0877561-e936-42ab-ba8c-f27f1fa5685b`.

**Verdict: not approved.** Six blocking flaws, each with a fix, followed by non-blocking items.

Model: Sonnet 5.5 (`claude-sonnet-5-5`). I did not check whether it differs from the design's author model.

Scope: static read-only review. I did no edits and no Cargo runs. Files read:
- `docs/phases/native-background-daemon.md`
- `cli.rs`: `run`, `main`, `command_socket`, `spawn_service`, and the tests
- `daemon.rs` in full
- `identity.rs`: `ProfileLease`, `open_private`, `check_dir`, `socket_path_for`
- `error.rs`, `diagnostics.rs`
- A grep of every site that spawns `marmishd`

I did not read the `ipc.rs` server, the worker spawn path, or `mdk_adapter`, so worker stdio inheritance is unverified.

## Blocking

**B1. The 180 s budget is shorter than the real worst case, and the SIGTERM grace is meaningless early on.**
- `daemon::open` runs `prepare` for up to 30 s, then `MdkOwner::open`. `activate` then gets 150 s, and `stop_reporting` gets another 30 s plus an unbounded `pending.finish()`.
- Worst case is therefore well over 180 s. The parent can SIGKILL a child that is mid-joined-cleanup, or one that is about to report ready.
- The SIGTERM handler is only installed inside `serve_owner` (`daemon.rs:238`). During `open()`, SIGTERM uses the default disposition and kills the process with no joined shutdown.
- **Fix:**
  - Give the ready deadline as `open` budget + 150 s, with explicit margin, for example 240 s.
  - When an error frame arrives, wait for child exit with a ≥45 s grace. Never kill a child that has reported an error.
  - SIGKILL only when no frame has arrived by the deadline, after a SIGTERM grace.
  - State that pre-`serve_owner` SIGTERM is an immediate kill. Rely on `flock` release on process death, and test that crash recovery of `prepare` is safe.

**B2. Concurrent client autostart becomes a hard failure.**
- Today a second client's spawned daemon loses the lock silently, and the client keeps polling status. In the design, the launcher takes a brief lease. If another daemon or launcher already holds it, the launcher returns `profile_in_use`, and `command_socket` propagates it with `?`.
- The same happens if the child loses the race after the parent drops its lease. Two parallel `marmish host cmd` calls on a cold start would intermittently fail.
- **Fix:** in autostart only, treat `profile_in_use` from either the parent lease phase or the child's error frame as "another starter is active". Fall back to status polling until the shared deadline. An explicit `marmishd` start still returns it as an error, as it does today.

**B3. The log open cannot reuse `open_private(path, true)`.**
- `open_private` (`identity.rs:577`) opens with write and create but no `O_APPEND`, and with `O_NONBLOCK`. Every restart would write from offset 0 and overwrite the previous log, which is truncation by another route. The `O_NONBLOCK` status flag would also propagate to stdout and stderr.
- **Fix:** add a dedicated `open_private_append`:
  - Flags: `O_WRONLY|O_APPEND|O_CREAT|O_NOFOLLOW|O_CLOEXEC|O_NOCTTY`, with no `O_TRUNC` and no `O_NONBLOCK`.
  - Post-open `fstat` check for regular file, `nlink==1`, uid, and mode exactly 0600, reusing `open_private`'s checks.
  - Parent directory validated by `check_dir`.
  - Test: two starts leave both runs' bytes in the file.

**B4. The readiness channel is under-specified, and a spoofable internal argument is a security hole.**
- The design doesn't say how the child learns the fd. If it is a hidden clap flag, any local caller can pass `--ready-fd 1`, or any fd they hold, and make the daemon write frames to it.
- **Fix:**
  - Use a fixed fd number, for example 3.
  - In `pre_exec`, use `dup2(child_end, 3)`, or `fcntl` to clear CLOEXEC if the fd is already 3.
  - Create both ends of the socketpair with `SOCK_CLOEXEC` in the parent. This matters in the client, which is multithreaded and spawns other children.
  - The child validates `S_ISSOCK` and `SO_PEERCRED` uid == euid, and checks that the peer pid equals `getppid()`. It sets CLOEXEC before building Tokio. Any mismatch is rejected before providers are touched.
  - The child also closes all other inherited descriptors above 2, using `close_range` or an fd loop. Otherwise the caller's stray pipes and locks stay open in the daemon forever, which undermines "detached".
- **Parent loss:** `PDEATHSIG` doesn't exist on macOS. State that the child detects loss only when the ready write fails with EPIPE (Rust ignores SIGPIPE). At that point it must run joined shutdown and exit nonzero. This is bounded by activation time.

**B5. A child that exits before the readiness channel can report loses the real error.**
- Layout validation failure, `runtime_startup`, a panic, and any `main()` error path all go to stderr, which is now the log. The parent sees only EOF, which contradicts "actual startup errors returned".
- **Fix:**
  - `main()`'s `Err` arm must send the error frame whenever a ready fd is present. This covers every error after argument parsing.
  - On EOF with no frame, the parent reports "daemon exited before ready (status N)" and the log path. It does not echo log contents.
  - Frames carry only the finite `Error` Display text. That text is safe: `Error::Io` displays as "local I/O failed" with no path. Bound the length and strip control characters.
  - Exit code is 1 (the `failure_code` for daemon operation) with the identical `marmish: {error}` text.
  - Without this, `installed_version_lifecycle.rs:1909` (`--max-uptime 15 daemon` expected to be rejected) may see different output.

**B6. The supervisor inventory is incomplete, and a missed site silently detaches.**
- The design says "existing test supervisors add `--foreground`" without a list. A missed site would see the parent exit 0, leak a daemon, and lose its exact shutdown receipt. Sites found:
  - `installed_matrix.rs:126`
  - `installed_autoexit.rs:616`
  - `installed_version_lifecycle.rs:1909`, the negative-rejection test
  - `installed_benchmark.rs`, the `Daemon` struct and its spawn
  - `tools/marmish_installed_runtime.py:712`
  - `tools/marmish_e2e.py`
  - README `marmishd --debug` guidance at lines 178 and 190. It must become `--foreground --debug`, otherwise diagnostics go to the log.
- `marmish_native_keychain_check.py:317` uses `init`, which is unchanged.
- **Fix:** put this list in the design as the ownership contract. Add a test that spawns the installed `marmishd` with no `--foreground` and fails, to catch regressions.

## Non-blocking but should be specified

- **Argument forwarding:** say whether the re-exec forwards the raw argv with `--foreground` and the ready-fd marker inserted before the subcommand, or re-serializes `Args`. It should not `chdir`, because a relative `--state-dir` or `--socket` would break. Also say what the parent prints on success. Anything on stdout changes script contracts.
- **Restriction check:** put `--foreground` and the internal arg checks next to the `server_limit_operation` check (`cli.rs:247`), before any filesystem access. Mirror the `new_session_is_only_a_new_remote_command_option` test.
- **Writes to the log can panic:** `eprintln!` panics when stderr writes fail, for example ENOSPC on the log (`diagnostics::event`, `main`). Use non-panicking writes in the daemon path.
- **Debug log growth:** `daemon_transport/retry_pending` is emitted every 100 ms tick while degraded, so with `--debug` the log grows without bound. Rate-limit it.
- **Client waiter thread:** the client must keep its `wait()` waiter thread on the child so it doesn't leave a zombie.
- **Client deadline:** `command_socket` currently has a 15 s deadline that covers status polling. Say how the new budget interacts with `--wait-seconds` and the 255 exit code contract.
- **Worker and helper stdio:** `identity.rs:847` already uses `stderr(Stdio::null())` for the helper. Audit the worker spawn so nothing inherits the log fd with `Stdio::inherit`.

Once B1–B6 are in the design, I'd expect to approve on a re-review.

## Author disposition

B1–B6 incorporated in the revised design before implementation: expanded readiness budget and explicit cleanup/crash semantics; concurrent-autostart retry; safe append flags; fixed descriptor and authenticated bounded two-way handshake; main error forwarding; and an explicit native supervisor inventory. The benchmark uses the shared native matrix supervisor. Python installed-runtime/e2e tools retain frozen v0.1 semantics. Active log rotation remains an explicitly documented separate limitation. Actual r2 approval is required.
