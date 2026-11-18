# Borrowed Pending owner validation

Linux AMD64 with the locked MDK dependency. The author built the isolated
exact source `07fbf22b7f885ebdf5766f949ed099510855535a` with
`cargo test --offline --locked -p marmish-native --lib --target x86_64-unknown-linux-gnu --no-run`
and `--cfg marmish_peer_receiver_candidate` in 49.07 s. The resulting
executable SHA-256 was
`bce3ca2d9f4e1ddbc01e536015ef649d4960554de3467a6cbcf783a1f17ee981`.

- The two opt-in `private_v2_pending_` parent selectors, excluding
  `private_v2_pending_child`, passed in 104.12 s. They used four fresh
  exact private NIP-42 WSS scenarios with authentic signed Delivery and
  cold SQLCipher profile reopens. They exercise a successful native Welcome
  and Pending replay, a replacement owner before any native poll, a
  journal-growth refusal with the original image unchanged, and a process
  restart after a withheld native completion result. Each scenario
  recorded zero publication attempts after Held.
- Individual managed operations in that four-scenario run took 132–541 ms
  on this host. These values time the fixture's native/SQL steps and exclude
  relay history, process startup, actor maintenance and IPC. The fixture
  uses the shared borrowed owner with its test clock. The assembled actor
  must measure its actual registered route clock and view publication
  separately.
- The prior `peer_capped_welcome_native_proof` covered successful ingest,
  quota capacity, mutation and cancellation. All four child cases passed
  under one parent selector in 78.23 s. Four inventory checks and the
  physical native-window horizon/fatal-cut check also passed.

The final owned source `4cf701565fbe4e9cc52f152f8d7fe725b37a80d5`
adds only a must-use annotation and a comment over the runtime-tested source.
With the independent managed reader warning fix, the exact integration
`f2ae74fbb2ede5e75e9a364d04d04aa3c34804ec` passed candidate
`cargo clippy --offline --locked -p marmish-native --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`
in 28.62 s. Workspace formatting and the patch whitespace check also passed.

The actor-facing entry is `PendingTurn::borrow(&mut owner, lease)`
with `PendingClock::Candidate { readiness, stop }`. Each method uses the
actor's checked recipient port and its signed deadline. The returned
`ClosedJoin` records the native result, and only current qualified readback
can commit the Pending row. This source introduces no daemon call site,
reader route, approval action, relay publication, or cross-platform
qualification. The [independent review](../reviews/peer-candidate-borrowed-pending.md)
records the exact source and its review follow-up.
