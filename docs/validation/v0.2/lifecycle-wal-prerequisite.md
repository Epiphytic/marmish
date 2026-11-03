# Synthetic lifecycle WAL prerequisite

Linux offline check on the lifecycle implementation worktree; not a hosted lifecycle qualification or a command performance measurement.

`cargo test --offline --locked -p marmish-native --features installed-checks --lib real_closed_owner_wal_preflight_preserves_main_and_sidecar_inventory -- --nocapture`: **1 passed**, 3.00 seconds of test execution. The fixture opens genuine MDK, command Journal and engine databases with native-derived keys, closes their owners, and runs the production read-only preflight while holding the profile lease.

The original strict inventory assertion failed: the MDK main database (2,129,920 bytes) and Journal main database (200,704 bytes) stayed byte-identical, but each gained a previously absent empty WAL and a 32,768-byte SHM. This motivated the [independently approved amendment](../../reviews/v0.2-lifecycle-wal-design-r2.md). Production SQLite flags remain unchanged.

The corrected prerequisite verifies all three main databases and all preexisting sidecars remain exact. Only previously absent regular, non-symlink, single-link, current-UID, mode-0600 empty WAL and 32,768-byte SHM files are allowed. No immutable mode, checkpoint, write-capable connection or hand-edited database is used. Additional helper tests cover rewritten preexisting sidecars, unexpected journal files, hardlinks, symlinks and incorrect modes; their final execution results are recorded with the integrated review.

The runtime report records finite milliseconds for each lifecycle stage, command wall time and restart readiness, plus exact output byte counts. Actual installed lifecycle latency samples remain pending the three-platform workflow; unit-test wall time is not substituted for those samples.

The integrated review requested a genuine preexisting-WAL reader case. The same
fixture now retains read-only SQLite `BEGIN` transactions on all three real
databases, verifies WAL mode and both actual sidecars exist, then requires every
existing main/sidecar byte to survive another preflight with zero auxiliary
creations. All five preflight tests passed in 3.16 seconds. The final bounded-read
hardening (`take(original_length + 1)`, rejecting a changed length) was added
afterward; its immediate rerun was blocked by an unfinished sibling provider
type. The final default-feature command,
`cargo test --offline --locked -p marmish-native --lib native_preflight::tests`,
then passed all five tests in 2.72 seconds, including the bounded-read hardening.
The final feature-enabled command,
`cargo test --offline --locked -p marmish-native --features installed-checks --lib native_preflight::tests -- --nocapture`,
also passed all five tests in 2.77 seconds. This remains Linux offline evidence;
three-platform installed lifecycle qualification is pending.
