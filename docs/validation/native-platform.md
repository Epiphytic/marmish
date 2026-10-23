# Native platform increment validation

The native runtime gate is pending. This document records local evidence without
claiming macOS runtime or Linux/macOS interoperability success.

The GitHub API independently resolved all four workflow action pins in their
official `actions/*` repositories: checkout `11d5960a326750d5838078e36cf38b85af677262`,
cache `0057852bfaa89a56745cba8c7296529d2fc39830`, upload-artifact
`ea165f8d65b6e75b540449e92b4886f43607fa02`, and download-artifact
`d3f86a106a0bac45b974a628896c90dbdf5c8093`. This verifies the references,
not runner compatibility or workflow success.

## Local checks

- Linux workspace: 129 Rust tests passed, including 91 engine unit tests,
  13 engine integration tests, two SQLCipher tests and seven bridge tests.
- Python discovery: 144 tests ran successfully, with four Darwin-only skips on Linux.
- Workspace warnings-denied clippy, formatting and diff whitespace checks passed.
- Actual Darwin platform module and its test bindings type-check for ARM64 and
  AMD64. A full cross-build is blocked by the absent Apple SDK; this establishes
  neither native execution nor runtime API correctness.
- Clean pinned, patched MDK source passed a Linux release build and the restricted
  relay transport test plus the restricted initialization/acknowledgement tests
  (three Rust tests, including eight subprocess configuration cases). Clean patch
  application, the full-source builder guard and portable C probe compilation
  also passed. The native daemon still requires a native build.
- The updated Linux engine passed all 14 real-relay assertions in
  [the local E2E report](native-increment-linux-e2e.json), including byte-exact
  special-character arguments/streams, additive command denials, disconnect,
  restart, retry/replay, cancellation, fresh groups and actual CLI processes.
  Duration was 198.827 seconds; owned local cleanup completed. This local run
  used the existing SHA-pinned Linux daemon, whose source provenance is unknown;
  it validates the incremental engine/protocol behavior, not the clean native
  build or cross-platform gate below. The native CI must build its own daemon.

## Structural quality check

`ripwire . --quality-delta` returned exit 2 against `fb8cbe6`: 79 gating findings,
including 56 short-horizon churn findings, five conditional-platform/delegation
reachability findings, six duplication findings, five verbosity findings, three
complexity findings, three new-clone findings and one parameter finding.
This check is not green. Some unqualified
symbol matches conflate new helpers with existing functions (for example `_pump`
and `main`); compiler/clippy checks provide separate evidence for Rust reachability.
The additional RPC parameters supply explicit socket/home and verified peer
identity. Small syscall wrappers and broker resolver/connect/cleanup duplication
remain deliberate platform separation during native validation. The larger native
recovery and stream pump functions warrant attention in code review; numerical
score reductions alone are not a reason to rewrite a security boundary.

## Descriptor audit availability

Darwin descriptor inspection fails closed if an FD changes during enumeration,
its vnode path cannot be resolved, or ancestry cannot be inspected. A transient
concurrent close/unlink may therefore refuse an operation. This increment does
not blindly retry `UnsafePath`, which also represents stable security failures;
a future retry would require distinct transient error classification and review.

## Required native results

The native workflow must establish confinement prerequisites, offline checks and
clean native build provenance on Linux AMD64, Linux ARM64 and macOS ARM64; then
native real-relay commands and both Linux/macOS command directions must pass at
one SHA in a complete run attempt. Only the explicit EU/US White Noise relay URLs
are used for these disposable-key tests. Runtime failures remain failures until
fixed and rerun. macOS AMD64 remains unvalidated without a native runner.
