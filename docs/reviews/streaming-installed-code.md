# Installed streaming qualification code reviews

Scope: integrated feature-gated native installed drivers for eight separately
scheduled cohorts, nonpublishing candidate/installed workflows, Node provenance,
artifacts and budgets, and matching release source fingerprint. Beads
`marmish-8ka.5`; [phase design](../phases/streaming-installed.md) and
[design review](streaming-installed-design.md).

The current worktree integrates canonical copy/stream runtime at `23105d8`, strict
physical cleanup observations, positive cross-platform cells, actual checkpoint
interruption/resume drivers, and independent stream/rsync/copy edge waves.
Released 0.2.3 cannot qualify features it does not contain. No candidate workflow
has been dispatched, no hosted platform is qualified, and no release is published
by this increment. The historical checkpoints below describe their source at the
time, not the final integrated source. Final integrated source approval and
author checks are recorded below; hosted execution remains pending.

## Historical scaffold reviews

## Round 1

Reviewer: Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session `924d51eb-da97-421a-a800-91385216149d`; 102.561s, 15 turns.
Verdict: **REQUEST_CHANGES**, one medium finding.

The shared `checkRun` allowed the new streaming candidate workflow for release
publication callers as well as installed validation. Later eligibility checks
still refused publication, but this weakened the intended first fence. The fix
restores a release-only default and passes the frozen two-workflow allowlist only
from installed `validateCandidate`. A regression calls real `inspectCandidate`
and proves refusal at its first API response, before artifact or git access.

Reviewer independently ran the full Node suite (119 passed) and existing release
automation workflow Python tests (4 passed). Inspection found the remaining
candidate source/run/attempt binding, fingerprint closure, old matrix/diagnostic
behavior, cohort timing, workflow permissions and bounded artifact exposure
consistent with the scaffold design. No source edits, Cargo, builds, network,
live relay tests, workflow dispatches or publication were performed.

## Round 2 scoped follow-up

Reviewer: Claude Code `claude-sonnet-5-5`, auto.
Session `a226b263-60de-4111-b3e1-930f94557aec`; 13.858s, 3 turns.
Verdict: **APPROVE**, M1 resolved with no new blockers in scope.

Reviewer verified the default release-only gate, installed-only explicit
allowlist, unchanged diagnostic restriction, exact streaming dispatch/head
binding and real publication refusal regression. Independently reran the full
Node suite: **120 passed, 0 failed**, approximately 1.06s. Cosmetic duplicate
module imports were noted and left unchanged.

## Author validation and integration boundary

- Full hermetic Node suite: 120 passed, 0 failed, 1.071s.
- Both new YAML documents parse; `bash -n` passes all 22 workflow run scripts
  after replacing Actions expressions with inert values.
- `git diff --check` passes. Ripwire quality delta reports zero regressions.
- Fingerprint parity tests compare both candidate producers and walk the native
  crate's local Cargo dependency closure, including engine, stream, core and vault.
- Timing tests preserve old matrix 2220s lifetime/+900s watchdog/+600s cleanup;
  six streaming cohorts use 3300s/+420s/+300s and separately specify server
  admission/readiness/traffic/cleanup arithmetic.

The new `streamingServerBudget` helper is an executable policy specification for
later native driver implementation; the Node supervisor enforces outer lifetime,
watchdog reserve and artifact cleanup only. The nominal 62-minute deadline is
cooperative; the independent Actions job timeout is 65 minutes.

Native integration must canonicalize and hash the maintained absolute rsync
image provided to preparation through `MARMISH_TEST_RSYNC`, persist it in private
state, implement strict context/record/receipt and cleanup validation, and enforce
the inner server budgets. The filtered child environment deliberately does not
inherit arbitrary rsync paths. Source fingerprint compatibility alone establishes
no platform result. No installed latency or throughput values are available yet.

## Native positive-matrix checkpoint, round1

Claude Code Sonnet5.5, `--permission-mode auto`, session
`941f9046-7ad9-4590-ad3a-4c65b4306368`, 239.794s and23turns:
**REQUEST_CHANGES**. This review covered the feature-only stream, rsync and copy
positive-matrix driver and finite-driver compatibility at33753e2 plus the
uncommitted installed increment. It ran no Cargo, network or live tests.

