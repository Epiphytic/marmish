# Native Peer Leave C2 design review

Claude Code Sonnet 5.5 reviewed the [C2 design](../phases/peer-native-leave-c2.md)
in `--permission-mode auto`. R1 requested changes in session
`242198eb-8245-4a08-8136-794877003812`. R2 approved commit
`f90ea0b7fd20119b7d2fe948726efc740b66e9e1` in session
`3166e733-d87d-4783-9b8f-a3996cc6a79b` with no blocking findings.

- R2 design blob is `dd1d8fa96ab0a9126afab9df434a7739786385e3`.
- R2 design SHA-256 is
  `d73f9e756a3d8f9ba13f50bfa91c886638ba8231ea59a92056e5f987221eae18`.
- The independent review used the locked upstream MDK source at
  `122ad45ece73c1d6f687a4e92a541b9b2febde34` and C1 commit
  `98a55dfda41863893a2451c41c1a5ae6aaf940d9`. Repeat dependency
  qualification at implementation against the integrated upstream lock.

R2 restricts native access to the closed Joined runtime, places cursor
transactions outside SQLite dispatcher scopes and puts the dispatch behind
one committed reservation. The trusted local owner holds the exclusive profile
lease. The reviewed recovery sequence opens a new guarded snapshot of the
original SQLCipher image to reconstruct evidence. The dispatch alone can
open the original, after validating the bound image and its absence of
companion files. If the native record pair is missing, proposal preparation
may already have mutated the original and the hold stays charged. R2's
pre-dispatch image digest and exclusive lease establish provenance without
comparing MDK's process-local monotonic timestamp to a custodial UNIX deadline.

The reviewer confirmed the exact native SHA-256 content marker, the DELETE
journal open contract, the closed adapter and the VFS scope boundaries. The
first implementation qualification must prove single-poll completion of the
actual unrouted Leave call. The reserved, charged snapshot needs a private
fixture-only path and dispatcher scope. Epoch 6 needs its own marker because
the C1 marker is constrained to epoch 5. A report of indeterminate recovery
when the durable pair is missing is acceptable.

This is design evidence. The review inspected source. Native execution,
crash cuts, builds and runtime qualification remain implementation gates.
