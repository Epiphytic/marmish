# Private heartbeat sampler reviews

Claude Code 2.1.301 with `claude-sonnet-5-5` reviewed the design and exact
implementation independently in `--permission-mode auto`. The code review
used an isolated snapshot based on
`3be8d3fde052c1726914a86c9a989ca50f45055c`.

## Design

Session `289faae1-607d-4627-8ad7-1d0c528ed4a9` returned REVISE. The revised
design added checks for owner and worker clock regression, specified
worker-owned permits and separate test domains, required a bounded full list
scan before retaining the smallest eligible addresses, identified both XNU
scope-embedding classes, named the shared visibility changes and clarified
the stand-alone integration test mount.

Session `b8c79188-f86a-4680-b765-8bd270c9915e` approved the
[phase design](../phases/peer-heartbeat-sampler.md) before implementation.
It also confirmed deadline handover, disk arithmetic and per-peer promotion
of local snapshots to authorized routes. The approved production boundary
keeps status opt-in and interface addresses off without explicit selection
and audience authorization.

## Implementation

Session `f95337dd-abb2-4171-886c-cdf462f7b673` approved the implementation
with no blocking findings. The reviewer inspected the worker permit and
clock invariants, privacy controls and native FFI contracts, and reran the
sampler target, codec target and native benchmark on Linux x86-64. The
reviewer suggested testing an overlong native interface name and explicit
IPv6 loopback, and requested the baseline codec source to confirm the
visibility-only changes. The final test update covers both cases. The
sampler implementation stayed byte-identical.

Session `4031a902-b5f4-4a0b-89a4-8aa43a97e4e1` approved the final
snapshot with no blocking findings. It confirmed the seven-line
visibility-only codec diff and again ran the 23 sampler-target tests, 12
codec-target tests and the opt-in 200-sample native benchmark. The reviewer
noted that the malformed-name fixture proves safe omission for that input
while the scan bound itself follows the audited implementation. The native
IPv6 check also allows hosts with IPv6 disabled. Both notes are non-blocking.
The final source manifest has SHA-256
`f8a6a7758c373250ecc33f99fd95b05cdff5a4c7f5a886c1a02025a1c1a13e05`.

[Validation record](../validation/peer-heartbeat-sampler.md)
