# Provisional SQLCipher quota VFS reviews

## Design

Claude Sonnet 5.5 in Claude Code Auto mode approved the isolated
`cfg(test)` plan before implementation, session
`b5e7a843-a4d7-4e7a-b78a-41711cde9ca2`. It checked the actual MDK opener
and the workspace's bundled SQLCipher, including initialization before
migrations, the absence of a second connection during opening, and DELETE
journal behavior with in-memory temp storage.

Its qualifications are incorporated in the [phase design](../phases/peer-provisional-quota-vfs.md).
They include the externally created zero-byte main file, the version 1 VFS
ABI, refusal of alternate database paths, independent post-operation metadata,
canonicity, rollback behavior, and the limits of a logical byte measurement.

## Code review

Claude Sonnet 5.5 in Auto mode approved `178aeb2` as an isolated Linux AMD64
proof, session `750a4d48-9fde-4aa1-9071-12934853978d`. It checked the Rust
FFI against the pinned Unix SQLite source, replayed the three opt-in cases
with the author's prebuilt binary, checked their sanitized output, and ran
formatting on the two new Rust files. The reviewer did not rebuild the
binary or run Clippy in its session.

The follow-up exercises the reviewer’s requested SQLite exclusive WAL open
and the exact combined limit with a nonempty journal. It also verifies that a
failed wrong-key attempt charges no quota rejection, denies delete-on-close
opens, keeps the evidence invariant assertion outside the lock, and removes
the duplicate peak field. Independent path metadata is checked after each
size-changing operation and failed stock write. The [validation record](../validation/peer-provisional-quota-vfs.md)
reports the author runs independently.

A second Claude Sonnet 5.5 Auto review approved exact follow-up `bc5f084`
in session `54b5c3fa-4a97-40c9-87d3-df35200ca7b4`. It independently reran
the affected boundary and path case with the author's prebuilt binary
(1 passed). It confirmed that exclusive locking reaches the WAL-open
refusal, verified the ABI of the direct file callbacks, and checked that
the byte limit covers a nonempty journal. The delete-on-close guard was
verified by code inspection. The reviewer used the author's final log
for the other two cases and did not rebuild.

Both approvals apply to the isolated test proof. macOS execution, a
production quota, and hot-journal recovery through the actual MDK opener
remain outside this evidence.
