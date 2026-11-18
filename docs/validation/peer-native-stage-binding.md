# Peer native stage binding validation

The reviewed increment uses SQLCipher and upstream MDK `0ad3c61` in
disposable local profiles. It opens no relay and creates no application
room. Author runs used Linux x86-64 on an AMD Ryzen AI Max+ 395 with
Cargo's unoptimized test profile and debug information disabled.

| Check on source tree `01c424fc` | Result |
| --- | --- |
| `cargo test --locked --offline -p marmish-native --lib peer_stage_ -- --test-threads=1` | 11 passed, 1 opt-in benchmark |
| `cargo test --locked --offline -p marmish-native --test peer_cursor_store` | 31 passed, 4 opt-in cases |
| `cargo clippy --locked --offline -p marmish-native --all-targets -- -D warnings -A clippy::large_enum_variant` | Passed, with the previously identified Terminal lint exception |
| `cargo rustc --locked --offline -p marmish-native --lib -- --emit=metadata,link` | Passed. The exact non-test metadata and library contain the `CursorStore` control and omit the provisional constructor, schema and `NativeStart`. |

The fixtures cover signed-Offer expiry and quarantine, lost native
signing, missing and corrupted stage files, byte-preserving restart,
foreign-key-off collision attempts and malformed lifecycle states.
SQLCipher reports authentication errors during intentional wrong-key
and corruption tests. Those cases return errors and keep the binding.
The separate downgrade test reopens the staged profile using the
production constructor and gets a typed binding refusal.

The opt-in benchmark uses one disposable profile for each first
preparation and then drops and reopens the state to measure exact
restart. Run it with
`cargo test --locked --offline -p marmish-native --lib peer_stage_local_latency -- --ignored --nocapture --test-threads=1`.
On the final reviewed source tree `01c424fc`, 20 samples per operation yielded:

| Operation | p50 | p95 |
| --- | ---: | ---: |
| First native stage preparation | 17.270 s | 36.999 s |
| Verified native stage restart | 151.853 ms | 176.687 ms |

These local debug-build samples are report-only. First preparation
includes isolated SQLCipher account initialization and native
KeyPackage signing. Restart includes reopening the profile, validating
the signed Offer and stored native package, and comparing encrypted
database digests. The measurements exclude relay latency.
