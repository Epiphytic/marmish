# Borrowed Pending owner review

The [Manual actor mount design](../phases/peer-candidate-manual-actor-mount.md)
specifies the single-cursor Pending handoff. Claude Code Sonnet 5.5
in `--permission-mode auto` approved exact candidate source
`07fbf22b7f885ebdf5766f949ed099510855535a` in session
`d7133123-2efd-4c27-ac7d-021fde3b41ea`. The reviewed owner commits are
`3339b5a`, `2eb3312`, `455d3b0`, `7a2d5fe`, `387cfc4`,
`28e78a2` and `07fbf22`.

The reviewer checked the actual Linux AMD64 candidate compilation, one
cursor and quota from the same profile lease, checked port scope and logical
wall floor, and the before-update and before-commit native fence in the
durable state change. The review confirmed that cold JoinStarted can only
read back the native outcome, that the original Welcome has one poll, and
that each SQL pass for an already Pending qualification is read-only.
The reviewer independently ran the candidate library and test-target
compile checks. The test-target check reported nine interface
warnings in the sibling package reader snapshot. The author used that
reader's follow-up to check the composed candidate strictly.

Review follow-up `4cf701565fbe4e9cc52f152f8d7fe725b37a80d5` marks
`ClosedJoin` as must-use and records why the candidate schema has no Active
publication inventory. The result contains both the closed native turn
and its outcome. The actor must inspect that outcome and use exact native
readback to resolve ambiguity. The review also called for including
near-cap images in the assembled actor's timing cohort. Slow storage can
exhaust a five-second native window during copy and verification, so the
cohort must count any resulting refusals.

See [validation](../validation/peer-candidate-borrowed-pending.md) for the
author's measured private WSS results and compile scope.
