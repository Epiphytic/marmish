# Pending decline route-rejection proof reviews

Claude Code Sonnet 5.5 in Auto mode approved the detailed design in
session `17031976-de12-4c44-829e-96546d6db969`. The final design
incorporated the exact routing error, the post-materialization payload
limit and the synthetic candidate-selection refusal. Claude approved
that revision in session `6f6a3f9f-de15-4f0a-b4bf-cca2899b2286`.
The design SHA-256 is
`d48caa1d9b1614dcbbc99f67d05cfe6bb7d0fef6a31d4a58ebceb1fe1a67cf17`.
[Phase design](../phases/peer-pending-unrouted-leave-proof.md)

The integrated Auto review in session
`efca0308-903b-4e58-ae3d-b3e6ef1744ae` approved the code and
independently reran both compiled selectors. The route-rejection proof
passed in 15.16 seconds. The existing exact-event and typed SelfRemove
proof passed in 16.94 seconds. The review confirmed the cold SQLCipher
reopen, native validation-cache behavior, exact recovered proposal,
deny-only fanout staging and persisted retry.

The reviewer requested explicit validation limits for attempted
restaging and preexisting conflicting fanouts. The updated validation
note records them, distinguishes the sibling authentication vectors and
records production ownership and selection limits. Claude approved the
updated scope in session `217d211c-ba03-408d-b176-978a1a62b6b8`.
The final eight-path scope manifest SHA-256 is
`25dbf60d51b05bb287b3abdbe7ab29a66530ccd089eee777d08ae60fdefd6bd7`.
It covers the lockfile, native manifest, three proof source files, phase
design and both validation notes. The two integrated reviews inspected
the same code and tests. [Validation](../validation/peer-pending-unrouted-leave-proof.md)

The author reran both tests with the lockfile in offline mode. Strict
native library and test Clippy, scoped rustfmt and staged diff checks
passed. Workspace rustfmt reported the identical existing differences in
`marmish-engine/src/lib.rs` and `marmish-native/src/native_setup.rs`
on the clean base and this candidate.
