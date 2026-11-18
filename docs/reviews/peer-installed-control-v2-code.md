# Installed Peer control v2 code review

Claude Code CLI in Auto permission mode, model `claude-opus-5-5`, issued
**Approve** in session `00926b9b-a700-4769-a763-c76cc307dd58` on 2026-11-18.
The review covered exact source `7c407f2372b0f6d9a5bc1bf4c3bb51ed83e8c286`
against the [approved design](peer-installed-control-v2-design.md) at
`2767d11c43dcdacdbc8120b89884a9a9b6c1a320`, including the new shared types
and both standalone and embedded test code.

The reviewer verified byte-canonical signed v2 bodies, mixed-version
refusal before selector lookup, the prepared Consent and custody binding
checks, v1 defaults and exact bytes, and construction of a status-room
candidate only through a v2 Delivery. The reviewer independently recomputed
the room vector and ran the exact author-built receipt, control and cursor
binaries. All 70 selected tests passed. The [author validation](../validation/peer-installed-control-v2.md)
records the strict Clippy command and opt-in benchmark.

There were no blocking findings. The later installed owner must take its
receipt coordinates from the sealed negotiation because raw coordinate
equality grants no authority. That owner also must verify current native
MLS sender and group authority, local publication, lease and generation
before accepting a receipt. Exact negotiated version and status room must
be restored from a retained proof, without using expired invitation
flights. The all-target Clippy pass checked the library test source. The executed
standalone tests do not include the library pending-join integration fixture.
The private classified-Delivery version fence has no isolated branch test.
Cross-version cases exercise both the earlier signed classification refusal
and the direct binder.

The review covered offline wire behavior only. No installed feature,
database migration, deployment or relay activity was part of this change.
