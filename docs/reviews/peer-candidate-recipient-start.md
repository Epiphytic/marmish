# Candidate recipient owned startup review

The [Manual actor mount design](../phases/peer-candidate-manual-actor-mount.md)
was approved in Claude Code Auto session
`958ef5b1-a122-4782-973d-4a1d97f6168a` at `5cd55a8`.
The [cursor supplement](../phases/peer-candidate-recipient-start.md)
was approved at `dfb4284` in Auto session
`abafa30d-56b5-4d9f-b48b-a977fd23c037`.

The first code review of `7809c6f` requested changes in Claude Code
Sonnet Auto session `bfff5ccd-3abc-4e5a-9b2c-093cc585d41b`. It found
that actor-initiated exit kept readiness current while waiting for the
raw drain and that startup inventory measured files without the exact
file-turn guard. The actor now drops its readiness witness before drain
and reconciles only under the same lease and quota's file guard. The
cursor preparation amendment also resolves the missing fresh-profile
file observed by the author's first native run.

The next independent review, Auto session
`3930d944-68c1-41eb-9a82-da738389684c`, approved those corrections, the
retained descriptor continuity, and the teardown cases. It did not compile
or run the tests. Its nonblocking evidence notes distinguish a check of
the pre-drain readiness order from an end-to-end actor-initiated failure,
and point out that the sidecar fixture exercises the WAL case. It also
noted that a socket-active-at-exit assertion alone does not count earlier
connections. The author's proof independently covers the physical socket
end for two stop cuts and the fatal case. The schema and route code ordering
is reviewed separately from that coverage.

Claude Code Sonnet in Auto mode independently **approved** exact source
`79c9d18900d957b14e1139944ede5488005b2b5e` in session
`de19a658-6087-4c6a-b245-270671516401`. The final review verified that the
existing v2 Pending opener retained its call contract, the same keyed
cursor and quota retained custody through startup, the paired raw drain
cannot complete ahead of local revocation, and the returned joined
status represents a real thread join. The reviewer inspected exact
source without running network or Cargo. The author completed the
[candidate link, strict lint, and real WSS checks](../validation/peer-candidate-recipient-start.md).
