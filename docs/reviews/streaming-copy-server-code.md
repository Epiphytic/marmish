# Phase 3 server-side copy code review

This is scoped implementation evidence, not whole-phase approval or live transport
qualification. Client/frontend and local Get receiver lifecycle/GC have separate
reviews. The sources included untracked copy modules and the tracked integration
diff; no review relies on the diff alone.

## Round 1

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Session: `4689e6cf-4389-4b8d-95a6-6ea869bf130f`.
Verdict: **REQUEST_CHANGES**. Read/search-only tools; no mutations, builds, tests,
product binaries, daemon/relay/network calls. Raw local record:
`/tmp/marmish-copy-server-code-r1.json`. Review duration: 282.276 seconds.
Reviewed sources were hashed locally and verified unchanged during inspection.

Blocking findings:

- Latest Resume attempt can finish before its helper opens the receiver. The
  checkpoint still identifies an older attempt, so requiring its `last_stream`
  to equal the latest attempt prevents eligible GC indefinitely.
- Failed GC removes only its claim, leaving the oldest candidate immediately
  eligible. Repeated selection starves later expired candidates; use durable
  retry scheduling or another bounded fair progression mechanism.

Useful additional findings:

- A stale checkpoint temporary file can conservatively prevent collection.
  Any cleanup extension must validate exact producer names, descriptor identity,
  ownership and file type; broad deletion patterns are not authorized.
- An absent tree under a changed working directory/parent does not prove that
  the original tree was removed. Independent investigation treats this as a
  blocking false-credit-release case, despite the review's lower severity.
  Preserve state unless original receiver anchoring is proved; do not synthesize
  authority from a freshly resolved empty directory.
- Nonblocking observations concern in-memory Verifying transition before persist,
  timestamp representability, repeated image hashes/fstat calls, and duplicate
  journal user-version assignment. Useful correctness fixes require affected
  validation and another scoped review.

The reviewer found no authenticated-authority bypass by inspection. It verified
atomic admission/index/receipt writes, cancellation/revision/grant fences,
uncertain-state retention, private helper context FD and executable fingerprint
checks, descriptor-relative no-follow paths, atomic rename/recovery, bounded
codecs and independent retention configuration. These are review observations,
not executed test results.

Inspection coverage was explicit: copy_store fully; copy_protocol lines 1–990;
copy_files lines 1–1000 and 1540–1753; copy_launch lines 1–620; relevant runtime,
worker and store handlers; OpenCopy/CopyRefused validation. Launcher/live,
configuration, service, lifecycle and journal integration were inspected through
the diff/search, not read fully. Tests were not read exhaustively.

Source scope: native copy_store/copy_files, stream_runtime/service/store,
command_engine/config/runtime/service/store/local/lifecycle and journal;
stream copy_protocol/wire; engine copy_launch/live/launcher and launcher tests.
The independent review excludes frontend/client/io/session and local Get GC.
No phase closure or code approval is recorded while these findings remain open.

## Scoped GC refinement design followup

Reviewer: same Claude Code session, `claude-sonnet-5-5`,
`--permission-mode auto`; raw `/tmp/marmish-copy-server-gc-design-r2.json`.
Verdict: **APPROVE (design only)**. This followup read no new source and rests on
round 1 inspection plus the concrete refinement proposal. It is not code approval.

Approved refinements:

- Current confirmed inactive revision plus a durable exclusive GC claim and
  physical flock supplies cleanup authority. The immutable namespace/manifest and
  descriptor identities still must match; GC no longer requires checkpoint
  `last_stream` to equal the latest admitted attempt, because that attempt may
  have stopped before opening the receiver. New Create/Resume remains blocked
  while the durable claim exists. Normal resume and proof retain last_stream.
- A bounded `copy_gc_schedule` stores fair turn and retry-after metadata. Attempted
  or blocked rows rotate behind unattempted rows, with a checked 60-second delay;
  at most 32 deferrals occur atomically per scan. Schedule rows are bounded by
  retained indices, pruned with their index, and orphan-cleaned on startup. A
  newly admitted revision clears its old scheduling delay. Overflow is an error.
