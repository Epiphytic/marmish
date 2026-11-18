# Candidate daemon startup composition design review

The [startup composition plan](../phases/peer-candidate-daemon-composition.md)
was independently reviewed with Claude Code in Auto permission mode
using Claude Sonnet 5.5. The public source base is `a14b8c8`. The review
also checked the exact private registration source and Pending view R4.
This review concerns the design. The phase has no runtime patch or
installed verification at this cut.

| Round | Exact plan | Claude session | Outcome |
| --- | --- | --- | --- |
| R1 | `9cf632166742303ce0bb0e1d19185558d6eca8c8` | `9f61f5e7-ce15-4bce-bcbf-72c0e9aad17f` | Changes requested |
| R2 | `66d6ea09886a658cf715e42feb2f906003d6cf37` | `50d45a5a-5833-4b2c-99fe-6cf4f38f4582` | Approved |

R1 required complete preflight, registration and recheck failure
classification, process-fatal fencing for Terminal and ordinary storage
operations, separate lifecycle and affirmative mount acceptance, and
correct provenance of the view-owned four-slot permit. R2 confirmed those
were resolved. It also confirmed the ordinary status and reporter order,
unchanged registered Ready manifest, preserved ordinary error precedence,
exact listener join and handoff, and stop-before-drain with the original
lease retained through cleanup.

The reviewer noted that the current attach path checks dispatch before
testing lease scope. Implementers should retain the registered dispatch
check specified by the plan before continuing ordinary work on that
path. A `ProfileFiles` witness retains a nested Arc clone, so the
Untouched test must prove temporary witnesses have all dropped before
unwrapping the original lease.

Pending view R4 (`7fa52a2`) received separate design approval in
`45b93a1f`. It supplies the memory-only read and permit contract. The
non-test actor and its exact ownership interface, charged Pending and
view integration, and audited storage-fatal cleanup remain dependencies
of the affirmative mount. The independently reviewable unavailable-only
lifecycle adapter carries no Peer authority.

The author checked the documentation diff with `git diff --check`.
The review inspected source and performed no builds, tests or network
qualification.
