# Local large-payload qualification: e461754

This is a Linux AMD64 **default-feature debug-build local qualification**, not a
relay, Marmot/MLS, Tor, installed-package or release-performance benchmark.
Source: `e4617547d77264dd733db5a924bffb36a46a1ebf`.
The ignored test
`copy_integration_tests::large::large_raw_zlib_actual_helper_four_way`
passed **1/1**, with test-harness elapsed time **1076.97 seconds**. It runs four
cooperatively scheduled concurrent cases in one test, one observation per
codec/direction combination. Their timers start at staggered offsets; blocking
fixture generation and hashing can delay sibling cases.

## Observed results

Each case copies **69,206,033 bytes (66 MiB + 17 bytes)** using the actual copy
test-binary helper (debug test executable re-exec) through a local IPC bridge.
Each verifies destination length and SHA-256, permission mode bits and mtime
seconds/nanoseconds, contiguous offsets, protocol
completion/commit, final output acknowledgement, drained pipes and natural
helper exit. The observer retains at most its first 1024 data-frame samples;
this cap is not evidence of a product memory bound. Source and destination names include spaces, quotes, dollar sign,
semicolon, backslash and newline. The common destination SHA-256 is
`023eff39a2963cf1653913ca8e7af127391bb8cfcca1a7a0f04b1f95804118cb`.

| Direction | Codec | Whole-case seconds | File KiB/s, derived | Encoded local payload bytes | Encoded/raw local stream ratio |
| --- | --- | ---: | ---: | ---: | ---: |
| put | none | 853.728 | 79.16 | 69,316,709 | 1.000000 |
| put | zlib | 852.005 | 79.32 | 36,181,620 | 0.521975 |
| get | none | 1072.812 | 63.00 | 69,316,508 | 1.000000 |
| get | zlib | 1071.434 | 63.08 | 36,181,579 | 0.521976 |

File goodput is `file_bytes / wall_seconds / 1024`. The timer starts after source
creation, source metadata setup and local bridge/client-IO setup. It includes
`copy_client::run`, helper cleanup/join, protocol assertions, subsequent complete
destination hashing and metadata verification, through report construction.
Concurrent scheduling and verification costs are included. It is neither a pure
transfer timer nor an RTT measurement. The aggregate test duration additionally
includes setup outside those per-case timers; case durations must not be summed
or averaged into a purported aggregate throughput.

The fixture alternates 1 MiB regions of a repeating 0–255 byte ramp and deterministic
pseudorandom bytes. Its approximately 0.522 zlib ratio describes this synthetic
fixture, not general compression effectiveness. The ratio divides encoded codec payload bytes by raw **local
protocol stream** bytes, which include protocol records beyond file content.
It excludes MLS, Nostr, relay and transport wire overhead. Zlib mode also emits
uncompressed blocks. The local bridge deliberately delays fresh-put reverse
checkpoints until the durable window fills, exercising backpressure rather than
optimizing transfer speed. Put and get also differ in codec-ledger accounting
(receive versus ACK boundaries) and get gating; their durations cannot establish
a directional performance difference. One concurrent observation per case cannot establish
compression speed benefit, steady throughput, latency distributions or p95.

## Separate duplex integrity result

`stream_runner::tests::large_duplex_integrity_with_slow_consumers_and_bounded_windows`
passed in **8.61 seconds**. It exercises a local `tee /dev/stderr` process with
**68,157,457 bytes (65 MiB + 17 bytes) on each of stdin, stdout and stderr**,
checks byte content/hashes, delivered counts, bounded windows, deliberately
withheld/slow output consumption and confirmed exit code zero. That elapsed
time is a test-harness duration, not a transfer-only metric. This test has no
relay or network connection and is separate from the four copy cases.

## Evidence and reproduction scope

[Machine-readable observations and separately derived values](local-large-payload-e461754.json)
preserve the parsed case values without rounding. The parsed input SHA-256 is
`b0d41d4df5f74efd403c8e79b1b165db5f2a2e9e12d93467cdbf487537502372`.
Private raw logs remain unpublished; their SHA-256 digests are:

- Four-way copy: `8a0ac6f54aae1a66324b7a3e69bf29a741adb05d287a54b18464c76c9684e51c`.
- Duplex: `2c156a0749c42dcad2902a96823ef129ca53a91d06317ea88c4f2ad8c47f2db7`.

The copy harness lives in
[`copy_integration_tests.rs`](../../crates/marmish-native/src/copy_integration_tests.rs),
its nested test/timer and assertions in
[`copy_large_tests.rs`](../../crates/marmish-native/src/copy_large_tests.rs),
and duplex assertions in
[`stream_runner.rs`](../../crates/marmish-native/src/stream_runner.rs).
The copy test uses an eight-worker Tokio runtime and requires at least 1 GiB of
available temporary space, with an 1800-second per-case and 2100-second aggregate
copy deadline; slower hosts can fail by timeout. Both tests are ignored by default and require an
explicit exact-name invocation with `--ignored --exact --nocapture`; these
results do not imply default CI, cross-platform or installed-release coverage.
CPU model, resource contention and filesystem characteristics were not captured
in the supplied evidence, so these measurements are a correctness baseline with
limited performance comparability.

## Independent documentation review

Claude Code, requested model `sonnet`, reviewed these two files against the timer,
bridge/codec metrics and duplex source in `--permission-mode auto`. Verdict:
**APPROVE, no blocking issues**. The reviewer recomputed rates/ratios using
Python and inspected source; it did not run tests or independently validate raw
logs. Useful findings were incorporated: precise timer/concurrency semantics,
metadata scope, test-binary helper identity, observer sample-cap distinction,
assertion-implied booleans, synthetic compression scope and asymmetric ledger/
gating limitations. The report author separately compared canonical values with
the SHA-256-verified private parsed input; all values and derived rates matched.
A final Claude documentation recheck also approved with no blocking issues.
