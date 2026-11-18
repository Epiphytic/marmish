# Peer heartbeat and permission codec benchmark

One opt-in run used the Linux AMD64 debug build of the integrated
`95ce7683d0bcc0ed8720011be5b187e1bbb6610d` code and lockfile. It ran locally
with Rust 1.97.1 and one test thread. The detached worktree had crate
sources, Cargo inputs and toolchain configuration identical to that commit.

| Workload | Samples | Encoded bytes per operation | p50 | p95 | p99 | Payload MiB/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Decode heartbeat | 2,000 | 346 | 12.68 µs | 18.01 µs | 18.49 µs | 23.44 |
| Receive heartbeat | 2,000 | 346 | 9.79 µs | 9.99 µs | 11.84 µs | 33.56 |
| Receive permission pages in order | 80 | 27,992 | 1.026 ms | 1.046 ms | 1.099 ms | 26.01 |
| Receive shuffled permission pages | 80 | 27,992 | 1.388 ms | 1.415 ms | 1.716 ms | 20.56 |
| Receive shuffled permission pages at the description limit | 25 | 511,414 | 20.084 ms | 25.662 ms | 31.334 ms | 23.10 |

Heartbeat receive builds a fresh in-memory receiver and applies one synthetic
record per sample. Each permission sample includes creating the receiver,
feeding all encoded pages, and checking that the completed permission view is
fresh. The benchmark prepares and orders the messages before timing. Payload
rate divides the total encoded bytes by the summed operation time. These
measurements cover local codec and state handling. The benchmark uses
nearest-rank percentiles. At 80 and 25 samples, p99 is the maximum. At 25
samples, p95 is the second-largest observation. Relay transport, MLS,
SQLCipher persistence and the daemon are outside the measured path.

Run the benchmark outside core CI from a warmed dependency cache.

```sh
cargo test --offline --locked -p marmish-native --test peer_status_wire \
  peer_status_codec_benchmark -- --ignored --nocapture --test-threads=1
```
