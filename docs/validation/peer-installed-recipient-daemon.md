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

Main extent is capped at 24 MiB. The conservative DELETE journal bound is 25,346,048 bytes with spilling disabled and the locked pager using one record per original database page. The combined bound is 50,511,872 bytes within the 49 MiB reservation. Ambiguous sidecars are kept and refused before SQLite opens them. The test-only opener still requires composed proof of ownership and path identity across first open, ordinary daemon startup, and both installed route outcomes. This slice does not qualify an installed build or macOS. [Design and code review](../reviews/peer-installed-recipient-daemon.md)

The integrated source `0f60a06` on the authenticated route branch passed
72 standalone cursor tests in 60.54 s with four opt-ins skipped. The
four installed cursor cap checks in the native test binary passed in
10.87 s. Rebasing over the published route evidence left the Rust and
Cargo trees byte-identical.

## Preexisting Ready startup proof

On Linux AMD64, exact source `d64dd85865c40d770e909ac2978fbcf581212746` passed three focused startup scope tests in 20.49 s, all 23 existing identity tests in 0.24 s, strict all-target native Clippy, and scoped rustfmt and staged whitespace checks. The tests use local temporary profiles with real prepared credentials and ordinary SQLCipher MDK owner bindings. They require no relay connection.

The checks cover preexisting Ready Manual Client and Both after reacquisition, an Initializing profile that remains ineligible even after Ready is written on the same lease, absent/Off/automatic receive modes, server-only startup, and Tor Always. They also cover a different lease or prepared identity, a mismatched opened binding, byte changes, inode replacement, and refusal after restoration. The preserved opened binding check is read-only. The ordinary Ready write still updates the manifest, and a later mounted qualified startup must use the read-only check to preserve its captured inode. [Design](../phases/peer-installed-recipient-daemon.md)

The composed source `fe0f28f` passed all three Ready scope tests in 18.33 s, all 23 identity tests in 1.25 s, and strict all-target native Clippy with the installed-checks feature. All 36 selectors in the trusted-master core workflow passed on local Linux AMD64 in 118.09 s across 160 test executions, including overlapping selectors. The publication rebase preserves the reviewed Cargo inputs, workflow, and Rust source byte for byte. [Integrated review](../reviews/peer-installed-recipient-daemon.md)

## Recovered recipient inventory

On Linux AMD64, exact source `9adbe329e2e4ebbbaff0303eb60993ee15622f0f` passed scoped rustfmt, whitespace checks, and `cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings`. The exact native library test binary passed the following groups.

| Filter | Result |
| --- | --- |
| `peer_receiver_inventory` | 3 passed in 1.56 s |
| `peer_installed_custody` | 6 passed in 3.10 s |
| `joined_retirement` | 3 passed in 0.38 s |

The focused tests reopened the exact cursor and retained identified or ambiguous charges under the same profile lease. They cover persisted rows, rollback of sampled time, stage file identity and main extent, unsafe sidecars or private directory replacement, and an unfinished receiver snapshot. The inventory reserves 49 MiB for the stock cursor and caps the aggregate at 128 MiB. Known stays charged and unavailable. A measured managed rollback journal remains fenced for native use until the dispatcher can verify it independently. The test-only owner grants no network or native join capability. Installed startup and macOS remain unverified. [Code review](../reviews/peer-installed-recipient-daemon.md)

Exact follow-up `834eb3ee8c33dee9855c81a3ada0450057e31ff4` passed the same strict Clippy and whitespace checks, the three focused inventory tests in 0.58 s, six installed custody model tests in 3.01 s, and three joined retirement tests in 0.29 s. The focused proof distinguishes a measured journal candidate from a clean image. Charging an additional Known file takes the combined capacity one byte over the bound and fences the exact still-present journal. Claude independently reran all three focused tests. [Code review](../reviews/peer-installed-recipient-daemon.md)
