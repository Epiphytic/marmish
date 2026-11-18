# Candidate Pending read view design review

Bead `marmish-dwt.4.9`. Claude Code Auto model `claude-sonnet-5-5`
reviewed the [Pending view design](../phases/peer-candidate-pending-view.md).

| Round | Exact design | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `f4155ef` | `ccb956a1-e514-438d-9193-64244e381cb0` | Changes required |
| R2 | `ff77555` | `df252386-b039-40ca-a5e3-3bb537ffe8c1` | Changes required |
| R3 | `1f8b006` | `45b93a1f-9bc0-4387-b60e-5b483fd1067a` | Changes required |
| R4 | `7fa52a2` | `45b93a1f-9bc0-4387-b60e-5b483fd1067a` | Approve |

The final review confirmed the mutating-turn order, one exact durable
readback after CAS, a wall floor carried from the same fence, and a
nonwaiting four-slot read budget owned by the shared projection. It
covered terminal listener and producer shutdown, a process-local expiry
latch and the documented limits on clock rollback after restart and
delivery to a slow local reader. All rounds used read-only design and
source inspection. Code and mounted-actor qualification have their own
gates.
