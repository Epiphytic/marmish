# Candidate recipient startup handshake validation

The handshake requires both exact listener registration and completion of
ordinary startup in one candidate generation. Stop permanently revokes
that generation. The module is selected by the private candidate build
scope. The only daemon and listener call sites are currently test-only.

| Check | Result |
| --- | --- |
| Standalone `clippy-driver --test -D warnings`, Linux AMD64, locked candidate Tokio `test-util` artifact | Passed for exact source `35f08c2f86cc412d15f584cda78df5b58b23a083`. |
| Exact standalone module tests | Six passed under a paused Tokio clock. Both edge orders, duplicate notifications, stop before release, waking suspended tasks, dropped endpoints, cloned read observations and generation independence passed. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |

The focused test binary links the existing candidate Tokio dependency only.
The [composed inert lifecycle tests](peer-candidate-inert-lifecycle.md)
exercise the real Unix listener with an inert worker and private relay.
The live candidate actor mount remains separate.
