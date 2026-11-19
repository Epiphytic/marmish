# Independent review of the local approval IPC

The local protocol and listener pair implement the first cut of the approved
[same-UID design](../phases/peer-candidate-local-approval-mount.md). The reviewed
source begins at `3b9edc94480a2785e93c27f8d51067ff60648407`.

- Claude Code in Auto mode requested changes to `1d23650` in session
  `7502857b-fce5-406c-964e-a0af989ce0bc`. The generic response parser could
  normalize duplicate nested fields before validating the typed reply. The
  revision decodes the exact reply as a closed type. It also binds the pair
  to the actual listener's fatal signal and held lease path and checks
  revocation during the response write.
- Claude Code Opus 5.5 in Auto mode **approved** exact
  `4a1faa863e50b3a63efd8ead8e052fa4f44230a7` in session
  `c31cd0ef-aa58-4198-83a3-e099db8981f2`. The reviewer checked all eight source
  files and found no blocking correctness or authority defects. The candidate
  all-target Clippy pass resolved the remaining build question. The reviewer
  recommended direct tests of registration and worker revocation.
- Claude Code Opus 5.5 in Auto mode requested one additional mutation-sensitive
  assertion for test-only `3cf7a91` in session
  `e8261e39-03b9-4be8-a383-b538741cf044`. The previous test isolated the lease
  path and fatal identity guards. The revision also isolates the kernel-bound
  path and checks that the slot clears after a reply and after worker drop.
- Claude Code Opus 5.5 in Auto mode **approved** the final test amendment at
  `8017602f0aea30cf4fb1d25ffce82634e388b855` in session
  `153d6c49-abed-4e62-814e-57ed98bde58c`. The amendment leaves the approved
  production code byte-identical. The reviewer verified the registration
  checks independently and confirmed that worker stop synchronously clears
  the claimed slot. The uncertainty-reply assertion uses Tokio's
  current-thread test runtime to observe the cleanup after the frame.

The reviewer inspected the exact committed bytes. The author ran the
separate compiled checks in the [validation record](../validation/peer-candidate-local-approval-ipc.md).
Each socket fixture starts from a fresh uninitialized profile lease. Its
runtime path is assigned a fresh random profile ID, so an interrupted test
cannot make a later run reuse that socket path.
