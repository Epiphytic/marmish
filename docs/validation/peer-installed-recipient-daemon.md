# Installed Peer recipient daemon validation

## Stock cursor cap proof

On Linux AMD64 with locked MDK `bdf9833` and the bundled SQLCipher pager, exact source `df09a01c1e6c38f64d48694264aa6e9138b542b3` passed the following offline author checks.

| Check | Result |
| --- | --- |
| `cargo test --offline --locked -p marmish-native --test peer_cursor_store -j2` | 72 passed, 4 ignored, 62.29 s |
| `cargo test --offline --locked -p marmish-native --lib -j2 peer_installed` | 16 passed, 10.88 s |
| `cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings` | Passed |
| Scoped rustfmt and staged whitespace check | Passed |
| Independent Claude Auto execution of the four stock cursor proof tests | 4 passed, 9.9 s |

The proof refused oversized or misaligned main images and preexisting journal, WAL, and SHM entries. It exercised a rollback journal near the page cap, observed `SQLITE_FULL`, and verified that rollback and cold reopen preserved all 1,450 rows and the previous extent. It retained every byte of 256 maximum-sized deferred records across a cold reopen and returned the bounded backlog error for the next record.

Main extent is capped at 24 MiB. The conservative DELETE journal bound is 25,280,512 bytes with spilling disabled and the locked pager using one record per original database page. The combined bound is 50,511,872 bytes within the 49 MiB reservation. Ambiguous sidecars are kept and refused before SQLite opens them. The test-only opener still requires composed proof of ownership and path identity across first open, ordinary daemon startup, and both installed route outcomes. This slice does not qualify an installed build or macOS. [Design and code review](../reviews/peer-installed-recipient-daemon.md)
