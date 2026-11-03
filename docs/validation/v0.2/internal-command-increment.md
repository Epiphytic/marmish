# Native internal command increment

This increment integrates the Rust command owner, dedicated Marmish room validation, SQLCipher command ledger, command service, worker bridge and joined owner shutdown. It is not Phase 2 acceptance or a release. Legacy wire and disk compatibility are optional.

Local Linux AMD64 validation:

The [final actual independent integrated review](../../reviews/v0.2-internal-command-code-r2.md) approved this increment and independently ran the final source: **229 library tests passed**, 0 failed (291.33 seconds), plus one other test binary with one passing test. It also passed native formatting and Clippy with only dead-code warnings allowed. The earlier observations below remain recorded separately.

- Native library suite: **222 passed**, 0 failed, 299.89 seconds. Subsequent store hardening and engine budget checks have separate passing focused suites below; this full-suite observation predates those narrow changes.
- Final focused command store suite: **31 passed**, 0 failed, 38.04 seconds.
- Final focused command engine suite: **14 passed**, 0 failed, including malicious over-limit read results at each Poll read position.
- Service conflict suite: **9 passed**, 0 failed. A changed Submit receives a durable authenticated refusal without changing the original invocation.
- The repository-wide Ripwire quality delta is not green. Its report includes cross-module short-name matches (for example, the new fixture `command` matched an unrelated existing function), SQL/test shape similarities, and real growth in the owner maintenance/test fixture paths. These are review inputs, not passing quality evidence; no baseline was changed to hide findings.
- Formatting and whitespace checks pass. Clippy passes with only dead-code warnings allowed (`cargo clippy --locked --offline -p marmish-native --all-targets -- -D warnings -A dead-code`). Strict Clippy does not pass: APIs awaiting the public command CLI/IPC are currently unused. No source-wide warning suppressions were added.
- [Actual direct Marmot command execution](local-command-direct-2026-11-02.md) passed special/non-UTF8 argv, 16 KiB binary stdin, stderr and nonzero exit, with explicit relays, disposable keys and actual cleanup. Its latency and goodput figures are single observations.
- A [post-review direct repeat](local-command-poll-pressure-2026-11-02.json) exposed local `command_poll_busy` after its first case, with all cleanup checks passing. The fixture now retries that transient pressure with the same request ID while progressing both owners, bounded by the original deadline. A passing earlier run does not erase this failed observation.
- The [final rebuilt direct repeat](local-command-direct-final-2026-11-02.md) passed all three cases after both Poll fixes, with exact source hashes and all cleanup flags true.
- [Arti command attempts](local-command-arti-2026-11-02.md) failed and are recorded separately, including successful cleanup. There is no Phase 2 Arti command acceptance.

Independent actual Claude component reviews cover [runtime integration](../../reviews/v0.2-root-command-integration-r2.md), [store and evidence](../../reviews/v0.2-command-store-code-r4.md), and [engine/lifecycle/check](../../reviews/v0.2-command-engine-lifecycle-code-r2.md). Exact reviewed hashes and actual reviewer sessions are recorded there. [Integrated round 1](../../reviews/v0.2-internal-command-code-r1.md) found a server Poll retry classification bug. The fix and the related fixture retry were approved in the final round-2 review linked above.

Public command CLI/IPC, restart while running, remote replay/conflict live acceptance, bounded-server command/uptime enforcement, native installation and the installed cross-platform command matrix remain unfinished or unvalidated. The accepted Phase 1 hosted foundation checks do not substitute for these command requirements.
