# Inert Peer candidate daemon lifecycle validation

The author tested source `61f4f31f07f60ab7ae2afc0e5dd54f1d2f6c5f0b` against
`24dc054`, which contains the independently reviewed candidate startup
handshake. The [composition design](../phases/peer-candidate-daemon-composition.md)
defines the lifecycle boundaries.

The five opt-in tests ran on Linux x86-64 against a private loopback WSS
relay, fresh owner-only file identities, the ordinary native owner and its
actual single same-UID Unix listener. The daemon used the existing background
reporter protocol through a private socket pair. The worker was inert,
with no Peer route or storage.

| Exact Linux check | Result | Elapsed |
| --- | --- | ---: |
| Warm offline locked library test build | Passed | 38.10 s |
| Inert daemon loopback cases | 5 passed | 21.85 s |
| Startup handshake unit cases | 6 passed | < 0.01 s |
| IPC filter, including legacy Peer and Terminal handshake | 59 passed, 2 opt-ins skipped | 0.15 s |
| Strict native library and test Clippy | Passed | 25.59 s |

The loopback checks covered:

- Ordinary Status and empty Received replies after a single injected interrupted
  accept, followed by stop before listener drain. The deliberately parked
  worker retained the original profile lease after the socket stopped accepting.
- Registration refusal with the ordinary listener and owner queue still serving
  Status and Received.
- A seeded regular file at the socket path. Actual bind refused it, the waiting
  worker stopped without becoming ready, and the file's bytes remained intact.
  Fixture cleanup removed the seeded file after the original lease released.
- An injected early error from the owned listener task. The daemon preserved its
  error through the actual join handle and released the lease.
- Background readiness bytes across the actual reporter channel. While the
  launcher acknowledgement was outstanding, the listener exposed ordinary Active
  Status and the worker remained blocked. Both an invalid acknowledgement and
  a stop observed during acknowledgement prevented worker release.

The first author run on `937d2f8` exposed that the default temporary base mode
was refused by the live profile owner. The final commit uses owner-only bases.
The injected accept and listener exits exercise the lifecycle branches.
The pending affirmative integration still needs actual candidate reads,
ordinary Command and Terminal operation with the candidate present or unavailable,
and the actor-backed shutdown and process-fatal checks. This author run used
the stock test compiler configuration.

Reproduce the focused checks from the reviewed source with a warm Cargo target:

```sh
RUST_MIN_STACK=33554432 cargo test --offline --locked -p marmish-native \
  --lib inert_daemon -- --ignored --test-threads=1
cargo test --offline --locked -p marmish-native --lib peer_candidate_startup::tests
cargo test --offline --locked -p marmish-native --lib ipc::
cargo clippy --offline --locked -p marmish-native --lib --tests -- -D warnings
cargo fmt --all --check
```

## Integrated validation

The composed Linux AMD64 source was
`956aabfe8d8fd5ac9e6b9b0bbb7905227c9580bb`, tree
`41a576c327edd25801cfa62998d5b50a51321683`. The stock test executable's
SHA-256 was `3366d12147fbc9ab3fa13dd2e5881a7a7dabfadb3184489e23ed6b410ec60b55`.
The explicit candidate test executable's SHA-256 was
`a3e2929dcdf2738f1fd0e1f0faf56ddaf47be9a8d6da573e8bdfc17f8db360ac`.

| Linux AMD64 composed check | Result | Elapsed |
| --- | --- | ---: |
| Stock inert daemon private WSS suite | 5 passed | 22.11 s |
| Candidate inert daemon private WSS suite | 5 passed | 21.99 s |
| Stock startup handshake and shared IPC | 65 passed, 2 benchmark opt-ins skipped | 0.11 s |
| Stock isolated registration subprocess harness | 1 passed, including 4 fixture processes | 14.74 s |
| Stock and installed-checks non-test binaries | Passed | 8.65 s and 14.81 s |
| Stock strict Clippy, all native targets | Passed | 28.39 s |
| Candidate all-target check and strict Clippy | Passed | 23.58 s and 28.70 s |

The five lifecycle cases remain explicit ignored tests. Both runs use a
private loopback WSS relay. The candidate run also checks that a connection
to the real same-UID listener receives `peer_unavailable`. The IPC run
includes the legacy paired listener. Exact stock and candidate warm libtest
build times were 42.17 s and 43.83 s. Formatting and diff checks passed.
The [integrated review](../reviews/peer-candidate-inert-lifecycle.md#integrated-code-review)
approved this source.
