# Durable private WSS PostJoin review

Claude Code in Auto approved the [design](../phases/peer-postjoin-private-wss.md)
in session `24593c87-795e-4aec-92cd-36a550a0c990`. It approved the exact
M5b implementation `dae2c94` against `c71f59d` in session
`30a10ac3-a624-4c11-b488-55b30e49dab9`. The review traced the full diff,
locked MDK contracts and author test evidence.

R1, session `9c1e6686-fc83-4860-a425-2239fbed6510`, required opaque
custody schema epochs and an independent native readback from the persisted
Captured journal. Both were added. The publication permit now requires the
recovered witness. R2, session `9f769745-8aaa-402e-af7a-9145dbac33a5`, upheld those fixes and withheld
approval when the additional gate assertions failed. Exact tracing identified
a refused MDK prepared KeyPackage publication. The handler now counts that
specific blocked operation separately, with every other unexpected refusal
still failing the gate assertion. The relay recorded three exact Group
writes in the [WSS proof](../validation/peer-postjoin-durable-wss.md).
The author ran strict native all-target Clippy successfully in 31.22 s.

Final approval allows the rejected KeyPackage housekeeping count to vary
at later points where physical egress remains closed. The exact initial
count can expose upstream MDK behavior changes on refresh. This increment
covers the private WSS PostJoin rotation and test-only publication storage
primitives. The full recipient receipt path, founder acceptance, durable
leave and installed connectivity are separately qualified.
