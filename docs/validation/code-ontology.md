# Code ontology validation

- The staged Marmish graph contains 418 tracked inputs, 15,734 declarations,
  and 92,413 syntactic source references. A warm regeneration
  took 2.24 seconds and a freshness check took 2.48 seconds on the Linux
  development host.
- A full SQLite export took approximately 3 seconds. SQLite reported
  `integrity_check = ok` and zero foreign key violations. A query for relay
  symbols returned declarations in `crates/marmish-core/src/config.rs`.
- All 12 ontology tests passed. They include stale staged and committed
  snapshots, parser coverage, secret-like literal exclusion, release-tag
  installer cutover, SQLite materialization, reproducibility across Python
  hash seeds, the pinned Ossie converter, and NVIDIA's pinned native model
  validator.
- All 19 release-workflow policy tests passed, including mutations that
  bypass freshness checks or substitute a different source revision.
  actionlint 1.7.11 passed on the four modified workflows. Ruff passed on
  `tools/code-ontology`.
- The project did not run a separate NVIDIA Auto Ontology service or
  publish a binary release for this tooling change.
- The generated inventory identifies 16 Rust files with parser recovery and
  16 JavaScript or C files with unnamed declarations omitted. Both classes
  retain their file hashes and all declarations the extractor recognizes.
