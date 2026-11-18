# Private recipient daemon mount design review

Claude Code 2.1.301 (Sonnet 5.5) reviewed the
[daemon mount design](../phases/peer-candidate-daemon-mount.md) in Auto.
Both rounds were design-only against `b05cb51` and the exact prior
packaging source. No product source, builds, daemon runs or relay
connections were part of this review.

| Round | Reviewed design | Session | Verdict |
| --- | --- | --- | --- |
| R1 | `c84f217` | `d89ad819-7a84-42de-a235-942e2b5c58e3` | Request changes |
| R2 | `af0dc2c` | `86d6d0ac-b9cb-4fb9-8817-da745f2ecdc9` | Approve |

R1 required exact candidate-only link gates, a distinct read wire variant,
an explicit Ready/mutable lease split, and an independently maintained
fresh Pending projection. R2 confirmed the revisions, existing listener
registration and shutdown ordering, preflight/default VFS behavior and the
bounded install matrix. Positive installed List/Show and restart remain
dependent on the real charged Held-to-Pending bridge. Ordinary same-PID
Command verification requires an existing authorized stock-trusted route;
the private Peer fixture CA supplies separate trust.

R2 recorded two implementation clarifications. The first binding uses
`ReceiverQuota::bind_receiver` semantics and writes the receiver's Weak
reference and snapshot family into `ReservedPaths` before the first
native open. Rebinding through `Dispatch::bind` cannot repair an initial
reservation missing that witness. The listener may use a mode enum or an
equivalent typed demultiplexer for candidate and fixture pairs, with one
shared accept/credential/framing loop and separate wire/worker types.
Neither clarification changes the design verdict.
