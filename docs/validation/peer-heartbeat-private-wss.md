# Private WSS heartbeat provenance validation

The [reviewed fixture phase](../phases/peer-heartbeat-private-wss.md) used
Linux AMD64 and Rust 1.97.1, the debug native test binary, upstream MDK
`122ad45e` and source `031edb6db380d92f9532d7173bf6e5fa62410ec5`.
The fixtures use fresh disposable native accounts, SQLCipher databases
and a process-owned loopback TLS relay. Keep the test executable in
Cargo's output layout so the fixture can locate its sibling helpers.

```sh
cargo build --locked --offline -p marmish-native --bins
cargo test --locked --offline -p marmish-native --lib \
  private_wss_heartbeat_observes_provenance_before_publish -- --ignored --nocapture
cargo test --locked --offline -p marmish-native --lib \
  private_wss_heartbeat_benchmark -- --ignored --nocapture
cargo test --locked --offline -p marmish-native --lib \
  private_wss_heartbeat_relay_refuses_unregistered_and_out_of_scope_messages -- --ignored
cargo test --locked --offline -p marmish-native --lib \
  callback_watchdog_refuses_missing_binding_and_stalled_provenance
cargo clippy --locked --offline -p marmish-native --lib --tests -- -D warnings
cargo check --locked --offline -p marmish-native --lib
```

The proof and benchmark each establish a fresh accepted native Peer group
over WSS before heartbeats. The first publisher call observes the complete
SQLCipher fanout through a clone of the runtime's storage handle while
still inside the callback, before any relay I/O. The retained native
record binds the application ID, exact signed Group bytes, sole target,
retention, and attempt 1. MDK then resumes those bytes through the
authenticated WSS target after its normal retry backoff. Each publication
has one Group write and matching ACK, target attempt count 2, and one
client projection from native ingress. The positive proof also replays
the original signed event. Native ingress reports a duplicate and the
client's existing projection and permission state remain unchanged.

The seven negative adapter turns use the same adapter implementation and
shared state as the runtime, with saved native requests. They preserve
the native attempt count, leave no ACK, and cause zero relay writes or
attempted writes. No authority represents status before acceptance. The
wrong endpoint case exercises changed authority scope, while the socket's
qualified origin stays fixed. The eleven receiver and protocol-lane
checks use direct malformed inputs where the native sender would refuse
to create them. The separate relay test refuses an unregistered signer,
an unrelated Group route, excess filter scope and public status kind
30104. No heartbeat creates or modifies a Peer control carrier in the
observed relay transcript.

The watchdog unit test and separate relay denial test passed in 0.00 s
and 0.13 s. The native sender-fence and status-wire selectors passed 12
and 16 tests respectively, with one opt-in test skipped in each. Strict native library/test Clippy and formatting on the changed
Rust files passed. Workspace-wide formatting reported pre-existing
differences in `marmish-engine/src/lib.rs` and `marmish-native/src/native_setup.rs`.
The ordinary native library check passed. A second non-test compilation
with three intentionally inserted imports in an isolated checkout failed
with the expected E0432 errors and explicit configured-out diagnostics
for `peer_heartbeat_send_fences`, `peer_status_wire` and
`peer_invitation::daemon_wss_tests`.

## Loopback timing and sizes

| Debug fixture | Records | Whole fixture | Heartbeat operations | Application bytes | Largest application | Signed Group bytes | Writes / projections |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Typical and maximal fields | 2 | 145.52 s | 59.847 s | 2,553 | 2,103 | 4,700 | 2 / 2 |
| Typical, separate benchmark | 12 | 446.53 s | 360.373 s | 5,403 | 451 | 14,988 | 12 / 12 |

| Debug fixture | Reservation to callback p50 / p95 | Callback lookup p50 / p95 | Reservation to ACK p50 / p95 | Reservation to projection p50 / p95 |
| --- | ---: | ---: | ---: | ---: |
| Typical and maximal fields | 199 / 441 ms | 1 / 1 ms | 29,691 / 29,890 ms | 29,771 / 29,991 ms |
| Typical, separate benchmark | 204 / 348 ms | 0 / 1 ms | 29,862 / 30,192 ms | 29,959 / 30,299 ms |

Elapsed sample timings use a monotonic clock and nearest-rank percentiles.
Callback entry includes native application setup; lookup measures time
inside the synchronous callback including binding validation. ACK and
projection include the closed-publisher path's normal approximately
30-second retry delay. With 12 samples p95 is the maximum. The callback lookup values use
integer milliseconds, where zero means less than one millisecond. The separate
[200-sample local planner and codec run](peer-heartbeat-planner.md)
measures local preparation and projection independently.

This qualification covers the test-only heartbeat path. Installed
heartbeat startup, durable Prepared recovery after restart, permission
page publication, macOS and Linux ARM64 were outside this run.

## Revalidation on MDK master and schema 214

Linux AMD64 source `6fe45ed` resolved MDK to `b3cd2cb1`. The two-record
proof finished in 132.26 s, with 60.158 s in heartbeat operations, 2,553
application bytes, 4,700 signed wire bytes, and two writes and projections.
The following percentiles include both records.

| Reservation to callback p50 / p95 | Callback lookup p50 / p95 | Reservation to ACK p50 / p95 | Reservation to projection p50 / p95 |
| ---| --- | --- | --- |
| 309 / 421 ms | 1 / 1 ms | 29,847 / 29,975 ms | 29,931 / 30,076 ms |

The revalidation passed `--features installed-checks` to each test command
above and the additional `peer_heartbeat_send_fences` and `peer_status_wire`
selectors. The lint command was
`cargo clippy --locked --offline -p marmish-native --features installed-checks --all-targets -- -D warnings`.
The watchdog and independent private relay denials passed. The sender
fence and wire selectors passed 12 and 16 cases respectively, with one
opt-in case skipped in each. Strict all-target Clippy with
`installed-checks`, the non-test library check, and the three-import
non-test exclusion probe passed. The deadline negative advances the wall
clock used by the authority check. The separate per-turn deadline branch
was outside this negative fixture.

The separate 12-record run passed in 436.16 s including a fresh enrollment,
with 360.049 s in heartbeat operations. All 12 reservations produced one
physical write and one client projection each. The records carried 5,403
application bytes (largest 451) and 14,988 signed wire bytes.

| Reservation to callback p50 / p95 | Callback lookup p50 / p95 | Reservation to ACK p50 / p95 | Reservation to projection p50 / p95 |
| --- | --- | --- | --- |
| 254 / 798 ms | 0 / 1 ms | 29,846 / 29,916 ms | 29,946 / 30,027 ms |

## Regression at the candidate physical route head

Three private WSS cases passed on Linux AMD64 source `9344290` in a
403.36 s harness run. The two-record proof covered provenance before publish
and the separate negative checked relay refusal for unregistered and
out-of-scope messages. The 12-record benchmark performed 12 writes and
12 projections, carrying 5,403 application bytes (largest 451) and
14,988 signed wire bytes.

| Records | Time in heartbeat operations | Reservation to callback p50 / p95 | Callback lookup p50 / p95 | Reservation to ACK p50 / p95 | Reservation to projection p50 / p95 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 2 | 59.869 s | 133 / 179 ms | 0 / 1 ms | 29,790 / 29,935 ms | 29,840 / 29,985 ms |
| 12 | 359.817 s | 97 / 156 ms | 0 / 0 ms | 29,941 / 29,949 ms | 29,977 / 30,000 ms |

ACK and projection each took about 30 seconds in this test-only path.
