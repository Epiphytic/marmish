# Registered daemon dispatcher-loss subprocess design review

Claude Code 2.1.301 ran in Auto mode with Claude Sonnet 5.5. The reviewer
inspected the design and existing registered launch, dispatcher, listener and
stop source. No tests, builds, live processes or source edits were part of the
review.

| Round | Exact plan commit | Session | Verdict |
| --- | --- | --- | --- |
| R1 | `01ede6a559265971209d86764e3269de80461e38` | `445780d9-07f4-4956-b4d2-168daa40083c` | Request changes |
| R2 | `b8730520b874ead11f211c41c91b9c8f869cd7c1` | `afbc0e5e-5f88-4909-82f0-337c735c68ed` | Approve |

The reviewed base was `5c4d933a14ca4a24bc81813c11fb86bdd809830a`. The
approved [plan](../phases/peer-candidate-dispatcher-loss-subprocess.md) has
SHA-256 `e73bdf737595e40b95ee1f3d0010554884c9b45a7951a4014d72c96d22a1c4a6`.

R1 confirmed the two production admission cuts and identified four blockers.
R2 approved explicit private controller framing and failure exits, exact
pre-C and live Status deadlines and failure discriminators, preservation of
all three real SQLite stores and their sidecars, and complete direct child
and owned group cleanup. No blocking findings remain.

Apply these nonblocking review suggestions during implementation.

- Mark the child entry as ignored and select it with an exact libtest
  invocation. Reject a nonconnected or non-Unix-stream fd 0 before using it.
- Measure the original ACK deadline from exchange entry and Status I/O
  from request accept. The 250 ms capture budget is for the small, disposable
  fixture databases.
- Require at least one observed ordinary WSS connection belonging to the
  candidate child before claiming closure. Count only the exact captured
  connection set.
- Calibrate exact pre-cut database and auxiliary stability with two
  consecutive read-only witnesses on the healthy composition. If the pinned
  runtime legitimately changes a tested auxiliary, the raw-byte proof stays
  unqualified pending a reviewed policy for that file.

The design approval qualifies the specification only. The private candidate
fault tests, stock regressions and release-artifact isolation still need to
be implemented and run against the approved healthy composition.

## Issuer receipt clarification

The corrected healthy fixture established that Ready and Manual alone can
select Stock and fail the registered startup channel. Claude Code Auto
Sonnet independently approved the receipt clarification at
`e6745fc92fb04e655036d3b28acdd983277f7187` in session
`f0d95fbd-518a-4baf-be6c-7417a77177c1`. The reviewer checked the exact
doc delta and the corrected fixture and registration code. No runtime
validation or production change was part of this review.

For implementation, expose a narrowly gated test-only Both wrapper next to
`qualified_control_fixture`. Use the same disposable account as trusted
remote and execution principal and admit it through the fixture's ordinary
relay path. Check the issuer wrote `peer-relay-qualification.json` and
require the normal route reader to validate it. A receipt's presence alone
does not establish qualification.

## Separate qualification and Command listeners

The healthy fixture exposed an ordinary key-package publication refusal when
both roles used the strict qualification-only listener. The dispatcher-loss
plan now requires exactly one ordinary Command URL and a distinct Peer
qualification origin with separate trust roots. The qualification connections
close before spawn. The child must establish ordinary transport before the
cut, and the closure check requires a nonempty attributed connection set.
The Peer transcript must show zero accepts between spawn and reap.

Claude Code Auto Sonnet 5.5 first requested those explicit assertions in
session `0cc3942c-8f1d-4659-b083-28588082b747` after checking the source paths.
The independent follow-up approved the exact plan commit
`0d1687ae5124454eadae2d32aac646cc26c57896` in session
`65c78d82-b0c3-4d6d-89d3-ee944c7ef20b`. The approved plan SHA-256 is
`f4a34bb6fffc27bbb9d0adbf5ffd4f2fce042de8f9d80dcf4013fe5ea6ecc3ee`.
The follow-up checked the exact documentation delta against the first
review's source findings. No source edits, builds or runtime checks were
part of this clarification.

During implementation, scope the transcript windows separately for each
spawn and each cut. Keep parent and helper SDK connections out of both
listeners during that window, and give the pre-spawn probe-close assertion
a finite preparation deadline.
