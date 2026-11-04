# macOS resolver fixture code review

Actual Claude Code, `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `5fc3f28a-645f-4785-a651-db22593efd3d`.

**APPROVED.** I found no blocking issues. I read the actual diff and ran only `git diff --check` and `rustfmt --check` on `tests.rs`, and both were clean. I did not run Cargo or any live process.

**Checks**
- **Linux-only test:** `non_utf8_filesystem_paths_and_argument_bytes_are_preserved` is gated with `#[cfg(target_os = "linux")]`. It keeps the raw `[b'a', 255]` executable file and adds a `[b'l', 255]` symlink alias.
  - It asserts the canonical `executable`, the lexical `invoked_path` and the exact argv bytes, including `[255]`.
  - The old test only checked `invoked_path`, so this is stronger. Raw invalid-UTF-8 filename coverage is kept on Linux. macOS can't create such files, which the design review also notes.
- **Imports:** `OsStringExt` is now imported inside the Linux-only function, and nothing else in the file uses `from_vec`. `symlink` and `PermissionsExt` are still used by the portable tests, so there is no unused import on macOS under `-D warnings`.
- **`as_bytes()`:** it resolves through the private `OsStrExt` import in `resolve.rs`, which `use super::*` brings in. The old test used it the same way.
- **Portable alias test:** `lexical_symlink_alias_and_non_utf8_arguments_are_preserved` uses ASCII names, a `real` file and an `alias` symlink.
  - The resolver (`resolve.rs:candidate` and `lexical`) canonicalizes only `executable`. It builds `invoked_path` lexically, dropping `.` and `..`, so `cwd/./.` + `alias` becomes `cwd/alias`.
  - On macOS, `executable` is `/private/var/...` and `canonicalize(dir/real)` gives the same. `invoked_path` stays `/var/.../alias`, so comparing it with the un-canonicalized `dir.path().join("alias")` is right.
  - `assert_ne!(executable, invoked_path)` holds because the alias differs from the target everywhere.
  - Non-UTF-8 bytes appear only in argv (`[255]`), never in a filename, so macOS never rejects them.
- **Product source:** `47d2a02` is unchanged. `git diff --stat` shows only `tests.rs` and the validation doc, plus the untracked review doc and JSON.

**Evidence**
- **macOS result:** the validation doc records candidate 41896579860 as 388 macOS native tests passing, but the engine run was 122 passed and 1 failed with EILSEQ (92) from `fs::write` before the resolver ran. It says plainly that this is not a successful candidate or an installer qualification.
- **Amended fixture:** the doc does not claim the amended fixture has passed on macOS. Earlier text in the doc already says the candidate results for the new source must be recorded separately.
- **Full-suite run:** the 483-test native run is attributed to `47d2a02` and this test-only change came after it. Your message says the same, and I checked it against the doc but did not re-run it.
- **Review doc:** it explains that its "121" engine count is not used as evidence and cites 122 passed plus one failure.
- **Throughput JSON:** it parses, and its `source_commit` is `47d2a02165abdab1cb0656ddcfc3f083fa739baf`. It claims Linux AMD64 debug binaries only, and the doc says it is not macOS performance evidence.

**Non-blocking**
- The rerun of the 6 resolver tests has to happen on both Linux and macOS. Root's targeted Linux pass shows the Linux-only test and the new alias test pass. It cannot show that the macOS fixture is fixed, and the candidate gate still has to record that.
- The new comment `// Darwin filesystems reject…` sits directly after the preceding `}` with no blank line. `rustfmt` accepts it, so this is cosmetic.

Disposition: no blocking findings. Linux targeted tests, strict Clippy, formatting and whitespace checks passed. Actual macOS rerun and installer qualification remain required and pending.
