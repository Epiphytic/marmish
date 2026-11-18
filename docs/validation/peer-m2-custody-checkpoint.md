# Peer Pending custody checkpoint validation

The test-gated SQLCipher component at
`497afba5d5ce41750152fcf852e607e228142921` was validated in isolation on Linux
AMD64. It was composed with the exact wire witness and restore codec, the
custody-only cursor cleanup seam, and native JoinedReadback commit
`ffd3481a5cac2149813acd17f0546ee4ba025433`. The installer, CLI and daemon
keep their existing production behavior. The actual native readback and
private WSS receiver are verified under separate integrated validation.

| Check | Result |
| --- | --- |
| `cargo clippy --offline --locked -j 2 -p marmish-native --all-targets -- -D warnings` | Passed in 9.73 s. |
| `cargo test --offline --locked -j 2 -p marmish-native --test peer_cursor_store --test peer_control_wire` | Cursor 35 passed and 4 opt-in benchmarks ignored in 37.89 s. Wire 8 passed and 1 opt-in benchmark ignored in 3.02 s. |
| `cargo test --offline --locked -j 2 -p marmish-native --lib prepared_consent -- --test-threads=1` | 9 passed and 2 opt-in tests ignored in 131.87 s. |
| Scoped Rust format and Git diff whitespace | Passed. |

The standalone custody tests upgrade only exact Prepared Consent epoch 1,
reject older constructors after the upgrade, exercise transactional
write denials and rollback, reject an outbox revision mismatch, and track
the charged generation and all dependencies through each retained state
and legacy sweep. They verify exact replay/deferred byte matching and invalid
disposition at the exclusive replay deadline. They seed synthetic row
bytes for SQL-only transitions that require native proof in the actual owner.

An initial regression run exposed broader trusted-cursor cleanup accidentally
requiring exact bytes for the legacy owner. The final cursor helper scopes exact
cleanup to custody's Held and exact replay, and the old rollback/replay test
passes alongside the new checks. The test suite also reports expected
SQLCipher diagnostics while opening intentionally wrong-key fixture files.

macOS and the integrated private WSS/native owner are outside this standalone
run.
