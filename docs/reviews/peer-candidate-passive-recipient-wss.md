# Fresh-process passive recipient fixture review

Claude Code Opus in Auto mode reviewed the opt-in fixture against the
[registered Manual recipient lifetime plan](../phases/peer-candidate-recipient-lifecycle.md).

| Round | Source | Session | Disposition |
| --- | --- | --- | --- |
| R1 | `92a1ff2aeec7447428255bcca1ba77443e0e47ac` | `02e94fd9-7e54-4ff3-9ff9-dde6df6586ad` | Request changes |
| R2 | `aedbaf6c0d0eaad1a5ea10d437d727ad468b9f95` | `11debf12-9781-4528-a440-0dea9dadae50` | Approve |

R1 found that the typed client translated a broken local socket into
`peer_unavailable` and the invocation query could hide an extra row on
another page. R2 requires the actual refusal reply and a complete inventory
with the exact invocation count. It also checks that the Peer cursor file
stays absent during the two-second provisional hold and appears after the
authenticated subscription.

R2 inspected all changed hunks and ran scoped formatting. The review confirmed
that the cold reopen uses the stock `MdkOwner` path in the candidate test
controller, which has no registered dispatcher. The separately qualified
stock package has its own process-level evidence. Compiled checks, live test
results and single-run lifecycle timings belong to the composed validation.
