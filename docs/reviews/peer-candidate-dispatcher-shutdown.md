# Candidate dispatcher-loss shutdown design review

Claude Code Sonnet 5.5 independently reviewed the
[shutdown design](../phases/peer-candidate-dispatcher-shutdown.md) in Auto
mode on 2026-11-18. Bead `marmish-dwt.4.12.2` tracks the increment.

| Review | Exact design | Session | Disposition |
| --- | --- | --- | --- |
| R1 | `5998bf4b8a7e32eecfa517978407235acc54bab4` | `3d09c09a-0bf1-41f5-a3bf-55738e93c3e2` | Requested changes |
| R2 | `245b15c86a43769e5922fe785f5657b51f55b91c` | `cfe0d516-4531-43d0-8fdf-60c2366dc956` | Approved |

R1 checked the daemon, Command engine and lifecycle, Terminal owner and
transport, and registration source. It requested a concrete pre-TLS
write fence and wake, an exact admission inventory and registry latch
edge, and explicit custody for SQLite destruction and live children.
It also requested a graceful-stop race trace and independent test
oracles. R2 approved the revised transport and slot proof, checked SQL
admission, process-lifetime SQLite custody, finite worker recovery,
direct-route eligibility and the already-running keeper transition.

The author inspected the Manual actor design via the exact design-review
ref `b96e4cee4e361a2587dbe1df937b372f76806ec9`. That ref is outside this
design's ancestry. The R2 reviewer relied on the R1 code inspection for
the Command, daemon and Terminal lifecycle and did not reread the Manual
review document from that ref.

Carry these review points into implementation and integrated code review:

- Check the fatal transition inside an already-running FIFO shutdown and
  keeper before collection, cleanup or a success report.
- Use a wake mechanism for every pending transport poll. Measure that SDK
  shutdown actually releases all route permits.
- Verify that the candidate process returns a failure after physical
  joins and exits without running the parked SQLite destructors.
- Detection is observer-driven. An idle daemon may remain unaware until
  a checked effect revalidates the default. The design proves the cut
  from observation and refuses fresh unchecked effects.

This is design-only evidence. The author verified the exact Git diff and
review dispositions. Runtime implementation, tests and release
qualification remain open.
