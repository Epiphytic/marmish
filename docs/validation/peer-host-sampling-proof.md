# Native host sampling calibration

The offline test exercises the collector candidates in the [approved sampling plan](../phases/peer-host-sampling-proof.md). It is ignored by default and has no CLI, daemon, transport, or publication integration. Run it from the repository root on a native host.

```sh
cargo test --locked --offline -p marmish-native --lib \
  peer_host_sampling_proof_tests::peer_host_sampling_proof \
  -- --ignored --exact --nocapture
```

The test passed on Linux x86-64 with Rust 1.97.1 in a debug build. It reads the effective account through `getpwuid_r`, compares `CLOCK_BOOTTIME` against `/proc/uptime`, collects three fixed-point load averages, samples one labeled filesystem, and filters local loopback addresses by selected interface and selected IP families. The disk percentage uses blocks used divided by the sum of blocks used and blocks available to the effective user. The load cap is one million whole load units, or one billion milli-units.

Synthetic cases cover passwd buffer exhaustion, missing and invalid names, malformed clock values, load bounds, disk overflow, address length and family, exact interface selection, null address and the eight-address cap. The child receives bogus `USER`, `LOGNAME` and `HOME` and verifies that the collected account matches none of them. Captured output is checked before printing. The report contains only timing data and fixed dispositions.

All native reads run in a worker thread inside a disposable child. The outer process verified that it could terminate and reap a child whose worker was deliberately parked, with a three-second startup and stall budget and a 500 ms reap budget. This exercises the test harness deadline. Daemon IPC responsiveness and production worker recovery still require the phase's own tests.

## Timing

Each measurement used 16 warmups and 200 samples. The whole-sample timer covers all five native reads and their conversion to bounded values. Percentiles are observations from one run and are excluded from CI pass criteria.

| Field | p50 (µs) | p95 (µs) |
| --- | ---: | ---: |
| Effective account | 19.867 | 25.998 |
| Host uptime | 0.090 | 0.150 |
| Load averages | 0.822 | 1.062 |
| Filesystem usage | 1.583 | 1.914 |
| Selected interface addresses | 38.301 | 52.417 |
| Whole sample | 61.424 | 84.446 |

## Platform coverage

The collector source passed `clippy --tests -- -D warnings` in an isolated check crate using the repository's Rust toolchain and pinned `libc` on `x86_64-unknown-linux-gnu`, and with `--target` set to `aarch64-apple-darwin` and `x86_64-apple-darwin`. The Darwin checks were cross-target type checks from Linux. Native Linux arm64 and macOS execution are unmeasured.

The Linux run reports `sleep=not_measured`. The macOS sampler always uses the wall-derived boot interval for its awake benchmark. The separate sleep trial compares the continuous and wall-derived clocks. On a Mac, run the following check and put the system to sleep within the first minute. Wake it after at least two seconds of suspend. The check reports whether the clock origin appears aligned with the boot timestamp or offset, and omits uptime when the observations are inconsistent. An awake run reports `sleep=not_measured`. [Linux address ABI implementation](https://github.com/bminor/glibc/blob/master/sysdeps/unix/sysv/linux/ifaddrs.c) · [Darwin sockaddr ABI](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/socket.h)

```sh
cargo test --locked --offline -p marmish-native --lib \
  peer_host_sampling_proof_tests::peer_host_sampling_macos_sleep \
  -- --ignored --exact --nocapture
```
