# Local client inventory validation

These tests exercise native Rust on Linux x86-64 with Rust 1.97.1 and
SQLCipher. The fixtures use disposable keys and 0700 test directories.

```sh
cargo test --locked --offline -p marmish-engine --lib policy::inventory_core_tests
cargo test --locked --offline -p marmish-native --lib inventory_
cargo test --locked --offline -p marmish-native --lib inventory_migration_rollback_preserves_legacy_copy_records_and_version
cargo test --locked --offline -p marmish-native --lib client_inventory_benchmark -- --ignored --nocapture
```

The engine tests compare normalized restrictions with admission semantics,
including wildcard paths, additive layers, explicit empty allows, and the
engine's independent match budget. The native tests cover typed replies,
canonical keys, bounded pages, stale policy generations, named-profile
isolation, local reads during relay degradation, the IPC deadline, queue
saturation and response limits. An additional 20 direct repetitions of the
stalled IPC fixture passed. The real Unix sockets use same-UID credentials
and mode 0600.

Encrypted database tests cover migration from the previous journal, v6
rollback and retry, wrong keys, fresh grants, duplicates, transient and
confirmed refusals, clock regression, observed removal and readdition,
late engine completion and exact source invalidation after command garbage
collection. The signed native fixture delivers a real MLS-encrypted Submit,
asserts authenticated projection, supplies a controlled engine-denial
completion, and exercises the native persisted invalidation state before a
delayed delivery. The fixture sets the invalidation state through the native
storage API.

## Live daemon and command check

Built the three native binaries together and used two disposable file-backed
profiles with explicit `wss://relay.us.whitenoise.chat/` and Tor off. The client
trusted the server's npub, and the server granted the client's npub. Both
daemons ran in the foreground. The built `marmishd clients list` showed the
grant and empty history before execution. A normal `marmish NPUB -- printf`
command returned its argument byte-for-byte, including literal quotes,
`$HOME`, glob characters, a semicolon and a non-ASCII character. The server's
`marmishd clients show NPUB` reported both timestamps and the default `/**`
allow rule. After stopping and restarting the daemons with an explicit deny
rule for `false`, `show` retained the timestamps and returned a new policy
generation. The next remote command returned `command_denied` and empty
stdout. On server restart, the inventory showed a newer authenticated receipt
and an unchanged engine-admission time. The query output contained no command
arguments or native invocation identifiers.

An earlier live run using a stale sibling `marmish-worker` returned
`recovery_uncertain`. The positive and deny results above came from
the first fixture after rebuilding the worker together with both
launchers. The three-launch check separately verified that a revoked
client disappears and a regrant starts with empty history.

In a different fresh-profile fixture, a live command returned
`recovery_uncertain` with matching build metadata in all three binaries.
The inventory still showed durable engine admission, and a bounded
process trace found no worker exec. The same fresh-profile fixture
returned `recovery_uncertain` with the pre-inventory source
`3cef7a798d1da9e435aa4c1fcf68f5138c509d67` after both identities had
published key packages. The failing and successful fixtures report
different outcomes. Beads `marmish-dwt.6` tracks qualification of command
launch in the fresh-profile fixture. Inventory acceptance rests on the
first live run and the local SQLCipher and IPC cases above.

Disposable profiles and processes are local to the checks. Linux ARM64,
macOS, and a separate-UID socket attempt need independent validation.

## Local query benchmark

Each operation measures one first request and 200 warm sequential requests
with a monotonic clock. The timer includes connecting to a real same-UID Unix
socket, request and response framing, dispatch to the fixture owner, and
SQLCipher reads or effective policy page construction. The owner uses the
production query function. The timer excludes profile setup, daemon startup,
relay access and CLI process startup. The 4,096-key list returns 64 rows;
each populated `show` returns a policy page between 7,400 and 8,192 bytes.

| Configured keys | Operation | First | p50 | p95 | p99 | Requests/s |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 0 | List | 741 µs | 62 µs | 112 µs | 184 µs | 13,244.1 |
| 0 | Show, unknown key | 143 µs | 64 µs | 162 µs | 267 µs | 12,119.6 |
| 64 | List | 5,541 µs | 1,545 µs | 2,109 µs | 2,376 µs | 620.8 |
| 64 | Show | 1,289 µs | 1,031 µs | 1,392 µs | 1,637 µs | 915.3 |
| 4,096 | List | 2,913 µs | 1,512 µs | 1,923 µs | 2,274 µs | 621.1 |
| 4,096 | Show | 1,164 µs | 1,008 µs | 1,075 µs | 1,211 µs | 980.6 |

The benchmark has zero unexpected errors. The self-hosted workflow selects
the short policy, query and encrypted journal core tests. The opt-in
benchmark and the heavier local acceptance fixtures are available separately.

## Release manifest compatibility

The Node release-tool suite passed 257 tests. Its inventory cases bind the
journal format to the selected Git commit, reject a changed marker or a
crossed manifest, and retain the historical v0.2 and v0.3 release checks.
The Python workflow-policy suite passed all 14 tests. These local checks
required neither a GitHub Actions dispatch nor a release.

## Native regression run

Across two executions on Linux x86-64, all 677 nonignored native cases
passed. The frozen broad run passed 672 and ignored 9. Its five native
packaging fixtures failed with an all-zero build source. A rebuild with
`MARMISH_BUILD_COMMIT` set to the checked-out commit passed the five
packaging fixtures. The Rust engine suite also passed, including 149
library cases, one ignored library benchmark, and the integration tests.
