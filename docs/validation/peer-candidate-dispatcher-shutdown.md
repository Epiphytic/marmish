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

## Composed candidate

The private Linux AMD64 candidate at
`61be37534b6f0549e34da7fc95c45a0b76bd21ec` combines the relay write fence
with the [Pending reader permit](peer-candidate-pending-view.md).
One exact native test executable covered the following selectors.

| Check | Result |
| --- | --- |
| `ipc::peer_read` | 11 passed in 0.01 s |
| `dispatcher_fatal` | 1 passed in less than 0.01 s |
| `transport::tests` | 18 passed in 0.01 s |
| `registration_scope_and_irreversible_dispatch` | 1 passed in 19.32 s, including the five isolated SQLCipher cases |
| `inert_daemon --ignored` | 5 passed in 21.74 s on private loopback WSS |
| Strict all-target Clippy | Passed in 29.73 s stock and 29.86 s candidate |
| Formatting and diff checks | Passed |

The exact candidate executable SHA-256 is
`dc3d7f8cdf6354107886a56fd2d17fcaa655990c778588fdc75411c82d73bcec`.
These tests cover the inert APIs and existing listener behavior. Active
Peer observations and socket teardown on daemon dispatcher loss require
the mounted phase.
