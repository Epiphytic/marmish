# Terminal redraw feasibility design review

Claude Code reviewed the standalone feasibility design for Beads
`marmish-dwt.2` in Auto permission mode on 2026-11-18.
The reviewer model was `claude-opus-5-5`. The review compared only the
design document to repository base `dbe3605` and inspected the existing
output policy, batching, frame encoder, tmux fixture and core CI selectors.

| Round | Exact design commit | Claude Code session | Verdict |
| --- | --- | --- | --- |
| 1 | `cad6561c43bb538592f4abfeeafc1c924951bfe3` | `9410ffc0-1263-4e43-8e2e-6c06914342ef` | Request changes |
| 2 | `cd14a0698b1db7daf57e7f25133d30eab31a5b33` | `0be3956f-8cd7-4658-bf96-ce34161d1dd1` | Approved |

The first review required an immediately preceding checkpoint as the
comparison base, a release rule tied to when the comparison can actually
be computed, and a concrete path from the private library replay to a
tmux pane. The approved revision advances the expected base after
typed fallback, feeds comparison bytes only at the final consumed
source byte and uses unmodified batching deadlines. The replay and
the independent tmux oracle run in the same test binary, without
exposing the batcher or frame encoder. Round 2 confirmed all three
findings were resolved.

For implementation, use the consumed count when re-pushing partition
remainders. Keep ordinary echo in the baseline-only cohort and let
scrolling end the comparison window. Batch timing often rules out the
comparison when the last raw byte arrives after the first complete
display unit. A null timed result is expected in that case. The
deterministic smoke selector is
`terminal_output_batches::terminal_redraw_evaluation_tests::terminal_redraw_evaluation_core`.
Core CI wiring, if selected in the implementation review, requires an
explicit entry in `.github/workflows/interactive-core.yml`. The tmux
oracle stays opt-in and uses an exact fully qualified child selector.

`git diff --check dbe3605..cd14a06` passed. No implementation, build,
test execution, installed measurements or relay traffic were part of
this design review. The benchmark measures replay app-frame counts
and bytes. An installed or signed-relay comparison requires a later review.
[Approved design](../phases/terminal-redraw-evaluation.md)
