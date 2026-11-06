# Native streaming runtime code review

Scope: Phase 2a, Beads `marmish-8ka.2`, uncommitted implementation atop
`eac9410a45fcc9251dbb84d47fd760bbdb1dfacf`. Review includes newly added source,
engine ownership, native preparation, encrypted persistence, publication
ownership, runner, IPC, CLI and cancellation. Rsync and file copy are later phases.

## Round 1

Independent reviewer: Claude Code, model `claude-sonnet-5-5`,
`--permission-mode auto`. Actual verdict: **REQUEST_CHANGES**.
Duration: 698.171 seconds, 34 turns. The reviewer inspected repository source and
ran local tests; it did not run live relays, modify source, or build production
binaries. Raw output was retained locally as
`/tmp/marmish-stream-runtime-code-r1.json`.

Blocking findings:

1. Terminal slots/payloads were never reclaimed, exhausting the per-principal
   slot cap after a small number of completed streams. Add bounded retention and
   preserve compact launch/cancel fences independently of payloads.
2. Failed/uncertain publication obligations never retired, exhausting publication
   capacity and preventing count-limited daemon drain. Distinguish definitive
   failed transport from successful ACK and preserve genuinely ambiguous state.
   Upstream MDK deletes terminal zero-accepted fanouts itself; a normal published
   ACK is neither possible nor necessary for those records.
3. Every 100 ms local poll could enqueue a newly timestamped relay publication.
   Retain logical request identity and allow explicit bounded retry rather than
   treating local polling as a new transport attempt.

Useful additional findings included late cancellation rewriting a natural exit,
closed stdin retaining undelivered accepted bytes without a closure fence,
collapsed IPC diagnostics, unknown-stream reconciliation, repeated revocation
writes, unknown Exchange allocations, and incoming invalidation behavior.
The unused runner identity accessor also needs removal. Fixes and follow-up
review are in progress; this document does not record approval.

Actual reviewer validation:

| Command | Result |
| --- | --- |
| `cargo test --locked -p marmish-native --lib` | 421 passed, 1 installer recovery failure, 401.93 s |
| `cargo test --locked -p marmish-engine --lib` | 120 passed, 1 ignored, 22.24 s |
| Installer tests run separately by reviewer | 20 passed |

The full-suite installer failure was `install_busy` during immediate lock
reacquisition. The reviewer did not identify its cause. Subsequent independent
inspection identified fork-inherited open-file-description locks as a concrete
possible mechanism; close-on-exec alone cannot prevent it. Scoped explicit unlock
now has a deterministic cloned-descriptor regression, and the affected installer
run passed **21 tests**. This does not replace a final full-suite run.

Nonblocking observations about shutdown grace/final publication and richer
startup diagnostics remain subject to explicit disposition before this increment
closes. No orphan PID will be signaled merely from persisted diagnostic metadata.
The review's requested large-volume/cross-platform qualification remains a later
gate; the live all-byte payload already includes NUL and exceeds the raw window.

## Round 2

Independent reviewer: Claude Code `claude-sonnet-5-5`, auto mode, including
three read-only review subagents. Actual verdict: **REQUEST_CHANGES**,
322.975 seconds. Raw output: `/tmp/marmish-stream-runtime-code-r2.json`.
The reviewer inspected integrated sources, ran Python syntax compilation, and
ran no Cargo, production binaries, live tests or network tests this round.
Root's coordinated full native suite was running independently.

The blocking finding was inactive Hello-only records bypassing age-based fence
reclamation, permitting permanent retained-quota exhaustion. Useful additional
findings covered stale publication snapshots after a Cancel flush, retry-ledger
writes before failed capacity admission, cancellation after natural leader exit
with blocked output, monotonic publication transitions, reserved request-slot
credit for Cancel, invalidation recovery, unknown local Exchange allocation,
explicit admission responses and local descriptor validation before Open.
These findings are being addressed with regression tests; this is not approval.

The reviewer confirmed owned process-group signaling/reaping, bounded child
pipes, delivered stdin cursors, definitive publication retirement prerequisites,
ACK-relative backoff, authority checks, shutdown keeper ownership, internal stop
completion gating and the installer lock fix. It also identified maintainability
and operational limits requiring documentation. In particular a retained Lost or
UnknownLaunch record prevents a clean stop until explicit administrator
reconciliation acknowledges that a process may still be alive.

### Static quality assessment during correction

`ripwire . --quality-delta` reported nonzero debt against the phase-1 Git HEAD;
it is **not a clean quality gate**. The round-3 correction scan reported 313 items,
102 classified gating. The subsequent round-4 scan reported 335 items,
105 classified gating against the same phase-1 HEAD (142 preexisting-worse,
193 new-symbol, 37 minor; zero acknowledged/suppressed findings). Many results refer to unqualified common names, macro test
registration or the intentional new protocol/API surface. Real concentration was
also present: runner observation, request projection and publication reconciliation
combine many state-machine conditions. Coherent preparation, Exchange and
publication-reconciliation helpers were extracted while preserving durable
ordering. The original progress method's complexity 92 was split into distinct
stages; publication submission fell from 66 to 26, and request projection from
70 to 46 in this tool's measurement. Local IPC dispatch remains a large explicit
match, and the existing owner event loops grew to integrate the new protocol.
These are maintenance risks, not evidence that correctness or security passed.
The independent review and behavioral tests remain necessary; no baseline was
pinned or findings suppressed to manufacture a passing scan.

