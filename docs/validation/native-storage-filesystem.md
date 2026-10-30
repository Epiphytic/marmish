# Native storage gate offline validation

Increment marmish-3cq replaces the broad prerequisite's diskutil dependency with
an open-descriptor system fstatfs check. Exact local APFS and private directory
metadata remain required. All existing durability operations remain in place.

Independent Claude Sonnet auto-mode code review approved the integrated C helper,
Python adapter, fixtures and workflow with no blockers. Evidence is in
[the review](../reviews/native-storage-code-initial.json); the reviewed detailed
contract is [the phase plan](../phases/native-storage-filesystem.md).

Offline validation on Linux:

- Focused spike suite: 14 tests, one Darwin-only skip; repeated after formatting.
- Full Python discovery: 262 tests, seven Darwin-only skips; independently
  repeated by the reviewer.
- Actual probe C source compiled with strict warnings and synthetic syscall and
  mount-header substitutions: all 19 closed-result and cleanup cases passed.
- Python compilation and git diff whitespace checks passed.
- Quality-delta findings and bounded dispositions are recorded in the phase plan;
  no metric suppression or baseline was introduced.

No native/provider/relay calls were made for this increment. Native acceptance is
pending: the pre-spike workflow must pass the actual system-header helper's APFS,
metadata and open-error fixture, followed by the full retained durability gate.
The synthetic header test does not establish Darwin ABI compatibility. The exact
reason for the historical diskutil exit 1 remains unproven.
