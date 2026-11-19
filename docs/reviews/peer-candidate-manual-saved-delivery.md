# Manual receiver saved Delivery review

The reviewed design is [the Manual scheduler plan](../phases/peer-candidate-manual-scheduler.md),
R4 `8a562799d1029ad215dd5c24ecc89748b840944f`, independently approved in
Claude Code Auto session `e3d81fc1-1601-4ea4-aa1f-7d77c897fd0f`.

Claude Code with Sonnet 5.5 in Auto mode reviewed the initial exact source
`bd78d7f6cc94cc2f2cfa3e27fc60535d6b4680af` in session
`b11ed092-e8d1-42ec-9b51-29dccbe851ac` and requested changes. The reviewer
asked for registered port and checkpoint regressions, same-ID revision
replacement, a signed cutoff crossing with valid parent window, and an
explicit permanent forwarding mismatch disposition. The revised checkpoint
also preserves the precise final owner refusal after rollback.

Claude Code with Sonnet 5.5 in Auto mode separately **approved** exact
`7f4e50cea01820ae34f8e1a607c3434dba4f480f` in session
`ecd8e56c-b95f-4375-b7d2-cf7788185fb1` with no blocking findings. It reviewed
the scoped diff from `7d69306`, ran the two deterministic cases from the exact
candidate libtest, and examined the author's two-case WSS run and stock
Clippy results. [Validation details](../validation/peer-candidate-manual-saved-delivery.md)

The reviewer identified opportunities for broader mount coverage. The Waiting
refusal is injected at the actual guarded precommit boundary. The Live cutoff
crosses before native measurement. A successful guarded Live Applied and an
expiry between native measurement and final commit were outside these two
WSS cases. The prior expiry fixtures exercise the shared retirement machinery
independently of the scheduler's three expiry dispositions. The review
covers the candidate component and its fixture seams.
