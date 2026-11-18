# Native foreground Observe and Takeover validation

The Linux AMD64 fixture ran three actual `marmish terminal` processes in
independent controlling PTYs against prepared native owners, an authenticated
private loopback WSS relay, the installed Terminal worker and real tmux.
The processes shared one canonical account. The test established a writer,
attached an observer, and required the writer's delimiter and geometry to
appear on both PTYs while the observer candidate was neither echoed nor
executed. A third CLI showed the complete cooked Busy menu and selected
Takeover. The successor recovered the writer's shell variable through the
same tmux socket path, device and inode. The fixture checked all exit statuses,
stderr, exact restored termios and original PTY sizes.

An ordinary command round trip with exact binary stdin and stdout ran before,
during and after handoff. The prepared fixture drives both Command owners on
one task for a whole round trip. Starting that task immediately after the
cooked write starved the successor's one-second binding lookup and produced
`terminal_takeover_uncertain` in the first two development runs. The
validation starts the during turn after the actor services the next client
binding request and requires the turn's start to precede observed raw Ready.
Only the successor is starting a Terminal connection in this window. Both
during turns finished before raw Ready. The serialized fixture actor can let
the two-second server admission snapshot go stale during a Command turn.
The observed times can therefore include fixture scheduling and renewal.
In the production loop
the remote process runs on a Command engine, and local Command requests
enqueue or read state. Production MDK transport and discovery turns can still
delay binding requests. This sample does not measure that scheduling.

| Linux AMD64 binaries | Entire test | Observe raw | Choice to successor raw | Successor state output | Relay publication attempts |
| --- | ---: | ---: | ---: | ---: | ---: |
| Normal Cargo binaries | 57.70 s | 7.464 s | 12.939 s | 213.893 ms | 123 |
| Shell-installed optimized binaries | 59.56 s | 6.824 s | 13.933 s | 211.260 ms | 123 |

| Linux AMD64 binaries | Command before | Command during | Command after | During start after choice |
| --- | ---: | ---: | ---: | ---: |
| Normal Cargo binaries | 2.963 s | 2.815 s | 3.429 s | 7.272 ms |
| Shell-installed optimized binaries | 2.828 s | 4.709 s | 4.120 s | 6.716 ms |

Both runs observed the old writer, old observer and successor exit with
code zero and empty stderr. The author checked source `7dec6df` and reused
the previously [checksum-verified shell installation](terminal-installed-controls.md)
from build `d091cc8`. Its three executables were byte-compared with the
original installation and the controller used the matching package stamp
and dependency lock. The normal binaries used the same build stamp and
production source. Strict all-target Clippy with `installed-checks` passed
in 43.34 s. Scoped formatting and whitespace checks passed.

The times above are single loopback wall samples. The raw observations include
up to a 25 ms local polling interval. The state output duration starts at the
successor's state-marker write. The relay attempt count includes the real
Command and Terminal workload and excludes AUTH frames.

See [the backend benchmark](terminal-installed-runtime.md#benchmark-with-the-refreshed-upstream-mdk-lock)
for distributions. These foreground runs use fixture-only private TLS trust
and prepared owners. The scope is one Linux AMD64 host, one canonical
identity and one healthy handoff. Standalone daemon startup, public relay
trust, distinct device identities and forced failure during native replacement
are separate validation.

To rerun with matching already built native binaries, use

```sh
cargo test --offline --locked -p marmish-native --lib \
  terminal_installed_runtime_native_foreground_handoff -- --ignored --nocapture
```

The integrated source `a9e581b` passed with freshly rebuilt normal binaries
on Linux AMD64 in 54.45 s. Observe reached raw mode in 6.954 s. Cooked choice
to successor raw took 8.949 s, preserved state took 182.064 ms, and the relay
counted 121 publication attempts. The during Command took 3.540 s and started
8.089 ms after the choice. All three exits were clean. The app test files
match the author candidate byte for byte. The shared relay file carries the
identical counter patch on top of the newer authenticated route fixture.
[Composed review](../reviews/terminal-native-foreground-handoff.md)
