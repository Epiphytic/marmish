# Unmounted Peer receiver quota validation

The Linux AMD64 run used exact source `0cad2b7f42cd73178ae678c9aa41b46f6fcc8dd9`
and tree `a239c40e615fbcc6aac2c607f0758d2ef50fd2be`, with Rust 1.97.1,
bundled SQLCipher 4.17.0 and MDK `b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1`.
Both Cargo dev and test debug information were set to zero. All commands used
the pinned lockfile, a warm cache, two jobs and offline dependency resolution.

| Check | Result |
| --- | --- |
| `cargo check --offline --locked -j2 -p marmish-native --bins` | Passed in 8.03 s |
| `cargo clippy --offline --locked -j2 -p marmish-native --all-targets --features installed-checks -- -D warnings` | Passed in 15.46 s |
| `cargo test --offline --locked -j2 -p marmish-native --lib peer_quota -- --include-ignored --nocapture` | 8 passed in 20.23 s, excluding compilation |
| `cargo test --offline --locked -j2 -p marmish-native --lib peer_config_` | 6 passed in 0.03 s, excluding compilation |
| Frozen test executable `peer_terminal_leave --include-ignored` | 4 passed in 101.64 s, including the shared snapshot route |
| `cargo build --offline --locked -j2 -p marmish-native --bins` | Passed in 26.57 s |

The quota tests exercised real MDK migrations, provision and reopen, ordinary
SQLite routing, aggregate main and rollback-journal charges, partial-write
accounting, recursive callback and panic refusal, hot-journal restart,
and reservation ownership. The native lifecycle used a 4 MiB fixture budget
and observed a combined peak of 2,445,888 bytes. The raw boundary case stopped
at exactly 32,768 bytes. The new reservation child refused wrong-owner,
stale-generation, and replaced-witness claims. It also checked that a held
turn cannot admit SQL until entered, denied a second live turn or entry,
and verified that dropping an unentered turn leaves its stage reusable.
The config checks covered default Off, explicit modes, direct WSS, and exact
policy and lease checks, including refusal when config changes.

The test executable SHA-256 was
`78d7866b1b1b2d548d08907c539db907e99e02930c5199d893afecf63e5726bb`.
Both normal binaries reported version 0.3.1. Their strings lacked the
test-only quota and dispatcher VFS names, fixture snapshot basename, registration diagnostic
and proof output marker. A scoped source audit found the dispatcher registrar
and every caller behind test compilation. Normal startup has no quota mount.
The artifact check used the dev profile and did not run an installed profile
or qualify native release packaging.

Ripwire reported the intentional unmounted symbols, short-horizon edits to
the original fixture, and increased source length around test accounting in
`open_managed`. Its clone heuristic also paired small role checks with
unrelated heartbeat authorization. These findings remain visible in the
static report. They do not change the runtime status of either path.

This run used Linux AMD64 only. Linux ARM64 and macOS ARM64 remain closed for
receiver eligibility. Actual mounted daemon qualification, the physical
release-shaped quota and restart proof and the APFS requirements remain
separate. [Approved phase design](../phases/peer-quota-unmounted.md).
