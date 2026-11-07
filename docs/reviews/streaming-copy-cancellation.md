# Actual-helper copy cancellation qualification

The approved addition is test-only: the real hidden helper, production CopyIo,
copy client, filesystem worker and receiver run over local framed IPC. It does
not prove daemon Cancel, relay delivery, CLI signal handling, journal lease
release or installed cross-platform cancellation. SIGTERM of the currently owned
helper substitutes for the production runner cancellation action. Installed
qualification must cover the daemon boundary separately.

Actual Claude Code (`--permission-mode auto`), model `claude-sonnet-5-5`, reviewed
the detailed design through four rounds. Round 4 **APPROVE**, 131.475 s, six
turns, session `14e60143-b7c8-466d-8b36-70fed208bd70`, preceded implementation.
The accepted design requires eight cases: Put and Get with a full canonical
256 KiB transport window; both directions at barriers immediately before rename
and after rename but before receipt persistence; and both directions after a
committed receipt. Every case uses a fresh transfer namespace and a fresh actual
helper Resume. The buffered source is 3 MiB + 65,537 bytes; the other sources are
65,537 bytes. The tests use zlib and preserve exact bytes, mode and nanosecond
mtime. Earlier raw/zlib and large-file tests remain separate qualification.

The commit barriers exist only under `cfg(test)`. Each worker receives its own
owned Unix socket through a synchronous thread-local handoff. The bounded
handshake is the exact point byte followed by `0xa5`; it expires after five
seconds. The test helper's inherited descriptor setup lives only in its ignored
test entrypoint. Ordinary helpers clear those test environment fields. No hook,
environment switch or additional inherited descriptor exists in production.
Only `overwrite=false` is qualified at these barriers; this does not exhaust
all crash windows or qualify the separate overwrite branch.

Full-window cases first inspect a real Receiving checkpoint and data beyond its
durable prefix, then stop input delivery (Put) or output exposure (Get) until
canonical window credit is below one maximum block. No Done has been observed.
Explicit cancellation retains the last valid checkpoint; a joined worker and
reaped helper precede disk assertions. Retained prefix hashes, monotonic durable
offsets, lock release, resumed Data beginning at the durable offset, exact
remaining Data count, final Committed state and final metadata are asserted.
CommitPrepared state is deliberately not reopened in-process: the fresh helper
must perform rename/receipt recovery itself. Committed outcomes are never
rolled back and Resume sends zero Data.

For helper cancellation, stdin remains open through the wait so EOF cannot
masquerade as cancellation. Qualification requires successful SIGTERM to a
currently owned live child, normal exit 1, no Error frame in either direction,
and empty stderr after a bounded post-reap drain. A locally cancelled Get after
rename may complete normally; that case asserts the committed disk outcome and
fresh Resume, rather than claiming the helper observed cancellation. No recovered
or persisted process ID is signalled.

Independent code review session `f643f3c1-9f5c-4890-b31d-af45c689ecfe` used the
same model and auto mode, with static inspection only. Reviewers were expressly
forbidden Cargo, binaries, network access and edits while the author's Cargo
lane was active. Round 1 **REQUEST_CHANGES**, 124.633 s / 11 turns: remove the
in-process reopen that preempted actual helper recovery; strengthen both Error
counters and Receiving/no-Done checks; document the handshake and overwrite
scope. Round 2 **REQUEST_CHANGES**, 34.745 s / four turns: acknowledgement
failures must return finite errors into checked teardown, rather than unwind
past ownership release. Both were fixed. Round 3 **APPROVE**, 16.749 s / three
turns, resolved all findings. A closed-control regression checks returned errors
and joins the task before assertions.

Every trigger is bounded by 120 s and each test family by 600 s. After dropping
the attachment, the local cancellation flag is set and the worker is joined
within 15 s; checked helper cleanup has a 10 s join bound and a separate failure
abort/join bound. Hook and cancellation-drain waits are five seconds. The
buffered and receipt cases explicitly signal the helper; connection loss alone
is never treated as cancellation.

Author execution on non-root Linux AMD64, debug/test profile:

| Check | Result |
| --- | --- |
| Final cancellation cohort: eight actual cases plus closed-control regression | 4 tests passed, 75.67 s |
| Full helper cohort before the final cleanup-only refinements | 13 passed, 2 expected ignored, 102.55 s |
| Existing descriptor/filesystem safety tests | 33 passed, 26.36 s |
| Worker ownership and bounded sender tests | 5 passed, 0.08 s |
| Strict native all-target Clippy | Passed, 8.85 s |
| Whitespace diff check | Passed |

The final Clippy run followed removal of a redundant test-only let-and-return;
this does not change trigger/cleanup order. The costly four-way 66 MiB helper
qualification was not repeated for this test-only cancellation addition.

Exact commands, run sequentially from the copy implementation worktree with
`CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target`:

```sh
cargo test -p marmish-native copy_integration_tests::cancel --lib
cargo test -p marmish-native copy_integration_tests --lib
cargo test -p marmish-native copy_files::tests --lib
cargo test -p marmish-native copy_worker::tests --lib
cargo clippy -p marmish-native --all-targets -- -D warnings
git diff --check
```

The full helper cohort preceded the last no-reopen and error-propagation
refinements; the final cancellation cohort exercised those changes. Existing
filesystem and worker tests ran afterwards. The author also ran ripwire's
quality delta: no preexisting-worse/gating findings; it reports new test symbols,
fixture complexity and repeated test wrappers. Those are test-only orchestration
costs, not a claim of structural simplicity or a production API expansion.
