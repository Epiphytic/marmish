# Candidate receiver managed KeyPackage design review

Independent Claude Code design reviews ran in Auto mode with Sonnet 5.5.
The final review compared exact design commit
`2bbf75795022391205d08cea31f5a58127fdf833` against
`8bcc2b09eab932a426181b302218d806f90e110c`, including the integrated
Pending source at `f7bb53f`. The approved design blob is
`eecfcc6717ead0c281867aadf736fa9cf1372784`.
[Managed KeyPackage phase design](../phases/peer-candidate-managed-package.md)

| Design | Claude Code session | Verdict |
| --- | --- | --- |
| `a7736c2` | `9a767ab8-6ee8-4b07-8046-2020f735ce5e` | Request changes |
| `047f332` | `f34cda35-af86-4a67-8cc6-9a310d58e679` | Request changes |
| `a0b173a` | `394234ab-54c7-4481-bc03-741ffc2d319c` | Approve |
| `2bbf757` | `13447423-2ec3-43f6-94ad-384e15c6f691` | Approve, no blockers |

The revisions separated MDK's infallible timestamp from fallible owner
checks, specified the original and disposable copy quota scopes, and
documented retained charges after unprovable crash cuts. The final review
verified the candidate configuration dependencies, including the guarded
rollback chain, against integrated source and confirmed the native proof
can reuse the existing strict unjoined package verification.

The reviewer clarified the implementation order. Managed recovery and
`CleanFiles::begin` both acquire the file mutex, so complete recovery
before taking the clean-file turn and move both signatures to the same
candidate fence. Use the `Dispatch.file_turn` mutex while retaining
fixture-only `ProfileSlots::file_turn` and `FixtureFileTurn` guards.
Include the real mid-generation DELETE-journal interruption and
candidate-configured refusal and guard coverage in validation.

The reviewer inspected local source and Git history. This design-only
increment ran whitespace and scope checks. Builds and native validation
belong to the implementation phase.
