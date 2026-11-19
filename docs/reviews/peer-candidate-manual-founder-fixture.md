# Manual recipient independent founder fixture review

Claude Code Sonnet 5.5 reviewed the test-only founder and relay oracle in Auto
mode against the [installed Manual proof plan](../phases/peer-candidate-manual-installed-proof.md).

| Source | Session | Disposition |
| --- | --- | --- |
| `525c91dd88285838f20240e4b1fa4abf4e488a80` | `9f2b1f1d-eca0-4ac3-892a-b3eeefd239d7` | Approved |
| `cb5429ab45234afcd16dcf68c59c77fcd8e1a453` | `eab02af2-6eaa-4e3c-b8e0-3baa99c58720` | Approved |

The reviewer verified independent founder keys and MDK storage, exact signed
Offer and Consent ancestry, a verified native Welcome, and recipient expiry
computed from the signed Delivery. The relay oracle scopes observations to the
exact authenticated socket. It sees only requests admitted by the relay's
strict kind, account and filter checks. The second review included the clarified
expiry window and the test for wrong lanes and bounds. The controller must
supply the relay's second account as founder because that is the permitted
recipient of Consent. The reviewer inspected source while the author ran the
local checks.

On Linux AMD64, the author mounted the exact founder temporarily in the
existing private installed-Delivery WSS fixture and enabled its candidate-only
oracle for that run. The loopback run took 19.36 seconds and observed one live
subscription, one bounded history request and a wrong-account refusal. The
actual relay accepted the three exact signed carriers in Offer, Consent and
Delivery order. The recipient independently observed durable Held with the
expected deadline. The lane classifier unit test passed. The exact source also
passed stock all-target Clippy with warnings denied, Rustfmt and Git whitespace
checks. The temporary mount was excluded from the committed patch. The
installed Manual controller, fresh child, same-UID List and Show projection,
and candidate build still require the composed qualification.
