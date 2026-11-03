# Installed local driver offline verification

This increment adds build-only autoexit, uptime, benchmark, recovery and policy cohorts plus
bounded Actions supervision and strict three-platform aggregation. It does not
qualify a release, a platform, real keychains or either public installer URL.

The common integration has actual independent Claude approval in
[round 3](../../reviews/v0.2-installed-local-integration-code-r3.md); the Node
supervisor has [its own approval](../../reviews/v0.2-installed-local-node-code-r2.md).
Component reviews separately control their internals. The final six-cohort registration has independent
[code approval](../../reviews/v0.2-installed-local-final-code-r1.md). Recovery has
[code approval](../../reviews/v0.2-installed-recovery-code-r2.md); policy has
[component code approval](../../reviews/v0.2-installed-policy-code-r2.md).
Its two first-review blockers were fixed and tested before source freeze.

A subsequent source-contract audit found driver assumptions that the offline
tests did not exercise: attached exit-7 stderr contains no handle, client handle
digests differ from group-bound admission digests, fresh no-grant clients need
not construct an engine, and cached stream hashes are nullable. Corrections have actual independent design approval and scoped code approvals in
[the contract amendment](../../phases/v0.2-installed-contract-corrections.md).
The earlier scoped approvals and passing checks below do not establish that
these drivers can complete installed workloads; final installed validation of
the corrected drivers remains required.

Observed checks on Linux AMD64:

- Node Actions suite: 44 passed. The supervisor tests cover retained child/group
  ownership, bounded drains and termination, cancellation, residual descendants,
  credential exclusion and exact bounded nonsymlink artifact collection.
- Installed-feature Rust suite: 68 passed in 5.20 seconds, including actual setup/grant/trust
  preservation, rejection of unexpected policy layers, closed Context dispatch,
  response bindings, natural-exit report checks and preservation of genuine
  failed benchmark, recovery and policy evidence in red rollups, including three
  bound failed records even when all Actions jobs report success.
- Workspace `cargo fmt --all -- --check`: passed. Six incidental formatting-only
  changes were independently proven byte-for-byte equal to `rustfmt(HEAD)` in
  [the scoped review](../../reviews/v0.2-native-format-code.md).
- Actionlint 1.7.12 passed for the installed workflow.
- Strict `cargo clippy -p marmish-native --features installed-checks --all-targets
  -- -D warnings`: passed in 4.46 seconds on the integrated six-cohort source.
- Full product/engine/vault Rust suite passed, including 373 native unit tests,
  100 engine unit tests and the engine/vault integration suites. Product-only
  strict all-target Clippy also passed.

These checks are not actual natural daemon exits or benchmark measurements.
No failed observation is discarded to improve timing statistics. The hosted
matrix and each local wave must use one immutable final candidate, source and
artifact set; a development candidate cannot qualify later source changes.

Ripwire's native quality delta was not clean. Its report includes name collisions
that classify unchanged `Request`/`run` symbols as regressions, duplicated small
bounded fixture helpers, larger closed record validators and explicit lifecycle
controllers. These were inspected rather than presented as a passing gate.
The independent reviews retain explicit process ownership and sealed cleanup
boundaries; introducing a general orchestration abstraction during this release
would enlarge the unqualified surface. New cohort internals remain under their
separate component reviews and affected checks.
