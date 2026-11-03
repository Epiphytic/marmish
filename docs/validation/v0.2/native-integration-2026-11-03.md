# Native integration checks, 2026-11-03

This is local Linux AMD64 evidence for the uncommitted increment based on
`36b09b78524aec60f54fd2c288557e67776dd846`. It is not a release qualification,
an installed test, or evidence for other platforms.

The command below completed successfully, with no failed tests:

```sh
cargo test --locked -p marmish-native -p marmish-engine -p marmish-vault -- --test-threads=1
```

The engine ran 100 unit tests and integration groups of 2, 14 and 5 tests.
The native library ran 299 tests; the vault ran 8 unit tests and 3 helper tests.
Other binary and documentation targets passed. The Linux isolated native
keychain test was explicitly ignored by this ordinary invocation; this result
does not establish default-keychain behavior. The engine unit suite took
53.56 seconds and the native library suite took 307.24 seconds. Child-process
tests print additional filtered summaries; those are not additional unit tests.

`cargo build --locked -p marmish-native --bins` subsequently passed. Later
installer fault tests, review fixes and recovery work require their own affected
checks; these results do not implicitly cover edits made after this invocation.

## Structural checks

`git diff --check` passed. Ripwire quality and test-map checks did **not** produce
a green result. The quality report flags substantial new state-machine and
transaction complexity, including CLI dispatch, command admission, installation
and manifest validation. Some symbol matches also collide across files (for
example `run`, `resolve` and Cargo `package`); those counts cannot substitute for
a source review. The independently recorded component reviews remain necessary.
No thresholds were suppressed or baseline rewritten to manufacture a pass.

The test-map gate reported 251 impacted symbols with zero mapped tests even
though the Rust suites above ran. It therefore does not establish that those
symbols lack tests, nor does it prove coverage. Actual Rust regressions and
installed acceptance remain the behavioral evidence. Both structural reports
were measured at `36b09b785+dirty`; they are not reproducible from the base commit
alone and must be refreshed for the published source.
