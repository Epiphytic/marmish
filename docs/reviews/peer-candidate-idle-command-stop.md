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
