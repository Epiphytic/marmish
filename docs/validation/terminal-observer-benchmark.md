# Installed observer benchmark results

One opt-in run passed on 2026-11-18 on native Linux x86-64 with tmux 3.4, Rust 1.97.1, the debug build and the fixture's single-thread Tokio host. The test ran a disposable private loopback TLS relay, real native Marmot clients and SQLCipher profiles, Unix IPC, and native tmux. The fixture completed all setup, attached workloads and teardown in 52.71 seconds. The first observer's named Connect to validated Ready was **7.230 seconds** in this one attachment.

| Local reader | Timed sends | Minimum | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Writer | 20 | 127.426 ms | 145.656 ms | 163.003 ms | 173.711 ms |
| Observer | 20 | 109.903 ms | 123.328 ms | 137.485 ms | 156.897 ms |

Each latency pair starts immediately before the credited one-byte local Input write and ends independently at the reader's complete marked Output and completed local Ack write. The shell is in byte-at-a-time, no-echo mode. It reads using `dd` and emits a marker with the byte and ordinal. The two readers are on distinct MLS attachments of the same persistent pane. Both are serviced between trials. This is a paired series within one attachment. Host scheduling, polling order and the shell contribute to the measurements.

| Local reader | Requested ASCII bytes | Measured local display bytes | Display rate | Output records read and acknowledged through measurement |
| --- | ---: | ---: | ---: | ---: |
| Writer | 20,480 | 23,876 | 4,359 B/s | 47 |
| Observer | 20,480 | 23,876 | 4,359 B/s | 44 |

Twenty serial shell turns each requested 1,024 printable ASCII bytes and a unique end marker. Their common elapsed time was 5.476 seconds, including 2.020 seconds of pre-turn drains. The measured bytes are complete local Output payloads through each turn's end marker, including the marker record and any shell echo, ANSI and redraw accumulated in that turn. Each reader wrote one local Ack per Output through the end of the benchmark. The counts start when each socket opens, so the writer count includes its setup before the observer attached. The relay accepted 321 event publications across setup, measurements and cleanup.

These figures describe local receipt and acknowledgment, on this single-host serial workload, with measurement pacing included in display rate. Physical display completion, internet transport, stalled observers, warm writer and takeover attachment distributions, and raw TLS wire throughput require separate measurements. The paired latency percentiles and relative role ordering describe this run.

## Shell-installed Linux AMD64 package

The same fixture ran in a debug test controller against the
[checksum-verified optimized package](mdk-master-3548.md#optimized-linux-installer-and-private-terminal)
from source `d091cc8`, installed using `scripts/install.sh`. The debug
controller used the release build stamp, source `d091cc8` and the reviewed
test-only fixture files from `8fa94ff`. It ran the MLS and local IPC readers
and collected timings. The worker and daemon were optimized. The run took
37.49 s and its first named observer
attach reached Ready in 6.767 s. The relay accepted 321 publications.

| Local reader | Timed sends | Minimum | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: |
| Writer | 20 | 115.502 ms | 130.872 ms | 149.493 ms | 171.098 ms |
| Observer | 20 | 91.797 ms | 112.657 ms | 132.350 ms | 141.086 ms |

Both readers received 24,441 display bytes in a common 5.106 seconds,
or 4,786 B/s, for the same 20,480-byte request workload. This includes
2.027 seconds of pre-turn drains. The writer acknowledged 47 Output
records and the observer acknowledged 44. This is a single run on a
shared host, so it does not quantify the effect of compiler optimization.

## Regression at the candidate physical route head

The Linux AMD64 fixture passed again on source `9344290` with the
private candidate libtest and unoptimized stock native helpers. All
helpers had the same source revision and MDK lock. Foreground attached
in 6.451 s and echoed one byte in 128.907 ms. Ordinary Command before
and after took 1.923 s and 1.658 s. The separate named resume and
takeover test passed in 73.04 s.

The observer benchmark completed in 38.78 s with 323 accepted relay
publications. First named observer Ready took 7.305 s.

| Local reader | Timed sends | p50 | p95 | Display bytes | Display rate | Output acknowledgements |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Writer | 20 | 146.534 ms | 173.875 ms | 24,347 | 4,511 B/s | 48 |
| Observer | 20 | 119.508 ms | 139.768 ms | 24,347 | 4,511 B/s | 44 |

The same 20,480-byte serial shell workload completed in 5.397 s,
including 2.024 s of intentional pre-turn drains. This shared-host
debug run measures local receipt and acknowledgement over private
loopback WSS.

## Reproduce

On a supported nonroot host, build the native binaries and run the ignored report-only test in the same Cargo target:

```sh
cargo build --offline --locked -p marmish-native --bins
MARMISH_TEST_TMUX="$(command -v tmux)" cargo test --offline --locked \
  -p marmish-native --lib terminal_installed_runtime_observer_benchmark -- \
  --ignored --nocapture
```

[Installed observer benchmark design](../phases/terminal-observer-benchmark.md) · [Installed terminal validation](terminal-installed-runtime.md)
