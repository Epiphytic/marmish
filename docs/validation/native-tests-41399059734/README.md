# Native offline suite failure inventory

[Run 41399059734](https://github.com/Epiphytic/marmish/actions/runs/41399059734),
Linux AMD64 job
[125127202519](https://github.com/Epiphytic/marmish/actions/runs/41399059734/job/125127202519),
tested source `6f6d42c60c73bc5ef2f8e2f6e61b17216c14b0a5` using Python 3.14.4.
The suite ran 733 tests in 25.756 seconds and failed with 43 errors, two assertion
failures and eight skips. This is failed native suite acceptance.

The [sanitized inventory](failure-inventory.json) contains all 45 failing public
test identifiers. Twenty-four `InstalledStateTests`, five `CompileScopes` cases
sharing that setup, and fourteen `DriverBoundary` cases failed during fixture
setup: default installer loading correctly refused
an unsafe checkout ancestor with `unsafe_state_path`. Their target behaviors
were not reached. The two managed-runtime assertions assumed absent optional
Cargo configuration and a particular JSON decoder recursion limit.

The [test-only design](../../phases/native-test-fixture-portability.md) preserves
production validation and confinement boundaries. The inventory omits raw
tracebacks, runner paths and environment values. A local repaired-test result
does not replace the failed hosted result; a subsequent native run is required.

## Local portability result

The [local report](local-portability.json) records a faithful disposable copy of
the source under an explicitly group-writable 0775 checkout. Before the test
changes, 141 tests produced the same 43 unsafe-path setup errors and the Cargo
environment assertion failure. After the changes, all 141 passed with no skips.
Both copies used identical dependency bytes, recorded by the same manifest
digest. The copied source came from the shared isolated initiative worktree;
sibling dependencies were frozen during the comparison.

This local run used Python 3.12.3. It does not reproduce Python 3.14.4's decoder
implementation; instead the provenance test covers actual deep JSON, an
explicit decoder recursion failure, and a successfully decoded non-object,
requiring the same finite application refusal in every case. Rust lookup covers
all four combinations of optional configuration and rejects unrelated inherited
settings. Actual uninstall, compiler-scope tests and strict unsafe-default-path
negative tests remain in the passing modules.

The [independent code review](../../reviews/native-test-fixture-portability-code.json)
approved the increment and reproduced both sides of the comparison. It also
observed one intermittent `test_engine_cleanup_callback_requires_both_positive_proofs`
failure with `cleanup_incomplete` and null checks. Five isolated reruns and two
full-module reruns passed. Its cause remains a separate investigation; it is
not omitted from the acceptance record.
