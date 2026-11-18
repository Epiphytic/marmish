# MDK master and native SQLite migration 214

At Linux AMD64 source `997ee5991a7b0a4053723556ac0ec0511f9644e5`, the
workspace follows upstream MDK master and resolves the lock to
`b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1`. All ten MDK entries advanced
together. The remaining lock bytes are unchanged from the parent commit.
A same-source rebuild stamped the updated MDK revision into the executable
build. The change adds two upstream tables for device-link chat progress.
[Upstream change](https://github.com/marmot-protocol/mdk/commit/b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1)
[Reviewed Marmish plan](../phases/mdk-master-schema-214.md)

The read-only startup check now recognizes schema 214. A real closed-owner
SQLCipher WAL fixture asserted the account had exactly that migration, inserted
215 into its test database, and verified fail-closed refusal with the
previously existing database and sidecar inventory intact. All five native
preflight cases passed in 11.39 seconds. Scoped Rust formatting,
Git whitespace checks, and locked offline Cargo metadata passed. Strict
all-target native Clippy passed in 39.31 seconds. A local replay of the 35
self-hosted CI core selectors passed all 156 test executions in 75.38 seconds.

The same-build private WSS fixture passed real Terminal input, detach and
reattachment and ordinary Command before, during and after Terminal in 69.82
seconds. Individual samples were 7.442 s cold readiness, 8.343 s warm
readiness, and 135.228 ms from one key to echo. The three ordinary Command
turns took 5.338 s, 4.028 s and 3.816 s, respectively. These are single
local samples. Installer, external authenticated relays, ARM64 and macOS
require distinct evidence.

Claude Code Sonnet 5.5 in Auto mode approved the exact source in session
`4ceabab0-6f7d-4bf6-9206-83bbabd5d323`. It independently checked revision
coherence, compatibility logic, test changes and scoped formatting.
Workspace formatting still reports an existing difference in unchanged
`native_setup.rs`. Runtime validation finished after the review.
