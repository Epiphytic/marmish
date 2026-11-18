# Installed Terminal foreground review

The independent reviewer was Claude Code in Auto permission mode using Claude Opus 5.5. The scope covers the foreground Terminal CLI, cooked preparation and bounded Start, same-UID IPC, local socket framing, job control, and signal-mask preservation for ordinary and Terminal-initiated daemon launch. See the [phase design](../phases/terminal-installed-writer.md) and [foreground validation](../validation/terminal-installed-foreground.md).

| Round | Claude session | Disposition and changes |
| --- | --- | --- |
| Initial foreground | `480bc5bc-3da3-4621-a47d-fdd6836546ad` | Revise. Replaced the unconditional job stop with normal SIGTSTP handling, including orphaned process groups. Added paired Start deadlines, auxiliary DNS worker signal masking, and cooked-mode admission tests. |
| Intermediate isolated foreground | `aae09187-9b90-4b41-9275-7f17993b1c74` | Approved the intermediate scoped implementation. Subsequent edits distinguished a TTY hangup from IPC errors and removed a duplicate test attribute. |
| Final isolated foreground before daemon composition | `f6ebb69c-027f-4976-a986-dca8000f1be9` | Revise for production lint and fixture visibility. Independently passed all 54 local integration tests and three foreground unit checks. The source enabled the foreground while the native daemon users were absent. Test-only wire and socket I/O modules are guarded in the integrated source. |

The last reviewer also found that a real TTY hangup could replace the caught SIGHUP disposition with a restoration error. The foreground now attempts its bounded restoration and preserves the signal-derived exit code. A physical PTY fixture closes its master and checks exit 129. The integrated daemon review covers the final source and native name and fixture gates.

## Private WSS foreground fixture review

Claude Code Sonnet 5.5, Auto mode, reviewed the initial design at
`8a7de95843697aa542fe6ea22a7ef35928e13e71` in session
`01ead3bb-5057-4628-bfe2-60eb018d96bb`. The reviewer requested
distinct key and shell-output assertions, concurrent I/O drain through
child reap, exhaustive terminal restoration checks, and explicit limits
on the test's claims. The revised design `3e424bd` addressed all seven
corrections and was approved in Auto session
`a19272fd-c943-4e1e-9d39-5ee2504be876`.

Claude Code Sonnet 5.5 Auto approved exact source
`464300cc98d87940a8cc83e2446374c7e2e6edab` in session
`1ed12d02-1351-4fe8-8a32-2edc4e465090`. The reviewer checked
controlling TTY ownership, raw admission, synthesized output, child reap
and bounded concurrent drains, local terminal restoration, and the
shared startup path used by the quiet AUTH test. The reviewer checked
Linux and Darwin API usage by source inspection and ran no tests. The
fixture author performed the native checks in the validation report.
The raw-attach measurement ends at both foreground raw mode and the
presence of one tmux socket. Shell readiness is asserted separately.
