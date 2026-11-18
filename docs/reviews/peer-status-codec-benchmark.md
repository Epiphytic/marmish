# Peer status codec benchmark review

Claude Code Sonnet 5.5 in Auto mode initially withheld approval in session
`6d81a9e0-4f75-4499-9058-81c9f4b341e9`. The saved log lacked stderr, which
contained the measured values. The rerun captured stdout and stderr, the
benchmark arguments, toolchain, debug binary digest and source comparison.
The report also records the percentile convention for short samples.

Claude approved the corrected report in session
`9f6f6afc-47e6-4b77-9c4a-dafc5fd4976d`. The review checked all five rows
against the combined log, matched the benchmark binary digest, verified
that the crate sources and Cargo inputs equal `95ce768`, and confirmed
the measured path excludes all live transport and persistence. The
reviewer made no edits and ran no additional benchmark.

[Benchmark results and reproduction](../validation/peer-status-codec-benchmark.md)
