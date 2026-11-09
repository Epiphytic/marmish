# Legacy managed environment review

Scope: Bead `marmish-d93.6`, isolated base
`fa119c6e876f3bed779945d0a3daee4275437058`; four legacy tooling/test files.
The [phase](../phases/legacy-managed-environment.md) was recorded before coding.
The [hosted diagnostic](../validation/legacy-confidential-trace.md) records
Linux missing-PyYAML imports and macOS isolated compiler selection failure.
No native code, release gates or workflow files change.

Claude Opus 5.5, session `34a9953e-e4cb-4ad1-aa90-01e7f0915da9`, approved
the amended design (evidence SHA-256
`6205162b50be327f5e0341e6dfbb800857c6722bf7f1e55874171761c56b2f02`).
The initial completed-code review requested stronger scalar-continuation and
quoted/nested token negative controls (evidence SHA-256
`f466fe8414af9d63e9577510980bc801c9de0d7c65987c7e6ff8a203e9b71762`).
The final implementation refuses unsupported scalar forms instead of parsing
YAML; whole-step token denial preserves the original trust assertion.
The second review found a literal-body indentation gap (evidence SHA-256
`5fc7f326f7c4a083b7c3af3a72a0d7fcea634a3474d311ed36fdceea57f9f303`).
A fixed-margin check now refuses any nonblank body line lacking the complete
10-space prefix before slicing it; extra spaces remain opaque content.
The final completed-code verdict was **APPROVE**, with zero blocking findings
(evidence SHA-256
`23d849a777c07d7e0aef789d8d28757d9cab704a125b1ad59f8d5405e297fe54`).
The reviewer independently reran all four configurations below.

The pinned managed Python 3.13.13 first reproduced exactly two missing-yaml
import errors. A sibling-selection regression failed against the unchanged
resolver for both mocked Linux and Darwin, then passed after correction.
The review regressions produced eight expected mutation failures across two
tests before correction; the same tests then passed. Two additional 9-space
literal-body mutations failed before the margin check and passed afterward.
The curl mutation targets the verification job; the shared refusal covers
all release steps, including publication. Final explicit mock
inventory: 102 tests, zero failures/errors/skips in each of repository/tools
cwd crossed with namespace flag absent/literal `1`. These tests execute no
compiler, Cargo, namespaces or hosted workload. In-memory workflow mutations
restore their source overrides; compiler selection preserves direct standalone
toolchains and refuses malformed/proxy results.

`git diff --check` passed. Ripwire quality-delta exits 2 for five churn/test-class
verbosity findings; dynamic unittest discovery appears as dead code and the
two small test-owned mutation runners share structure. No findings were
suppressed. The earlier resolver-complexity increase and duplicate step parser
were removed through a focused extraction and reuse of the existing helper.

Actual Linux full-suite and macOS compiler/cache operation remain pending a
fresh hosted run. The failed macOS run did not retain the selected wrapper
identity, so sibling mismatch remains conditional. No platform or native
qualification PASS is inferred from mock results.
