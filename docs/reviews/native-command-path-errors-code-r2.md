# Native command lookup code review — round 2

Actual Claude Code `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `183c7c87-1474-42a8-a728-5438fff3e3f2`. Verdict: APPROVED, static integrated review.
Root performs executable tests and live validation separately.

# Verdict: APPROVED

I found no blocking findings. This is a read-only static review. I ran no Cargo, no tests and no live sessions. r1's blocker (B1) is resolved in the code, but nothing here has been run, so root's test, Clippy, live-relay and macOS CI gates still decide it.

**Model:** Claude Sonnet 5.5 (`claude-sonnet-5-5`), no sub-agents.

## Checks actually performed
- **Read:** the frozen design with the direct-`execve` amendment, the launch-design review, code-r1, and the full diffs of every modified engine, native and prototype file. Also all untracked source: `launcher/exec_image.rs`, `resolve.rs`, `resolve/tests.rs`.
- **Docs read:** the README, agent-spec, build-plan and background-daemon doc changes.
- **Formatting:** `rustfmt --check` passes on `launcher.rs`, `exec_image.rs`, `resolve.rs` and all six touched native files, using edition 2021 for the engine and 2024 for native. `rustfmt` does not build anything.
- **Not done:** I did not run tests or Clippy, and I did not verify macOS behavior, including `EBADARCH`.

## What I verified in the code
- **B1 is fixed.** `spawn_child` now builds an `ExecImage` in the parent. The `pre_exec` callback runs `platform::pre_exec`, then returns `Err(image.execve())`. It can never return `Ok` into libc's `execvp`, so ENOEXEC reaches `InvalidExecutableFormat` with no `/bin/sh` fallback. `headerless_executable_is_not_interpreted_as_shell` tests this directly, including that no side effect occurs.
- **`ExecImage` ownership is sound.**
  - Pointers reference `CString` heap buffers owned by the same struct, so moving the struct does not invalidate them.
  - Fields are private, there is a single constructor, no `Clone`, and no `Drop`.
  - The closure calls a method on the whole `image`, so under edition 2021 it captures every owner, which is what amendment A3 asked for.
  - The callback does no allocation: `from_raw_os_error` and `last_os_error` use the non-allocating repr.
- **Environment and argv0 (A4/A5).**
  - Environment keys are validated as non-empty, unique, with no `=` and no NUL, and failures map to reason 7.
  - `Command` and the image take their environment from `spec.environment`.
  - argv0 comes from `argv[0]`. `get_args()` excludes argv0, so the argv array is built correctly.
- **Errno mapping.** `PRE_EXEC_FAILURE` (0x4d41) cannot collide with a kernel errno, so injected EPERM/ENOENT stay reason 7. `EBADARCH` is mapped under `cfg(macos)`. `ETXTBSY`, `EAGAIN` and `ENOMEM` map to resource unavailable.
- **Legacy `Exec`.**
  - A bare name goes through the resolver against the sanitized child PATH and never against the cwd.
  - A name containing `/` is joined to `spec.cwd`.
  - The lookup error mappings match the spec.
- **Resolver.**
  - It covers PATH order, skipping and remembering unusable candidates, unset/empty/relative entries, limits, and ELOOP stopping the search.
  - `canonicalize` plus `faccessat(AT_EACCESS)` is fine on macOS.
  - The selected-denied-candidate test shows a denied first hit does not fall through to a later allowed one.
- **Policy.**
  - Allow layers match the canonical path. Deny layers match both the canonical path and the alias. Fuel is charged for both before short-circuiting.
  - Exhaustion and compile failure return `InvalidPolicy` and surface as `command_policy_error`, not a fake denial.
  - `ResolvedExec` always forces the canonical recheck, in `check_execute_request`, `store.admit` and `spawn_child`.
  - The `store.rs` guards and `finish` use `is_startup_failure`, and `LaunchFailed` is rejected from Running (`launch_failure_is_persisted_only_from_launching`).
- **Native.**
  - `execution_refusal` maps definite errors explicitly. A successful submit with no durable facts becomes `Uncertain` rather than `Missing`.
  - Known invocations bypass resolution. `native_known_invocation_bypasses_lookup_after_target_and_path_change` uses an oversized PATH that would fail if it were re-resolved, and also removes the target.
  - The 140-denial test makes 140 real `execute_operation` calls and asserts no job is created each time. A 141st allowed dispatch then succeeds.
  - No engine worker runs in these fixtures, so copying `current_exe` there is inert.
- **Wire and CLI.**
  - Exact code/detail pairs are enforced, and the historical `noncanonical_executable` bounds are kept.
  - `launch_failed` accepts only `Some(1..=7)`, using explicit values rather than enum ordinals.
  - The CLI selects fixed local text. Hostile detail strings cannot leak into the explanation.
  - A launch failure goes through `terminal_exit`, consume and Finish, and only then reports. The request order is asserted in the test.
- **`daemon_launch.rs` (Darwin fix).**
  - `poll` plus `recv(MSG_DONTWAIT)` under the original deadline reads buffered bytes before inferring EOF on HUP.
  - Spurious wakeups are harmless, and the `ready == 0` and `EINTR` paths loop correctly.
  - The remaining `set_write_timeout` before sending `A` is safe, because the child is alive at that point.
  - The code and the amendment text agree.
- **`identity.rs`.** The change is fixture-only. The script now reads the request before `kill -TERM $$`, and production code is untouched. The amendment states this and says the race explanation is source-supported, not confirmed.

## Nonblocking items
1. **Test name overclaims.** `direct_exec_image_uses_exact_envp_and_original_argv0` never checks argv0, because `env` ignores it. argv0 is covered by `worker::resolved_program_preserves_original_argv0`. Rename the launcher test or extend it.
2. **Race mislabel (carried from r1).** An executable that vanishes between resolve and submit becomes a durable `invalid_request` instead of `command_not_found`.
3. **chdir EACCES (carried from r1).** A child-side `chdir` failure with EACCES is labelled reason 1, filesystem permission. The cwd preflight narrows this but cannot eliminate it.
4. **`invoked_path` and argv[0] not cross-checked (carried from r1).** Only native builds `ResolvedExec`, so this is defense in depth.
5. **Lexical normalization.** `lexical()` also collapses `..`, while the design says only redundant separators and dot components. Canonical deny matching still applies, so no bypass results. Either correct the doc wording or restrict the code.
6. **`Unauthorized` and `QuotaExceeded` become durable refusals.** `execution_refusal` now turns a transient `Unauthorized` from `check_execute_request` into a permanent refusal for that invocation. Previously it fell through to `Missing`. This was accepted in r1 but is worth a sentence in the doc.
7. **`EBADARCH` is mapped but untested.** The mapping is behind `cfg(macos)`, so macOS CI is the only evidence for it.

## Docs versus published 0.2.2
- The README and agent-spec now say clearly that published 0.2.2 requires canonical absolute paths and may refuse spellings like `/bin`. They also say the new behavior is source-only and needs both peers upgraded. This fixes r1 refinement 5.
- The no-implicit-shell statement matches the code. The "restart `marmishd` after changing PATH" statement matches `Worker::new`, which captures PATH once. The daemon spawn does not clear the environment, so PATH is inherited.
- Both docs say PATH limits apply only to bare names. That is accurate.

## Closure gates still open (root-owned)
- **Cargo:** `cargo test -p marmish-engine` (especially `worker::tests` and `launcher::tests`), `-p marmish-prototype`, and `-p marmish-native --all-features`.
- **Lint:** `cargo fmt --check` and `cargo clippy --workspace --all-features -- -D warnings`.
- **Live validation:** the relay run covering bare command, symlink/canonical policy, nonexistent bare and explicit paths, a non-executable target, invalid format, denied command and literal special-character arguments. There is no `docs/validation` record for this increment yet, and the spec requires one.
- **CI:** Linux AMD64/ARM64 and macOS, especially the `daemon_launch` and `identity` fixtures.

## Disposition

The exact-envp test is renamed to describe only its asserted behavior. The phase
now explicitly names parent-dot normalization and durable Unauthorized/QuotaExceeded
refusals. Replacement/cwd-race diagnostic refinements and engine caller defensive
consistency checks are tracked in Beads marmish-wyl. No macOS EBADARCH runtime
claim is made. The asynchronous cleanup fixture also waits for committed ACK
registration drain before expecting begin_cleanup to succeed.
