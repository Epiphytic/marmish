# Legacy compiler admission diagnostics

Beads `marmish-d93.5`; implementation base `fd30018`.
[Detailed design](../phases/legacy-offline-fixture-cache.md#compiler-admission-diagnostics-follow-up)
was approved independently before implementation.

Claude Opus 5.5, Claude Code auto mode, session
`4af03911-fa76-4970-ab88-25057c2a5135`, approved the integrated two-file
implementation and the final test-only refinements. Final record SHA256:
`888195b5135a38d6916d85168c5706701c36dba943eff11e1a9d0a847702bd7e`.
The reviewer independently repeated all34 focused mocked cache/compiler cases
with zero failures. The full mocked spike module passed116 tests before the
final test-only refinements; no later full-module pass is claimed. Diff checks
pass. Quality-delta exits2 with eight major findings, including added checker
complexity, test verbosity, fixture duplication and churn. These were reviewed
and accepted explicitly; no clean quality-gate claim is made.

The exact final source SHA256 values are:

- `tools/marmish_managed_runtime_spike.py`:
  `53bc349abf473f34d1dc4bc2a2c455a8ceac0c2533eadddb8a29442f518080a5`.
- `tools/test_marmish_managed_runtime_spike.py`:
  `388a85a3b74e412b110d00bb883a55842787c86a3e9d38d62f162f4ecb275ffa`.

Tests cover all three fixed tools/hosts, each admission predicate, output bounds,
executor failures and partial capture, private-log refusal, adversarial output,
and clearing between tools and before later provenance/fetch failures. No real
compiler or cache operations were needed for these focused tests.

The new report metadata identifies the rejecting tool, execution/validation
stage, exit code, output length and version/host comparison booleans. It emits
no raw compiler output or paths. Admission, commands, ordering and deadlines
remain unchanged. Historical macOS `fixture_compiler_invalid` remains unexplained
until a fresh hosted report supplies the actual predicate. No loader correction,
release qualification or new public version is claimed by this increment.