The two high findings accurately identify integration dependencies: this worktree
has neither the canonical native-copy CLI/protocol nor the additive physical
stream-cleanup metadata developed in the sibling copy increment. The driver
refuses missing proof; its parser tests do not establish a functioning installed
cohort. These findings remain open until canonical integration and actual
product-to-driver round-trip tests. A retained copy receiver namespace must be
validated with the canonical checkpoint decoder before it can be excluded from
file inventory; ignoring its name is insufficient.

Useful findings being incorporated include finite failed/skipped cell evidence,
terminal/no-cancel journal checks, client destination inventories in artifacts,
independently recomputed literal argument receipts, fixture-control launch proofs
and counts, hashed public stream/session/transfer identifiers, and continuing
owned cancellation attempts after one error. The author also found and tightened
the common readiness/traffic/stop cutoffs to the earliest server start.

The request to require a server-owned `stream_stop_complete` observation from the
client daemon is declined: the canonical emitter deliberately reports that
optional field only for a daemon with admitted server Open state. The client
still requires its actual physical stream-cleanup status and confirmed engine,
MDK and runtime shutdown. A missing server-only field is never interpreted as
proof of server cleanup. Finite candidates retain their existing separate checks.
This distinction and the exact additive DTO must match the integrated design.

Author checkpoint: feature-enabled native installed tests **104 passed,0failed**
in14.95s, before these review fixes. No workflow dispatch, candidate build,
hosted-platform or relay qualification has occurred. Follow-up compile/tests and
independent review are required; this is not native implementation approval.

### Native positive-driver follow-up validation

The revised installed-only source compiled against `33753e2` with the
`installed-checks` feature. The complete installed subset initially reported
105 passes and one new fixture failure: the rollup regression used the default
temporary-directory allocator and received `unsafe_profile_path`.
The fixture now uses the existing canonical, explicitly mode-0700
`journal::private_test_directory` helper. The two affected rollup/barrier tests
then passed (0.55 seconds); the fixture fix does not weaken production path
checks. Logs: `/tmp/marmish-installed-native-r2.log` and
`/tmp/marmish-installed-native-r3.log` (local diagnostics, not committed artifacts).
This is hermetic driver validation only. Canonical copy and actor observability
remain integration prerequisites; the checkpoint observer and interruption
validator are unlinked until that integration. No candidate build, hosted
platform execution, relay transfer or installer qualification is claimed.

### Local pure copy API import for qualification development

At the parent's direction, the two canonical standalone copy modules were copied
exactly from the local `/home/openclaw/repos/marmish-copy` worktree while the
complete Phase 3 integration is still pending. Their captured SHA-256 values are:

- `crates/marmish-stream/src/copy_protocol.rs`:
  `cd25b010838a5f2d29d49550abe7ea99092064c5303d727a05efb1868612cbf2`
- `crates/marmish-native/src/copy_files.rs`:
  `42d4d2ff113c78e3b779912242cfc4555a681b555d4312898b2ab8950ac55958`

This development import permits real decoder/receiver and owned-child tests.
Only the pure protocol and feature-gated filesystem primitive are linked; no
copy CLI, actor, runtime GC, installed support or production qualification is
implied. The full Phase 3 commit must be merged and overlaps reconciled before
qualification activation or publication. These are source provenance checksums,
not released package checksums.


### Native checkpoint and interruption driver validation

The pure filesystem source was refreshed from the frozen canonical copy worktree
before compilation. Its final captured SHA-256 is
`750b4dfa315799d520e8f583b527fb29feb70a5cf9df2a0dcf78b36953497a2b`;
the protocol checksum above is unchanged. The full installed feature subset now
passes **112 tests, 0 failures, 16.23 seconds**. The new receiver observer,
owned-initiator detach and interruption evidence subset independently passes
6 tests. Local diagnostics are `/tmp/marmish-installed-native-r6.log` and
`/tmp/marmish-installed-copy-observers-r1.log`.

