# Rsync live qualification harness review

Scope: `tools/marmish_rsync_runtime.py`, its focused tests, the shared runtime
harness's factory/case hook, and real local rsync capture tooling. This is
qualification tooling, not a Python product dependency. Approved product design:
[rsync adapter](../phases/streaming-rsync.md).

## Round 1

Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, read-only review.
**REQUEST_CHANGES**, 116.659 seconds, 7 turns. Reviewer ran 22 focused Python
harness tests successfully, help, and local rsync version inspection; no builds,
native commands or relay traffic. Root's broader 40-test discovery included
other runtime tools and was a different selection.

Findings incorporated:

- Negative tests now require exact exit 255, empty stdout and the expected
  category. A stream-ID diagnostic distinguishes pre-Open bootstrap rejection
  from a remote missing executable. Generic nonzero exit is insufficient.
- Rsync stats distinguish literal from matched bytes. An unchanged transfer must
  send zero literal file data, a delta must have matched data and substantially
  less literal data, and restart requires an actual partial file plus matched
  bytes. Final tree equality alone cannot prove delta or partial reuse.
- Binary fixtures stay larger than the stream window even with a small requested
  byte count. Reports identify same-host partial qualification and enumerate
  uncovered acceptance cases; no latency result is inferred.
- Directory traversal errors fail instead of silently hiding unreadable subtrees.
  Tree-size throughput is named accordingly, not represented as network traffic.
- Cancellation uses a bandwidth limit and waits for received file bytes. Outer
  rsync exit and adapter confirmation are observed; direct adapter exit status is
  explicitly not established by that case.
- Version/hash checks precede daemon startup. Tests cover the custom managed main
  hook, transfer stats and mismatch rejection.

After revisions, root's broader runtime-tool selection passed 42 tests in 1.131
seconds. The new local capture suite independently passed 8 tests using rsync
3.2.7; see [capture evidence](../validation/rsync-local-captures.md).
Round 2 independent review and actual relay execution are in progress.

## Round 2

Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, read-only:
**APPROVE**, 138.503 seconds, 17 turns. Ran 42 broader runtime-tool tests and
32 focused stream/rsync/capture tests, all passing; the latter includes the 8
real local rsync captures. Also ran help, syntax checks and a disposable local
rsync stats probe. No Cargo, native Marmish or network test was run by reviewer.
All round-1 blockers were resolved. Optional notes concern more hermetic negative
and cancel tests, consistent tooling version floors, top-level requested versus
clamped payload metadata, and asserting fresh stream IDs during partial restart.
The report's uncovered cases remain explicit. Root's separate actual relay run passed on the final reviewed native snapshot;
see [actual evidence](../validation/rsync-runtime-live.md). This result is separate
from the harness approval.
