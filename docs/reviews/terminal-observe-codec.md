# Terminal observation wire and one-use offers

The detailed design is [Terminal observation and takeover](../phases/terminal-observe-takeover.md).
Claude Code Sonnet reviewed the exact files in Auto permission mode.

| Round | Session | Finding and disposition |
| --- | --- | --- |
| 1 | `14310d3e-d037-4e6a-9aa9-c059d39fb95f` | Revise. Reject the predecessor claim on all Accepted offer legs, avoid Debug formatting for bindings and transcripts, and pin the byte layout. |
| 2 | `9f0c2311-a0ee-43b4-a953-9cc5d2240e23` | Approve. The owner must prune retired writers before minting and use the continuous clock. Suggested more negative tests and golden encodings. |
| 3 | `752fd3ca-861f-4d7b-bc3b-513d677cf19c` | Approve exact final artifacts with the additional encoding and parser tests. Read-only review with hash verification. |

The Linux x86_64 author run passed three codec tests and two ledger tests.
The vectors pin request target and choice, exact slot and binding, role, and
big-endian geometry and generation. The negative paths cover stale and
replayed offers, a repeated old claim, expiry, malformed and truncated
records, invalid direction and both global and per-client capacity.

The codec and ledger are mounted for tests. Production selection still uses
the existing protocol until native owner and foreground integration pass
their own review and end-to-end checks.
