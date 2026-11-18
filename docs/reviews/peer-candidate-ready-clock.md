# Candidate startup clock review

Claude Code Auto approved design `2cea9fb` in session
`a8d71ed3-be89-43a4-999c-00e98a134442`. It confirmed that readiness
uses the existing shared watch and that elapsed time is taken from
the second edge. The approved clarification separates the process
monotonic clock from signed wall-time checks.

Claude Code Auto approved source `84068b9` in session
`f78ea5e3-b9fd-42f6-b818-38a3bc1e8dc5`. The static review inspected
both edge orders, the stopped state, preservation of the original
instant through duplicates, and strict refusal at the 15-second
boundary. Runtime evidence is recorded separately.
