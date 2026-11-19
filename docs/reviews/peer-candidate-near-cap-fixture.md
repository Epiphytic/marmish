# Near-cap SQLCipher fixture reviews

Claude Code Auto with `claude-sonnet-5-5` reviewed the [near-cap stage design](../phases/peer-candidate-near-cap-fixture.md)
for the installed Manual timing workload. The stage is qualified input
for the recorder. The workload exercises an allocated SQLite image with
mostly freelist pages.

| Round | Exact scope | Claude session | Verdict |
| --- | --- | --- | --- |
| Design R1 | `1769ad8` | `2a2d5620-12c1-453f-b3f8-67928c650a70` | Changes required |
| Design R2 | `650556c` | `675cf22b-e02d-43cc-a971-92aacbf9423b` | Approve |
| Design amendment | `12e55e5` | `6911fae6-1117-4213-9ba9-f04302322fdf` | Approve |
| Code R1 | `cdfcef4` | `4b6707a8-8e99-4d41-b3a8-16dde5afa7e4` | Changes required |
| Code R2 | `df78ef3` | `7e371e53-2dd1-41f8-b072-7be70076d87e` | Approve code with runtime evidence required |

Code R1 found the first native run stopped at a generic pager check,
the exact custody selection accepted a wider inventory than the design,
and the offline test did not exercise the installed cursor. The revised
helper uses the locked SQLCipher pager query, verifies the existing
installed custody epoch and counts both complete inventories before
accepting the matching row. Code R2 confirmed that the native offline
test exercises the actual joined stage and that the installed controller
must separately exercise check-only cursor selection and reopened Pending.

A direct test of linked SQLCipher found that a rolled-back scratch write
can change encrypted header bytes even when the original logical schema
and pager extent are restored. Claude approved moving the small-cap
negative to a separate 0600 byte copy, checking its logical metadata,
schema version, integrity and sidecar cleanup, and preserving the
original stage digest.
