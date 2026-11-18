# Local Terminal supervisor review

Claude Code Sonnet in auto mode returned **APPROVED** in its third design
review in session `f59166c8-a420-42e8-91bb-d70a5cfd8071`. The frozen
[design](../phases/terminal-supervisor.md) covered the independent SQLCipher
registry, exact tmux session ownership, writer and read-only observer claims,
and recovery under the existing exclusive profile lease. The review inspected
the native dependency and ownership contracts and relied on the documented
local tmux spike for executable behavior.

## Completed-code review

Claude Code Sonnet auto session `13238ae6-c91d-4fe3-b42f-bbdc38a12703`
returned **REVISE** and then **APPROVED**. It reviewed the test-gated native
supervisor, management and journal helpers, worker role and pathname checks,
native process tests, and validation. The first round independently passed
two core and three then-existing native tests. It found that single-sample
tmux client counts could refuse a valid attachment under contention and
that several planned assertions lacked evidence.

- The corrected version waits for the exact client count under one shared
  one-second deadline across tmux queries and polling. It allows only the
  transient count direction expected during registration or departure. A
  failed registration retires the newly started attachment.
- Expanded tests cover a different account presenting the exact claim,
  foreign socket inode rejection, external writer exit and the three-observer
  cap, 16 quarantined slots before allocation, ambiguous retirement, exact
  worker role and fd inventory, existing store isolation, distinct HKDF
  domains, and a flushed uncommitted SQLCipher transaction interrupted by
  process death.
- The second round inspected the corrected wait, coverage and test list,
  and independently read the exact single-core contention log through all
  20 successful trials. It did not re-run the native suites. The
  [validation report](../validation/terminal-supervisor.md) records the agent's
  separate suite and benchmark runs.

The approval covers local native Linux x86_64 validation with tmux 3.4.
Draining trailing readable PTY bytes, releasing ambiguous retirement rows,
native macOS and ARM64 execution, and authenticated live routing are outside
this increment's validation.
