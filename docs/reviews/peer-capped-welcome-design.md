# Capped recipient Welcome design review

Claude Code `claude-opus-5-5` independently reviewed
[the capped Welcome design](../phases/peer-capped-welcome.md) in
`--permission-mode auto`. Reviews used the frozen source, exact parent
custody design and locked MDK `bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`.
The reviewer performed local source inspection. These are design reviews
and contain no implementation or test results.

| Round | Exact design commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `2681e93d02c77eab39521ccccb56d4df5656f437` | `96fa8129-1657-4898-9bf6-8453591a2abf` | Request changes |
| R2 | `87536555b8d64996f8e0d67b5c68156c7c3d9f87` | `471315e2-bd1a-43d6-b746-f7832249c680` | Request changes |
| R3 | `ca9900da1ad148b4337d97653c3abeef703595a9` | `e9bd67bd-309e-42ec-93e3-eea225fc5143` | Approve |

The R3 document SHA-256 is
`4d0a8751a0081ace900e9220ce8693cbb1c364b30f836fc821d3f9fd5caca7ff`.

R1 found that the existing C2 fence constructor and wall sample were
inaccessible to the new owner. R2 exposed them too broadly for their
private clock and error types, which would fail strict Clippy. R3 keeps
those signatures within `peer_invitation` and exposes only the native
`StageError` sampler to the native Join seam. Claude confirmed the
caller visibility, bounded snapshot and original exclusion, exact
proof type, one-use SQL ordering and five-second per-operation fence.
No blocking design findings remain.

The implementation review must check strict all-target Clippy, C2 serial
behavior under the shared guard, nonblocking contention refusal and
fresh per-operation fences within the absolute deadlines. The composed
v2 proof must show the successor Pending entry accepts a capped witness
and its owner cannot reach the legacy unmetered paths.
