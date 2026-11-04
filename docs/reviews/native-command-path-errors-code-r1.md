# Native command lookup code review — round 1

Actual Claude Code `--permission-mode auto` read-only integrated review.
Model: `claude-sonnet-5-5`. Session: `52e35ee8-ca2b-43f9-b849-8aec454b5494`.
Review includes the daemon startup-read correction and all tracked/untracked feature code.

**Verdict: NOT APPROVED.** There is one blocking finding, and it is likely a failing engine test plus an acceptance gap. Everything else is nonblocking.

Reviewer: Claude Code, Sonnet 5.5 (`claude-sonnet-5-5`), read-only.

## Checks performed
- I read the frozen design `docs/phases/native-command-path-errors.md`, the r2 approval, and the full source diffs for the files named below.
- I read all of `resolve.rs` and the diffs for engine `launcher`, `policy`, `lib`, `store`, `worker`, `model` and `admission`.
- I read native `command_engine`, `command_service`, `command_store`, `command_wire`, `command_cli`, `command_config`, `daemon_launch`, the prototype mapping, and the README, agent-spec, build-plan and background-daemon doc changes.
- I checked every `check_exec_restrictions` caller after it was made `#[cfg(test)]`. The remaining callers are in test modules or `cfg(test)` code.
- I confirmed Rust 1.95's std spawns through `libc::execvp` (`sys/process/unix/unix.rs:408`) after fork.
- I ran one non-building experiment: `env` on an executable text file with no shebang, in a temp directory.
- I ran no Cargo and no live relay tests.

## Blocking
**B1: ENOEXEC never surfaces on glibc or macOS, so `InvalidExecutableFormat` is effectively unreachable.**
- `spawn_child` (`launcher.rs`, around line 1141) relies on `spawn()` returning ENOEXEC. Because `pre_exec` is always set, std forks and calls `execvp`, and libc's `execvp` retries ENOEXEC through `/bin/sh`, as POSIX requires.
- I confirmed this on this host (glibc 2.39). `env` on a +x file containing `invalid executable` printed `…/prog: 1: invalid: not found` and exited 127. The file was run by `sh` instead of returning ENOEXEC.
- Consequences:
  - `worker/tests.rs::definite_launch_failures_have_finite_reasons` case 1 should fail. It uses `b"invalid executable"` at mode 0o700, spawn returns `Ok`, and the test expects `Err(NotStarted(InvalidExecutableFormat))`.
  - The design's acceptance item "invalid executable format" (launch tests plus the live relay test) cannot be met as implemented.
  - A bad-arch ELF also hits the `/bin/sh` fallback, so it is not a workaround.
- Options: sniff the header (`#!`, ELF, Mach-O) in `spawn_child` and return `InvalidExecutableFormat` before spawning, or avoid `execvp` fallback semantics. Alternatively, amend the design to say this reason is only reported where the kernel's ENOEXEC reaches the caller, and change the test and live-test expectations to match. The doc and design wording should reflect whichever you choose.
- The `#!/marmish-nonexistent-interpreter` case (reason 2) is fine. `env` returned ENOENT for it.

## Reviewed with no blocking finding
- **Resolver (`resolve.rs`):**
  - PATH order, skip-and-remember of unusable candidates, and unset/empty/relative entries behave as specified.
  - `faccessat(AT_EACCESS)` on the canonical path is compatible with macOS.
  - Limits are validated before and after lookup.
  - ELOOP and other filesystem errors stop the lookup.
- **Policy bypass:**
  - Native resolves, then runs `check_execute_request`, then issues the token. `Engine::submit` repeats the check after the existing-job lookup.
  - Allow layers match the canonical path only.
  - Deny layers match both the canonical path and `invoked_path`, and fuel for both is charged during preflight.
  - A denied first PATH hit stops the search, so a later entry cannot evade it.
  - Shell requests under a restriction return `CommandDenied`.
  - The wire carries only argv, so clients cannot send `ResolvedExec`.
