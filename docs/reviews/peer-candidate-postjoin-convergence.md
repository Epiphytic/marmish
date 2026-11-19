# Private installed Peer PostJoin convergence design review

On 2026-11-19, independent Claude Code in `--permission-mode auto` reviewed
the exact two-document design atop `7f7d7bc1d71523f0950d7f7ed3647738616b3422`
with locked upstream MDK `e4421e86a6c73f336b0154ee09c13826d8fafff5`.
Reviewer model `claude-opus-5-5`.

| Round | Exact commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `f5bfdd170a8fd11e2e1004d5af3d388d52ffd412` | `431e2224-fe3d-4a7a-8274-a895fe8c70cb` | Request changes |
| R2 | `5ef6932faf582bd8f30cc58f0698661f3581b627` | `6369237a-f126-4477-a335-dfcbef391aa3` | Request changes |
| R3 | `d004f5b50a6ddf906ab72857a5ee127cd358723e` | `c20c4af0-b137-4d9d-94fd-ecda5e5e4db6` | **Approved**, no mandatory findings |

The approved [installed design](../phases/peer-candidate-postjoin-convergence.md)
has Git blob `31c0f515831d792feb3ad46f3f9ab39551dbf654`.
The approved [first proof](../phases/peer-postjoin-closed-native-split.md)
has Git blob `2ff7b79bcb26ca0c29202657800d1a8eac59d455`.

R1 required the exact native ambiguity classification, native and physical
attempt counters, a real-time budget, all six foreign-layout checks, and
separate local and founder native proof authority. R2 confirmed those changes
and found that MDK saves `Attempting` before calling its adapter, then treats
that state as immediately due after reopen. R3 fences an interrupted capture
or settlement before any resumed native effect. Fully persisted first
`PossiblyExposed` and exact stable confirmation retain their cold readback
paths. The reviewer verified that the locked public `outbound_fanouts_for_group`
and its target status, count and failure accessors distinguish the cases.

Carry the following nonblocking findings into implementation and code review.

- Add a narrow test-only verifier seam in `peer_postjoin_verify.rs` for the
  first no-network capture if needed. Reuse exact fanout and native evidence
  checks and require the sole target's persisted `PossiblyExposed` failure,
  `pre_send: false`, and one attempt. The existing `post_capture` expects
  `RetryableUnavailable`. The existing broader recovery helper admits
  `Attempting` and two attempts. Preserve both older entry points.
- Refuse a cold `Accepted` target before complete native resolution and stable
  obligation readback. Keep the exact existing fresh bounded cleanup sweep
  for the uninterrupted path. A cold terminal target supplies no further
  publish or settlement authority.
- Parameterize the existing staged `reach_activating` helper or use a wrapper
  so the new proof takes the actual same-UID Manual decision. Preserve its
  original Automatic caller. Pass the elapsed-time-derived wall through the
  native verifier calls.
- On the before-due probe, open the measured store, verify the persisted
  fence, perform one bounded zero-settlement sweep, then close before
  waiting. Reread the exact fence after reopening at the due time.

Design-only static inspection and `git diff --check` passed. No builds,
native runtime tests, relay activity, implementation or publication were
performed. Runtime estimates remain unmeasured. The phase stops at durable
PostJoin confirmation and charged Manual Activating custody.
