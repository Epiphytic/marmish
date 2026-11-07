# Large local helper qualification

Four concurrent actual-helper copies passed on non-root Linux x86_64 using an unoptimized debug/test executable: Put and Get, each with raw and zlib transport codecs. Every case copied **69,206,033 bytes (66 MiB + 17)** with exact SHA-256 `023eff39a2963cf1653913ca8e7af127391bb8cfcca1a7a0f04b1f95804118cb`, mode 0640 and mtime 1700000000.123456789. The whole test passed in 1075.17 s.

| Direction | Codec | Case seconds | File bytes/s | Raw stream bytes | Encoded payload bytes | Local payload ratio |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Put | none |854.552|80,985.2|69,316,709|69,316,709|1.000000|
| Put | zlib |850.763|81,345.8|69,316,709|36,181,619|0.521975|
| Get | none |1071.095|64,612.4|69,316,508|69,316,508|1.000000|
| Get | zlib |1069.638|64,700.4|69,316,508|36,181,579|0.521976|

The deterministic source alternates 1 MiB of compressible all-byte cycles with 1 MiB of seeded pseudorandom bytes. These ratios measure codec payloads on **local framed IPC**, excluding JSON/base64, Marmot encryption and network framing. They are not network compression ratios or relay throughput measurements. Times include concurrent test execution, filesystem work and production polling delays; this is qualification rather than a release-performance benchmark.

The real production CopyIo, copy_client, copy_worker and hidden helper ran with bounded pipe/stream windows and protocol durable credit. All cases asserted contiguous offsets, exact Data totals, reverse checkpoints, Done/Commit/Finish, natural helper exit, fully acknowledged stdout/stderr EOF and an empty final codec ledger. File-data frames never exceeded 8179 bytes. Each case retained at most 1024 frame samples; generation and hashing used 64 KiB buffers. Canonical stream windows were 256 KiB per channel and durable credit 2 MiB. Four cases ran on an eight-thread Tokio test runtime; each case had 1800 s and the aggregate 2100 s. The fixture checked at least 1 GiB free temporary space. No RSS measurement or multi-platform installed result is claimed.

Invocation from a private immutable copy of the compiled test executable:

```sh
marmish-native-tests --exact copy_integration_tests::large::large_raw_zlib_actual_helper_four_way --ignored --nocapture
```

The executable SHA-256 was `8077deb8de9a32329edf592972d7746320728456bc5598d347d11c6add5e32e9`, built from base 23105d859e995db8ba036d4db3f8d15e1ba913bf plus test-only changes. [The JSON evidence](copy-large-helper-local.json) records exact source hashes, bounds, counters, metadata and log hash. Private fixture paths and process state are excluded.

This executable predates the reviewed aggregate-timeout drain, first-error preservation and mutex-poison failure-path fixes subsequently committed in 52ac75a. Its successful data/codec path is unchanged. The final-source failure-path followups passed three focused tests (one expensive test intentionally ignored) and strict native all-target Clippy separately. The expensive run therefore proves the recorded executable's four successful data paths; it does not claim final-source failure paths were exercised by that long run.

Independent evidence verification recomputed the test-executable and run-log
hashes, matched the published source hashes to the build provenance, and compared
all four case records exactly against the original successful log. It also
checked the complete direction/codec set, payload lengths, frame/sample bounds,
final acknowledgement/exit flags and encoded/raw ratio arithmetic. This validates
the recorded evidence; it is not a second execution.
