# Registered candidate listener review

Claude Code Sonnet Auto approved the [design](../phases/peer-candidate-ipc-fatal.md)
in session `ef56abaa-dc4d-42fe-b5da-ca458b08c8e5`. The independent exact code
review approved `8dea700` in session `82415022-3ff2-40be-89d1-3901d2e07684`.
The reviewer independently ran all stock and candidate IPC tests, the eight
Pending reader cases, and eight runs of the five focused fatal-listener tests.

The code review traced the cutoff through generic frame I/O and each raw
UnixStream write, including an already admitted length prefix and a withheld
body. It also confirmed that dropping the listener revokes the read view
before joining request tasks and that an already transferred Terminal socket
remains with its consumer. The full daemon must independently guard that
consumer. The real accept-retry error remains an unforced branch here.
An admitted owner operation can complete before its response loses authority.
The installed composition must report that distinction as an uncertain outcome.

See [validation](../validation/peer-candidate-ipc-fatal.md) for the author runs
and exact test scope.

Claude Code Auto session `b7f52005-a452-4105-81ce-759b2f2734f9` approved
the exact composition `0cab7eb` with the reviewed recipient and Command
startup code. It independently confirmed byte-identical IPC sources and
repeated both archived IPC suites. Strict all-targets engine/native Clippy
passed on the composed stock and candidate trees. The production daemon
continues to use the default listener entry.
