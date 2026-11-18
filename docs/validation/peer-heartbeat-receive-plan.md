# Peer heartbeat receive transition validation

The private heartbeat codec prepares an in-memory change and applies it only
when the caller consumes the prepared transition. This increment exercises the
offline codec. It adds no production receive path, storage schema, native ACK
or migration.

## Observed results

- Linux AMD64, Rust 1.97.1. `cargo test --offline --locked -p marmish-native
  --test peer_status_wire` passed 16 tests. The existing synthetic codec
  benchmark remained ignored. All pre-existing assertions ran unchanged.
- The transcript captured 45 outcome and state fingerprints from the original
  in-place receiver at `a1c6559` before its replacement. The baseline temporary
  harness changes only added test access and the fingerprint printer. The
  committed test runs each transition once with a simulated failed commit,
  checks unchanged live state and reservation, then replans and verifies the
  original outcome and state after apply. The fingerprints cover the binding,
  seen receipt, display sample, current and completed permissions, and the
  stage metadata and buffer contents. Fixtures use synthetic status values and
  describe no actual profile.
- The stage append test checked that prepare and apply reuse the original
  stage allocation and the prepared append owns one page. Separate tests
  qualified the original receipt after dropping a prepared heartbeat,
  completed-policy freshness only after a matching refresh applies, and
  independence of two receivers sharing a binding.
- A disposable Rust compile probe called `receiver.sweep(now)` between
  `receiver.plan_receive(...)` and `prepared.apply()` on the same receiver.
  Rust rejected the sweep with E0499, “cannot borrow `*receiver` as mutable
  more than once at a time.” The probe file was removed afterward. The test
  target also compiles a static check that prepared transitions are neither
  `Clone` nor `Copy`.
- `cargo clippy --offline --locked -p marmish-native --test peer_status_wire
  -- -D warnings`, scoped Rust formatting and staged whitespace checks
  passed.

The test models only the in-memory half of a future checkpoint transaction.
Durability and authority across restart await the reported schema and owner
dependencies in the [heartbeat activation plan](../phases/peer-heartbeat-activation.md).

Completion still calls the existing canonical decoder. Its temporary
serialization buffer is additional to the stage, assembly and decoded
description, and is released before a plan returns. A future owner needs to
account for that bounded workspace separately from staged pages.
