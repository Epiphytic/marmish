# Candidate server inventory CLI review

The [approved mount design](../phases/peer-candidate-daemon-mount.md) defines
these candidate-only List and Show commands. Claude Code 2.1.301 in Auto mode
with `claude-sonnet-5-5` reviewed the exact native CLI, selector and IPC client
delta from `08ca5d7`.

- R1 `611922d3-999a-43ca-b1ce-146584b8bccd` approved source `1b37232`. It checked
  candidate-only client registration, explicit remote parsing, the shared
  exact selector and early, local-only IPC dispatch. It suggested comparing
  original argv parts directly, checking an explicit npub relay destination
  and testing flag-looking local values. The source follow-up adds those
  tests without changing runtime behavior.
- R2 `6b5a1838-a0c7-47cc-8f91-c1041397ab92` approved the complete source through
  `f9c6f572e92a5de7581d4673e7e2244832212739`. Its static review again checked
  candidate isolation, exact remote precedence, closed selectors and one
  typed IPC request. It found no material issues. Its remaining test caveat
  is covered by the [exact author runs](../validation/peer-candidate-read-cli.md).
  The existing keychain argument check is irrelevant to these commands.

The reviews did not run Cargo while shared test targets were in coordinated use.

## Composed review

Independent Claude Code Auto (Sonnet 5.5, session
`dddded29-16b4-4660-900d-58130119f343`) approved exact composed
`cfd68df` against `08ca5d7`. It confirmed that the crate diff against
author source `f9c6f57` is empty and traced candidate registration,
explicit remote parsing, typed selector and local credential-checked
dispatch. This review was static. The author ran the separate stock
64-test regression on the exact composed checkout.
