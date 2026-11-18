# Terminal frame review

Claude Code reviewed the detailed frame design in session
`3059af0e-8a33-4eff-9533-07eac87d2d84` and approved its revision. The
completed-code review ran in auto permission mode with Sonnet in session
`5e5252db-d04c-44b2-8561-66fb54830006`.

The first code review found a receiver deadline bug. A newly unblocked head
could inherit the previous record's gap timer and fail before receiving its
own presentation opportunity. The revision tracks the deadline for the
current head or gap and preserves the independent original signed age for
each queued record. The tests exercise reordered traffic, slow consumption,
old signed frames, counter exhaustion, and valid activity-based lease renewal.

Claude approved the revised frame and delivery code and independently
reran the ten offline core tests. The branch includes the frame-specific
parts of the shared TLS fixture. The staging repeat runs the core selectors
and the WSS publication and Command exclusion tests without unrelated
in-progress fixture changes. Only the ten offline tests are in routine
interactive core CI. [Validation](../validation/terminal-frames.md)

Claude separately approved the phase-only snapshot in auto session
`7ac39b37-86cc-48a0-b27f-2613d66ea7c4`. The exact snapshot passed all ten
core cases again and both opt-in WSS cases in 33.96 seconds.
