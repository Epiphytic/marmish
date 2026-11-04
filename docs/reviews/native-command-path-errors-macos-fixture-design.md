# macOS resolver fixture design review

Actual Claude Code, `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `7033894a-2be4-436a-9de9-373cf4b7e4d3`.

**APPROVED**, with one condition the implementation must meet. I read `crates/marmish-engine/src/resolve/tests.rs` and the macOS CI log and ran nothing.

**Why the design holds up**
- The log confirms your diagnosis. The panic is at `tests.rs:16:50`, which is `program()`'s `fs::write` on the `[b'a', 255]` filename. The error is `Os { code: 92, "Illegal byte sequence" }`. The resolver was never called, so there is no product defect. APFS rejects non-UTF-8 names, and a Linux-only gate on that test is the right fix. It isn't a broad ignore, and no filesystem error is swallowed.
- The other 121 engine tests pass on macOS. The existing wire literal and non-UTF-8 round-trip tests are untouched.
- Putting the `[255]` byte in an argv entry is safe on macOS. It is never a path, so it never reaches the filesystem.

**Condition (blocking if missed)**
- `use std::os::unix::ffi::OsStringExt;` at `tests.rs:2` is only used by `OsString::from_vec` in the raw-bytes test. Once that test is `#[cfg(target_os = "linux")]`, the import is unused on macOS. `cargo clippy --workspace -- -D warnings` is required by the project instructions, so it would turn that warning into a macOS failure.
- Move the import inside the Linux-only test function, or gate it with the same `cfg`.
- `as_bytes()` comes from `OsStrExt` via `super::*`, so it is not affected.

**Non-blocking suggestions for the portable test**
- Make the fixture a symlink (`real` → `app`, as in `path_order_alias_and_argument_bytes`). A plain file makes `canonicalize(fixture)` and the lexical path nearly identical on Linux. With a symlink, "invoked_path is lexical" and "executable is canonical" become two distinct assertions.
- On macOS, `tempdir()` sits under `/var`, which is a symlink to `/private/var`. Comparing `executable` to `canonicalize(fixture)` handles that. Keep `invoked_path` compared against the un-canonicalized `dir.path().join(...)`, matching the existing assertion.
- Use an ASCII filename. A non-ASCII name invites Unicode normalization differences on macOS filesystems and adds nothing to this test.

**What each platform covers**
- **Linux:** this is the only place the raw invalid-UTF-8 executable filename is exercised end to end through the resolver. It checks the exact executable, the alias and the argv bytes.
- **macOS:** it covers the lexical alias, `PATH=./.` normalization, canonicalization, and the argv byte-for-byte pass-through including `[255]`. It does not cover non-UTF-8 filenames on disk, because the OS forbids them. The coverage gap is real and platform-imposed, and Linux CI is the sole guard for that path.
- The resolver's path handling looks shared across Unix targets, since these tests use `std::os::unix` `OsStr` byte APIs, so the Linux result should carry over. That is an inference from the test code. I did not read the resolver itself.

You should say the same in the commit message or test comment. Without that note, a reader could think macOS verifies non-UTF-8 filenames.

Disposition: implement the approved fixture-only split, including the local Linux-only import and portable ASCII symlink test. The completed CI log has **122 engine passes and one fixture failure**; the review's incidental “121” count is not used as validation evidence. No product source or filesystem error handling changes.
