# Installed default-auto provider qualification tooling

Status: independently reviewed implementation; real hosted provider qualification pending.

The CI-only supervisor installs the exact candidate through its pinned shell
bootstrap, creates owned disposable real providers and runs the installed native
quickstart without selecting file custody. Its closed report covers two named
identities, distinct Nostr/storage references, reload, locked-provider refusal
and Linux absent-bus selection/refusal. macOS uses an owned disposable account
and keychain, preserving the runner's ambient keychain state. These are intended
hosted checks, not completed provider observations.

[Actual final Claude auto review](../../reviews/v0.2-native-keychain-code-r4.md)
approved the integrated supervisor, tests and workflow after four rounds. The
combined supervisor and quickstart offline suites passed 26 tests; the isolated
supervisor suite independently passed 8 tests. Actionlint passed the workflow.
No provider, account or privileged acceptance test was run locally.

A scoped `ripwire tools --quality-delta` also ran. Its one gating heuristic was a
19-token clone between the new test's mocked process constructor and legacy test
constructors. It also reported new-symbol complexity/verbosity and false
unreachable classifications for dynamically invoked CLI and unittest entries.
Those findings were inspected; a shared abstraction across unrelated legacy
mock fixtures would not improve this change. This was not a clean Ripwire gate.
The independent review and focused tests remain the concrete code evidence.

No v0.2 release, default keychain qualification, installed command qualification
or HTTPS bootstrap availability follows from this document. Require successful
source-bound provider reports on Linux AMD64, Linux ARM64 and macOS ARM64.
