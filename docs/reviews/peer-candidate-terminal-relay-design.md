# Terminal relay physical cut design review

Claude Code ran in Auto mode with repository inspection. Session
`699445c8-717a-4bf4-b716-2b19a3465dda` reviewed R1 `1f551a0` and
re-reviewed R2 `cbc5ac988f9269f70b7d2001844735bc9dfa09fd`.
R1 requested changes to define a buildable first cut and correct the
volatile Room open, parking and close assumptions. R2 approved the
first real Terminal Lane transport change. The existing volatile
and persistent SQLite owner gaps still gate any candidate mount.

The implementation must hold the raw slot structurally with an
active Fatal, exercise Command-source authority, deny a fenced route
before native Room startup, and prove startup custody before making
the cut. The exact two-half proof uses fixture-owned real network
halves. The active SDK Lane proof separately checks relay EOF and
actual reaper completion. The alias watcher and detached alias
remain outside this aperture. Verify clean reaper state after closure
and compile both stock and candidate with strict lint.

The direct Terminal bootstrap configures the connector but does not
dial. Hold the real slot across SDK activation to exercise pending
dial. Early custody before bootstrap is a separate setup guarantee.
