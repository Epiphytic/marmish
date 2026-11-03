# macOS strict-Clippy candidate failure and mechanical correction

Candidate [41644799701](https://github.com/Epiphytic/marmish/actions/runs/41644799701), attempt 1, source `8c47014094365144c8046d659d57051acdf9bd0a`, failed its macOS native verification after successful tests. The macOS log records engine library 108/108 (35.90 seconds), engine integration 14/14 (7.44 seconds), native library 335/335 (232.25 seconds), and vault library 8/8. The two failures were `clippy::manual_is_multiple_of` in engine `platform/macos.rs`, lines 231 and 300, at 2026-11-03 13:41:52 UTC. This failed candidate is not release acceptance; packaging, installer and provider qualification cannot be inferred from these test successes.

Exactly two predicates now use `!(count as usize).is_multiple_of(size_of::<T>())` instead of `count as usize % size_of::<T>() != 0`. The unchanged earlier `count <= 0` predicates short-circuit negative/zero counts; both concrete structure sizes are nonzero. Buffer limits, truncation retries and finite error categories are unchanged. No lint allowance was added.

Changed source file: `crates/marmish-engine/src/platform/macos.rs`, SHA256 `d9ee38a2ceee3dffe008ba6b36a060b48966c1abf4fd6f85accba20c449bc551`.

Local Linux validation on Rust 1.97.1:

- `cargo test --offline --locked -p marmish-engine --lib`: 100 passed, 0 failed, 22.20 seconds.
- `cargo clippy --offline --locked -p marmish-engine --all-targets -- -D warnings`: passed.
- `cargo fmt --all -- --check`, matching-edition direct rustfmt, and `git diff --check`: passed.
- Exact diff audit: only two replacements; no remaining modulus-by-`size_of` patterns in the macOS file.

[Actual independent Claude auto review](../../reviews/v0.2-macos-is-multiple-of-code-r1.md) approved the mechanical change and independently checked negative, zero and positive boundary equivalence. Linux checks do not compile/run the macOS-only module. A fresh complete candidate must establish actual macOS strict-Clippy and release qualification; no pending gate is claimed passed here.
