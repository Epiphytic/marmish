# Delivery cursor transaction validation

The Linux AMD64 standalone SQLCipher suite tested integrated commit
`a0a68d799b7c9d3d78de4a752799c19566689d3e` on
`d26c57040d84916867efae62100c93f68b53e1c3`. All three modified source files
are byte-identical to the reviewed author source
`09d617ba19f30f80c6dea7d19975003bfb17cc9f`. The source contains no
installed Delivery ingress or new runtime mount.

| Check | Result |
| --- | --- |
| Locked offline `cargo test -j2 -p marmish-native --test peer_cursor_store` | 74 passed, 4 existing ignored, 0 failures. Build took 28.96 s and execution took 64.98 s. |
| Locked offline `cargo clippy -j2 -p marmish-native --test peer_cursor_store -- -D warnings` | Passed in 18.13 s. |
| Rustfmt check of all three source files and `git diff --check` | Passed. |

The tests verify that a failed owner check rolls back both clock floors,
preserves the cursor revision and keeps exact deferred rows. They cover
empty due lookups, stale retry snapshots, early expiry retirement refusal,
capacity and lexical conflict after a fence, missing receipts, and
rollback when page completion fails. They prove the owner fence executes
before a proven count/tie quarantine is committed and that an active
quarantine stops the page. The full-page advance SQL error was not
separately injected.

The archived author and integrated standalone executables both have
SHA-256 `67585a1cdeac607f775cbac0b49bd3d06d163162bf0adba2fe2f7a9f75003ef5`.
The tests used local fixtures, with the real Delivery owner callback and
authenticated WSS left to the subsequent integration. See the
[exact Claude review](../reviews/peer-installed-delivery-ingress.md#cursor-transaction-increment).
