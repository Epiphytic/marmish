# Manual Pending approval storage validation

Linux AMD64 ran source `dea0089f196356813e889eddb847efc0c6d19f08`
with locked upstream MDK `e4421e86a6c73f336b0154ee09c13826d8fafff5`.
The stock native libtest was built offline with Rust `1.97.1`, bundled
SQLCipher, two build jobs, incremental compilation disabled, and test
debug information disabled. The executable SHA-256 was
`7bd3840fa3cebc69c5419a9dae528713bdfbc42102234e9d2106f837dac56a95`.
The libtest link took 43.96 s with a warm dependency cache. Native Clippy
passed for all targets with `-D warnings` in 31.75 s. Formatting and the
Git diff whitespace check passed. The v2 DDL and marker/state verification
block matched the baseline byte-for-byte.

| Exact libtest filter | Passed | Ignored | Wall |
| --- | ---: | ---: | ---: |
| `installed_approval_` | 3 | 0 | 1.85 s |
| `peer_installed_custody_` | 9 | 1 | 3.99 s |
| `custody::tests::pending_` | 5 | 0 | 2.34 s |

Each filter used `--test-threads=1`. The ignored test requires a private WSS
relay. Installed v2 reopen tests verified signed Prepared Consent and Held
Delivery. The new cases opened fresh keyed SQLCipher v2/v3 stores and
reopened each exact inventory. They checked refusal without layout mutation
for partial, mixed, legacy and drifted inventories.

SQL-level Pending fixtures exercised a Manual Unknown state transition,
the exclusive deadline, repeat refusal, owner-only writes and rollback of both
wall floors. The corruption cases verified that a valid Manual approval
passes the v3 reader and that independently altered class, mode, decision
or terminal presence returns `Binding`. In the provisioning database, Unknown
is class `0`. The quota array uses its own index.

This increment validates the test-only schema and stored-state predicates.
It leaves the installed v2 custody path in use. The mounted writer and
an authenticated installed Pending → Activating reopen require the later
decision phase.

## Composition with the near-cap fixture

The composed source at `0cb418674ca9537c5bc140627dee55ccffb50217` retains
the exact v2 near-cap fixture helper in the shared schema file. Stock
Clippy and private Linux AMD64 candidate Clippy passed for all targets with
`-D warnings` in 31.83 s and 32.68 s. The locked offline stock libtest
passed the same three filters with 3, 9, and 5 tests respectively. The
private WSS test remained opt-in.

The locked candidate libtest
`360991b41a8b18aaecb4dee89893e6f9f0f84c92871065b361fc05489da60bd5`
passed all three approval schema tests in 3.26 s. The same executable
passed the isolated offline near-cap test of a genuine joined stage in
49.05 s. This test preserved the selected stage geometry from the separate
[installed refresh evidence](peer-candidate-near-cap-fixture.md).
The rebase over the published refresh report preserved the tested crate,
Cargo, and CI files byte-for-byte.
