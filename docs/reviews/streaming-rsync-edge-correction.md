# Installed rsync edge correction reviews

Scope: [bounded design](../phases/streaming-rsync-edge-correction.md), prepared
against a7c3db4 after failed installed edge-rsync run 42433877249/1. No product,
wire, workload, deadline, workflow, public version or installer pin changes.

Actual independent Claude Code `--permission-mode auto --model claude-sonnet-5-5`
session `f5f3ced3-8525-486e-863c-f199dc424a30` returned **NOT YET APPROVE**
(161.812 seconds, 28 turns). Private review JSON SHA256:
`9e4af388e74ff9acc775b1bdf6fe16793e0c2dcc8bf9b56b6042f99996030156`.
It inspected source read-only, without builds, relays, CI or private logs.

Blocking findings: failure-mode startup must wait through valid active
Cancelling with a current Open before the final natural-exit23 observation;
Cancelled remains a failure. Breadcrumb semantics must explicitly distinguish
the original operation's last pre-Cleanup Begin from a final Cleanup error,
while preserving abort-error precedence. Useful nonblocking findings tighten
None disposition, ordinary event counting, both coordinator gating sites,
marker eligibility and the documented residual USR1-versus-natural-exit race.
The author incorporated these before implementation.

Beads remains inaccessible because available clients do not safely match the
existing schema. No migration, reinitialization, bypass or manual database edit
was attempted. This review record is not a replacement task tracker. Fresh
three-platform candidate and all eight original installed cohorts remain
required; current historical partial passes do not qualify a release.

The resumed actual review returned **APPROVE for implementation** (18.519 seconds,
2 turns), JSON SHA256
`95816ab465a703fa88bc912125008f8568accf87b0a3cc01048670c8e83b39de`.
Approved design SHA256:
`bf7dba8e37b6d370e765eb3da2748aa7a01eefbfc652a32161be9d7e6a6cabac`.
Approval is recorded before delegated source implementation. The reviewer
verified both blockers resolved, without edits, builds, live traffic or CI.
Its bounded nonblockers clarify that a second Cleanup Begin can make eleven
ordinary events, and that an early Cancelled is Startup/Invalid while a later
one is JournalCheck; neither is accepted. These clarifications and the approved
status were incorporated editorially after review. Code and hosted qualification
approval remain separate requirements.

## Integrated local checks before code review

The installed-checks library filter passed 240 tests, zero failures or ignored,
633 filtered out, in 5.68 seconds. Command:
`cargo test -p marmish-native --features installed-checks --locked --lib installed_check::`.
Log SHA256:
`fbe7fc3ee5790f2fc81275fceded04cbd250c3f10ea30d8ba8757d52013c7a19`.
Strict installed-feature all-target Clippy (`-- -D warnings`) passed in 15.91
seconds; log SHA256
`51aa3f530abd1f4b4096b88203ff14fcd930d92c77ba4de0168a1ac7ca91326d`.
Scoped rustfmt and diff whitespace checks passed. Python fixture checks are
recorded in [their review](streaming-native-https-fixture.md).

The quality delta exited 2, with 12 gating findings (29 total), not clean.
Its log SHA256:
`f94547390b439c339f25f172e1f41353db2c5649a241f21a3c44b497184d1537`.
The change-context report was retained and inspected, SHA256
`1586c5240ef82c792674fd95824efa678e4320864b0f7452f695317ead87eb24`.
The unchanged admission one_inner verbosity row is a name-resolution collision.
New cfg(test) functions actually executed; the new Startup enum is used by the
classifier. Private Entry fixture copies, trace-test repetition and correction
churn require independent review; no baseline or acknowledgement suppression
was applied. Source wiring tests are architectural guards, not runtime IPC
proof. Synthetic trace cleanup tests do not prove the actual Child cleanup.
The independent integrated reviews below completed after these checks.

## Actual integrated code review

The same actual Claude Code AUTO / `claude-sonnet-5-5` session returned
**APPROVE** for the full 14-file staged diff, including every new file/test,
the HTTPS fixture and sanitized JSON evidence (84.478 seconds, 12 turns).
Private JSON SHA256:
`7aaedfe94f89a41c3b0d0d1756d070bfef6858cd8e44bc8742668784c1cff92d`.
It independently ran the edge-filtered Rust suite (58 passed, zero failures),
the exact HTTPS unit under default TMPDIR, and the staged whitespace check.
It relied on root logs for the wider filter, Clippy, formatting, long-TMPDIR
fixture and quality delta. It verified the 72 partial observations and rate
arithmetic, not live transport or hosted cleanup.

Root, a separate read-only agent and Claude inspected the quality findings.
Six gating rows double-count real private test Entry constructors with different
premises; widening unrelated test-module APIs would couple their fixtures.
Five gating rows measure corrective churn. The remaining gating row locates
one_inner in unchanged admission code; the real rsync body gained fixed stages.
The observer clone is local sink/trace/decode scaffolding with distinct name/cap
versus stage/error assertions. Explicit fixtures and exhaustive test cases are
retained; no suppression, baseline manipulation or unrelated refactor was used.

Useful nonblockers: root added rejection rows for Exited23 in nonterminal states
as suggested. Synthetic trace tests and source-text wiring limits remain explicit.
The failure-mode classifier intentionally has separate state handling; future
shared-classifier edits require checking both branches. Outer family-validation
failure may still be labeled coordinator Begin; case stages remain distinct.
The formatting-sensitive wiring assertion is retained as an architectural guard.
Reversing exactly the three recorded postapproval editorial substitutions
reconstructs the approved design hash bf7dba8e...6cabac; root verified equality.
The added test rows require affected checks and a final review acknowledgement
before publication. No hosted qualification or release approval is implied.

After the added nonterminal-exit regression rows, the complete installed-checks
filter passed again: 240 passed, zero failures/ignored, 633 filtered, 5.73 seconds.
Final test log SHA256:
`94b09099bf56f129be05911cf7ce968df1eb80ee722a1dfe69b6c93697fffdcf`.
Strict all-target installed-feature Clippy passed again. Final log SHA256:
`9868a3e258a0c818ca87c967b441e42006713d152b56bb14514b6b0f279b5247`. Scoped formatting was applied to the added array and checked.
The repeated quality delta still exits 2 with the same 12 gating rows (30 total,
one additional test verbosity row); final log SHA256:
`99e73aff742c65ab3ed71a8dfc8814e6aede6595791765034f9212fea6b58620`.


## Final acknowledgement after regression additions

Actual Claude Code AUTO / `claude-sonnet-5-5`, session
`f5f3ced3-8525-486e-863c-f199dc424a30`, returned **APPROVE**, with no
remaining blockers (21.892 seconds, three turns). Private review JSON SHA256:
`964ad8a9f615516a3ba25e34b96b1d869ea7cd44b08c59bc92569eb6d263cebb`.
The reviewer checked the added wrong-state rejection rows and unchanged runtime
code, and independently ran all five startup tests and staged whitespace checks.
It relied on the root logs for the complete 240-test filter, Clippy, formatting
and quality disposition. It accepted the reported design-hash reconstruction
without independently deriving it. This follow-up did not reread the updated
review records or validation prose. Its approval covers code publication;
hosted qualification and release readiness still require the fresh full chain.
