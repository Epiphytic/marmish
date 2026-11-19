# Managed Consent port source review

Claude Code Auto independently inspected the bounded turn and tests
against the [approved design](../phases/peer-candidate-managed-consent-port.md).

| Source | Claude Code session | Disposition |
| --- | --- | --- |
| `ee13fc940473b16c88326a43110dedfb1ddbc775` | `74360845-0e0c-4f01-82c9-3f20588c169e` | Requested changes. An independent WSS run reproduced failure on cold readback after the checked route had already been consumed. |
| `0cd78e69bb5b1edeb68442a709dd5e6155089ee9` | `fc3a1df8-de0b-43ff-a76f-b675d9cf51fc` | Requested changes. The run crossed into a new second during durable reservation, exposing a stale wall-floor assertion. The reviewer found the production turn consistent with the design. |
| `24571ae7cb8b1921a81589c3cca2766b3c4be764` | `52ccd378-24e8-4ea5-b2d9-a9f2460e3b42` | Approved after exact stock Clippy and compile checks. Claude independently inspected the source and observed the passing seven-case private WSS matrix, checked ACK suite and cursor tests in the archived logs. |

The test now reopens the exact charged checkpoint using the registered
lease and quota without reacquiring its consumed physical route. The
durable attempt retains its retry time even after ACK. The test
compares both SQL wall floors against the durable attempt, ACK or
wake appropriate to the exact case and verifies retry eligibility.
It exercises a stop after ACK settlement and verifies rollback at a
genuinely later wall time. The fixes use existing product semantics.

For the deliberately retired or failed sends, the matrix checks the
exact retained durable retry and relay transcript. Live repeat checks
are limited to the successful settlement and the rolled-back wake.

Claude Code Auto approved the integrated patch at `d2c1b5f` in
`58005b8b-8b9b-41b2-8baf-813986316a81`. It verified matching changed
blobs and stable patch ID, with no file overlap or new reachability
from the registered Command changes. Author strict stock Clippy on
the composition passed in 29.44 s. The private WSS measurements above
and in the validation report use the exact author source before
composition. The review suggested a test-only gate for the only
fixture constructor that does not require a managed proof. The
follow-up adds that gate.

Claude Code Auto approved the follow-up `50a3cf5` in
`d528ac6c-f172-4ef4-8dd3-340380baae1c`. It confirmed that the
only caller is test-only and the enclosing module already had the
same restriction. The new attribute records the fixture boundary
at the constructor as well. The reviewer ran no additional tests.
