# Installed approval reply-loss fixture review

Claude Code CLI in Auto mode, Claude Opus 5.5, approved source
`dd198e2e4c954614f44d2b6231795ff8a76509c9` against `979ab3f` in
session `ea805388-5064-4b32-b47c-82b66b844b6b`. The review covered the
two changed test files and the
[approved local approval design](../phases/peer-candidate-local-approval-mount.md).
No blocking findings.

The reviewer checked the exact request and reply framing, same-UID checks on
both sockets, the private one-connection listener and its 14-second bound,
and the installed client's EOF-to-uncertainty mapping. The original positive
case keeps its immediate retry. The new case waits until after the daemon
restarts, which lets the first cold SQLCipher read attribute the retained
decision to the request whose reply was withheld. The second read compares
the decision time and keeper identity and requires both wall floors to stay
at least as high. The read uses the schema's primary-keyed Unknown custody and
stage rows and has no independent duplicate-row counter. The private relay
publication counts remain part of the fixture assertions.

Targeted rustfmt and `git diff --check 979ab3f..dd198e2` passed. The review
was static. Run the selector `--ignored installed_registered_manual_approval_lost_reply --nocapture`
with the same source-matched candidate and stock installations as the existing
Manual approval fixture to qualify actual timing, reply loss, relay counts
and the cold retry. Cargo and private WSS were reserved for integration.
