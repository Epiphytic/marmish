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

## Below-TLS transport primitive

Claude Code Sonnet 5.5 Auto approved exact source
`121dc730f437881a14847a79210bd26e11bfc8ae` in session
`c557a516-aed0-4629-94ee-5b43c265ce32`. The reviewer inspected the
candidate/Linux AMD64 compile gate, separate pending poll wakes, the
direct-stream placement before TLS, biased cancellation of opening
connections, and the exact per-route socket permit ownership and reaping.
The reviewer allowed the increment to close after both focused tests and
a candidate compile passed. The author completed those checks and
recorded the [results](../validation/peer-candidate-dispatcher-shutdown.md).

The reviewed code has no installed caller. Every future registered
candidate connector must receive the same fatal observer before sharing
or bootstrap. The raw Peer opener must transfer its socket with its
lifetime proof. Full SDK, Lane and daemon teardown evidence belongs to
the mounted shutdown increment.

## Composed relay fence and Pending reader

Claude Code Sonnet 5.5 Auto approved exact composed head
`61be37534b6f0549e34da7fc95c45a0b76bd21ec` in session
`2012ef7b-79ea-49de-8a98-59ef42b2da13`. It checked the transport and
IPC compile gates, socket fencing and reaping, reader ownership and
deadline handling, and the absence of installed API users. The
author ran 36 focused libtest cases plus strict stock and candidate
Clippy. See the [composed validation](../validation/peer-candidate-dispatcher-shutdown.md#composed-candidate).

The mounted increment must trip on an observer failure, cover a fatal
signal during graceful stop, and preserve the read permit through the
actual IPC write and its deadline. The reviewer scoped these obligations
to the future mount.
