# Capped Pending refusal code review

Claude Code Auto, model `claude-sonnet-5-5`, approved exact implementation
`0bf8107e825d39795676e097bd304e604bc480bb` in session
`fcb8d301-31f8-4a33-a3b2-1d183de47061`. It checked the compiled source, the
complete private WSS outputs, and the typed refusal assertions that the
fixture leaves out of logs.

The first source review, session `70dead0c-8978-44e9-a892-d04d388d60cc`,
requested changes after a compile failure. The author corrected the inventory
module path and recorded the policy classification before moving it. Session
`43c17ae5-4e5c-4c95-a76b-27cd9868fb45` independently reran the exact signed
store selector (passed in 0.81 s) and approved pending completion of both WSS
selectors. The final session verified both selectors passed and confirmed all
outstanding conditions.

The reviewer noted two nonblocking limits. The live foreign-lease refusal
measures the typed `Path` error and zero Welcome or custody effects. It does
not instrument quota registration. The uncertainty hook sits in the join
routine of `receiver_inventory`, which is currently compiled only for tests on
Linux AMD64. Any future production mount of that module must keep the hook
test-only.

See [author validation](../validation/peer-installed-capped-pending-refusals.md)
for the test selectors, times, and evidence limits.

## Integrated review

Claude Code Auto, model `claude-sonnet-5-5`, reviewed exact composed source
`f7bb53fcce407cb7d1ca407cd9fb8e7d61811c29` in session
`6664ccf4-7640-4553-8b13-26de2f5449bb`. It approved contingent on the
specified executable gates and a documentation update. The exact WSS cuts,
signed custody reopen, managed journal and capped Welcome, receiver
inventory, default and candidate compiler gates, and cursor target passed.
The validation includes the measured effect tuples and the narrower claims
for expiry, retained evidence and uncertainty.
[Integrated validation](../validation/peer-installed-capped-pending-refusals.md)

The reviewer checked that the changed dispatcher functions and counters are
test-gated, the original snapshot path is unchanged, and the registration
and daemon files match the public base. The review and executable evidence
qualify the test bridge. The installed worker and application activation
remain separate.
