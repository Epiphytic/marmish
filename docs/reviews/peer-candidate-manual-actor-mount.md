# Manual recipient actor mount design review

Claude Code Sonnet 5.5 in `--permission-mode auto` approved the
[Manual actor mount design](../phases/peer-candidate-manual-actor-mount.md)
at `5cd55a8cfb121591447fb167fe852e7d23143cc9`, design blob
`07b8131e8afc9e2152c41c9d35f85ba41b2fc829`. The final review session was
`958ef5b1-a122-4782-973d-4a1d97f6168a`. It inspected composed Rust at
`194d1902cb7838f376c8229e0377d14ce511341d` alongside the approved Manual
actor, readiness, raw transport, managed package and ordinary fatal designs.

R1 session `62bac90a-356b-4be7-8212-ef49885a0e37` requested changes on
`8a6575b97f0199f85878e14108a261cfd6ac5638`. The revised design specifies
exact route revocation before raw reap even during startup, registry-issued
Fatal paired with the Command keeper, concrete managed original-image
attestation and a single borrowed cursor for Consent and Delivery, and
separate origin-to-publication latency and prior-proof expiry criteria. R2
also checked the current-incarnation authenticated recovery barrier required
for Healthy Empty and confirmed `git diff --check` on the reviewed design.

During implementation, schedule each 500 ms turn from its scheduled origin,
exercise loss or a gap on the live feed as invalidating the Healthy Empty
barrier, and prove normal-stop raw drain after exact revocation with Fatal
untripped. Managed package re-digest applies to new and deferred Delivery
before Held. A cold Held, JoinStarted or Pending proof revalidates the retained
signed Delivery under the existing exact custody aperture, with clean native
Joined readback where required. That path can prove consumption even after the
original KeyPackage has ceased to be available.

Pin the independently reviewed composed publication base before source work
and coordinate ownership of the daemon, receiver, and managed Consent turns.
This review covered design only. It did not build or test the candidate or
qualify any installed activation, cross-platform behavior, or latency claim.
