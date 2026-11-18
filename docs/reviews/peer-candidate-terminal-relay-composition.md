# Terminal physical relay cut composition

The Terminal relay increment at `8d5e9ae` was applied to published
`dd823d18`. The exact composed source is
`825ed5649219b394009e8e5d96b6fc07104ed37c`. Both deltas have stable patch ID
`206fd45cecf7b8f8393c59a4d60f2e208799faa1`.

Claude Code Auto, Sonnet 5.5, session
`4358fde2-5cea-47db-b109-cc1711bde44f`, approved the exact composition subject
to its validation checklist. The reviewer inspected the shared change to how
raw TLS halves retain the slot, confirmed the stock path awaits its existing
operations, and confirmed the guarded Terminal lane stays unmounted. The
reviewer identified the existing direct test of guarded Room refusal and
verified the Owner refusal by inspection. The stock failed-bootstrap path now
retires the authority and includes the cleanup in the counted lane reaper.

Offline locked Linux AMD64 builds produced the stock debug libtest SHA-256
`3ae9f5d075181a154d5ac34a7afb92510a2745a3ebb5cb1fab41d1b3f45d5ce5` and the worker
SHA-256 `195c62b164f7ac1b05f0149cd16b59047b8d446fc391f013d9dbd9360a2bc9aa`.
Fmt, diff whitespace, and strict stock all-targets Clippy passed.

| Libtest selector | Result |
| --- | --- |
| `terminal_fatal --include-ignored` | 5 passed |
| `dispatcher_fatal::tests` | 3 passed |
| `transport::tests` | 18 passed, including `fatal_drains_each_exact_route_slot` |
| `mdk_adapter::terminal_enrollment` | 15 passed, 2 opt-in cases counted above |
| `mdk_adapter::terminal_live_enrollment` | 9 passed |
| `tests::registered_route::peer_registered_ --ignored` | 2 passed in 76.39 s |
| `raw_fatal::peer_registered_raw_fatal_private_wss --ignored` | 1 passed in 85.07 s, 14 scenarios |

Each test command used one libtest thread. Network fixtures used disposable
identities and private loopback WSS. The raw tests exercise the physical
post-cut poll fence. WSS tests observe EOF at the exact sockets; bytes already
forwarded to the kernel may still arrive. Further Terminal work covers owner
and supervisor accounting, native SQLCipher custody, daemon stop, and tmux
lifecycle.

The first echo/resize run failed before Terminal enrollment because the
shadow debug layout omitted the sibling `marmish` executable. The worker
reported `worker_installation` when probed directly. The affected Command
fixture reported `not_started`. No source edit was needed for that layout
diagnosis.

With both source-matched launchers and the worker in place, Command and
Terminal enrollment completed, then the debug fixture exhausted its explicit
32 MiB test stack during native Group ingest. A GDB backtrace localized the
failure to the same current-thread runtime that runs this fixture. Raising
`RUST_MIN_STACK` left that explicit stack size unchanged and reproduced the
failure. Follow-up
`9da2b47cafa4d9766c81c312010b67efb9dd9832` increased only that opt-in fixture
stack to 64 MiB. Claude Code Auto, Sonnet 5.5, session
`7aa277c9-4f2d-4c58-b7af-610eed86ea77`, reviewed the exact single-file change. The reviewer confirmed it targets the
failing thread and granted final approval after checking the successful rerun
and strict stock Clippy log.

The revised offline locked debug libtest SHA-256 is
`8e86187fbab3270cc34c0d83050172fcac5ec7596e99c9aabfc6693646ac22e8`. Its matching
`marmish`, `marmishd` and `marmish-worker` all reported the revised source SHA.
The focused Terminal, transport and dispatcher tests passed again on the
revised libtest. Exact stock all-targets Clippy passed in 34.18 s.
Candidate all-targets Clippy also passed with
`--cfg marmish_peer_receiver_candidate` on `x86_64-unknown-linux-gnu`.
The candidate check validates compilation and lint; all runtime evidence here
comes from the stock binary. These results complete the composition review's
validation checklist.

The matched debug `terminal_selection_native_wss_echo_and_resize` test
passed in 103.25 s with no `RUST_MIN_STACK` override. It used tmux 3.4,
`MARMISH_TEST_DAEMON` pointing to the matching `marmishd`, and the three
matching sibling binaries in the Cargo-style layout. The exact selector was
`mdk_adapter::terminal_route_proof_tests::terminal_command_fixture::terminal_enrollment_wss_tests::selection_proof::terminal_selection_native_wss_echo_and_resize`,
run with `--exact --ignored --nocapture --test-threads=1`. The fixture verified
20 quoted UTF-8 echoes, 16 KiB of output, the resize to 30×90, isolated and
resumed sessions, invalid selection handling, and Command progress before
and after Terminal use. The unique anonymous 64 MiB writable mapping had
32,816 KiB resident in one Linux process-map sample. That sample crossed
the prior 32 MiB allocation and does not measure peak stack depth.

| Loopback workload | Observed |
| --- | --- |
| Signed room enrollment, one sample | 4.664 s |
| Initial selection, one sample | 279.9 ms |
| Resumed selection, one sample | 187.2 ms |
| 20 quoted UTF-8 echoes | 73.55 ms p50, 98.01 ms p95 |
| 16 KiB output burst, one sample | 124.8 ms, 131,231 B/s |

These debug loopback measurements describe the successful run. They do not
set a performance threshold for a network or an optimized build.

[Physical cut design](../phases/peer-candidate-terminal-fatal-stop.md)
· [Original code review](peer-candidate-terminal-relay-code.md)
· [Author validation](../validation/peer-candidate-terminal-relay-fatal.md)

## Integration with the reviewed candidate storage and listener changes

Claude Code Auto session `11808f2a-0ba1-4f77-be2a-8e6fe25bf846` approved
`65fcfae09cabc639714c05e29c8dc555786e9ef9`. The two branches changed disjoint
files. All 13 files from the Terminal branch match their reviewed versions
byte for byte. The reviewer checked the shared transport callers and passed
independent stock and candidate all-target Cargo checks. The combined
candidate also passed strict Clippy in 40.53 s. The reviewer's separate
runtime build exhausted disk space. The source-matched live Terminal results
above and the candidate storage and listener results below are from their
respective reviewed source cuts.

[Candidate storage and listener validation](../validation/peer-candidate-owner-leaves-composition.md)