Compilation first caught a fixture-only API mismatch, then the real receiver
inventory test exposed the fixed parent admission lock as an extra payload file.
The observer now validates that exact empty, private regular lock through the
owned parent descriptor before excluding its exact basename alongside the
validated transfer namespace. Unknown namespaces remain visible, and a modified
admission lock is rejected. These checks neither acquire cleanup authority nor
remove retained receiver state.

The copy positive and interruption execution paths compile, but cannot yet run
against this baseline's production CLI: the complete Phase 3 actor, frontend and
physical-cleanup metadata must still be merged. The transfer-edges cohort is
still deliberately refused until its complete execution driver is integrated.
No hosted platform, actual installer, candidate package or live relay result is
claimed by these hermetic tests. Four unused pure-filesystem API warnings remain
pending that canonical integration; no lint suppression was added.


### Copy execution driver scoped review, revision 1

Claude Code Sonnet 5.5 in `--permission-mode auto` reviewed the checkpoint,
initiator detach, interruption execution and evidence paths without Cargo,
network or live tests. Session `f1e52a6b-410b-4e70-beb6-20b579cefcea` took
279.023 seconds and 26 turns; the verdict was **REQUEST_CHANGES**. The earlier
112 passing tests predate these review fixes and do not validate them.

The required correction was resumed goodput: the first intermediate checkpoint
can advance before the initiating CLI is killed and remote cancellation finishes.
The driver now reopens and validates the old receiver checkpoint after confirmed
physical cancellation, before Resume, and records that stable prefix separately.
Resumed payload accounting subtracts this stable prefix, not the earlier live
sample. A complete or regressed prefix fails qualification.

Useful small findings were incorporated: server Busy evidence requires nonzero
exit; GET disconnect evidence claims a live, charged helper rather than unproved
transport backpressure; Busy-before-cancel is explicitly driver-enforced ordering,
not an independent rollup timeline; observer timeout includes only a finite last
error category; absolute receiver parents must already use the canonical owned
path; interruption execution reserves 60 seconds for reconciliation; the Busy
parser has negative fixtures; and owned initiator cleanup has an independent
post-return existence probe in addition to requiring SIGKILL and reap.

The reviewed 1.25 MiB interruption refinement is recorded in the design review.
These source changes still require compilation, affected tests and independent
follow-up review. Full Phase 3 frontend/actor integration, remaining edge
orchestration and installed three-platform qualification remain prerequisites;
no scaffold or pure-module test is presented as an installed platform result.


### Copy execution driver scoped follow-up, revision 2

Claude Code Sonnet 5.5 auto, session
`21ea351b-57b8-4123-8f4f-be1912c0aa1a`,95.400seconds,16turns:
**APPROVE** within the positive/interruption checkpoint-driver scope. The
reviewer inspected all required r1 corrections and the relevant canonical lock
ordering without edits, Cargo, tests, binaries or network operations. They did
not independently run the author's tests. This is not overall Phase4 or hosted
qualification approval; absent Phase3 actor/frontend integration, actual byte
checkpoint behavior and client-detach cleanup remain explicit prerequisites.

The useful low findings were incorporated: equal live/stable durable offsets
must also have identical prefix hashes; the negative evidence test covers that
case. The phase prose now names exact launch-derived namespace lookup and the
1140-second execution plus60-second reconciliation split. Later edge modules
were outside this review. The author's complete installed feature subset passed
**118tests,0failures,17.58seconds** before those final low corrections and the
new rsync module registration (`/tmp/marmish-installed-native-r7.log`).
Strict feature Clippy currently fails on unlinked edge/pure-copy integration
paths and several local style findings; local findings are being corrected
without suppressions. No actual installed platform or live relay PASS is claimed.

## Integrated orchestration review

Claude Code Sonnet5.5, auto, session
`1613edc7-6223-44ce-aee0-184011dc5f10`,34turns,203.946s tool duration
(206.793s wall): **APPROVE**, orchestration scope only. The reviewer ran120 Node
tests and parsed all workflow shell blocks. It verified nonpublishing provenance,
eight closed cohorts, explicit install-script invocation, bounded cooperative
and Actions deadlines, immutable artifacts and unchanged release-only gates.
It did not review full native drivers or perform Cargo, relay or hosted tests.

