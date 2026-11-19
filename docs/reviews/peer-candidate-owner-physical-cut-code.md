# Ordinary owner physical cut code review

Claude Code Sonnet in Auto mode reviewed the implementation against the
[approved design](../phases/peer-candidate-owner-physical-cut.md). Session
`ddd6f19b-34d7-4820-81c3-6602c5db79fe` requested changes on `16ea370`.
The stock archive exposed a missing pair of required Command config fields
and a relay setup that closed its slots too early for the owner to open.
Commit `0c134ed` fixed both. Commit `a95f4e4` added the MDK database and
sidecar byte oracle following the review.

Claude Code Sonnet in Auto mode approved exact source `a95f4e4` in session
`901d35ba-5a05-4d8f-a405-25cf5f72bee8`. The reviewer checked the source,
independent barrier ownership and the locked SDK behavior, verified the
executable hash and embedded source stamp, and confirmed the focused
runtime and candidate Clippy outputs. Review R1 ran the failing archive.
Review R2 used the final archive and recorded outputs for verification.

The passing fixture gives the Journal byte oracle a deliberate graceful
rewrite as a positive control. The MDK image comparison is a direct
before-and-after observation, with no corresponding deliberate native
mutation. The SDK shutdown path can't expose internal nested task panic
results. Those limits were accepted.

The structural quality scan of the new helper and fixtures also identified
common Rust patterns for field snapshots, fixture setup and bounded
polling. These matches did not justify coupling shutdown proof to other
test lifecycles or unrelated runtime changes.

See [validation](../validation/peer-candidate-owner-physical-cut.md) for
the stock runtime results and candidate compile checks.

## Runner uncertainty amendment

Claude Code Sonnet in Auto mode approved exact source
`b071e45fec9efbe57721afdf385f677efd202f7d` in session
`daa45305-8e96-46fa-9396-ea55f521aadd`. The review rechecked the runner marker
and uncertainty ordering, physical engine join evidence, nested task and raw
ownership, lease retention and cancellation safety. It also verified stock
and candidate strict checks and reran five focused physical tests without the
WSS fixture. The uncertain runner regression passed five additional runs.

The reviewer suggested exercising a mixed runner set and a live OS child with
unproved cleanup. The direct cut includes a real caught panic before spawn and
a separate real reaped-child control. The full runner suite also exercises
stale cleanup without an exit observation. This increment leaves the
controlled live-child/process-hold proof to the reviewed daemon composition.
