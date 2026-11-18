# Native writer selection on a private relay

Linux x86_64, debug standalone test with the native command worker and tmux.
The relay was a disposable loopback TLS fixture requiring NIP-42. Both
accounts used fresh keys and independent SQLCipher profiles. Each terminal
flight used a separately prepared Command carrier to isolate the fixture's
publications while exercising the same native server registry.

```sh
cargo build --locked -p marmish-native --bins
MARMISH_TEST_DAEMON="$PWD/target/debug/marmishd" MARMISH_TEST_TMUX=/usr/bin/tmux \
  cargo test --locked -p marmish-native --lib terminal_selection \
  -- --include-ignored --nocapture
```

Three deterministic selection codec tests and the native WSS proof passed. An
authorized ordinary binary Command result succeeded before and after the
terminal flight. Default obtained a writer. A second Default was Busy and
allocated no additional pane. New obtained a distinct pane and did not see the
first shell's variable. Reattachment returned to the original pane with a
different room transcript, claim and greater native generation. Twenty samples
preserved the expected marker, including spaces, a semicolon, percent and
Unicode. A remote resize to 30 by 90 was visible inside the pane. Revocation
closed the exact native attachment, both owned panes and registry rows were
released, and ordinary Command succeeded again. An early Confirm created no
pane. A wrong Confirm claim and an unanswered Accepted both left no attached
writer, and each retained pane was removed by the fixture through the exact
registry and tmux ownership checks. After authority retired, the fixture
attempted a native write and verified the absence of both its completion and
the requested file creation. The serialized relay events and durable
profile database files contained no generated output marker.

| Measurement | Samples | Result |
| --- | ---: | ---: |
| Private room enrollment after ordinary Command readiness | 1 | 5.865 s |
| Cold native writer selection | 1 | 447 ms |
| Warm selection of an existing pane | 1 | 635 ms |
| Key input to native PTY and validated echo received over WSS | 20 | 81.9 ms p50, 92.9 ms p95 |
| 16 KiB shell output receipt over WSS, records of at most 4 KiB | 1 | 86.9 ms, 189 kB/s |

This fixture times native PTY and private WSS but excludes installed foreground
display and WAN conditions. The single payload demonstrates a bounded
workload and gives a point measurement. The installed throughput run will
require repeated payloads. At this stage, the direct wrong-generation,
duplicate Request during Start, dropped Start reply and lost Accepted
network cases are outstanding. The installed command path and Linux ARM64
and macOS ARM64 runs remain to be qualified.
