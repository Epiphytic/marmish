# Direct execve launch amendment design review

Actual Claude Code `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `0d5f1aec-53b1-4f38-a25a-6714b10f53b2`. All required bounded amendments are incorporated
in the phase specification before implementation. A1 chooses explicit sanitized
legacy PATH lookup, preserving prior capability without cwd execution.

**Verdict: APPROVED, with one required spec amendment (A1) and five small bounded ones (A2–A6) folded in before coding.** I found no design-level blocker. A1 is a real behaviour change the amendment doesn't mention, so it must be written down before code starts.

I read the end of `docs/phases/native-command-path-errors.md`, `launcher.rs` (`spawn_child` and the `pre_exec` setup), the Linux and macOS `platform::pre_exec`, and Rust 1.95's `sys/process/unix/unix.rs`. I ran no Cargo.

## What holds

**ENOEXEC and the `/bin/sh` fallback.** The proposal is sound.
- With a `pre_exec` closure, std takes the fork path. `posix_spawn` is skipped when closures are present.
- The child's order is: `dup2` stdio, then `chdir`, then the SIGPIPE reset, then the closures. Only after that does it set `environ` and call `libc::execvp`. That `execvp` is the only place the implicit `/bin/sh` retry happens.
- If the callback calls `execve` and never returns `Ok`, std's `execvp` is unreachable. A failed `execve` returns `Err(errno)`, std writes it to the CLOEXEC pipe, and the parent reaps the child and returns `Err(from_raw_os_error(errno))`. ENOEXEC then reaches the existing mapping at `launcher.rs:1148`.
- Success closes the CLOEXEC pipe, so the parent reads EOF and gets a normal `Child`. This is the same on Linux and macOS, because both use std's fork path and macOS's libc `execvp` also falls back to `sh`.

**What the correction keeps.**
- **Stdio, cwd, handshake and reaping:** all still done by std.
- **Platform setup:** `platform::pre_exec` (setsid, umask, close-on-exec for fds 3 and up) still runs after `dup2`/`chdir`, as it does today.
- **argv0:** it is preserved because the image carries the original `argv[0]`.
- **Environment:** `execve` receives the prepared `envp`, so std's later override of `environ` is moot.
- **Shell requests:** they keep their shell semantics, because the image is built as `[account.shell, "-c", command]`.

**No post-fork allocation.** `io::Error::from_raw_os_error` and `last_os_error` use the non-allocating OS repr. Both platforms' `pre_exec` already avoid allocation. The existing `PRE_EXEC_FAILURE` wrapping stays valid, so an injected EPERM or ENOENT from setup stays reason 7.

**Send/Sync.** `pre_exec` requires `FnMut + Send + Sync + 'static`, so the narrow `unsafe impl Send + Sync` is legitimate. It is justified only if all of these hold:
- The raw pointers point into heap allocations owned by the same struct. Moving a `CString` or `Vec` doesn't move that heap data.
- The fields are private and the type is built only by a constructor.
- Nothing mutates it after it is built.
- It has no interior mutability and is not `Clone`.
- It stays local to `spawn_child` and is not put into `ChildSpec`.
- The only reader is the forked child, which sees a copy-on-write copy.

## Required amendments

**A1. Legacy `Exec` with a bare `argv[0]` changes behaviour.**
- Today `Command::new("ls")` goes through `execvp`, which searches the sanitized `PATH=/usr/bin:/bin`.
- `execve("ls")` instead resolves `ls` against the child's cwd, so it can run `./ls`. `execvp` could never do that, since `PATH` has no `.`.
- This reaches only legacy `Exec`. `ResolvedExec` and `Shell` use absolute programs. Legacy `Exec` is still readable, and `lib.rs:767` and `command_config.rs:256` can still produce it.
- The spec must pick one. I recommend a definite `NotStarted(MissingExecutableOrInterpreter)` when `argv[0]` has no `/`, and a test for it.
- Paths containing `/`, such as `./x` or `bin/x`, behave identically under `execvp` and `execve`.

**A2. Add `EBADARCH` on macOS.**
- On macOS a wrong-architecture Mach-O returns `EBADARCH` (86), not ENOEXEC. The `libc` crate defines it for Apple targets.
- Map it, behind `cfg(target_os = "macos")`, to `InvalidExecutableFormat` next to ENOEXEC. Without it, the macOS "invalid format" acceptance claim only covers shebang-less text.

**A3. Capture the whole image in the closure.**
- With edition 2021, a `move` closure that touches only `image.argv_ptrs` captures just that field. The `CString` owners would then be dropped when `image` goes out of scope, leaving dangling pointers.
- Require the closure to call a method on the whole image, as in `image.execve()`. Also give the struct no `Drop` impl of its own. Disjoint-field partial moves still compile on a type with no `Drop`, so the method-call capture is what protects against dangling pointers.

**A4. The callback must be unable to return `Ok`.**
- Write the image method as returning `io::Error`, and the closure as `platform::pre_exec().map_err(sentinel)?; Err(image.execve())`.
- Keep `Command::new(canonical)` and the existing `env_clear().envs(..)`, built from the same source as the image. That way a future bug can't launch a different target.

**A5. Validate before building the image.**
- Environment keys must be non-empty, contain no `=` and no NUL, and be unique. All of this happens in the parent.
- Reuse the existing argv limits, and map a `CString` NUL error to `OtherStartupFailure`.

**A6. Tests to add.**
- Plain text with mode 0700 gives `InvalidExecutableFormat`. This is the failing case from code review r1, and it must also run with no `/bin/sh` output.
- A missing interpreter gives reason 2.
- A real `envp` check shows exactly the five sanitized variables.
- The argv0 symlink test.
- A bare-`argv[0]` legacy `Exec` follows A1.

## Not blocking
- `E2BIG` and `ELOOP` from `execve` land in `OtherStartupFailure`. `ETXTBSY` is already mapped to resource unavailable.
- The r1 refinement about `chdir` EACCES being labelled reason 1 is unchanged. The `chdir` runs before the callback, so this correction doesn't affect it.

I haven't verified anything on macOS. The macOS claims are from reading the Rust std source and the `libc` crate, not from running it.