Useful nonblocking findings are incorporated before dispatch: missing platform
artifacts remain absent for native sanitized FAIL reporting; Homebrew acquisition
is bounded and its installed formula/version/bottle status pinned; a Node/Rust
budget crosscheck catches constant drift; watchdog/nonpublishing assertions and
source fingerprint closure include inspection/license packaging inputs. Historical
scaffold wording is retained only under historical headings. Follow-up review
must cover these bounded changes and native rollup integration.

### Orchestration follow-up, revision 2

The same Claude Code Sonnet5.5 auto reviewer approved the bounded orchestration
corrections:65.721seconds,10turns (68.239seconds wall), session
`1613edc7-6223-44ce-aee0-184011dc5f10`. It ran121 Node tests and shell syntax
checks, with no Cargo, network, dispatch or hosted execution. The follow-up
verified missing-artifact propagation, Homebrew package pinning, bounded setup,
watchdog guards and fingerprint parity. Its four low findings remain distinct
from native driver qualification.

The phantom inspection-module fingerprint entry was removed from all three
mirrors. Tests now prohibit cargo publication, OIDC permission and GitHub API
publication in the candidate workflow, associate the five-minute limit with the
rsync step, and tie named Rust budget expressions to the Node bounds. The
updated Node suite passed121tests in1.079seconds. The native immutable rollup
writer's partial-publication contract is receiving a separate design review;
this orchestration approval does not resolve that native concern or claim any
hosted platform result.

### Report-publication design correction

A separate actual Claude Code Sonnet5.5 auto review, session
`4b52899c-495b-4190-91e4-96f7b03aba8b`, requested changes in73.340seconds/5turns:
JSON-last ordering alone cannot cover an error after the writer's successful
hard-link. The revised contract adds an authoritative immutable failure marker,
fresh-output preflight, complete rendering before output publication, original
error preservation and the green-job/complete-JSON/absent-marker consumer rule.
The follow-up **APPROVE** took20.572seconds/1turn. It accepted the design only;
implementation and fault-injection tests are pending. No Cargo, network, hosted
or live execution occurred in either review. The reviewer clarified invalid
context behavior, non-authoritative Markdown and RAII-scoped test faults; all
are recorded in the phase contract. Legacy finite matrix work is Beads
`marmish-yc6`, outside this implementation scope.

## Integrated native driver review, revision 1

Claude Code Sonnet5.5 auto, session `108af339-f9c9-4449-802e-f75eef891dd1`,
returned REQUEST_CHANGES. The final machine-readable response contains only
the final summary; the full report below is the actual reviewer assistant text
recovered from the same structured session, not an invented combined verdict.
The reviewer delegated bounded read-only subreviews and personally checked the
closed case-ID mismatch. No Cargo or live/platform execution was authorized.

The review verdict is **REQUEST_CHANGES**, on one blocking finding: five edge-copy case IDs in code don't match the closed IDs in the spec. I read the code and ran only the Node tests. I did not run Cargo or any build, relay, workflow or git mutation, and nothing in the review is platform evidence. I read most files directly. For the edge-copy, copy-interrupt and edge-stream/transfer-case files I relied on sub-reviews, which I checked only on a few points. The checks I did confirm: the case names in `installed_edge_copy.rs:68-80`, the `third_never_started` constant, and that the macOS workspace is canonical in CI.

## Blocking

**B1. Edge-copy case IDs differ from the spec's closed table.** `installed_edge_copy.rs:68-80` (`Case::name`) is confirmed against `streaming-installed.md:591-592`:

| Code | Spec |
| --- | --- |
| `existing-refusal` | `existing` |
| `symlink-source` | `source-symlink` |
| `directory-source` | `source-directory` |
| `resume-changed-source` | `resume-source-changed` |
| `resume-changed-binding` | `resume-binding-changed` |
| `resume-corrupt-checkpoint` | `resume-checkpoint-corrupt` |

