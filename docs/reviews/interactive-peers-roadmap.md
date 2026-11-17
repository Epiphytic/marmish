# Interactive terminal and peer operations roadmap review

Claude Code, auto mode, `claude-sonnet-5-5`, session
`1b234abc-80d6-4155-bc67-608490d18a35`, 2026-11-17.

Claude inspected the roadmap against the native room authority, command policy,
route mode and journal contracts. Its first verdict was approval with four
required changes. The revised roadmap received an explicit approval with no
remaining architectural blockers at roadmap scope.

| Required change | Incorporated decision |
| --- | --- |
| Handle unapproved incoming Welcomes | Give unknown invite work a separate budget. Resolve unjoined versus quarantined intake against the MDK contract before coding. Preserve separate Peer and execution trust. |
| Specify shell authority and revocation | Use a default-off per-key grant, a private checked tmux socket, a minimal environment and immediate fencing, detach and owned-session termination on revocation. |
| Resolve terminal payload durability | Prove a route without durable application payload storage before the lossless PTY implementation. Retain authenticated MLS state and replay protection. Document relay retention separately. |
| Keep discovery outside authorization | Require explicit confirmation of a complete candidate npub, pin the key, keep lookups off the admission path and fail closed on a required Tor route. |

The revised roadmap also records a monotonic lease clock, restricts IP
disclosure on Tor routes, preserves existing regression gates, and requires
numeric resource and latency goals in the phase designs. The author incorporated
the follow-up suggestions to show the MDK proof as a dependency, stay on
upstream MDK and credit Claude Chill as a candidate for evaluation. The
review checked documentation and source contracts. No runtime tests or
CI dispatches were requested or performed for this documentation increment.
