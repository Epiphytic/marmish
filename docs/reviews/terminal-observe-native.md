# Native Terminal observation groundwork review

The reviewed change implements the slot decision table and links each native
observer PTY to the exact writer attachment. The independent reviewer used
Claude Code in Auto permission mode and could inspect the frozen source and
run local tests. The [approved design](../phases/terminal-observe-takeover.md)
covers the full installed Observe and Takeover phase.

| Round | Claude session | Disposition |
| --- | --- | --- |
| Design | `59a808da-a13f-48d5-988f-d17e0b3ebfb2` | Approve the final design after three revisions. |
| Code 1 | `bfe03d80-abb4-4acf-b863-5e8ceaa4bc0d` | Revise. Temporary writer or observer authority contention could retire an observer during resize. The reviewer ran 17 deterministic checks. |
| Code 2 | `ffec540c-61a2-4652-8a06-bcf5e3bf75b0` | Approve the scoped baseline. The reviewer reran 17 deterministic checks and noted that bounded catch-up could keep observers attached through brief contention. |
| Code 3 | `59f37535-2f8f-4411-ab15-647ec89ba7d5` | Approve the final native increment. Claude Sonnet ran all 19 deterministic checks and checked the non-test library build, which reported unused code for routes awaiting installation. |

The subsequent code revision pauses observer output while the mirror catches
up within an absolute 250 ms deadline. A newer size cannot extend that
deadline. The author ran 19 deterministic checks covering successful catch-up,
deadline expiry, and withholding queued output while geometry is stale.

The installed owner retries a temporarily withheld geometry hint only within
its existing deadline and refuses an unavailable hint. The foreground may
receive previously queued bytes followed by tmux's resize redraw. It cannot
infer the applicable geometry of individual output records. Qualification
still needs verified tmux client flags and PID, the confirmed exact writer
link, server egress checks and real tmux geometry after abrupt writer loss.

The [validation notes](../validation/terminal-observe-native.md) list the
commands and the integrated scenarios still to qualify.
