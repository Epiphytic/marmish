# Terminal Command authority handoff reviews

Claude Sonnet 5.5 reviewed this increment in Claude Code auto mode.

| Scope | Session | Decision |
| --- | --- | --- |
| Design R2 | `28aa00f3-b1d2-4224-a85b-bd7b6b9e2be2` | Approved the test-gated actor authority handoff and mutation contract. |
| Code R1 | `6a682461-49e3-4466-a8f6-cd48bc1fe351` | Requested shared Command semantics preservation, fail-closed stale authority, actor-level tests and a native receiver-side oracle. |
| Code R2 | `d54d1420-128e-4772-b5cc-8b98dd17e002` | Approved exact source after the corrections. Independently passed four deterministic tests, the real WSS fixture and strict all-target Clippy. |

The frozen code gates the new route checks and all fence mutations to test
builds. A locally stale binding retires before any next effect. A wrong
route supplied by a new caller refuses that caller while preserving an
already verified binding. The WSS fixture isolates actual received native
effects from the general actor tick and confirms both membership removal
and protected metadata changes. It also verifies retained ordinary Command
operation on a distinct room.

The reviewer initially reproduced the Command fixture's worker installation
error, including in an older regression. Building all native binaries
installed the expected sibling layout and both the installed-worker probe
and the new WSS case passed. The validation report includes the exact
reproduction prerequisite. The reviewer also noted that the granted binding
survival check exercises actual actor ticks in the WSS fixture, while the
deterministic pruning test covers only the absent-grant case.
