# Registered idle Command stop design review

Claude Code Sonnet 5.5 in Auto mode **approved** the [design](../phases/peer-candidate-idle-command-stop.md)
in session `170e1427-2df1-4cad-8c61-1afc118f38f1`. The reviewed commit was
`922af43c97c4d42dd9006950d5c525e29fd386ac`, with design blob
`b1731d4cdbb5b66b95be12824e4490b26c2ab38c` and unchanged Rust source
`9dea56c8d4f26ee3f9b9fcde9c6a944cc261d138`. The reviewer inspected the
owner shutdown, Command worker and Journal code. This review covered design
only. No implementation, builds or runtime tests were done.

The initial review `69ebbda4-de3a-470f-850b-117d1f9c1302` required precise
Journal and ACK effects, a bounded blocking turn, an exact quiescence proof
and recovery after interrupted progress. R2
`39f5085b-789e-43e4-9af4-2c761009af93` closed those findings and required
an owner state model, a compile-time ownership gate, expiry after the
consuming enqueue, budget and cadence clarification, and a definition of
idle. R3 found no remaining blockers and checked the added passive guard
for outstanding Copy reclamation.

Carry these review notes into implementation.

- Check `Box<MdkOwner>: Send + 'static` in the exact candidate build before
  implementing the blocking handoff. Prefer a private owning prelude before
  `HealthyStop` if that keeps whole-owner and engine ownership distinct.
- Add a passive engine getter for the existing Copy GC flag and narrow
  Candidate Command gates before durable fence allocation and commits,
  and before worker effect enqueues. Preserve stock semantics. A typed
  admission refusal or expiry must bypass generic failure handling that
  would enqueue `FailCleanup`, or explicitly fail its admission check.
- Require on-time same-fence quiescence and physical joins. A late engine
  closure cannot authorize Journal, account or runtime cleanup. The six-
  second prelude qualifies only measured private workloads. Large retained
  histories may exceed it, and pending publications remain recoverable.

The existing first private registration qualification with no engine stays
independent. This design has no mounted code or release claim.

## First implementation review

Claude Code Sonnet 5.5 in Auto mode **approved** the bounded implementation
at `26fd90e1ec118ef6150a4ba57299444924c23317` in session
`629b363b-c290-45d9-9f27-202256a50447`. The reviewer inspected the six changed
Rust files against the approved design and ran the immutable stock test binary.
The four new tests and 71 related tests passed, with one related test ignored.
The reviewer found no blocking issues.

The review confirmed the single retained blocking join, exact admission and
fence checks, and physical-only disposition when the prelude fails or expires.
It also identified proof limits. The cancellation regression cuts before the
first admitted call, and the zero-row timing fixture prints a literal row
count. The installed started-engine lifecycle and transition-specific cuts in
the design still require qualification. See the [validation record](../validation/peer-candidate-idle-command-stop.md).

## Worker liveness follow-up

Claude Code Sonnet 5.5 in Auto mode **approved** source
`0429ed1b46ca2b88b61e9806520303b8d428842b` in session
`745df915-328d-4d96-b406-78d1d6dea36f`. The reviewer verified that every
trusted prelude poll rejects a disconnected engine even before the worker
thread finishes. The existing Copy GC flag keeps queued and unapplied work
busy. Review was by source inspection. The separate Linux AMD64 author
checks ran the affected tests and passed strict stock and candidate Clippy.

## Composed registered daemon review

Claude Code Sonnet 5.5 in Auto mode approved composed source
`918b92cbcf74d79483cc1219ded34bd0ffe356a3` in session
`df66a84d-ef0f-46d0-8b2d-4591780fef1e`. The review compared the
private daemon integration with the reviewed idle and worker-liveness
sources. It checked that failed or expired prelude attempts cannot
authorize cleanup, the owned engine and lease survive canceled
observations, and stock turn behavior is unchanged.

The reviewer independently reran 32 in-process and owner tests in
54.85 seconds in Auto session `92968976-1c12-426c-ad03-3814147885ac`.
It confirmed the executable hash before and after the run. The review
notes a remaining timing robustness limit. A successful prelude late
in a one-second physical observation slice can leave little time
for the single graceful attempt. Exhausting that time keeps the
evidence and yields a nonzero exit. The started-engine installed
scenario and per-subeffect interruptions still need live evidence.

## Fresh physical observation slice

Claude Code in Auto mode approved the design in
`409c84e8-f3be-4fd5-b4d8-3fc872594947` and the exact implementation
`8adc5b3ad1144e49969ff047891053cc7e9300af` in
`21782461-878f-434d-b4ca-42c8cfdf457d`. The reviewer traced the original
lease and owner across the synchronous handoff and confirmed that
pending physical barriers still prevent trusted cleanup. Fatal,
failed-prelude and NeverStarted paths retain their earlier behavior.
This addresses the timing limit described in the composed review.

In separate Auto session `946202ee-8932-4850-ac28-ff7907897925`,
Claude ran the seven focused cases against the author's archived
stock binary and approved the runtime results. The three selectors
took 12.03 s, 3.50 s and 0.41 s respectively. An initial invocation
used an unsupported nightly test-runner flag and ran zero cases.
The successful reruns used the stable runner without that flag.
