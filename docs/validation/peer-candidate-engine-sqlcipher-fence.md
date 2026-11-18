# Candidate Command SQLCipher fence validation

Tested source `22ee5fff9fb8da946bc1584e90643e92d7547a73` on Linux
AMD64, with the workspace lockfile and bundled SQLCipher. The worker
and Engine were exercised directly with disposable private state. The
checked worker remains unmounted. These checks used no relay and did
not change SQLite's default VFS in a live daemon.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check`, `git diff --check 33c7338..HEAD` | Pass |
| Locked offline stock Clippy for `marmish-engine`, `marmish-native`, `marmish-prototype`, all targets, warnings denied | Pass, 27.96 s |
| Locked offline stock Engine and native libtest link | Pass, 42.58 s |
| Locked offline `marmish-native --lib` candidate check for `x86_64-unknown-linux-gnu` | Pass, 9.55 s |
| Engine libtest, full suite | 154 passed, 1 ignored, 40.04 s |
| Native libtest, `command_engine` | 31 passed, 1 ignored, 7.80 s |
| Native libtest, `command_lifecycle` | 6 passed, 4.57 s |
| Native libtest, `dispatcher_fatal` | 1 passed |

The tests used two libtest threads. The existing ignored native test needs
separately built installed helpers. Guarded spool tests exercised real
SQLite Busy without those helpers.

## Observations

- A failed raw open returned a non-null SQLite handle. The guard made one
  close call while admitted and made zero close calls after a refused
  check. Real SQLite Busy on raw and rusqlite-owned handles retained each
  exact owner after one failed close, with no implicit retry during
  unwinding. The paired unsafe-owner callback made the refusal sticky.
- State startup stopped before file preparation, before raw open, before
  configuring the opened SQLCipher handle and before schema access in
  separately cut tests. A reopened database refused the first pending
  reconciliation turn without incrementing the saved launch attempt or
  producing worker identity.
- A guarded saved-prefix read preserved binary output while admitted.
  The tests also covered refusal before open, refusal at close, a
  corrupt spool, and SQLite Busy. A destructor refusal suppressed the
  storage result ahead of the original failure. Native Poll stopped
  before reading its second stream on one typed refusal.
- The candidate worker suppressed a real consuming-close refusal and
  a nested Dispatch refusal including bookkeeping updates, retaining
  the frontend lock until the child process exited. An admitted
  candidate cleanup closed the real Engine and transferred its
  separate cleanup guard to the caller.

The exact native artifact also ran these selectors individually. All passed.

- `command_engine::admission::tests::close_poison_notifies_only_the_original_admission_and_fatal`
- `command_engine::tests::fatal::first_stream_admission_refusal_never_polls_second_stream`
- `command_engine::tests::fatal::real_worker_suppresses_consuming_close_and_dispatch_refusals`
- `command_engine::tests::fatal::healthy_candidate_cleanup_closes_real_engine_and_retains_guard`

The exact debug libtest binaries had these SHA-256 digests.

- Engine `f051ebc78d087d5e8c565002a3c0b0883b6bf3156cfd8d96e593ea007b05d7db`.
- Native `6f5b7a92c729888c8458099d224bedcd93bdb4f015c80a2d0d5618559536b095`.
