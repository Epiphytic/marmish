# Native zombie-group cleanup: offline evidence

The macOS lifetime helper now accepts a group SIGKILL EPERM only after its
unreaped direct-child leader and every enumerated group member pass the bounded,
two-snapshot zombie/native-identity proof described in
[the approved phase](../phases/native-lifetime-zombie-groups.md).
Apple's pinned XNU implementation is consistent with the previously observed
zombie-only EPERM; the exact runner kernel path remains unproven.

Local Linux checks after the review-driven fixture additions:

- Full Python discovery: 260 tests passed, six Darwin-only skips, 5.133 seconds.
- Actual proof-header portable C harness: 50 states, including the 256-member
  acceptance boundary and 257-slot truncation refusal, compiled with
  `-Wall -Wextra -Werror`.
- Cleanup portable C harness: 14 modes, including preserved child exit 7 after
  a proven EPERM and strict refusal diagnostics for uncertain/live groups.
- Python compilation and `git diff --check`: passed.

No keychain, provider or relay calls were made. Portable fake libproc declarations
establish neither the Darwin ABI nor zombie lookup/group enumeration behavior.
Native CI must pass the independent positive/negative group fixture and existing
72-byte bootstrap shim test before the broader Seatbelt prerequisite and live
transport checks can establish additional support. Failure remains closed.

The workflow's unconfined step invokes both native test names explicitly. A
workflow assertion checks the combined command and its position before the
broad prerequisite. Independent Sonnet design and completed-code reviews are
stored under `docs/reviews/native-lifetime-zombie-*`.

Actual native followup: [run 40983389092](native-run-40983389092.json), at
`84882c7`, passed both targeted macOS tests in 0.637 seconds, including the
positive/negative group proof and original exact bootstrap handoff. The same
job passed explicit-Keychain provider tests, then stopped in the separate
`storage_diskutil` prerequisite with subprocess exit 1. Broader confinement and
default-keychain frontend acceptance are still pending.
