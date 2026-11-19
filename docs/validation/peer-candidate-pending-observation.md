# Candidate sealed Pending observation validation

Source `cb468b59e5cf269707e12fc102e0d4135c669a8d`, Linux AMD64. Formatting
for changed Rust files and `git diff --check` passed. The workspace formatter
flagged an unchanged assertion in `command_engine_admission.rs` at the base.
Stock and candidate strict
`cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --all-targets -- -D warnings`
passed in 29.43s and 29.67s respectively. The candidate used
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`
and the exact `MARMISH_BUILD_COMMIT`. The candidate `--lib --no-run` test
link took 45.02s. The test binary SHA-256 was
`32698beaa8bf33c3434683958a745502bc2f6c0acec4f43c050414973241d693`.

The immutable binary passed these selectors with `--test-threads=1`:

| Selector | Result |
| --- | --- |
| `peer_read` | 25 passed in 18.50s, including view freshness, expiry, admission and listing tests |
| `pending_observation_requires_one_pending_slot` | 1 passed, covering zero, one and multiple Pending rows |
| `peer_invitation::prepared_consent::installed_consent::delivery::pending_bridge::private_v2_pending_survives_cold_cuts` (`--ignored --exact`) | 1 passed in 62.10s, including all three child-process scenarios |

The loopback private WSS run used disposable profiles, real MDK custody and an
actual candidate native-window guard in the joined scenario. The Pending
observation returned exact List and Show entries from charged, joined custody.
Wrong generation, the final-fence stop and retained epoch mismatch refused
observation. The pre-Pending and sealed-observation windows each recorded zero
new Welcome polls, zero closed native joins and zero committed custody rows.
All three scenarios observed zero bridge relay publications. The guarded path
tripped and reaped its owned route at teardown.

This evidence qualifies the sealed observation and existing capped custody
fixture on Linux AMD64. The fixture uses a test-only physical guard. Installed
registration, actor scheduling, listener mounting and a healthy-empty scan
are outside this increment.