- Keys are `{put|get}-{name}`, and `expected_keys()` and `validate()` agree with the code names.
- Nothing outside Rust consumes these names today, so there is no runtime failure.
- The spec says the closed membership is exact, so either rename the code or amend the spec table. The fix is mechanical.

## Non-blocking, by decreasing importance

1. **macOS EPERM on group kill.**
   - `reap_owned_group` (`installed_edge_rsync.rs:226`) tolerates only ESRCH. If macOS returns EPERM for a group whose only member is an unreaped zombie leader, rsync and edge-stream cases would fail on macOS.
   - That fails closed, not a false pass, but it could block macOS qualification. I could not verify it here.
2. **No dwell for "no implicit kill on disconnect"** (`installed_edge_signals.rs:296-300`).
   - It is proven by one `running()` poll after the client exits.
   - A delayed server-side kill followed by the explicit cancel would still end as `Cancelled` and pass.
   - Suggested fix: require several "still running" polls over a fixed interval, or a fixture marker written after the disconnect.
3. **`stream-signal-closed-output` can pass as a plain INT** (`installed_edge_signals.rs:285-288`). The observation never shows an EPIPE. The spec already disclaims kernel-surfaced EPIPE, so this is acceptable if the case is described that way.
4. **Quota third stream is not reconciled against the server journal.** `third_never_started` is hard-coded true at `installed_edge_quota.rs:267` and `refused_stream_sha256` is never looked up. The "no launch" claim rests on marker absence plus status counts. The quota `cancel()` also doesn't check the "confirmed" stderr that `second_cancel` checks.
5. **`detach` flag checks are tautological.** `detached_helper_live_and_charged` and `second_client_cancel` are set from `case.detach()` (`installed_edge_signals.rs:323-324`), so `validate` can't verify them on deserialized evidence.
6. **Rsync pull has no empty or absent local pre-inventory.** A stale identical tree would pass. Also, no per-direction compress flag is recorded in `Pair`, so rollup can't confirm "opposite settings" independently.
7. **Copy duplicate-binding tolerance** (`installed_edges.rs:98-104`). Identical `(binding, terminal)` duplicates are accepted for all of Copy. Only the seed's binding should repeat; duplicate index-0 bindings should be rejected.
8. **Binding-refusal case accepts a missing stream record** (`installed_edge_copy.rs:408-433`). Nothing requires the put-direction record to exist.
9. **Weak preservation and identity checks in edge-copy.**
   - Refusal preservation compares only sha, mode and mtime, not the inode (`:457-465`).
   - `alternate.try_exists()` follows symlinks (`:1033`, `:1062`).
   - The identity check at `:929` stats the same fd twice, so it is vacuous.
   - `invoke` maps `Timeout` to `Invalid` (`:361-380`).
10. **Copy-interrupt validator gaps** (`installed_copy_interrupt.rs`).
    - The wall-time bound is 1200 s (`:74`), not 1140 s.
    - `validate()` doesn't bind `put_busy` session/group to the old stream, or the payload hash to the deterministic digest (`:80-91`).
11. **`copy-interrupt-put` flake risk** (`installed_copy_interruption_cases.rs:47-51`). The payload repeats every 256 bytes and `--compress` is on, so on a fast relay the transfer may finish before the 20 ms poll sees the checkpoint. That would be a false FAIL, not a false pass.
12. **Edge failure records have no per-case detail.** They are `complete:false` with an empty `failed_cells`.
13. **PreOpen state check.** `confirm_server` doesn't check `state=="hello"` for the PreOpen record. Its other checks imply it.
14. **Edge rollup markdown prints "Speed 0.000"** although `payload_goodput_measured` is false.
15. **`gather` change.** It now tolerates missing records for legacy cohorts too; the native rollup turns that into a FAIL.
16. **Weak Node check.** The test that native budget constants mirror the Node ones is a string-contains check.
17. **Unconfirmed assumptions.** Whether `remote_terminal == Exited{1}` holds for get-existing-refusal, and whether a missing-executable path returns NotFound rather than Denied, depend on production behaviour I did not read.

## Checked and consistent with the spec

