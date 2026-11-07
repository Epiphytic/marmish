# Large local helper and component migration tests

These are test-only additions, independently reviewed with actual Claude Code
Sonnet 5.5 in `--permission-mode auto`. They do not change product behavior.

The large helper design needed three rounds: first reject optimistic output
offsets as codec acknowledgement evidence; then reject double-polled join
handles and task abort without child reaping; finally approve the acknowledged
codec ledger and cooperative cancellation/drain design (round 3, 15.613 s).
The code review requested bounded cleanup after aggregate expiry, preservation
of the first direction/codec error, and poison-safe metric inspection. The
revised code was approved in 20.697 s, three turns, session
`7755f2df-3daf-4c9c-a1a1-400716e89946`.

The migration design's second round approved the genuine released-format-6
component fixture in 12.328 s. Code was approved in 27.155 s, five turns,
session `d6b5fa4a-47c3-4013-9eb8-a3b38b22b0da`. It compares complete serialized
Exec records and SQL columns, checks fresh copy schema/replay floor, and
verifies row/schema/version rollback after a malformed second record.
This is in-memory `copy_init(6)` qualification from historical DDL, not a full
SQLCipher `Journal::open` or installed old-binary upgrade.

Author execution on Linux AMD64: focused large-fixture tests **3 passed,
one expected ignored**, 0.01 s; migration component tests **2 passed**, 0.00 s;
strict native all-target Clippy **passed**, 12.68 s. The independently reviewed
66 MiB-plus-17-byte four-way test is explicitly ignored and runs separately;
its long execution is pending and no large helper throughput/result is claimed
by these focused passes.

An immutable test executable started before the failure-path refinements has
SHA256 `8077deb8de9a32329edf592972d7746320728456bc5598d347d11c6add5e32e9`.
Any eventual result from it must retain that exact provenance and distinguish
the separately tested failure-only changes from the unchanged transfer path.
