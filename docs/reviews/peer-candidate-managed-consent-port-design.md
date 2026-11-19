# Managed Consent port design review

Claude Code Auto independently reviewed the design against the candidate
source at `65ad6fb`. The reviews were read-only and ran no builds, tests, or
relay traffic.

| Design | Claude Code session | Disposition |
| --- | --- | --- |
| `d85e16e50059f9afe8c563eef589e89d69515d37` | `a4680fd5-3bfa-46cd-87a1-7510523fc2e8` | Requested changes. The existing wake was mounted only with test-only Delivery/Held code. |
| `1d43c9efd48667d8fc063bf0234b3c0d089d7097` | `47f6e6c2-292b-471d-9f6f-1a4cb4754175` | Confirmed the candidate wake extraction, original window, same-port send and settlement. Requested a concrete same-owner source for the fixture's native package attestation. |
| `bc322464e9a6a0963d62e407b1fcbcdd90de155c` | `e57e654e-16db-44b2-a07e-0256aae538a9` | Claude Sonnet 5 approved. Confirmed the test-only charged package bridge, existing wake predicates and transaction rollback, and the unchanged five-second turn. |

Use a fixture-specific constructor name such as
`fixture_attestation_from_package`. Assert that the managed queued package
target matches the stage coordinates returned by the checked Delivery before
deferring the record. The existing Delivery proof independently rejects a
mismatch. The approved design adds no daemon mount or Welcome operation.
