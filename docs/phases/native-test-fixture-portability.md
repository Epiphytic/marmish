# Native test-fixture portability

This bounded test-only increment addresses Beads `marmish-vy6`. Native run
[41399059734](https://github.com/Epiphytic/marmish/actions/runs/41399059734),
Linux AMD64 job 125127202519, ran 733 tests using Python 3.14.4: 43 setup errors,
two assertion failures and eight skips. The sanitized
[inventory](../validation/native-tests-41399059734/failure-inventory.json)
records public test identifiers only; raw runner paths and environment values
are excluded.

## Scope and contracts

Only three test modules change. No production path validation, installer,
runtime, compiler environment contract or workflow changes are authorized here.

* `test_marmish_installed_state.InstalledStateTests` and
  `test_marmish_installed_runtime.DriverBoundary` currently use the default
  repository installer lookup. A group-writable checkout ancestor correctly
  triggers `unsafe_state_path`, before those unit tests reach their target.
  Supply installer bytes and their SHA-256 through the existing verified-byte
  handoff. InstalledStateTests reads the actual test installer bytes using
  `test_marmish_install.INSTALLER.read_bytes()` so its real archive uninstall
  test retains the actual installer. DriverBoundary may use fixed synthetic
  bytes, as it never executes the installer. Repeated state creation tests must
  use the same handoff
  so they still exercise pointer immutability, and assert the exact
  `state_exists` category. Preserve the separate
  `InstallerHandoff` tests proving default lookup rejects unsafe checkout paths,
  invalid or mismatched handoffs refuse before allocation, and private-copy
  integrity is checked. Preserve all compiler-scope tests: five of the 29
  state-module errors are
  CompileScopes cases that share InstalledStateTests.setUp (the other 24 are
  InstalledStateTests proper).
* The managed Rust compiler lookup intentionally forwards optional `RUSTUP_HOME`
  and `CARGO_HOME` into its restricted host lookup environment. Replace its
  ambient-environment-dependent assertion with controlled environments covering
  four cases: neither optional setting, RUSTUP_HOME only, CARGO_HOME only, and
  both optional settings. Assert exact environment keys,
  exact forwarded values, and omission of an unrelated sentinel variable. Use
  `patch.dict(spike.os.environ, {...}, clear=True)`, a fixed HOME, and assert
  the complete lookup environment dictionary. Preserve the exact command
  assertion that the lookup invokes the selected rustup shim.
* Deep provenance JSON must refuse with `bwrap_provenance_invalid`, whether the
  interpreter decoder raises `RecursionError` or successfully decodes a nested
  list. Keep the actual bounded deep JSON fixture and assert the application
  refusal. Independently inject a decoder `RecursionError` and assert the same
  finite refusal, without requiring a particular Python decoder implementation.

## Validation and acceptance

Before edits, reproduce the unsafe-checkout setup failures locally using an
uncommitted external runner script: copy the current source files (tracked plus
non-ignored new files, without Git metadata or ignored private/build files) into
a disposable checkout whose directory is 0775 under a private temporary parent.
Run the three modules from that copy without patching module locations. After
edits, refresh the copied sources and run the same modules there, including
CompileScopes, bound-transport fixtures and actual uninstall coverage. This
mirrors the unsafe-ancestor condition while preserving relative source paths and
leaves real worktree permissions unchanged. Record before/after outcomes. Run
the Rust environment test both with optional Rust configuration exported and
with it absent.

Run the three complete affected unittest modules, coordinating an exclusive
local subprocess-test window with the compiler agent. The existing unsafe-path
and integrity negative cases remain mandatory. Actual Claude reviews this design
before edits and the completed test diff in auto mode afterward. No live
transport or hosted job is launched by this component. Local passes establish
test portability, not a successful hosted rerun or nine-pair command acceptance;
the parent coordinates native validation and publication.

The [final design review](../reviews/native-test-fixture-portability-design-final.json)
and [completed-code review](../reviews/native-test-fixture-portability-code.json)
approved this increment. The [local evidence](../validation/native-tests-41399059734/README.md)
retains before/after results and an intermittent cleanup observation separately
from the failed hosted baseline.
