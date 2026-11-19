# Independent review of the candidate Manual approval decision

The design and its review are recorded in
[local approval mount](peer-candidate-local-approval-mount.md). The native
decision was reviewed with the separately owned candidate local socket token
on the same source snapshot. The decision has no actor mount, listener route
or installed command entry point in this increment.

- Claude Code Opus 5.5 in Auto mode **requested changes** in session
  `4fa06ee7-e9f9-468d-b94f-97782091a8bd`. It reviewed source
  `945f0ab` with token overlay at `5e482b3`. The native copy had collapsed
  physical lifetime, signed expiry and storage mismatches into Stale, and
  tests did not reach the exact transaction and replay checks. The revision
  carries the physical cause through the scoped window, treats lost inode
  evidence as integrity failure, and adds tests for SQL rollback, clock
  floors, signed expiry, replay and the installed v3 gate.
- Claude Code Opus 5.5 in Auto mode **approved** exact combined `a7bba81`
  and owned leaf `395a9f7` in session
  `b2210725-e8f1-473f-93f8-303f401af32d`. It independently ran the
  linked approval, native fence, installed Pending, local approval listener,
  physical route and v2 Pending non-live filters. It found no remaining
  blocker for the unmounted decision and suggested more precise readback
  storage handling and a prewrite request-consumption fault. The R3 follow-up
  separates native storage and unavailable operations from mismatched native
  evidence. Its refusal test verifies neither the row nor the two wall
  floors changes when request consumption fails.

- Claude Code Opus 5.5 in Auto mode **approved** exact combined
  `076b5ad9d05875601e1431e4b6e033affa3e4735` and the follow-up bytes
  from `8ff321f` in session `c6bc27b4-6767-42b1-b8f2-cbae90edf9b0`.
  It traced the remaining native error origins and verified the refusal
  assertion covers the custody state and both persisted wall floors. It
  noted that authenticated native artifact decode failures remain proof
  mismatches. The scoped R3 review was static. The author subsequently
  passed fresh exact candidate Clippy and the linked regression filters.

The later actor owns projection invalidation and refresh, exact current
lifecycle and port recheck before replying, Peer generation closure for an
ambiguous commit, and keeping a durable Activating slot. Its fresh v3
installed run must cover a real invitation, charged original-stage readback
and an exact retry after lost reply with no repeated Join.

[Validation](../validation/peer-candidate-manual-approval-native.md)
describes the synthetic row graph in the transaction tests, actual installed
schema gate tests, executable hash and checks. The ignored native WSS
fixtures were outside this leaf's validation.
