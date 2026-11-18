# Native MDK master refresh validation

The native Rust build follows the upstream MDK `master` branch. The checked-in
Cargo.lock resolves it to `0ad3c61088e1acac58e0fcf1a9f1142bae5d997a` and aligned
rust-nostr to `e93cc06b5624f06c21ecd8111a124176eb0c7589`. The upstream head
matched at the time of this validation.

Linux x86_64, debug, offline against the locked dependencies. All-target
`cargo check --locked -p marmish-native --all-targets` passed. Two installed
native Copy tests passed individually in 102.69 and 87.60 seconds. They covered
raw and compressed binary transfer, special paths, metadata, cancellation with
a full window, and checkpoint-based resume.

The first full run had an invalid test setup. A second Cargo build replaced
the running test executable and caused self-executable startup failures.
The archive-only source copy also omitted Git metadata required by native
release fixture tests. The next run used a private copy of the compiled
test executable and completed with 789 passing, six failing and 61 ignored.
Five packaging tests inherited all-zero build metadata from the earlier
build, and the encrypted WAL restart test exposed the stale MDK migration
ceiling of 166.

The registry in the locked MDK ends at migration 212. The read-only
preflight now accepts that schema and continues to reject future formats.
An immutable rebuild with an explicit Git source commit passed all five
preflight tests in 8.75 seconds and all five packaging tests in 0.75
seconds. The preflight tests verified a closed owner can reopen and that
the main databases and existing WAL sidecars are preserved.

The final consolidated run used the corrected immutable library executable and
four test threads. It passed 795 tests with zero failures and 61 explicitly
ignored opt-in cases in 1,141.62 seconds.
