# Native startup correction: local checks

Scope: [approved A+B design](../phases/macos-native-startup-limits.md).
These local checks do not qualify macOS or the installed release matrix.
No public release/version changed. The local host is Linux AMD64, UID1001,
Rust1.97.1. Disposable offline state and owned child processes were used.

## Engine/platform increment

With `CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target`:

| Command | Actual result |
| --- | --- |
| `cargo test --locked -p marmish-engine --lib process_limits_tests -- --test-threads=1` | 3 outer tests passed; 0.07s |
| `cargo test --locked -p marmish-engine --lib` | 141 passed, 1 existing ignored; 21.05s |
| `cargo clippy --locked -p marmish-engine --all-targets -- -D warnings` | Passed; 2.30s |

The ignored outer `live::tests::fd_probe_child` is an existing exec-only
probe explicitly invoked by the multithreaded live-spawn test; it is not counted
as an independently passed test. Isolated subprocess inner test output overlaps
the outer counts and is not added. The nonroot offline engine test actually ran:
exact binary stdin/stdout, binary stderr, exit7, owned process-group kill-before-reap.
Linux no-op and lower-limit/descriptor-flag preservation checks passed.

Private log SHA256 values, respectively:
`7a0ac1159ac18e57b629cfaf84efb6327a38ee6bfbea3df0c48a17955d1936fc`,
`b52f462ce2030d46a98ef034b02813f441e626ab65d36a07def5b826bb4821c7`,
`5b745cf443ca65a8043b0170076843b2f1e5c5694bf179c71bc6c5e80dc1897f`.

Darwin-specific high-soft normalization/readback, infinite-hard behavior and
existing Darwin descriptor checks were not executed on Linux. Complete fresh three-platform candidate and unchanged installed cohorts remain
required; no local result replaces them.

## Native bootstrap and binary checks

The final post-extraction focused bootstrap tests passed **8/0/0**, 89.65s;
adjacent `command_engine::` tests passed **19/0/0**, 14.21s. The actual
`process_limits` integration target passed **1/0/0**, 0.01s after an 18.42s build:
it launched the built marmish and marmishd help entry points. This is Linux
binary smoke, not Darwin elevated-limit proof. Strict native all-target Clippy
with `installed-checks` and `-D warnings` passed in 9.95s. Workspace fmt and
diff checks passed.

The focused cases exercise actual gated Opening→OpenFailed with an invalid
worker; queued and unqueued durable FailedInvalid/quota/counter idempotence;
a real SQLCipher transaction abort with pending IDs preserved then next-tick
success; queued Cancel and Stop bookkeeping retirement; an absent journal row;
active authority/binding/revision/launch guards; NativeCopy failure before helper
context construction; Full/Opening retryability; and a real owned-runner exclusion.
The no-launch checks assert no engine state-root directory, no process record,
no local runner, no live-prepared result, zero owned runners/active quota and
cleanup readiness, alongside the structural OpenFailed-before-Worker proof.
They are not a syscall spawn trace or independent PID census. Cancel/Stop tests
verify unchanged durable Cancelled records and cleanup/stop completion flags.
The owned-runner exclusion test installs a real child at the preparation seam,
verifies ownership remains, then cancels and waits for completion; it does not
assert whole-service cleanup for that exclusion case.

Development failures included fixture control-rate exhaustion and an invalid
assumption that queue refusal retains an idle local entry. Those fixtures were
corrected; their failures are retained privately and are not counted as passes.
A helper extraction compile error was corrected before the final run.

Private final focused/adjacent/binary log SHA256 values:
`da0bbf16c5cf28c9ab98c177bf8d87677f9614c93392b2c890acbe7b9a1e237f`,
`a0635125d68258e517f872e355c4618778a80061a3eefcb557bd69e7b9f1af9b`,
`f7532875324e46746e5f905a259431b670da4cd3d3a8fdd9b200610f3b232ad7`.

Exact native commands, with the same target directory as above:

```sh
cargo test --locked -p marmish-native --lib stream_failed_bootstrap -- --test-threads=1
cargo test --locked -p marmish-native --lib command_engine:: -- --test-threads=1
cargo test --locked -p marmish-native --test process_limits
cargo clippy --locked -p marmish-native --all-targets --features installed-checks -- -D warnings
```

Private Clippy log SHA256:
`1bb1d22609bb2c31a697bde00ab4d8b4ed83fd482f744af777d5b01de63e0ef7`.

## Static quality disposition

The first delta flagged preparation-loop complexity 18→41 and lines 55→115.
Extracting a coherent private durable-finish helper reduced the loop to
complexity 20 and 65 lines. The final delta still exits 2 for that necessary 10-line
increase, and reports new test/cfg symbols as unconnected. No green static-quality
result is claimed; no extra trivial abstraction was added just to suppress the
number. Compiler/Clippy and actual tests remain the executable evidence. The
staged change-context map covered all 19 intended files; its 2000-token budget
retained structural counts and explicitly dropped nested detail. It is a
bounded map, not exhaustive dynamic reachability or qualification.
