# Peer native leave C1 validation

Linux AMD64 source `98a55dfda41863893a2451c41c1a5ae6aaf940d9` on
`a9780fd8c875524a0f489222aef29705a3a36cad` uses upstream MDK
`122ad45ece73c1d6f687a4e92a541b9b2febde34`. The Cargo.lock SHA-256 is
`9ff66fb199fa7130fe462ac5a08ac44019b6e5b72355dd988868ed38f0fe4ac7`.

`cargo test --offline --locked -p marmish-native --lib -j2 custody:: -- --test-threads=1`
passed 18 tests in 24.14 seconds with zero failures. Three existing opt-in
private WSS tests were ignored. The deterministic tests cover:

- Fresh and cold epoch-5 open, wrong account or origin, weakened SQLCipher
  connection hardening, older and partial inventories, and preservation of
  the exact schema when a different owner attempts to open it.
- Atomic hold and decision, a rollback between inserts, exact local authority
  and generation, both refusals to add a hold after a no-hold decision, and
  checked SQLite deadline arithmetic at the signed integer limit.
- Exact historical Offer route, deadline boundaries, retained charge after
  expiry and reopen, global and origin wall floors, and the independent
  terminal-row fence with the existing progress aperture open. The new
  epoch-5 fence is a separate trigger, preserving the previous terminal DDL.
- Immutable hold scope, the independent SQL recovery transition guards and
  accepted transitions, cold-reopened 1 s and 2 s floors, stale owners and
  selectors, the third stop, and fairness between both bounded slot classes
  and ordinary no-hold expiry and retirement.
- Existing M5 recipient publication and Active reservation and founder
  enrollment and acceptance on epoch 5 as well as their epoch-4 checks.

Strict native Clippy passed with
`cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings`.
Scoped Rust formatting and the staged Git whitespace checks passed.

The fixtures exercise durable SQLCipher state. The C1 hold does not open a
native stage or grant a Leave or egress operation. Actual sidecar recovery,
native membership changes and macOS evidence are outside these results.

On composed source `5d8a8a5` with the same MDK revision and the private
Peer TLS substrate, Linux AMD64 passed all nine epoch-5 cases in 13.79
seconds. The full custody selector passed 18 with three opt-in cases
skipped in 31.85 seconds. Strict all-target Clippy with `installed-checks`
passed in 51.68 seconds. The publication rebase preserved all validated
Rust source and Cargo.lock bytes.
