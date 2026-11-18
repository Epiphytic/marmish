# Directional Peer heartbeat planner validation

The [planner phase](../phases/peer-heartbeat-planner.md) ran in the stand-alone
native integration target. It uses scripted owner authority and transport
facts with the real status codec and command policy implementation. Required
CI target selection was unchanged.

```sh
cargo test --offline --locked -p marmish-native \
  --test peer_heartbeat_planner --test peer_heartbeat_sampler --test peer_status_wire
cargo test --offline --locked -p marmish-native --test peer_heartbeat_planner \
  planner_native_roundtrip_benchmark -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native \
  --test peer_heartbeat_planner --test peer_heartbeat_sampler --test peer_status_wire -- -D warnings
```

The planner target passed 31 tests. Eight are new checks for role, exact
generation, field and route gating, interval and retry bounds, clock changes,
sample expiry and late delivery, revision changes, sequence reservation,
engine permission parity, partial permission issuance, replay and retirement.
The remaining tests cover the native sampler, its continuous clock sources,
and the status codec. The standalone sampler and codec targets passed
23 and 12 tests respectively. Strict Clippy and formatting checks passed.

## Local latency

The opt-in test first verified every enabled host field using the real sampler
with one temporary disk path and explicitly selected IPv4 loopback. It then
timed 200 local round trips in a debug build on Ubuntu 24.04.5 Linux x86-64
with Rust 1.97.1. It used nearest-rank percentiles. Only the scheduler and
projector readings advance between samples to make the next sample due. The
native sampler and elapsed latency clocks run continuously. Test output
contains only counts and timings.

| Stage | p50 | p95 | p99 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Local round trip | 94 µs | 114 µs | 159 µs | 322 µs |
| Planner and encoding | 15 µs | 17 µs | 22 µs | 34 µs |
| Native worker and polling | 69 µs | 90 µs | 133 µs | 270 µs |
| Client projection | 9 µs | 10 µs | 15 µs | 17 µs |

The test covers local preparation, native sampling, release, codec decoding
and projection. Live MLS transport, persistence and CLI/daemon integration are
future work. Native Linux ARM64 and macOS execution were unmeasured for this
increment.

Ripwire's isolated quality delta flagged token-shape matches between the
new binding conversion and unrelated ledger, file-copy and cursor records,
and between the scripted sampler and a terminal close port. These pairs
have different types and responsibilities. The integration tests exercise
the fixture code that Ripwire reported as dead in the production graph.
