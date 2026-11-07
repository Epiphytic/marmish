# Darwin copy `openat` mode correction

Beads: `marmish-8ka.5.1`. This is a bounded portability correction to the
reviewed streaming candidate, not a change to copy policy or file permissions.

Candidate run `42326868886`, macOS job `128264081959`, source
`cba16b7622dbe858f89b372791009125ebc35d6c`, failed during native compilation with E0617 at
`copy_files.rs:95`: a `u16` cannot be passed to a C variadic function. The original
macOS verification **did not run tests**. Its failed compilation is retained as
failure evidence, not a platform pass.

The locked libc 0.2.189 declares `openat(..., ...)` variadically, Apple `mode_t`
as `u16`, and Linux `mode_t` as `u32`. The correction adds exactly two lines at
the beginning of `open_at`: a macOS-only `cfg` and a lossless conversion from
`mode_t` to `libc::c_uint`. Linux keeps the original argument type. The syscall,
parent/name arguments, `O_CLOEXEC | O_NOFOLLOW`, error handling, and all callers
remain unchanged. No new redundant tests were added.

## Independent reviews

Claude Code used `--permission-mode auto`, model `claude-sonnet-5-5`, session
`b619d556-dbc1-460a-a12d-438d46171017`. Both reviews were read-only, with no
reviewer Cargo, network, or source mutation.

- Design: **APPROVE**, 16.486 seconds, 3 turns. Inspected actual `open_at`,
  call sites/cfg style, and local libc Apple/Linux type and variadic declarations.
- Code: **APPROVE**, 9.013 seconds, 2 turns. Inspected exact two-line diff and
  `open_at` body; reused the earlier libc contract inspection. Confirmed lossless
  mode bits, unchanged Linux compilation, flags, path handling, and error paths.

Local raw responses are `/tmp/marmish-darwin-mode-{design,code}-r1.json`.
This sanitized record contains the substantive findings and disposition.

## Validation

The author ran existing filesystem tests and strict native all-target Clippy
with `installed-checks`, using the coordinated shared Cargo target on Linux as
UID 1001. `cargo test --locked -p marmish-native --lib copy_files::tests` passed **33
tests, zero failures**, in 24.86 seconds. `cargo clippy --locked -p
marmish-native --features installed-checks --all-targets -- -D warnings` passed
in 7.59 seconds. `cargo fmt --all -- --check`, `git diff --check`, and
native-scoped `ripwire --quality-delta` passed;
the quality delta reported zero regressions and zero gates.

A fresh three-platform hosted candidate remains required. Local Linux tests
cannot prove compilation or execution of the macOS-only branch, and neither
review approval claims a Darwin pass. The running Linux jobs from the original
candidate were left untouched for independent evidence.
