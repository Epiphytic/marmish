# Upstream MDK refresh for private WSS recovery

Claude Code Sonnet 5.5 in Auto mode approved the exact lockfile change
`936686ebe2e3c5c7fb4cc3010a0fb5a5bd820a60` in session
`18b2dbd8-55d9-451a-8c7f-1bfd45741422`. The reviewer checked the diff,
the public upstream changes, locked Cargo metadata and dependency tree,
and the upstream branch head. Ten MDK entries moved together from
`f89bd59089b92ddbb2492def6972e8bc75d09cf6` to
`59b7ed3ff007494b25f8404c144c1d82809b3973`. The native manifest tracks
upstream `master`. The existing lock keeps the other resolutions.

MDK excludes unpublished local commits from convergence at this revision.
Its storage migration adds a field to the epoch-gap retry table. A native
account database opened and migrated by this revision requires the new
schema on later opens. Marmish's separate journal schema is unaffected.
[Upstream convergence change](https://github.com/marmot-protocol/mdk/pull/3531)
[Upstream storage change](https://github.com/marmot-protocol/mdk/pull/3541)

Linux AMD64 validation on the same lock and rebuilt native binaries passed
strict all-target native Clippy in 28.43 seconds and all three private WSS
founder history cases in 122.29 seconds. The recipient caught up in 1.809
seconds with complete history. It retained its original native epoch with
the commit omitted and when the relay withheld the history end marker.
The ordinary Command regression during a private Peer relay outage passed
in 48.06 seconds, including 6.372 seconds of command execution. These are
standalone samples from a local TLS relay and disposable profiles.

```sh
cargo clippy --locked --offline -p marmish-native --all-targets -- -D warnings
cargo build --locked --offline -p marmish-native --bins
cargo test --locked --offline -p marmish-native --lib postjoin_private_wss -- --ignored --test-threads=1 --nocapture
cargo test --locked --offline -p marmish-native --lib peer_daemon_wss_peer_outage_preserves_ordinary_commands -- --ignored --test-threads=1 --nocapture
```
