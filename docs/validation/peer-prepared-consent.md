# Prepared-stage Consent proof validation

The detached Linux AMD64 proof uses upstream MDK `9a29d1e4`, keyed
SQLCipher storage, disposable keys and private profiles. Tests publish
captured bytes to a synchronous in-process sink. No relay, native Welcome,
room or application command participates. The run used Rust 1.97.1 and an
AMD Ryzen AI Max+ 395 with the unoptimized Cargo test profile and debug
information disabled.

Use the workspace root with `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0`. Keep the locked dependency cache available
for the following offline commands. The final correctness tests and
non-test library check use `bef9e8c` (tree `57aab739`).

| Command | Result |
| --- | --- |
| `cargo test --offline --locked -j 2 -p marmish-native --lib peer_invitation -- --test-threads=2 --skip raw_socket --skip benchmark` | 63 passed, 6 ignored, 209.46 seconds. |
| `cargo test --offline --locked -j 2 -p marmish-native --test peer_cursor_store -- --test-threads=2` | 33 passed, 4 ignored, 48.14 seconds. |
| `cargo build --offline --locked -j 2 -p marmish-native --lib --message-format=json` | Built the non-test rlib. `strings` found none of `peer_prepared_consent`, `reserve_prepared_attempt`, `settle_prepared_ack`, `SnapshotArea` or `prepare_capped_fixture_consent`. |
| Scoped `rustfmt --edition 2024 --check` for the invitation and standalone cursor test modules and `git diff --check` | Passed. |

The lifecycle tests cover native preparation and byte-preserving image
inspection, owner and schema guards, exact row/slot collisions, crash
cuts around signing and publication, expiry behavior, SQLCipher reopen,
and the positive attempt ACK compare-and-swap. Negative cases cover
legacy claims, corrupt parent rows under the exact schema, failed cursor
writes, dirty sidecars, wrong keys, copy migration, and altered saved
frame, carrier and package values. SQLCipher authentication errors are
expected from the deliberately damaged and wrong-key images. The recipient decrypts
the actual saved frame and checks the whole recorded transcript. The
independent [Claude review](../reviews/peer-prepared-consent.md) records the
source fingerprint and test scope.

## Standalone codec follow-up

On `0003703`, workspace Clippy passed with the existing large-enum exception.
The standalone wire tests passed eight cases and skipped one opt-in benchmark.
That suite checks the sender and recipient transcript against the capped
Consent deadline.

```sh
cargo clippy --offline --locked -j 2 --workspace --all-targets -- -D warnings -A clippy::large_enum_variant
cargo test --offline --locked -j 2 -p marmish-native --test peer_control_wire
```

## Local timings

Run `cargo test --offline --locked -j 2 -p marmish-native --lib prepared_consent_local_benchmark -- --ignored --nocapture --test-threads=1`.
Each of 20 turns uses a newly encrypted profile, performs real MDK stage
preparation, queues one signed frame, reserves an attempt, drops and
reopens the owner at the 60-second retry boundary, then sends identical
bytes and settles a matching local ACK. The fixture advances its clock
with no actual 60-second sleep. The run measured 20 identical-size
5,997-byte frames and completed in 331.57 seconds. It used the initial
integrated fixture at `23acc89`. The review follow-up added negative tests
and supplied transaction context to the existing test fault hook. The
benchmark workload and owner state transitions stayed the same.

| Operation | Min (ms) | p50 (ms) | p95 (ms) | Max (ms) |
| --- | ---: | ---: | ---: | ---: |
| Native stage preparation | 10,942.435 | 13,394.521 | 19,988.775 | 20,035.011 |
| Copy, verify, sign and durable queue | 429.243 | 475.906 | 530.888 | 542.672 |
| Reopen, verify and reserve exact retry | 430.950 | 476.117 | 556.617 | 575.223 |
| Recheck and publish to local sink | 63.874 | 67.957 | 88.656 | 95.203 |
| Verify and commit matching ACK | 103.145 | 132.270 | 168.298 | 181.263 |

Report-only debug-build timings. Local send includes the exact state
recheck and digest custody, with no relay round trip. This workload
measures fixed-size control events. Terminal, command and bulk-transfer
throughput require separate workloads.

The final Linux AMD64 composition on `fa22cdf` passed workspace all-target
Clippy with `-D warnings`, then the codec suite (8 passed, 1 ignored),
Prepared Consent library selector (7 passed, 1 ignored), and standalone
cursor suite (33 passed, 4 ignored). Active test times were 2.21, 100.70
and 42.47 seconds respectively.
