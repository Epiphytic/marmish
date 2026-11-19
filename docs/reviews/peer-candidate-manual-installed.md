# Installed Manual actor integration review

Claude Code Sonnet 5.5 in Auto mode approved source
`78b4d398e5d943089a6ff896f4495b4f9cc4640f` in session
`7ab3b55f-a21d-410a-ae00-e22d5b8aece8` against the mounted scheduler and
installed-proof designs. It inspected actor mount and shutdown, Fatal
precedence, selector re-observation, rollback guards, publication and
view ordering, telemetry, and the installed controller. It found no
blocking issues. Its review was static and drew on prior component reviews
for their internals. The independent exact installed run is recorded
in the validation document.

The review identified limited integrated fault coverage and two timing
details. During history recovery the actor uses the 20 ms live probe
and consumes scheduled ticks without publishing a view or emitting
refresh telemetry. A signed Offer expiring during provisioning can
conservatively end the current Peer generation. The candidate has no
automatic redial. The near-cap benchmark must measure after recovery and
account for every missed tick in its recorded interval.

A second Claude Code Auto session
`bfb8e2a9-b580-4cdb-b077-8e95487195d9` approved the exact private installed
proof increment. It inspected both native WSS logs and the test assertions
for ACL scope, control isolation, exact Pending selection, no redial and
stock command attribution. It ran no additional tests. Cold Pending was
identical across restart with relay history available. The test did not
independently rule out reconstruction from history. The reviewer also noted
that attribution depends on the authenticated socket and its ACL, and that
two changed fixture call sites needed their own installed regression runs.

- [Manual work selectors review](peer-candidate-manual-selectors.md)
- [Saved Delivery review](peer-candidate-manual-saved-delivery.md)
- [Refresh telemetry review](peer-candidate-manual-refresh-timing.md)
- [Exact installed validation](../validation/peer-candidate-manual-installed.md)
