# Peer M2 native join and readback review

Claude Code Sonnet 5.5 independently reviewed the scoped native join and
snapshot reader in Auto permission mode. The parent phase design approval
is session `ae2a5faa-bace-4c04-bf0b-dd44970964cb`. This proof uses the
same locked upstream MDK revision as the standalone preflight.

| Review | Claude session | Exact scope | Verdict |
| --- | --- | --- | --- |
| Initial code | `64cccd08-e61d-42a6-9a0e-dbaba7d4e502` | `81f0a50df92739a7c166f1370aad010852d85a2f` | Changes required |
| Follow-up code | `bfed8fc9-e101-4fec-9d65-b6f15fe641b2` | `ffd3481a5cac2149813acd17f0546ee4ba025433`, tree `c67ebb6a19309beb75fb0675d5fa7703b962fd9a` | Approved |

The first review required independent verification of the closed stage
before native ingest and negative regression coverage. The exact follow-up
uses the existing protected disposable copy guard and the native prepared
lifecycle validator to check the signed package and reference before the
original opens. It tests a wrong digest, an authenticated Delivery for a
different package in the same slot, and attempted replay against the
already joined file. All rejected attempts leave the original unchanged.
The custody owner owns and tests the committed one-use JoinStarted
guard separately.

The source makes the Welcome kind and recipient check explicit in a
comment. It retains original inode and sidecar error precedence even
on a native join failure because those checks fence further progress.
The opaque joined proof carries the native typed group ID, epoch and
local leaf for the parent checkpoint comparison.

The follow-up reviewer ran the preserved binary. The native proof passed
independently in 54.9 seconds. The opaque proof carries file identity and
native authority. The reviewer recommended taking the fresh snapshot
directly before the Pending CAS in the serialized custody turn. The
Pending CAS and activation decision are integrated separately. Changed
identity or a surviving sidecar takes precedence over a native error.

The original review response SHA-256 is
`edd69b52c2380d89518d7ff78ea630d4e28d6bfc42604f7a144a2bc60c02ad19`.
The follow-up review response SHA-256 is `3c1cd8759d578741d654ee4f67a4a5630d9a3947d2da94004bf924605ffaf458`.
The [local test results](../validation/peer-m2-native-join-readback.md)
describe the test scope and correction to Prepared Consent error handling.
