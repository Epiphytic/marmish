# Upstream MDK recovery refresh

Source `d091cc81125036a1bfcb04723d44e05c47ea9594` updates the ten MDK
entries in `Cargo.lock` from `b3cd2cb1` to `bdf9833e`, the upstream
`master` head checked during validation. The workspace manifest still
follows `master` and all other resolutions are identical. The upstream
change covers empty polling and route recovery status in `marmot-app`,
which is absent from Marmish's locked dependency graph.
[Upstream MDK commit](https://github.com/marmot-protocol/mdk/commit/bdf9833e340909f29c628f4fc2f9a3d6873f1cf5)

Claude Code Sonnet 5.5 in Auto mode approved the exact source in session
`cf00a1bb-35d0-4289-b29c-b4961797c34b`. The reviewer independently checked
the lockfile diff, upstream branch head and public compare, and linked
package names. The review was static. The following tests were run by
the integration author on Linux AMD64 with locked offline dependencies.

| Check | Result |
| --- | --- |
| Locked metadata and normal native binaries | Passed. Binary build 44.34 s |
| Native relay-qualification library cases | 15 passed in 10.19 s |
| Native controlling PTY qualification against private WSS | Passed in 0.59 s |
| Recipient Offer malformed-route cursor regression | Passed in 0.50 s |
| Strict installed-checks Clippy, all targets | Passed in 53.49 s |
| Native interactive writer and ordinary Command against private WSS | Passed in 70.39 s |

The writer fixture used real tmux, Linux PTYs, SQLCipher and separate
loopback TLS and IPC sockets. This run measured cold and warm attach at
8.198 s and 8.965 s, key echo at 161.508 ms and ordinary Command times
of 5.819 s before, 3.510 s during and 4.230 s after the attachment. Each
metric has one sample. Other compilation was active on the test host,
and the local relay does not represent internet latency.

The 36 test filters defined in `.github/workflows/interactive-core.yml`
were replayed locally using the workflow's script. All filters matched
passing tests. The 160 total executions completed in 169.21 s, including
local compilation. This was a local replay of the self-hosted workflow's
selection. macOS and Linux ARM64 were outside the run.
