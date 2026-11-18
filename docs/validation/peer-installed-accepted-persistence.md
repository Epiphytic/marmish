# Installed Peer v2 Offer persistence validation

Source `7b4c4d3fcded2e6f4377e868d32c1bfbc67f9ef4` builds on
`97a1cadc0501f8e9c0f5f76e59722ceff9f9c93b`. The checks ran locally on Linux
AMD64 with the workspace lockfile and bundled SQLCipher. The debug builds
and tests used `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`.
The release checks used the default optimized profile.

| Scope | Command | Result |
| --- | --- | --- |
| Signed installed Offer persistence | `cargo test --offline --locked -p marmish-native --lib -j2 peer_installed_offer_` | 6 passed in 3.12 s |
| Consent, provisioning, custody and wire | `cargo test --offline --locked -p marmish-native --lib -j2 peer_invitation::store::consent_checkpoint` | 52 passed, 3 excluded by opt-in, in 67.61 s |
| Existing cursor store | `cargo test --offline --locked -p marmish-native --test peer_cursor_store -j2` | 63 passed, 4 opt-in benchmarks ignored, in 63.44 s |
| Native all-target Clippy | `cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings` | Passed in 34.20 s |
| Non-test native release compile check | `cargo check --offline --locked --release -p marmish-native -j2` | Passed in 35.55 s |
| Optimized signed Offer persistence | `cargo test --offline --locked --release -p marmish-native --lib -j2 peer_installed_offer_` | 6 passed in 2.44 s, 282.33 s including build |

Scoped Rust formatting and `git diff --check` passed. The installed Offer
cases cover Automatic and Manual behavior, exact repeat and signed replay,
transaction rollback for admission and sweep, claim retention during sweep,
capacity, layout exclusion, persisted integrity refusal and both version
fences. A full cursor page includes signed malformed routing, signed
configured-route mismatches, a blocked sender and a valid Offer. The
cursor commits the dispositions, advances its boundary and completes the
next short page. The pure native transcript case also rejects a
cross-version Delivery using a locally generated package.

The opener checks all retained signatures when reopening. A lookup or
replay checks the relevant parent. Existing test-only native provisioning
stays separate from this increment. Legacy retirement of a marked Offer
fails at its DELETE guard. The sweep currently restores that guard in a
transaction even when no parent is due for deletion.

These runs used no relay connection or daemon. They do not qualify installed
activation, live acceptance, macOS or Linux ARM64.

The prebuilt debug selector ran in 0.29 s for signed malformed routing
and cursor continuation, and 0.80 s for Automatic restart and exact v2
ancestry. The composed branch adds the first selector to the existing
self-hosted core workflow. The remaining Offer persistence cases can run
locally using the cohort selector above.
