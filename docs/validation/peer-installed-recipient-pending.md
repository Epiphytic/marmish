# Installed v2 recipient custody validation

The exact Linux AMD64 source reviewed here is `08683dee521625893d0ddeea5071f0171eab41d6`. This increment covers the test-only recipient custody schema, version-aware signed transcript checks, and per-founder admission mode. The marker-only installed path remains unchanged. The test-only successor rejects legacy writers and the uncapped fixture path for Join and Pending.

| Check | Result |
| --- | --- |
| Installed v2 custody schema and checkpoints | 5 passed in 0.51 s |
| Installed Offer regression | 6 passed in 2.55 s |
| Existing custody and terminal schema regression | 6 passed in 1.45 s |
| Existing Prepared schema regression | 2 passed in 0.37 s |
| Native crate all-target Clippy with `-D warnings` | Passed |
| Native binaries with `installed-checks`, dev profile | Passed |
| Rustfmt on changed files and `git diff --check` | Passed |

The focused tests cover exact signed Offer ancestry, rollback on an ineligible predecessor, catalog and writer isolation, exact stage reservation and binding, cursor floor and replay behavior, route and profile mismatch, and per-founder Manual, Automatic, Off, blocked, and self policy. The existing fixture continues to exercise v1 storage semantics.

The author ran `cargo test --offline --locked -p marmish-native --lib -j2 peer_installed_custody`, `cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings`, and `cargo check --offline --locked -p marmish-native --features installed-checks --bins -j2`. The three regression cohorts ran on the same compiled library test binary. The workspace formatting check identified existing differences in `crates/marmish-engine/src/lib.rs` and `crates/marmish-native/src/native_setup.rs` outside the diff.

The next integrated proof must create the actual Prepared Consent and Held custody from the signed private WSS route, use capped native Welcome ingest with the one-shot Join token, reopen for exact native readback, and commit Pending from the sealed readback. This source has no installed recipient owner or CLI approval path. The optimized release and macOS runs were outside this increment.
