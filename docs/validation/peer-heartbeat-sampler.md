# Private heartbeat sampler validation

The [private sampler increment](../phases/peer-heartbeat-sampler.md) uses a
stand-alone test target. It leaves daemon and CLI entry points and required CI
target selection unchanged. The test runs without a profile, relay or keys.

```sh
cargo test --offline --locked -p marmish-native \
  --test peer_heartbeat_sampler --test peer_status_wire
cargo test --offline --locked -p marmish-native --test peer_heartbeat_sampler \
  sampler_native_worker_and_benchmark -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native \
  --test peer_heartbeat_sampler --test peer_status_wire -- -D warnings
```

The sampler target passed 23 tests, including seven new sampler tests, four
existing clock tests and 12 existing codec tests. The native sampler and codec
benchmarks remain ignored by default. The separate codec target passed all 12
ordinary tests. Strict Clippy and formatting checks passed.

Deterministic cases exercised request bounds, source selection, malformed
native results, fixed-point and byte arithmetic, per-interface address family
selection, malformed native interface names, scope handling, the canonical
address cap and codec round trips.
Clock and channel fixtures covered start failure, deadline expiry, clock
regression, undelivered late results, worker panic, permit isolation, permit
retention after owner drop and recovery. The native run verified that disabled
address collection omitted the field, explicit loopback selection produced
only the chosen IPv4 loopback address, and a missing configured disk path did
not suppress a successfully sampled disk label. It checked all base fields and
encoded and decoded the result. An IPv6-only request accepted a missing
loopback address on a host with IPv6 disabled and otherwise required only
the IPv6 loopback address. The output contained no sampled values.

## Local latency

The opt-in debug build completed 200 timed local worker samples on Ubuntu
24.04.5, Linux x86-64, with Rust 1.97.1. Timings start just before submission and
end at the caller's successful handover. Each sample collects the effective
account, uptime, all load averages, two configured disk paths (one missing),
and explicit IPv4 loopback. The test uses nearest-rank percentiles.

| Samples | p50 | p95 | p99 | Maximum |
| ---: | ---: | ---: | ---: | ---: |
| 200 | 107 µs | 164 µs | 198 µs | 378 µs |

The numbers include worker scheduling and polling. They measure the local
collector. Relay transport, publishing and authorization are outside this
increment. The synthetic timing loop and native test remain outside required
CI.

## Platform coverage

The exact sampler, native backend and existing continuous clock sources
passed strict cross-target Clippy for `aarch64-apple-darwin` and
`x86_64-apple-darwin` using pinned `libc` 0.2.189 and a type-only shim for the
status codec and native error interface. These checks cover the Rust native
API bindings. They used Linux as the build host and did not link the app or
execute on macOS. Native macOS execution, macOS sleep behavior and native
Linux ARM64 remain unmeasured.
