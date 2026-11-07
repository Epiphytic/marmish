# Installed owned-group cleanup review

Scope: Beads marmish-8ka.5.4, bounded harness correction from 5d00a159ffb24e9d743b668448d7c602a58e177d. No release asset or product cleanup-policy change.

## Independent design review

Actual Claude Code, Claude Sonnet 5.5, `--permission-mode auto`, session `abaa3c77-9505-4fb8-876a-f3d6207fd726`.

Round 1 requested changes: deterministic orchestration tests rather than classifier-only coverage, and explicit error/status contracts. Both were incorporated in [the phase design](../phases/streaming-owned-group-cleanup.md) before implementation. Round 2 **APPROVE**, no blockers. Both reviews were read-only and ran no tests or network commands.

| Round | Duration | Turns | Raw result SHA-256 |
| --- | --- | --- | --- |
| 1 | 74.720 s | 10 | `5fed15c05dc76aba6c766a203778d7e3e40f364a96721f2bb3c02f13308fa072` |
| 2 | 11.969 s | 3 | `566fcc90789465852032ce7ecf6d0f9f4d0941c247935f1bfb1add600a09c4e1` |

The retained Child fences the sole nonzero group signal. A post-reap zero-signal probe can conservatively refuse a reused group; only fresh ESRCH proves absence. Wait timeout and unknown cleanup remain failures. Real macOS fixture execution remains required; Linux injected EPERM is not native macOS evidence.

## Independent integrated code review

Actual Claude Code, Claude Sonnet 5.5, `--permission-mode auto`, session `7b99e00c-ec90-4a56-8186-f6789689f003`: **APPROVE**, no blockers. Duration 56.323 seconds, 9 turns. Raw result SHA-256 `2f95c1e1adfbdd5d494613d9f6b94b973b96a5dc3224b1ca80d50750f814bf79`.

Reviewer independently ran all 30 focused installed edge tests, strict native installed-checks all-target Clippy, native formatting and diff checks; all passed. It inspected both changed/new Rust files and failure evidence. No source edits, Git mutation or network execution. Full 762-test suite and native macOS remain required in the fresh candidate. Root added an intended specification-only paragraph summarizing the old failing candidate and this bounded cleanup contract; it changes no interfaces or scope.

The reviewer verified exact Child WNOWAIT ownership, single bounded wait, preserved exit/error categories, lazy operation ordering, no nonzero post-reap signal, refusal for unknown groups and retained descendant/double-cleanup fixtures. Quality-delta exited zero with zero gating findings; six discovered Tokio tests were labeled dead code and two small independent test assertions were labeled duplicates. These are recorded informational test-symbol findings, not suppressed or a claim of no measurements.

Reviewed Rust SHA-256:

- `crates/marmish-native/src/installed_edge_rsync.rs`: `7e6b98f89101ac929926b9694e8f4affa0b202244fbcde50c60327b0c6cf3904`
- `crates/marmish-native/src/installed_owned_group_cleanup_tests.rs`: `5d8b0858a24668e1c6d8d3779695087693f2f6d1d9002feeda600607a8401044`
