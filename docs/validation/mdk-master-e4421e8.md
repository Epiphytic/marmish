# Upstream MDK master refresh

The feature branch refreshed its Cargo lock from `bdf9833e` to
`e4421e86a6c73f336b0154ee09c13826d8fafff5`. The exact upstream
`master` head was checked before and after the refresh. The old
revision is an ancestor of the new one. The upstream range changes
only `integrations/hermes/marmot/README.md`.

The Cargo manifests continue to track upstream `master`. The exact
lock commit `f458829` changes ten Git source revisions. A regular
Cargo update also selected six unrelated Windows dependency edges.
Those edits were removed. Offline locked metadata independently
resolved all ten MDK packages to the audited head on Linux AMD64.
The replay in `6aefd6c` preserves the entire lock patch and the
checked source tree.

Linux AMD64 strict all-target Clippy passed in stock and private
candidate modes in 37.07 and 32.20 seconds. The candidate check
used the explicit exact build commit. This refresh covers dependency
resolution and compilation. Earlier installed qualification keeps its
own recorded source revision.
