# Native copy frontend review

Round 1: **REQUEST_CHANGES**. Actual Claude Code, Sonnet 5.5,
`--permission-mode auto`, session `779fccf6-97cd-4622-8c9f-a33f47a54a5a`,
397.475 seconds, 23 turns. Read-only review; no tests run by the reviewer.

Scope: the integrated copy CLI, bounded transport session, frame driver, client,
worker, hidden helper and CLI wiring, against the approved copy design. This
preparatory review excluded server admission, filesystem internals and pending
local receiver garbage collection; it cannot approve the complete phase.

Blocking findings:

- Closed stdin obscured helper Error frames and typed preparation failures.
- Copy failures all exited 255 and lost their finite categories.
- Idle polling lacked the required backoff and minimum turn interval.
- Definite local file/protocol failures left the peer helper waiting forever.
- Capability discovery used the eleven-minute idle timeout rather than a
  separate, short configurable probe deadline.

Useful additional findings: validate retention before Open; retain consistent
worker ownership across dropped work futures; emit progress heartbeats; reject
malformed hidden-helper invocation with exit 1. Half-sent frames are not resumable
through a dropped send future: current callers abandon the attachment instead of
attempting a different frame. Transport retry retains exact request bytes and
original deadlines; it never implicitly cancels remote work.

Fixes and focused regression tests are in progress. Another actual review and
integrated phase review remain required. No approval is implied by this record.

Round 2: **REQUEST_CHANGES**. Actual Claude Code Sonnet 5.5 auto,
session `dfbb9576-232d-4514-a693-e9be154edd96`, 173.847 seconds, 22 turns.
Read-only source review; no reviewer tests. Remaining blockers were delayed
helper Error after stdin closure, incomplete output ACK accounting before
returning a peer error and failure-drain behavior, and admission refusal
categories. The prior polling, probe and independent cancellation fixes were
accepted within this limited scope.

Current fixes remove the early stdin-closure rejection, consume/account the
whole output block before returning a finite peer error, discard ordinary
frames during failure drain and avoid sending Error to an already failed or
closed peer. Denied admission now reports `authorization_denied`; recovery
refusal reports `recovery_required`, both exit 1. Commit uncertainty remains
255. Probe deadlines clamp to a shorter idle deadline. Six CopyIo regressions
and six copy CLI/client validator tests passed in local focused runs. Round 3
is reviewing the four affected frontend/session/error modules; the complete
phase remains unapproved and unqualified.

Round 3: **REQUEST_CHANGES**, same scoped read-only review. Sonnet 5.5 auto,
session `9f113385-4536-4de2-829f-54ff189d4431`, 67.654 seconds, 11 turns.
A malformed frame or invalid credit could poison/partially consume the decoder
before an output block was ACKed, preventing failure drain. Abort now bypasses
application decoding entirely and ACKs transport bytes; dedicated regressions
cover malformed and bad-checkpoint replay. Terminal EOF ahead of backpressured
output now waits for the consumed byte count before classifying missing Error.

Round 4: **APPROVE**, limited to `copy_io.rs`, `copy_cli.rs` and session/error
dependencies. Actual Sonnet 5.5 auto, same session, 28.809 seconds, 4 turns.
No reviewer tests. Worker/lease lifecycle and the complete phase are excluded.
The suggested additional safety check was incorporated afterward: Failed or
Uncertain terminal states cannot count as confirmed helper cleanup. Tests for
these latest changes are pending the shared build slot, and integrated review
must inspect this followup and the worker/lease interaction.
