# Installed Peer recipient daemon design review

Claude Code Sonnet independently reviewed the Linux AMD64 private receiver plan in Auto permission mode. The reviewer inspected the local exact source and the locked bundled SQLCipher pager. These reviews cover the design. They ran no builds, test processes, credential inspection, or live network operations.

| Round | Exact reviewed commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `3e8373fd2c36c3ed60ccb788212bf0d721c2809f` | `d4be7986-556b-4bad-aa6c-894ad4d13916` | Request changes |
| R2 | `006a492fd6ddf33b7b9a0f83360014d4326f9c01` | `275013e6-3d61-4276-a609-031460fd5440` | Request changes |
| R3 | `70e1cc1faf4cbb5a8b4e40a9b00d229b9aec8505` | `0e294239-63ae-4c88-aeae-36b592e0953f` | Approve |
| R4 | `c740e91a233d02fe1f3fb4a445f371e15ed996f0` | `0e294239-63ae-4c88-aeae-36b592e0953f` | Approve |

The review moved ordinary credential and manifest checks before registration and the opened MDK binding check after the first native open. It required a route-specific permit and correlated settlement, an inventory-derived quota capability, explicit ACK and overflow rules, and a single claim for the Linux private Manual Unknown recipient through Pending. The existing configuration supplies no installed Known pin. Default and `installed-checks` artifacts stay closed, and the private selector cannot qualify macOS.

The reviewer verified the checked single-database DELETE journal bound from the locked pager's spill-off and final commit sync. The final plan preserves the fail-closed ordinary identity comparison and fences ambiguous stock cursor sidecars before recovery. Exact-lock format checks, disk extents near the cap, quota refusal and transaction rollback remain proof obligations of the implementation.

A disposable profile can qualify while an expired or ambiguously charged slot remains unavailable across crash or clock rollback. A founder that sends no Delivery can permanently consume the Unknown position. Release promotion requires an independently reviewed bounded retirement or retention and operator-recovery policy. macOS ARM64 requires its own APFS and installed evidence. The installed mount also depends on typed exact physical control issuance/settlement and the separate capped Welcome/readback proof. [Phase design](../phases/peer-installed-recipient-daemon.md)

## Stock cursor cap proof

Claude Code Sonnet in Auto mode approved exact source `df09a01c1e6c38f64d48694264aa6e9138b542b3`, session `58f73d22-623a-46b3-841f-503ddf2cf6be`. The reviewer compared the three Rust files to the previous opener, inspected the locked SQLCipher pager, and ran all four focused tests directly from the author's preserved test binary. A separate scratch SQLite check reproduced an oversized hot journal growing the main database before the page limit could protect it.

The reviewer found no blocking changes and noted that spill-off is verified by pragma readback and the locked pager's behavior. The extent threshold has enough slack that the journal test alone does not distinguish spill-on from spill-off. The test uses one origin when filling the existing globally bounded deferred queue. The composed owner still needs the sole profile lease and its cursor inode checks. [Validation](../validation/peer-installed-recipient-daemon.md)

Claude Code Opus in Auto mode approved the exact integrated source
`0f60a06` in session `271f4996-52ba-4870-932e-2f90233803b7`. The
review traced the locked pager's one-record-per-original-page bound,
verified production callers retain the existing opener, and confirmed
that the three cursor source files match the author-reviewed source.
The independent journal upper-bound correction is incorporated in the
[validation](../validation/peer-installed-recipient-daemon.md).

## Preexisting Ready startup proof

Claude Code Sonnet in Auto mode approved the initial four-file Rust source `be63e4875aa1bd677e8bb22749311ff459eb45b3` in session `e8285f7a-c4e5-475c-b9d2-e89d2e144ed0`. The reviewer recommended proving the profile was Ready when its lease was acquired. The revision records that validated manifest state at acquisition, changes the fixture to reacquire an existing Ready profile, and proves a lease that writes Ready during setup remains ineligible.

Claude approved exact revised Rust source `d64dd85865c40d770e909ac2978fbcf581212746` in Auto session `ee8b386c-af0c-47fc-8619-20cae902b3ee`, with three independent focused passes in 19.3 s and no blocking findings. The review checked the immutable acquisition bit, exact Manual intent, prepared account, same lease, held files, and read-only opened binding against the phase design. The Linux AMD64 test-only scope has no daemon mount. [Validation](../validation/peer-installed-recipient-daemon.md)