- **Cohorts and budgets:** the eight closed cohorts, the 3300 s lifetime, the 720/180/1200/180 s budget arithmetic and the 62/65-minute watchdog. The old `transfer-edges` name is rejected.
- **Membership counts:**
  - edge-stream 9 observations (14 relay attempts plus one local call).
  - edge-rsync 6.
  - edge-copy 22 keys (20 cases plus 2 seeds).
  - rollup constants 27/18/60 observations and 42/18/66 relay attempts.
- **Deadlines:** one 1140 s execution deadline per edge family with a 60 s reserve, and no per-case renewal.
- **PreOpen:** unreaped server leader, WSTOPPED proof, queued Hello required, exit 255 with the exact diagnostic, RAII resume before the mutex drops, no server IPC while paused, and the Hello-only record kept until final cleanup.
- **Cleanup and cancellation:**
  - PID/PGID authority comes only from live `Child` handles.
  - Rsync group cleanup retains the leader via WNOWAIT before the group kill.
  - Stream status and stop-report checks are strict, and server `stream_stop_complete` is required true.
  - SIGKILL/EPIPE detach sends no implicit cancel; the cancel is explicit from a second client.
- **Copy checkpoint and interruption:**
  - The checkpoint observer uses the canonical decoder with NOFOLLOW and inode binding and hashes the prefix.
  - Corruption touches only owned state after the full idle check.
  - Checkpoint, Busy and cancel ordering is as specified, and resume uses the stable pre-resume prefix.
  - Rollup arithmetic can't underflow or divide by zero.
- **Provenance:** workflow permissions, candidate allow-list, source fingerprint and artifact binding include the cohort name.

## Tests actually run

- Node, hermetic: `node --test` in `tools/v02-actions` gave 121 pass, 0 fail. The three relevant files (`workflow`, `streaming-policy`, `transport`) gave 43 pass.
- Not run: Cargo and its native tests, clippy, actionlint (not installed), live relay, macOS or ARM execution, and the private-binary relay test.

I'd approve after B1 is fixed. I'd also address items 2, 4, 5 and 7 as quick hardening, and item 1 if macOS EPERM shows up on the live run.

## Integrated revision 1 dispositions and scoped checks

The author corrected the closed case table to the actual executable identifiers.
The full native review's useful false-qualification findings were also addressed:
quota refusal now reconciles both endpoint records and absence of a third runner;
detach requires at least three real charged, live polls over two seconds; duplicate
bindings are permitted only for exact dedicated committed seed evidence; rsync
pull requires an absent destination; interrupted copy binds the Busy room, original
room, deterministic payload digest and 1140-second execution budget. Copy edge
checks now require the actual Put refusal record, preserve inode/link/owner facts,
inspect names without following symlinks, and recheck the named checkpoint after
corruption. A 180-second per-copy ceiling remains clipped by the family deadline.

The disjoint quota/signals review used Claude Code Sonnet 5.5 auto, session
`09d900ad-1357-42af-89cd-c03fe661c4cc`: revision 1 **APPROVE**, 143.511 seconds,
15 turns; revision 2 **REQUEST_CHANGES**, 81.468 seconds, 9 turns, caught a private
`Codec` import in a followup; revision 3 **APPROVE**, 12.649 seconds, 4 turns,
confirmed its public path. The edge-copy followup used session
`2fa22001-708b-45e1-858b-c995efe09b2b`: revision 3 **REQUEST_CHANGES**, 49.684
seconds, 10 turns, caught the wrong local DTO type; revision 4 **APPROVE**, 8.899
seconds, 2 turns, confirmed the exact projection type and retained all checks.
These were source reviews, not live or platform results. Author compilation and
tests below execute the corrected sources.

## Rollup publication contract review and execution

An actual independent Sonnet 5.5 auto design/code review, session
`4b52899c-495b-4190-91e4-96f7b03aba8b`, examined failure after the immutable writer
has linked a PASS file but returns an I/O error. Design revision 1 requested a
failure marker with explicit consumer precedence; revision 2 approved it. Scoped
code revision 1 **APPROVE** took 49.053 seconds and 5 turns, with no Cargo or live
execution. The author incorporated its missing invalid-context/original-error
assertions and derived the artifact glob guard from the actual workflow.

