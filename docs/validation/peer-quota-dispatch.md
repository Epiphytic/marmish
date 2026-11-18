# Peer quota dispatcher validation

## Linux AMD64 proof

Frozen source commit `e65bed1597afadf2fffe6a0c76a4adfe67fc0bf6` (tree
`889ceb4d045ab32da4100227acf7fd77fed6c900`), based on
`1c6de7eadce0732478b2dc51c7eb431735494fa1`. The exact review binary
logged SHA-256 `e4b6be2c15ab4c35196fc5068aedcc814e0d6eacf247a6024fa10e715d8e414e`.
Rust 1.97.1 and SQLCipher 4.17.0. The offline dependency tree resolved exactly
one `libsqlite3-sys 0.38.1` from rusqlite `5ae7fdf8` across Marmish, MDK
`9a29d1e4` and Arti.

- `cargo test --offline --locked -j 2 -p marmish-native --lib quota_ -- --ignored --nocapture`:
  7 harness entries passed in 22.73 s. The new dispatcher parent launched nine
  isolated child executions, with one intentionally stopped at the synced
  native write cut. The existing quota fixture also passed its isolated
  quota refusal and rollback, exact boundaries and raw hot-journal checks.
- Real MDK rejected zero and 32 KiB combined reservations at the durable
  native-started state. A 4 MiB allocation supported migrations, generation and
  signing, an exact read-only reopen, and wrong-key refusal. The reopen and
  wrong-key check preserved the encrypted main bytes.
- The named and anonymous routing proof rejected out-of-scope managed paths,
  symbolic and hard-linked aliases, conflicting scope, foreign named files,
  anonymous or subjournal opens, explicit proxy flags, WAL and ATTACH under
  managed scope. Concurrent ordinary work used the same stock I/O method table
  and anonymous I/O result, preserving WAL/SHM, savepoint rollback, and data.
  The managed live-handle check blocked a clean receipt until close.
- The fault child charged the actual partial extent after a simulated stock
  write error, blocked recursive callback execution, caught a simulated
  callback panic and blocked subsequent growth. Both pre-open and post-open
  displaced-default checks failed closed.
- A fresh process reopened the durable checkpoint after the real MDK crash.
  It recovered the same generation and witness, refused the hot-journal stage,
  checked preservation of both files and wrote to an ordinary SQLite database.
- `cargo test --offline --locked -j 2 -p marmish-native --lib peer_invitation::provisional::tests`:
  10 passed and 2 opt-in ignored in 50.14 s. The checkpoint recovery suite
  exercised expected SQLCipher wrong-key diagnostics.
- `cargo clippy --offline --locked -j 2 -p marmish-native --all-targets -- -D warnings`,
  scoped Rust formatting and `git diff --check` passed. Workspace formatting
  still reports two unchanged baseline files, `marmish-engine/src/lib.rs` and
  `marmish-native/src/native_setup.rs`.

## Qualification still needed

This increment is opt-in and compiled only into the native test binary. The
actual installed Command and ephemeral Terminal clients and servers still need
to run concurrently in an opted-in dispatcher process. The Linux fixture uses
ordinary SQLite parity checks for this increment. It does not establish all
full-design negatives and fault scenarios, including OS-level mounted-volume
substitution, cancellation of an external awaiter, and named-VFS/direct-write
bypass controls. The complete macOS ARM64/APFS run, Darwin lock method
qualification, native packaged execution and both cross-platform runtime
qualification gates remain open. A production activation requires an accepted
production owner for charged non-slot bytes and durable generation, completion
of the planned platform proofs, and separate design and rollout approval.

## Diagnostic and unwind follow-up

Frozen source commit `5e93c1537a520ea5abc5d1ae3d3d234e91a79ecf` (tree
`842412ade0ac596e9b22e3a9dd3768f82dddda7b`). Exact Linux AMD64 test
binary SHA-256
`c57a433cee055cb1befb44c76cbcac79729c9e23191a0ae3d89060b2b7049606`.

- The same offline quota test command passed all seven harness entries in
  21.63 s with ten isolated dispatcher child invocations. A simulated short
  stock write returned `SQLITE_IOERR_WRITE` and `ENOSPC` through the
  wrapper's Unix `xGetLastError` after independently checking and charging
  the partial extent. This exercise uses the bundled Unix metadata probes.
- The new poisoned-lock child unwound a live turn with a deliberate panic.
  Drop cleared the matching claim and thread scope, retained quarantine
  and left the reservation mutex poisoned. The child finished normally
  and confirmed later scoped and ordinary opens were refused.
- Strict native all-target Clippy, scoped Rust formatting and diff checks
  passed again.
