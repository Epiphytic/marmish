# Observe and Takeover local foreground review

The [approved design](../phases/terminal-observe-takeover.md) defines the
installed Observe and Takeover phase. This code review covered the local
CLI, the same-UID socket admission protocol, the controlling terminal
and their tests. Claude Code Sonnet used Auto permission mode to inspect
the frozen snapshots and run local tests. The separately owned live
admission and selection transport require an integrated review.

| Round | Claude session | Disposition |
| --- | --- | --- |
| Design R4 | `59a808da-a13f-48d5-988f-d17e0b3ebfb2` | Approve the full design. |
| Code R1 | `2814bca3-8a35-48f1-b8b0-daa0823c86bd` | Approve. Claude ran 31 socket, two foreground, three CLI and 60 native local integration tests. The reviewer identified a missing diagnostic after suspending an offered New and requested more real PTY cases. |
| Code R2 | `cf378796-4b11-4d11-82d3-1c043a39f882` | Approve the final revision. Claude ran 16 unit and 61 native local integration tests. |

The final revision reports `terminal_new_reattach_unavailable` when an
offered New is suspended before the slot is known. It closes that socket
and sends no second New on continuation. Added PTY coverage exercises
suspend during the cooked prompt and during the offered New leg, Ctrl-D
cancellation, Ready with typeahead flushing for explicit Takeover, and a
definitive stale-offer refusal distinct from a lost reply. A local
prompt cancellation returns success and opens no second socket. The
strict client decoder rejects Busy on an explicit New request.

The [validation notes](../validation/terminal-observe-foreground.md)
record author checks and local admission timings.
