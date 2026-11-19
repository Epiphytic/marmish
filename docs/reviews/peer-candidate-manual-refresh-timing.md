# Candidate Manual refresh timing review

Claude Code Auto, Claude Sonnet 5.5, session
`3e8217d1-f08d-45f2-a41a-e04f5ccec994`, approved the exact timing source
`3f6e2caead4ecc431880546e9e15d0a244c132a6` against `1be3388`.
The review covered the approved
[actor-mount timing contract](../phases/peer-candidate-manual-actor-mount.md)
and [scheduler](../phases/peer-candidate-manual-scheduler.md). The reviewer
inspected all seven changed files and the immediate call paths and ran scoped
formatting and diff checks. Cargo and live tests were excluded from the
reviewer's commands while shared build caches were occupied.

The first linked candidate build then exposed a test-only integer inference
error. Claude Code Auto session `c77e5105-a345-4a5c-8356-31b1d6c061ef`
approved exact source `66fa3c0f29ed8068a966c63e9b744723c2953c8e` after
the one-line fixture correction. The reviewed runtime source is identical.

The reviewer verified that a timing sample follows exact ticket acceptance,
that the ticket still precedes the first SQL read, and that refusals leave
the preceding success intact for the next pair. They verified fixed typed
relative fields, candidate-only opt-in, safe duration arithmetic, and
one event per scheduled slot. No blocking findings remained.

The fixed `proof_refused` outcome covers both readback and admission refusal.
The validation records the reason scope, null-pair semantics, preeligibility
schedule behavior and synchronous stderr overhead. Installed path and
near-cap measurements belong to the qualification controller.
