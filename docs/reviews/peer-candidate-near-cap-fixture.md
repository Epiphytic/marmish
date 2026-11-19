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
| Code R3 | `c45c926` | `8aeb596f-eb86-4306-8ce5-298732f58c18` | Approve code, qualification reported separately |

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

Code R3 checked the final pager reads, singleton custody and stage witnesses,
and exact SQLite full and rollback guards on the private copy. It reviewed
source without compiling or running tests. The exact native offline run and
installed qualification are recorded in the [validation report](../validation/peer-candidate-near-cap-fixture.md).

## Composed fixture and timing oracle

Claude Code Auto session `58de8b4f-204c-4f1f-9ae4-b3377761d964`
approved the integrated code at `9b30b76`, including the checked
original and copy, the lifetime and pressure recorder, and the ordinary
retained result checks. The reviewer ran the exact offline native
selector with private state. It passed in 25.2 s with the same pager
geometry as the author's run. The review left installed timing to
the separately scheduled live exercise. It identified a temporary
offline fixture directory retained inside its private scratch area.
The reviewer removed that directory. The installed timing and footprint
gates require separate evidence.

## Optimized installed evidence review

Claude Code Auto with Claude Sonnet 5.5, session
`0a75ba4a-098b-4140-a408-29299b0dd063`, approved the evidence for
the source-matched `9b30b76` driver and packages. The reviewer verified
the package manifests and installed binary hashes, the logged metrics,
charged image sizes, and the scheduled proof limits. The concurrent
cohort establishes continuity under offered load. The separate
deterministic listener test establishes what happens when all four slots
are occupied. Its exact release-profile selector passed separately.

The report now names the proof-age threshold, notes the sample size and
collector-only timing cost, and identifies the fixture assertion that
verified closure and unchanged original bytes. It omits the supplemental
optimized hash timing. A reviewer-initiated rebuild exceeded local disk
capacity and did not run the listener test. The already-built exact
selector and the completed installed run supplied the runtime evidence.
