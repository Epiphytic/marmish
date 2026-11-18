# Candidate relay write fence validation

Source `121dc730f437881a14847a79210bd26e11bfc8ae` on Linux AMD64.
The new observer and transport branches compile only in tests or the
private candidate build on this platform. This increment does not
connect them to daemon startup, the registrar, or a live Peer owner.

| Check | Result |
| --- | --- |
| `dispatcher_fatal::tests::blocked_duplex_polls_wake_without_forwarding_after_fatal` | Passed. Two tasks blocked on separate read/write halves woke with `ConnectionAborted`. Only the pre-fatal byte reached the peer. Later vectored write, flush and shutdown failed. |
| `transport::tests::fatal_drains_each_exact_route_slot_and_blocks_new_connects` | Passed. Real TCP on two local routes. The fenced raw stream refused a post-trip write, new open failed, and reaping timed out while the second route's last permit was retained. Reaping completed after release. |
| `transport::tests`, serial | 18 passed in 0.01 s, including the new connector case and existing socket-half ownership, cancellation, route and byte-preservation regressions. |
| `cargo check -p marmish-native --locked --tests -j2` | Passed in 20.32 s. |
| `cargo clippy -p marmish-native --locked --lib --tests -j2 -- -D warnings` | Passed in 28.80 s. |
| `cargo check --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib` with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'` and exact `MARMISH_BUILD_COMMIT` | Passed in 9.91 s, including the cache lock wait. |
| `cargo fmt --check` and `git diff --check` | Passed. |

Both focused passes used one native test executable (40.73 s warm
compile/link). Its SHA-256 was
`a040dad527177c77852f6bab42cf300e28cfd4c18d9ae7aa12678750a72b8b95`.
The raw TCP test owns real transport streams and withholds an explicit
slot clone. It does not measure a live SDK holding the permit, actual
SQLite dispatcher loss, an installed daemon, or script and package
behavior. Those remain in the [mounted phase](../phases/peer-candidate-dispatcher-shutdown.md).