- **argv0/multicall:** the launcher uses the canonical executable as the program and `arg0(argv[0])`. Both `store.rs` guards accept `ResolvedExec`, and the worker recheck is always on for it.
- **Replay and durable refusal:**
  - Known invocations skip resolution.
  - `execution_refusal` maps definite errors explicitly and no longer falls through to `Missing`.
  - Unknown errors map to `Failed`.
  - `Ok` with no durable facts maps to `Uncertain`.
  - `InvalidPolicy` becomes `command_policy_error`.
- **Wire and diagnostics:** validation allows only exact code/reason pairs and keeps the historical `noncanonical_executable` bounds. `launch_failed` values 1..=7 map explicitly and are not enum ordinals. Diagnostic strings are static and finite.
- **CLI finish semantics:** `LaunchFailed` goes through `terminal_exit` and is not a `status_problem`. The launch error is raised only after consume and Finish, and a test checks the request order. Refusal detail survives the submit, status, poll and resume paths.
- **`daemon_launch.rs` Darwin fix:**
  - `poll` plus `recv(MSG_DONTWAIT)` under the original deadline reads buffered bytes before inferring EOF on HUP.
  - It never calls `setsockopt` on a closed peer.
  - `poll`, `recv` and `MSG_DONTWAIT` are available in the libc crate on Apple.
  - `ACK_TIMEOUT` and the remaining write-timeout call (before sending `A`) are intact, and the removed `Read` import is not used elsewhere.
  - The amendment text matches the code.

## Nonblocking refinements
1. **Missing tests the design lists:**
   - A native-level test of more than 128 consecutive denials followed by allowed work. The current test only exercises `Engine::check_execute_request` directly.
   - A test that a denied selected PATH candidate does not fall through to a later allowed one.
   - An engine-known invocation staying attached after PATH, target or policy changes, using a real engine.
   - A test that a `pre_exec` EPERM or ENOENT is never reported as reason 1 or 2. The r2 review asked for this.
2. **Race mislabel:** if the executable vanishes between resolve and `check_execute_request` or `submit`, `require_canonical_exec` now returns `InvalidRequest`. The client gets a durable `invalid_request` instead of `command_not_found`. Mapping it to `ExecutableMissing` would be more helpful.
3. **Hardening:** `validate_request` does not check that `invoked_path` is consistent with `argv[0]`. Only native creates `ResolvedExec`, so this is defense in depth.
4. **Wrong label for chdir failures:** a child-side `chdir` EACCES is reported as `filesystem_permission` (reason 1). The design accepts conservative wording for cwd problems, but this one is mislabeled.
5. **Docs:** the README and agent-spec removed the statement that symlink spellings such as `/bin` may be refused, without saying that is still the behavior of published 0.2.2. The "published 0.2.2 predates" paragraph is accurate but easy to miss next to the new quickstart text. Neither doc mentions the `/bin/sh` fallback for files without a shebang.
6. **PATH validation order:** PATH size limits are only checked when `argv[0]` is a bare name. This is acceptable but worth stating in the doc.

## Tests for root to run after the current native run finishes
- `cargo test -p marmish-engine definite_launch_failures_have_finite_reasons` — expected to fail until B1 is fixed.
- Full `cargo test -p marmish-engine`, with attention to `resolve::tests`, `policy::tests`, `store::tests` (`denial_precheck…`, `launch_failure_is_persisted_only_from_launching`) and `worker::tests` (`resolved_program_preserves_original_argv0`).
- `cargo test -p marmish-prototype`.
- `cargo test -p marmish-native --all-features`, including `command_wire` (`lookup_error_reasons_are_closed_per_code`, `launch_failed_outcomes_have_fixed_finite_values`), `command_cli` (the four new tests), `command_store::executable_refusals_reopen_replay_and_release_active_budget`, `command_service`, and the `daemon_launch` tests `buffered_failure_is_preserved_after_child_exit` and `child_requires_parent_ack_and_rejects_inapplicable_context`.
- `cargo fmt --check` and `cargo clippy --workspace --all-features -- -D warnings`.
- Live relay run: nonexistent bare and explicit path, a non-executable file, and the invalid-format case. The invalid-format case must follow whichever B1 resolution you pick.
- macOS CI after the push, especially the `daemon_launch` tests.

After B1 is resolved, with a test or acceptance run that reflects it, I would expect to approve. I would want the refinements in item 1 tracked at least as follow-ups.
