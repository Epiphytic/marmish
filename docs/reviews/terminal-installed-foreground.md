# Installed Terminal foreground review

The independent reviewer was Claude Code in Auto permission mode using Claude Opus 5.5. The scope covers the foreground Terminal CLI, cooked preparation and bounded Start, same-UID IPC, local socket framing, job control, and signal-mask preservation for ordinary and Terminal-initiated daemon launch. See the [phase design](../phases/terminal-installed-writer.md) and [foreground validation](../validation/terminal-installed-foreground.md).

| Round | Claude session | Disposition and changes |
| --- | --- | --- |
| Initial foreground | `480bc5bc-3da3-4621-a47d-fdd6836546ad` | Revise. Replaced the unconditional job stop with normal SIGTSTP handling, including orphaned process groups. Added paired Start deadlines, auxiliary DNS worker signal masking, and cooked-mode admission tests. |
| Intermediate isolated foreground | `aae09187-9b90-4b41-9275-7f17993b1c74` | Approved the intermediate scoped implementation. Subsequent edits distinguished a TTY hangup from IPC errors and removed a duplicate test attribute. |
| Final isolated foreground before daemon composition | `f6ebb69c-027f-4976-a986-dca8000f1be9` | Revise for production lint and fixture visibility. Independently passed all 54 local integration tests and three foreground unit checks. The source enabled the foreground while the native daemon users were absent. Test-only wire and socket I/O modules are guarded in the integrated source. |

The last reviewer also found that a real TTY hangup could replace the caught SIGHUP disposition with a restoration error. The foreground now attempts its bounded restoration and preserves the signal-derived exit code. A physical PTY fixture closes its master and checks exit 129. The integrated daemon review covers the final source and native name and fixture gates.