- Missing namespace returns retained/false, since a newly resolved empty parent
  or changed cwd cannot prove original cleanup. Present state still requires its
  parent/private/data identities. Unknown files and stale checkpoint temp files
  remain conservative manual-recovery cases; no broad unlink pattern was added.

Accepted limits are explicit: never-created or moved namespaces can retain index
capacity indefinitely; interruption after physical deletion but before journal
completion also retains state. There is no supported automatic or operator CLI
for clearing these uncertain filesystem/index cases in this increment. Existing
stream reconciliation does not manufacture filesystem cleanup authority. The
finite CopyRefused Capacity result remains distinguishable. Fair scheduling keeps
one retained failure from starving other transfers. Restarted physical claims
remain cleanup_uncertain and cannot be auto-retried.

Required post-implementation checks include older-checkpoint/unopened Resume,
forged bindings/path/manifest and active lock negatives, absent/replaced parent
and changed cwd, blocked/failing candidate fairness beyond 32 rows, overflow,
revision scheduling reset, and interrupted-claim retention. Implementation and
code re-review are pending; no final code approval is recorded here.

## Round 2 code re-review and targeted validation

Reviewer: same independent Claude Code session and model, read/search-only
`--permission-mode auto`. Raw local record:
`/tmp/marmish-copy-server-code-r2.json`; duration 49.151 seconds.
Verdict: **APPROVE (source)**, with execution of the new regressions required
before landing. No source correctness blocker remained in this scoped re-review.

The reviewer inspected the new fair schedule, admission reset, startup orphan
cleanup, GC claim/completion transactions, SQL regressions, receiver cleanup and
replaced-parent/cwd regression. It verified that removing only the GC
`last_stream` equality retains immutable binding, descriptor identity, expiry,
physical flock and durable claim authority. It verified failed cleanup retains
credit and scheduling delay, successful cleanup atomically removes retained
state and advances the copy-only replay floor, and interrupted claims remain
uncertain. Engine/runtime security observations rely on round 1 inspection;
these call sites were not read again. The reviewer did not read every filesystem
regression or the local Get observer accessor in round 2.

The implementing agent then completed these actual targeted runs against the
integrated native source, with `--locked --lib -- --test-threads=1`:

- `copy_store`: **12 passed**, 0 failed, 6.33 seconds. Includes failed-candidate
  fairness, more than 32 blocked rows, checked schedule overflow/orphans, retained
  credit, claim exclusion and interrupted-claim recovery.
- `copy_files`: **26 passed**, 0 failed, 3.04 seconds. Includes older checkpoint
  after unopened Resume, forged manifest, held flock, replaced parent,
  same-named namespace under the wrong parent, unrelated cwd/base, expiry,
  unknown-entry retention and uncertain commit.
- `copy_receipt_policy`: **1 passed**, 0 failed.
- `stream_stop_observation`: **1 passed**, 0 failed.

Local logs are `/tmp/marmish-copy-store-r5.log`,
`/tmp/marmish-copy-files-r3.log`, `/tmp/marmish-copy-config-r1.log` and
`/tmp/marmish-copy-stop-r1.log`. These targeted checks satisfy the new regression
execution condition; the integrated whole-phase cohort and live qualification
remain separate acceptance gates. The previous copy cohort had 53 passing tests,
one root-owned fixture failure and one intentional helper-test exclusion; this
record does not claim that complete cohort was rerun here.

Nonblocking observations remain explicit: corrupt attempt serialization fails
closed and can stall that scan; sustained eligible attempts can delay secondary
receipt housekeeping; retained trees retry at a fixed 60-second delay. Automatic
recovery of moved/never-created namespaces and stale checkpoint temp entries is
still outside this increment. Local Get GC and any later changes to the shared
filesystem reclaim implementation require their own affected validation/review.
