# Terminal writer through native runtime IPC

The opt-in fixture runs two native Terminal runtimes and two independent durable
Command owners with fresh identities and SQLCipher profiles. It uses the same-UID
framed request socket, the real `marmishd` Terminal worker, the real
`marmish-worker` Command worker, and real tmux. The only relay is a disposable
loopback WSS server requiring NIP-42. Its temporary CA enters through a test-only
connector scoped to that exact origin.

Build both binaries from the same source and profile as the test executable.
Running a library test alone can leave old binaries in the target directory.
Run as a nonroot user with tmux available.

```sh
cargo build --locked -p marmish-native --bins
cargo test --locked -p marmish-native --lib terminal_installed_runtime_ \
  -- --ignored --skip benchmark --test-threads=1 --nocapture
cargo test --locked -p marmish-native --lib terminal_installed_runtime_benchmark \
  -- --ignored --nocapture
```

The separate [installed observer benchmark](terminal-observer-benchmark.md)
measures paired local writer and observer receipts and serial shell display
rate. Both benchmarks are covered by the exclusion above.

The EOSE retry test withholds the exact authenticated server alias subscription
completion. The relay queues the first client Probe before completion on the
same connection, suppresses duplicate outer event IDs, and requires at least
two distinct signed envelopes addressed to the server before its first reply.
The test then requires Ready within 30 seconds and reports its timing separately.

## Observed results

| Linux x86_64 debug test | Result |
| --- | --- |
| Native writer | Passed. Ordinary binary Command worked before, during and after Terminal traffic. The terminal echoed one key, rendered UTF-8 and shell punctuation, resized to 30 by 90 and interrupted a foreground process. Busy preserved the original writer and allocated no new tmux socket. New started an isolated shell. A boundary socket half-close produced a typed transport close. Reattach preserved a shell variable. |
| Pending local EOF | Passed. The relay held the first addressed gift wrap while the local request socket half-closed. Releasing the message produced zero additional response bytes and no tmux socket. A later Default attach worked. |
| Final shell output | Passed. After warm reattach, shell exit delivered the distinct final output marker before the local typed Close(Ended). |
| Terminal runtime restart | Passed after the authenticated EOSE fix. The first writer detached and the old runtime joined. The new owner's alias subscription opened without responding to relay history. A fresh Default Start reattached to the existing pane and preserved shell state. |
| Delayed authenticated EOSE | Passed. The same signed Probe used a fresh outer relay ID on retry. The first reply followed the retry and Ready took 10.037 seconds in total and 9.982 seconds from EOSE. |

A serial rerun of the three passing tests with strict typed Detach took
235.51 seconds. Before the EOSE fix, a separate restart test reproduced an
unsolicited reply to retained history in 44.35 seconds. Both the restart and
delayed-EOSE checks passed together in 109.04 seconds after the fix.

These runs used upstream MDK master `0ad3c610` and a private loopback relay.
The writer's original 70.67-second run produced the point measurements below.
Debug stage diagnostics and other builds shared the same host. The first prior
ordinary Command timing includes room warm-up. The report-only benchmark warms
the room before recording that comparison.

| Workload | Samples | Debug private loopback result |
| --- | ---: | ---: |
| Cold Ready | 1 | 8.266 s |
| Warm Ready | 1 | 8.872 s |
| One key through native PTY and local output record | 1 | 159.9 ms |
| First ordinary Command before Terminal | 1 | 4.992 s |
| Ordinary Command during Terminal output | 1 | 5.311 s |
| Ordinary Command after Terminal | 1 | 4.346 s |

The relay transcript, six raw durable database files and any present SQLite
companion files omitted the generated terminal marker. SQLCipher hides
persisted content from a raw marker scan. This run did not inspect decrypted
rows or capture worker logs. Independent Terminal storage and ingress tests
cover additional persistence boundaries.

The optional benchmark repeats the latency workloads across 20 independent
fixtures. Its serial shell output workload uses a 42 by 120 terminal and requests 4 KiB
of varying printable ASCII per command until the client acknowledges at least
1 MiB of displayed bytes. The reported rate
includes the shell round trip, echoed input and any tmux redraw bytes. It
measures the observed display workload rather than network capacity. The local
client, loopback relay and ordinary Command actors share one Tokio LocalSet.
Each production Terminal owner runs on its own thread. The benchmark
completed in 1,142.78 seconds. The latency sample uses 20 independent profiles.
The throughput workload sent 245 serial shell commands and acknowledged
1,050,591 bytes in 58.874 seconds, or 17,844 B/s.

| Linux x86_64 debug workload | Samples | p50 | p95 |
| --- | ---: | ---: | ---: |
| Cold Ready | 20 | 8.635 s | 9.984 s |
| Warm Ready | 20 | 8.652 s | 10.174 s |
| Single key through PTY | 20 | 155.1 ms | 174.6 ms |
| Warm ordinary Command before Terminal | 20 | 4.265 s | 5.340 s |
| Ordinary Command during Terminal output | 20 | 4.400 s | 5.130 s |
| Ordinary Command after Terminal | 20 | 4.403 s | 5.315 s |
| Serial command requesting 4 KiB output | 245 | 238.5 ms | 275.8 ms |

The external CLI, installer, full daemon-process restart and Linux ARM64 and
macOS ARM64 are separate validation gates. The restart fixture replaces the
server Terminal owner while keeping the ordinary Command owner available.

The expanded writer fixture subsequently ran from Cargo's normal debug binary
layout in 66.53 seconds. It passed private WSS, concurrent ordinary Command,
multibyte input and output, resize, Ctrl-C, Busy against the live writer,
a separate New slot, half-close and warm reattach with prior shell state.
The single-sample cold and warm Ready times were 7.189 s and 8.165 s, and the
single-key echo was 137.530 ms. The harness keeps acknowledging live output
while trying another attachment. Cargo-layout workers must launch from the
build directory for the binary integrity check.

## Benchmark with the refreshed upstream MDK lock

The ignored benchmark ran on Linux AMD64 with the normal debug binaries
and matching harness from exact source `464300c` and MDK `bdf9833e`.
All 20 independent loopback WSS trials passed in 696.34 s. No warm
attach received Busy. The serial display workload acknowledged
1,052,024 bytes in 53.084 s, or 19,818 B/s. These are local measurements
of the native backend through its framed fixture client. The
independent [foreground CLI sample](terminal-installed-foreground.md)
uses the controlling terminal instead.

| Workload | Samples | p50 | p95 |
| --- | ---: | ---: | ---: |
| Cold Ready | 20 | 6.847 s | 8.138 s |
| Warm Ready | 20 | 7.449 s | 8.216 s |
| Single key through native PTY | 20 | 141.7 ms | 158.8 ms |
| Warm ordinary Command before Terminal | 20 | 1.702 s | 2.026 s |
| Ordinary Command during Terminal output | 20 | 1.752 s | 1.994 s |
| Ordinary Command after Terminal | 20 | 1.822 s | 2.020 s |
| Serial command requesting 4 KiB output | 244 | 215.2 ms | 268.3 ms |

The serial result includes a shell round trip and display receipts for
each request. The benchmark completed its process and fixture teardown.
