# Native background daemon code review, round 1

Actual Claude Code review, `--permission-mode auto`.

Session: `4d07522e-3c90-4d4b-a11e-cbd24ee5fa96`. Model: `claude-sonnet-5-5`.

# marmish-nqm review: NOT APPROVED (1 blocking finding)

**Model:** Sonnet 5.5 (`claude-sonnet-5-5`). I made no file edits and no git changes. My only artifacts are `/tmp` scratch files and builds in the shared target dir.

## Blocking

**B1. `cargo clippy -D warnings` fails.** `crates/marmish-native/src/daemon_launch.rs:211` has `.write(true)` alongside `.append(true)`. Clippy reports this as `ineffective_open_options`, which the project's pre-commit rule treats as an error.

- **Fix:** delete `.write(true)`. `append` already opens `O_WRONLY|O_APPEND`, so behavior is unchanged.
- **Other lints:** this was the only lint. With it allowed, `clippy --all-targets -D warnings` on `marmish-native` was clean.

## What I checked and found sound

- **Before-runtime re-exec (`cli.rs main`):**
  - `--foreground` is validated before anything else, and the layout check runs before launch.
  - The parent re-execs `current_exe()` with `--foreground` before building Tokio or enabling debug output.
  - Child failures at layout, runtime and run time go through the reporter.
  - A clean exit before readiness sends `daemon_stopped_before_ready`. After acknowledgement, `failed()` is a no-op because the stream has already been taken.
- **Private append log:**
  - Open flags are `O_APPEND|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK|0600`, with a nlink/uid/mode check after open.
  - `O_NONBLOCK` is cleared and append is retained. The lease is dropped before spawn, and the child inherits the descriptor rather than the path.
  - The test covers a pre-existing 0644 file, hardlink, symlink, directory and FIFO, and checks that the FIFO is rejected quickly.
- **fd 3 two-way readiness:**
  - Child entry removes the marker, checks the socket and peer uid and ppid, and sets `CLOEXEC` before closing descriptors above 3.
  - `ready()` sends `R` and then requires `A` within 5s.
  - If the parent is lost before `R` or `A`, `ready()` errors, the daemon runs its joined cleanup, and nothing unacknowledged is left running.
  - `R` is sent only after `owner.activate` and the first active snapshot are published.
  - The `fd==3` collision is handled, and a test shows the channel doesn't survive an exec'd `/bin/sh`.
- **Cleanup:** only the owned `Child` is signalled, after a `try_wait` check. An `F` frame is waited on normally. Missing or malformed frames, and timeouts, get SIGTERM, then a 45s grace, then kill. The worst case is 240s plus 45s, which is under the client's 300s deadline.
- **Concurrent autostart:** a loser's `profile_in_use` comes back as an `F` frame and is tolerated. The shared deadline is kept, and launch runs in `spawn_blocking`. A direct `marmishd` against a running daemon still fails nonzero. This matches the design, including the fact that a client that lost the race never relaunches.
- **Foreground sites:** all three native supervisors now pass `--foreground`: `installed_matrix::Daemon::start`, `installed_autoexit`, and the negative case in `installed_version_lifecycle`. The remaining `marmishd` hits in the repo are the legacy Python/`tools` tooling, which the design says to leave alone.
- **Finite errors:** the `Error::DaemonStartup` arms were added in `command_check`, `command_local` and `daemon.rs`. Relayed text is validated as printable ASCII up to 256 bytes. Diagnostics and error printing use non-panicking writes. Degraded-tick notices are rate limited to one per 30s.
- **Log privacy:** every runtime child process either pipes or nulls its stdio (`marmish-mdk` pipes both, the identity helper pipes stdin/stdout and nulls stderr, the engine worker nulls all three). Nothing inherits the log, so the `agent-spec` claim holds.
- **Docs:**
  - The phase doc matches the code. It says there is no auto-rotation, and the README and `agent-spec` repeat that.
  - They keep published 0.2.2 behavior (foreground, no `--foreground` flag) separate from the new source.
  - `Cargo.toml` is unchanged, so the dev binary label stays 0.2.2.

## Tests run (local, offline, `CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target`)

- `cargo test --offline -p marmish-native --lib -- daemon_launch cli::tests`: 41 passed, 0 failed.
- `cargo test … -- daemon_launch`, repeated 5 times: 16/16 each time, about 0.28s, so no flakiness seen.
- `cargo clippy --offline -p marmish-native --all-targets -- -D warnings`: fails on B1 only.
- `cargo check --target aarch64-apple-darwin`: could not run here because the `ring` C build needs Apple's toolchain.
- Built the dev `marmish` and `marmishd` binaries, with no network and a temp state directory:
  - `marmish --foreground status` and `marmishd --foreground init` both exit 1 with `invalid input: foreground_operation`.
  - `MARMISH_INTERNAL_STARTUP=… marmishd --foreground daemon` without fd 3 exits 1 with `daemon_startup_context`.
  - `marmishd … daemon` on an uninitialised profile created `owner.lock` and a 0600 `daemon.log`. The child failure showed on the terminal as `marmish: local I/O failed` with exit 1, and the same line landed in the log.
  - I did not run the success path, since it needs network.
- I did not touch the live validation or the root suite.

## Nonblocking refinements

1. **`close_unrelated_fds`** loops `close()` from fd 4 up to `RLIMIT_NOFILE`. I measured about 40ms per million. Hosts with an unlimited or around-1e9 nofile limit (some Docker/systemd setups) would add roughly 40–85s to every child start, inside the 240s budget. Capping the loop, or using `close_range` or `/proc/self/fd`, would avoid this.
2. **EOF race:** on EOF, `try_wait` runs immediately, and the child may not be waitable yet. The message can then be "unavailable or timed out" instead of "exited before readiness (exit N)". This is analysis only, not measured. A short bounded wait on EOF (not on timeout) would make the documented exit-status message reliable. The test only covers the formatter.
3. **No automated test of the CLI default path:** `main`'s background branch and the autostart `profile_in_use` mapping aren't unit-tested. I only exercised the failure path by hand. Root's live validation must supply the success-path, active-status and joined-shutdown evidence before close.
4. **macOS unverified:** `validate_peer` (`getpeereid` and `LOCAL_PEERPID` on a socketpair) is not compiled or run on macOS. Record the actual macOS CI result separately, as the phase doc already says.
5. **Two small `current_exe()` points:** `launch` calls `current_exe()` again rather than reusing the validated path from the layout check. The child revalidates its own layout, so this is minor. Separately, the unsafe blocks lack `SAFETY` comments, though clippy doesn't require them.

After B1 is fixed, re-run clippy and `daemon_launch` and I expect APPROVED, provided root's live validation covers item 3.

## Disposition

B1 is resolved by removing the redundant write option. The closed-stdio socket descriptor edge found during implementation is hardened and regression tested before round 2. The autostart construction is extracted from status polling to reduce command_socket complexity. Root live Linux validation supplies success, concurrent autostart, byte preservation, log isolation and joined-cleanup evidence. Platform CI results remain separate from local Linux evidence. The linear descriptor-close loop and EOF diagnostic race are nonblocking refinements; no timing or platform support is inferred.
