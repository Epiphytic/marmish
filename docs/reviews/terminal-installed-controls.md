# Native Terminal CLI controls review

Claude Code Sonnet 5.5 independently reviewed the following exact versions
with Auto permissions.

| Review | Source | Session | Verdict |
| --- | --- | --- | --- |
| Design R1 | `67ccf90` | `5b7e2cd8-6757-4795-a5bf-69c843ae73c2` | Request changes |
| Design R2 | `3975b6f` | `f530ef9d-2888-42b2-8efd-8bb1fc0e0fe4` | Approve |
| Initial test code | `014d754` | `f6944ef0-b91e-4f5e-9418-091c03ac5fb3` | Approve |
| Integrated test code | `8fa94ff` | `364eff0b-f397-440d-b3b1-89eed0f087e5` | Approve |

R2 specified checked geometry and a single retry budget, atomic tilde-dot
after the state marker, distinct cooked Busy failure, one exclusive config
and two independent PTY captures, plus an unchanged socket identity and a
fresh resume marker. The exact code reviews confirmed these checkpoints,
lease-independent owned child cleanup, the separate quiet path and the
absence of product or CI changes. One code review ran a scoped formatting
check. Neither code review ran the author's live test sessions.

A quality scan found two pairs of duplicated test helpers in the initial
source. The integrated version shares the write loop and timing reporter.
The read and write helpers remain separate because they use different
readiness APIs on the same owned PTY. The remaining static flags include
that similar structure, the opt-in test selector and recent edits to the
shared fixture. Strict Clippy and the [execution evidence](../validation/terminal-installed-controls.md)
cover the integrated test itself.
