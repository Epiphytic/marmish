# Single-origin live Terminal enrollment reviews

Claude Sonnet 5.5 reviewed the [phase design](../phases/terminal-live-enrollment.md)
and the implementation in Claude Code auto mode.

| Scope | Session | Decision |
| --- | --- | --- |
| Design R5 | `62d1ebca-7a82-4191-ab7f-25428d540e3a` | Approved the direct, single-origin writer slice after reconciling consumed-Probe retention with the signed freshness and clock bounds. |
| Code R1 | `8f41baeb-54b8-4d23-a3f1-7b6243d91c62` | Requested the initial 32 KiB cap, an independent accepted-Probe deadline and real WSS rejection coverage. |
| Code R2 | `b0dcc25e-1c39-4533-b971-fd8faf3a6a59` | Approved those corrections by inspection and ran the deterministic cases and real AUTH refusal and source retirement. A teardown panic in one run needed follow-up. |
| Code R3 | `63ac19e1-74e1-4e5a-9e1c-de7f3d502d53` | Approved the implementation and sink fix. Independently ran all four opt-in loopback WSS fixtures, broad Terminal and transport tests, and Clippy. Found no panic in the fixture logs. |
| Code R4 | `408e99cb-1608-42f4-afdb-37ddda21660e` | Approved the final scope frozen on `3be8d3f`. Independently passed all four WSS fixtures, the deterministic clock, capacity, bootstrap and transport checks, and strict Clippy. Found no panic in the fixture logs. |

The teardown panic came from the WebSocket sink polling a completed
conversion future after a write error. The shared connector now returns
a repeatable error on that path. The final tests also distinguish grant and
capacity refusal, advance an injected clock past the initial socket budget,
and verify an ordinary Command during a live Terminal attachment.

The R2 reviewer initially built only the native worker. The control Command
stopped with `not_started` because the worker needs a sibling `marmish`
executable. The [validation instructions](../validation/terminal-live-enrollment.md)
build both executables in the same Cargo target directory.

Follow-up qualification before public activation includes active-phase
backpressure, warm admission, old-epoch AUTH delivery on a socket, and
Welcome-first delivery through the verified owner. A phase transition can
currently retire while Command authority is suspect. A Confirm arriving
during clock uncertainty can require retransmission. Both paths fail closed
in this test-gated slice. The reviewer also called out that future grant
providers must avoid re-entering the authority while it changes phase. The
proof uses an atomic grant predicate.
