# Installed recipient Delivery design reviews

Bead `marmish-dwt.4.4.4`. The approved scope is a private Linux AMD64 library
fixture through durable `Held`. Welcome ingestion and the Pending transition
belong to the separately reviewed capped join phase. The Consent publication
prerequisite has its own code review and tests.

| Round | Reviewed design commit | Claude Code Auto session | Verdict |
| --- | --- | --- | --- |
| R1 | `7172640b4fbce18a8f60f79db815c196203b7172` | `ecb1ddb6-f7aa-4d28-b991-d1f42dd282fc` | Request changes |
| R2 | `9d2b2587ceda247ddd9b9f49d2d3280aeefed9b8` | `b40c05f2-47c3-4cc0-bd5d-cb197387bb11` | Approve |
| R3 | `4d42cdb4d937d2861e687e5e9f27b9c7b87f6793` | `d3e0472c-644e-4762-b232-12ddc17de736` | Approve |
| R4 | `3170e7cbf56b6994d73e2d1be76bcacf04cb9bfd` | `2b608bd6-7fe3-4c69-b6ec-29680c5a394b` | Approve |

Reviewer model `claude-sonnet-5-5`. Final approved
[design](../phases/peer-installed-delivery-ingress.md) has SHA-256
`9a7ddf8722e376f3ce2947d8399f1da3c00a0771090f79c6aea01733da612b3a`.

R2 closed all seven R1 blockers. It pinned stable invalid dispositions and
receipt rules, parser retirement and retry bounds, separate checked saved
snapshots, atomic cursor and custody wall fences, one bounded deferral per
Consent, two-sided checked native inspection, and the exact Consent
settlement and wake prerequisite. R3 verified the added retry ownership and
restart semantics, competing Delivery limit, snapshot provenance,
exclusive owner scheduling, and single-pass classification verification.
R4 verified the wording changes against the exact custody transaction.
`Context::verify` checks durable epoch, identity scope, wall and quarantine.
The separate owner incarnation check binds the in-memory observation and
snapshot to the current owner. An active quarantine refuses the mutation
without persisting a wall advance.

The reviewers inspected exact tracked Markdown and current Consent, cursor,
port and custody source. The author verified `git diff --check` and the
document digest. The design used read-only source inspection. No runtime
code, build, or relay test was part of this review.
