# Candidate recipient startup handshake validation

The handshake requires both exact listener registration and completion of
ordinary startup in one candidate generation. Stop permanently revokes
that generation. The module is selected by the private candidate build
scope and has no daemon or listener call site yet.

| Check | Result |
| --- | --- |
| Standalone `clippy-driver --test -D warnings`, Linux AMD64, locked candidate Tokio `test-util` artifact | Passed for exact source `35f08c2f86cc412d15f584cda78df5b58b23a083`. |
| Exact standalone module tests | Six passed under a paused Tokio clock. Both edge orders, duplicate notifications, stop before release, waking suspended tasks, dropped endpoints, cloned read observations and generation independence passed. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |

The focused test binary links the existing candidate Tokio dependency only.
A full candidate Cargo check and the daemon tests remain part of composition.
Actual registration refusal, transient accept retry and stop-before-drain
ordering require the [integrated mount](../phases/peer-candidate-daemon-mount.md)
and its real listener and worker ownership.
