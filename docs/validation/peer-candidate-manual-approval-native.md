# Candidate Manual approval decision validation

The unmounted Linux AMD64 candidate decision binds one claimed local request
to the existing checked Peer route, profile lease, exact persisted Manual
enrollment and same original charged MLS stage. The native read uses the
existing bounded disposable copy. The first success commits one custody
transition and the same value in both persisted clock floors. A subsequent
exact request can verify the committed decision in a read-only transaction
under the original Pending deadline.

The source depends on the separately reviewed candidate local approval
exchange. Validate and land the two source leaves together for the candidate
build. Stock behavior and executable recipient startup selection are
unchanged. The decision leaf has no installed caller and does not provision
the approval schema from the installed daemon.

The deterministic SQL transaction tests use a synthetic Pending row graph
with the v3 guards. They verify the exact transaction functions called by the
native decision. Separate SQLCipher tests attach the actual installed-family
marker and inventory, validate the v3 read entry point and cover rejection of
v2, automatic policy and missing/wrong-account selections. Native timing and
route horizon tests use a local controlled clock and exact no-I/O fences.
The local exchange tests are separate evidence for actual Unix socket behavior.
An installed invitation through Approve, checked native reopen, projected
read invalidation and final reply after the same lifecycle/port checks remain
requirements of the composed Manual actor phase.

Generic SQLite and cursor invariant errors at the durable boundary close the
decision as integrity failures under the exclusive lease. A reported error
from the commit attempt is ambiguous. Request interruption, stale evidence,
signed expiry and a clock regression remain distinguishable from this case.

## Exact checks

The final native source checkpoint was `8ff321f`, integrated and built as
`076b5ad` with the separately reviewed local request-token overlay
`5e482b3`. The previous `395a9f7` leaf was also built, tested and reviewed
as combined `a7bba81`. The final run used locked MDK `e4421e8`, Linux AMD64
target `x86_64-unknown-linux-gnu`, `--cfg marmish_peer_receiver_candidate`,
offline Cargo, two build jobs and debug information disabled. The final
linked test executable SHA-256 was
`06dd78208d8b78829780a1979ea948bd0fae6a5907e4897b97f0144e3b752a77`.

| Check | Result | Elapsed |
| --- | --- | --- |
| Candidate `cargo clippy --all-targets -- -D warnings` | Passed | 33.07 s |
| Candidate library test link | Passed | 49.61 s |
| `approval` | 13 passed | 7.27 s |
| `native_window_honors_absolute_route_horizon_and_fatal_cut` | 1 passed | <0.01 s |
| `custody::tests` | 9 passed | 7.30 s |
| `peer_approve_listener::tests` | 5 passed | 0.01 s |
| `peer_invitation::native_fence::tests` | 2 passed | <0.01 s |

Filters overlap. Scoped Rust formatting and `git diff --check` passed.