The implementation renders both outputs before publishing, writes Markdown
before JSON, and publishes `matrix-rollup.failure.json` first on any error. The
marker overrides a surviving PASS JSON; a current-run, current-attempt green job
is also mandatory. Invalid context returns red without writing untrusted output.
Existing files are never replaced. The production immutable record writer is
unchanged; a scoped one-shot test fault models a post-link error. Actual tests
prove that a visible PASS plus publication error receives the overriding marker,
foreign bytes and inode remain intact, the original error survives, failed jobs
cannot qualify complete evidence, and malformed context leaves output empty.
Legacy finite-matrix ordering is separately tracked as Beads `marmish-yc6`.

## Integrated revisions 2 and 3

Same actual native-review session `108af339-f9c9-4449-802e-f75eef891dd1`, Sonnet
5.5 auto. Revision 2 **REQUEST_CHANGES**, 112.361 seconds and 8 turns, found four
remaining documentation identifiers; the author corrected them to
`stream-preopen-interrupt`, `stream-peer-quota`, `rsync-group-sigint`, and
`rsync-timeout-usr1`. Its useful presentation finding was fixed: unmeasured edge
goodput renders as `N/A`, with a direct regression, rather than measured-looking
zero speed.

Revision 3 **APPROVE**, 29.272 seconds and 6 turns, actually inspected the seed
reuse policy and callers, final server hello/refusal and idle reconciliation,
real detach dwell production/validation, interruption room/Busy/payload/budget
validation and canonical server/client rollup consumption. It found no concrete
blocking issue. The separately approved product fix `23105d8` was excluded from
this harness review. The reviewer inspected source and used the earlier Node
execution; it did not independently run Cargo or any platform/live test.

Final author checks at the integrated source with that product fix:

- Installed feature library suite: **143 passed, 0 failed**, 23.96 seconds.
- Native feature `--all-targets` strict Clippy: **passed**, 19.44 seconds.
- Workspace formatting: **passed**.
- Full Node suite: **122 passed, 0 failed**, 1.089 seconds, including the actual
  workflow upload-glob/marker guard and explicit nonroot native test gate.

The candidate's unfiltered native/engine suites explicitly refuse UID 0, so
root-skipped actual child-spawn fixture bodies cannot silently qualify those
runners. The exact macOS alias and closed-stdio/original-FD3 tests are scheduled
there, not claimed as already executed on macOS or ARM.

The quality-delta pass is **not clean**: against `23105d8` it reports 239 findings,
89 gating. Some globally named symbols are misattributed across unchanged files,
but real new validation hubs remain, including `build_rollup` (complexity 39),
`Evidence::validate` (36), transfer `copy` (33), `collect_receipts` (29), and
`Pair::validate` (25). The coherent build/render/publication separation was made
for the tested publication correctness fix. Further broad restructuring is
explicitly deferred while qualifying this increment; the compiler, tests and
independent review do not erase that maintenance debt.

No candidate dispatch, actual installed matrix, hosted macOS/ARM execution or
release publication has occurred in this increment. Kernel EPERM remains a
conservative failure; production refusal diagnostics and checkpoint timing must
still pass the actual hosted workload. Requested rsync compression settings are
not independently measured negotiated-codec evidence. Closed-output INT evidence
does not claim that the kernel EPIPE branch necessarily won the race. Failed
cohort records remain red even when detailed per-case failure cells are absent.

The final candidate gate also sets
`MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS=1` only in `Verify native components`.
This follows the separately reviewed permission-fixture contract: a writable
mode-0500 receiver parent must fail the hosted gate even with a nonroot UID, so
capability-based permission bypass is not counted as coverage. The matching
workflow policy regression passes; the complete final Node suite remains
**122 passed, 0 failed**, 1.083 seconds. The native source and tested binaries are
unchanged by that workflow-only addition. The reviewed qualification amendment at
`3d6fc8f` was merged without conflicts; its execution gates remain pending.