## Round 3

Independent reviewer: Claude Code `claude-sonnet-5-5`, auto mode, 467.116
seconds, 29 turns, including two read-only review subagents. Actual verdict:
**REQUEST_CHANGES**. Raw output: `/tmp/marmish-stream-runtime-code-r3.json`.
The reviewer ran 12 hermetic Python tests and help checks; no Cargo, production
binaries or live network tests. Root's concurrent native snapshot independently
passed 446 tests, with one ignored, in 431.93 seconds; that snapshot predates the
corrections below and does not validate their final source.

Blocking findings concerned shared stdio descriptor flag restoration, stdin
closure before a child consumes piped input, and invalidation overwriting
uncertain launch states. Additional findings concerned untracked refusal
allocations bypassing retained quotas, client Open fences expiring while commands
could remain live, publication timestamps changing on refused transitions, and
weak rollback coverage. Corrections and regression tests are in progress. No
approval is recorded by this round.

## Round 4

Independent reviewer: Claude Code `claude-sonnet-5-5`, auto mode, 221.517
seconds, 24 turns, no subagents. Actual verdict: **APPROVE**. Raw output:
`/tmp/marmish-stream-runtime-code-r4.json`. The reviewer independently inspected
sources and untracked files, ran 14 hermetic Python tests and harness help,
and ran no Cargo, native binaries, live or network tests or edits.

All round-3 blocking and medium findings were confirmed resolved. The reviewer
also confirmed the publication transition guard, stronger rollback test and
SIGINT listener placement before room preparation. One nonblocking low finding
remains useful: when a live runner coexists with a record terminal/cancel fence,
Status must combine those record fences into `stdin_closed` to encode a valid
response. A focused correction and regression are being incorporated, followed
by scoped review. The isolated preparation-interruption subprocess test was
added during this review and needs its own execution evidence. The integrated
full native suite is still running; this approval does not substitute for it.

Remaining acknowledged maintenance limits include string state representation,
large SQL statements, a synchronous maintenance sweep every 30 seconds, permanent
Lost/Unknown/uncertain holds and no reconciliation API for uncertain publications.

## Scoped follow-up, round 5

Claude Code `claude-sonnet-5-5`, auto mode, 54.918 seconds, 15 turns, read-only
source inspection. Actual verdict: **REQUEST_CHANGES**; no tests, binaries or
network actions were run. Raw output:
`/tmp/marmish-stream-runtime-code-r5.json`.

The new real-child regression exposed a second aspect of round 4's low finding:
a record-level terminal fence combined with a still-live runner lacks physical
stdout/stderr EOF, so forcing only `stdin_closed` still fails terminal frame
validation. This follow-up is not approved. A correction must report uncertainty
without inventing output EOF or claiming child cleanup. The reviewer confirmed
the preparation-interruption test's signal registration and descriptor restoration
ordering. Root's full native run was compiled before these two follow-ups.

## Scoped correction, round 6

Claude Code `claude-sonnet-5-5`, auto mode, 50.379 seconds, 11 turns, read-only
source inspection. Actual verdict: **APPROVE**. No tests, Cargo, binaries,
network actions or edits. Raw output:
`/tmp/marmish-stream-runtime-code-r6.json`.

A durable terminal fence with a physically nonterminal runner now produces a
finite `Error{Uncertain}` before projecting output, offsets or EOF. Cancel-only
state still produces a valid Exchange with truthful cursors. The review confirmed
that uncertainty does not claim physical cleanup, and approved the isolated
preparation-interruption regression. Root's subsequent compilation found a test
fixture still calling a removed accessor; the fixture now uses the supported
snapshot identity field. No production accessor was restored. Affected tests
must pass before publication; static approval did not detect that compile error.

Final staged-source quality scan still exits nonzero: 352 findings, 106 gating,
against the phase-1 HEAD, with zero acknowledgements. Counts include 144
preexisting-worse, 208 new-symbol and 38 minor findings. Most gating categories
are duplicate shapes, helper-clone and churn heuristics; real owner-loop and
publication complexity remains. One unqualified `validate_response` attribution
points at unchanged SRV code, illustrating name-resolution limits. No baseline
was pinned and this is not represented as a clean quality gate. The staged
change-context map covered 43 indexed files (JSON files excluded) and confirmed
broad native command/IPC/installer/engine reach; coordinated full native, engine,
focused stream, installer, CLI and actual relay checks address those boundaries.
Static graph reach is neither runtime coverage nor a security proof.

## Startup readiness correction, round 7

Claude Code `claude-sonnet-5-5`, auto mode, 39.950 seconds, seven turns. Actual
verdict: **APPROVE**. Read-only source/caller/IPC inspection, no tests, Cargo,
binaries, live/network actions or edits. Raw output:
`/tmp/marmish-stream-runtime-code-r7.json`.

`--no-autostart` and explicit sockets now wait for an already-authenticated
activating daemon within the existing 300-second budget, while an absent daemon
still fails without launching. The real Unix-IPC regression passed all three
selector variants and missing-socket refusals (**0.54 s**); the broader CLI
selection passed **39 tests** (**1.00 s**). Strict all-target Clippy passed
(**3.91 s**). Separately, the readiness harness has Claude approval and
**18 passing hermetic tests**. A daemon disappearing after reporting activating
can still consume the bounded startup budget; that existing operational limit
is accepted rather than silently changed in this fix.
