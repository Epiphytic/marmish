# Terminal Command authority validation

Linux x86-64, isolated disposable profiles and loopback WSS. No public
relay or user identity was involved. All four deterministic cases, the
actual native WSS case, the timing case, and strict all-target Clippy
passed. Claude independently passed the four deterministic cases, native WSS
and strict Clippy.

```sh
cargo test --offline --locked -p marmish-native --lib terminal_command_authority_core
cargo build --offline --locked -p marmish-native --bins
cargo test --offline --locked -p marmish-native --lib terminal_command_authority_native_wss -- --ignored --nocapture
cargo test --offline --locked -p marmish-native --lib terminal_command_authority_bench -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native --all-targets -- -D warnings
```

Build all native binaries in the test target directory. The existing Command
worker checks the sibling executable layout. A missing or independently
built worker can yield `not_started` before the fixture reaches its
authority assertions.

The test qualifies export from a ready Command room, fail-closed absence of
server grant, rejection of a different caller-supplied route, and exact
receiver-side fence retirement after native membership and metadata
mutations. It executes an ordinary Command on a separately prepared room.
The deterministic owner test covers incomplete projection, non-durable input,
route deletion, transport loss and owner teardown. GitHub Actions selects only
the actor retirement test here.

| Measurement | Samples | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Local native WSS Suspect interval, author | 20 | 22.860 ms | 28.891 ms | 51.217 ms |
| Local native WSS Suspect interval, independent review | 20 | 14.1 ms | 22.1 ms | 30.7 ms |
| Debug empty-table stage | local loop | 40 ns | 71 ns | — |
| Debug stage and resume with 32 bindings | local loop | 14.516 µs | 18.835 µs | — |

The author's native fixture completed in 50.91 seconds. The independent run
completed in 48.67 seconds. The Suspect interval covers actor work blocking
the fenced effect. It excludes physical terminal display and Internet relay
transit. The p95 target was 50 ms. A separate idle single-client tick and a
two-peer tick pair were exercised to observe normal actor scheduling, with
different workloads and no direct causal latency comparison.
