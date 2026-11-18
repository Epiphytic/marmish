# Terminal v1 record validation

The [approved Terminal frame increment](../phases/terminal-frames.md) is
compiled into the native test binary. The tests use disposable ephemeral
accounts and private loopback TLS. Product activation, supervisor authority
checks, relay scheduling, and PTY or display integration are later increments.

## Reproduce on Linux

Run from the workspace root with pinned dependencies in the local Cargo cache.

```sh
cargo test --offline --locked -p marmish-native --lib terminal_frames::core_tests -- --nocapture
cargo build --offline --locked -p marmish-native --bins
cargo test --offline --locked -p marmish-native --lib -- --ignored --nocapture --test-threads=1 \
  codec_and_admission_benchmark maximum_terminal_records_round_trip_through_memory_rooms \
  maximum_terminal_records_round_trip_through_private_wss terminal_copies_never_ingest_in_durable_command_profiles
cargo test --offline --locked -p marmish-native --lib -- --ignored --nocapture --test-threads=1 \
  unpublished_pending_package_joins_and_carries_terminal_frames \
  authenticated_relay_accepts_native_events_and_rejects_cross_room_dispatch
cargo clippy --offline --locked -p marmish-native --tests -- -D warnings
```

Ten core tests passed in 0.01 seconds after the build. They check a fixed
binary vector, record shape and bounds, binding and direction, generic app
kind isolation, freshness and independent clocks, ordering, exact partial
progress, resize, cumulative acknowledgment, immutable handoff, queue
pressure and independent controls. Deterministic core tests reorder the
records and verify that resolving one sequence allows the next its own
presentation or gap timer while preserving its original signed-age limit.
They also check clock and sequence exhaustion and activity-based lease renewal.
Four opt-in tests passed in 43.71 seconds
including native setup, private relay negotiation, real worker commands and
cleanup. The disposable durable Command profiles processed ordinary Commands
before and after foreign Terminal records were injected. The Terminal event
created neither an ingress dedup marker nor a processed transport ID on the
two Command profiles, including after database reopen.
The existing CHAT-based native room and private relay regression tests passed
in 10.67 seconds. The native binary build and Clippy also passed.

## Native message sizes and latency

The Linux x86_64 host ran Rust 1.97.1 in the unoptimized development profile.
Each direction carried a 4096-byte payload containing all byte values and
used the maximum unsigned 64-bit sequence and generation. Native delivery
matched the expected MLS account, leaf, room, current epoch, exact app bytes
and authenticated 60-second expiration. A separately subscribed room rejected
both records. The WSS fixture measured the exact client text-message payload
that the loopback TLS relay accepted.

| Measurement | Input | Output |
| --- | ---: | ---: |
| Binary Terminal payload | 4096 B | 4096 B |
| Encoded app event | 5761 B | 5761 B |
| Protected outer event content | 7936 B | 7936 B |
| Signed kind-445 event | 8380 B | 8380 B |
| Accepted WSS text payload | 8390 B | 8390 B |
| Local MDK seal and open | 27.366 ms | 21.280 ms |
| Loopback publish, delivery and MLS open | 39.369 ms | 41.282 ms |

The app event fit the 6144-byte app limit. Protected content and the outer
event fit the corresponding 16 KiB and 32 KiB limits. Sizes exclude
WebSocket framing and TLS overhead. The last two rows give one observation
per direction and exclude room creation and, for the WSS case, connect/setup
and sealing before publish.

## Local codec microbenchmark

The benchmark took 2000 samples per payload size after 100 warmups. The codec
measurement includes the JSON wrapper and its NIP-01 ID, base64, and Terminal
validation. Ready-after-admission measures one local ordered record and sink
offer on an empty attachment. All timings exclude native encryption and relay
communication.

| Payload | Codec median | Codec p95 | Payload throughput | Admission to ready median | Admission to ready p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 3 bytes | 30.677 µs | 34.463 µs | 0.091 MiB/s | 350 ns | 361 ns |
| 4096 bytes | 410.438 µs | 696.345 µs | 8.781 MiB/s | 410 ns | 1773 ns |

Public relay, Tor, macOS and Linux arm64 performance remain unmeasured in
this phase.
