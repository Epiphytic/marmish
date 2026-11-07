# macOS native startup compatibility reviews

Scope: [bounded A+B design](../phases/macos-native-startup-limits.md).
No implementation or approval is inferred from the diagnostic observation.

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`, session
`bb10bf2b-651a-4e78-bff1-16f3d974d334`, first review returned
**REQUEST_CHANGES**, 171.290 seconds / 19 turns. Private result SHA256
`a61a6066180dd6002975e500aeaa7ac06a8090ae157e0186dc9e9151dc46ea4c`.
It inspected the platform guard, launcher, engine adapter, stream runtime/store,
CLI and executable wrappers. No files changed or tests ran during that review.

Blocking findings required the bootstrap failure check before request construction
and the existing preparing guard; journal revalidation and commit before local
reclamation; a separate failure-proven retirement sweep for already inactive or
absent entries, including cancellation and stopping; reuse of existing readiness
instead of dead retained-error data and an impossible path-bearing error fixture;
and normalization without descriptor enumeration at every CLI invocation. The
engine's existing high-descriptor refusal remains at bootstrap and spawn.
Additional corrections cover three shipped binaries versus the engine crate's
standalone worker, panic-hook ordering, explicit pre-argument exit status, a
deterministic queued-opening fixture and next-tick retry after a failed commit.
An open-thread panic remains outside the definitive failure proof.

The same actual Claude AUTO session approved the corrected design with no
remaining blockers: **APPROVE**, 35.762 seconds / 4 turns. Private result SHA256
`ca43f0e42d999b8707dc404911ea6504f7338c3a0ef37c4c22567acefbfac16e`.
It verified B1–B4 against source, including the stop-path sweep reachability,
existing sticky readiness and blind journal update contract. No builds or tests
ran. It did not audit the separate diagnostic evidence; those approvals are
recorded in the diagnostic review. Darwin behavior and built binary startup
remain validation gates. An open-thread panic remains outside this fix.

The coordinator recorded approval before authorizing bounded implementation
with the phase's exact disjoint A/B file ownership. The gated-open hook must
exist only in test builds; the local sweep excludes owned runners and entries
without pending preparation. Integrated code approval remains required.

## Integrated code review

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`, new independent
session `69e6d8ba-23c3-437d-83aa-c01a85cd500d`, returned **APPROVE**, no blocking
findings, 216.809 seconds / 20 turns. Private result SHA256
`c5e5c35535f5e9b31863e39b4f94a19689bd31d4aa326e87c5e6ddaeeeae2654`.
It inspected the entire staged/unstaged increment, including new files/tests
and docs, against the approved design. It made no edits or live dispatch.

Independently passed the three Linux process-limit tests with actual isolated
children and binary byte/status assertions, all eight bootstrap regressions,
19 adjacent engine tests and the actual marmish/marmishd binary smoke test.
Strict all-target Clippy passed for engine/native default and installed-checks;
workspace formatting and diff checks passed. It verified startup ordering,
unchanged descriptor guards, definitive bootstrap proof, refreshed authority and
record checks, commit-before-retirement, retry and cancellation/stop sweeps.

It did not compile or execute Darwin: its attempted cross-check failed in the
vendored OpenSSL build. Actual hosted macOS high-soft/infinite-hard tests and
fresh installed qualification remain gates. It did not re-audit the separately
reviewed diagnostic/historical artifacts. It did not mutation-check the NativeCopy
placement test, and noted the existing stream-ID-keyed pending set's uniqueness
assumption across groups. This correction does not redesign that set.

Useful nonblocking refinements were incorporated: rename the Linux no-op test
so it does not claim to establish a high-limit precondition; use raw errno errors
on the Darwin test's post-fork failure paths; clarify that the measured Node
observation is reported by its child shell. The coordinator reran the three
engine checks and actual binary smoke and workspace formatting successfully.
Final corrective review is recorded below when completed.

### Final corrective review

The same actual Claude AUTO session returned **APPROVE**, no blockers,
16.625 seconds / 3 turns. Private result SHA256
`fc86be76bed9daf03ed0c002ff52d4399bce1b0f9c636d1c01bc76b527ed4da1`.
It verified all three refinements and final source/docs consistency, independently
reran three engine tests, actual binary smoke, strict all-target installed-feature
Clippy, workspace formatting and diff checks. It inspected the coordinator's
three/one passing logs without counting those as independent runs. Unchanged
bootstrap tests were not redundantly repeated. All Darwin, historical-artifact
and installed-qualification limits above remain.
