# Terminal relay physical cut code review

Claude Code, Sonnet 5.5, ran in Auto mode under session
`ed52f232-5acd-4cb1-92e4-4da033dbb116`. The initial review of `5e512f2`
requested changes. The issued-attempt wait needed to preserve its eager deadline
check. The deactivation test also needed to observe a real wake and cancellation
of the pending future. `10091e5` added both. The later compile fix replaced a
missing counter type with a one-bit wake witness and retained the first-poll,
wake and drop assertions.

The first complete review approved exact source
`1a6e6da2e401d58b3aab5741bfe5fe9cfecfc429` with no blocking findings. The
reviewer inspected the complete implementation delta from the approved design,
including the separate signed Bootstrap and Room subscription scopes, the
native Room and Owner admission guards, and the wake witness. The reviewer
checked the final delta for whitespace errors. Exact-source runtime results were
collected afterward by the author and are recorded below.

A supplemental debug stock test exposed stack-size pressure in the large
Terminal fixture. `91061a44e94c442191f4a6a54e8be34b7cefd61e` pins the pending
operation once and passes a borrow to the Fatal selector. The same Claude
session approved the exact successor. The reviewer verified the scope,
pre-poll cut, enclosing future destruction and unaffected raw transport fence.
The reviewer checked formatting and whitespace. The author ran the exact
focused test binary again.

The review suggested recording stock failed-bootstrap teardown, clarifying the
protocol CLOSE observation, and adding a direct Owner refusal fixture if that
startup path gains a candidate caller. The first two observations are recorded
in validation. This increment exercises the native Room refusal directly and
uses inspected control flow for the earlier Owner guard.

[Physical cut design](../phases/peer-candidate-terminal-fatal-stop.md)
· [Validation](../validation/peer-candidate-terminal-relay-fatal.md)
