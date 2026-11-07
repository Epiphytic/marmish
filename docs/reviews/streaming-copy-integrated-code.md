# Native copy integrated code review

Round 1: **REQUEST_CHANGES (incomplete review coverage)**. Actual Claude Code
Sonnet 5.5, `--permission-mode auto`, session
`f1d1c701-8e54-49fc-9a62-2b51f75a327a`, 287.796 seconds, 26 turns.
Read-only source review; no reviewer tests, because the owned Cargo and real
relay suites were running. No confirmed blocking code defect was reported.

The reviewer inspected helper FD/image launch, early helper dispatch, server
admission and portions of client/worker/GC integration. Approval was withheld
because protocol/transport decoders, filesystem commit/recovery internals, some
CLI/daemon integration and integration tests remained unread; files also changed
during the review. Earlier limited-scope approvals do not imply complete phase
approval. Followup bounded reviews are inspecting the remaining code and final
frozen worker/lease/GC implementation.

Known design limitation retained: native image validation is a pathname-based
consistency guard; a same-UID process can replace the image before exec. It is
not an authentication boundary. The optional helper stderr-descriptor check was
suggested; descriptor validation and actual launch behavior remain in scope for
followup review. The reviewer reported lost notes after context compaction;
no unrecorded suggestion or approval is assumed.

Validation remains separate: current strict Clippy passes and local receiver GC
has 15 passing tests. The early real-relay copy attempt timed out, as recorded
in [runtime evidence](../validation/copy-runtime-live.md). New-build integration
and live qualification, and installed platform qualification, remain pending.

## Round 2: remaining product source

**APPROVE, bounded source scope only.** Actual Claude Code Sonnet 5.5 auto,
same session, 130.095 seconds, 24 turns. No reviewer execution. The pass read
all transport session logic, copy framing/context/checkpoint decoders, wire
diff, filesystem create/resume/commit/recovery/persistence, client commit
uncertainty, CLI routing/signals/receipt operations and daemon error mapping.
`stream_runner.rs` has no phase diff. It found no source blockers.

This pass explicitly did not approve integration-test bodies, filesystem test
bodies/reclaim, or final worker/CLI/IO lifecycle revisions. Separate bounded
reviews cover those remaining areas. Source-only approval does not replace live
or installer qualification.

Useful documentation suggestions: an incomplete checkpoint directory left by a
crash is retained for manual recovery; immutable request deadlines require a
fresh attachment after expiration; a local Get destination can already be final
when failure to send its Commit confirmation reports uncertainty. Image pathname
races remain a documented same-UID consistency limit. Early Data-header rejection
and a more specific unsupported preserved-mode diagnostic are nonblocking
followups, not implemented claims.

## Lifecycle round 1

**REQUEST_CHANGES.** Actual Claude Code Sonnet 5.5 auto, session
`ed93abb6-2038-42ea-813a-4a5872960607`, 220.872 seconds, 28 turns.
The reviewer verified worker-join-before-release, independent owned lifetimes,
current durable activation/claim retry checks, replay-floor/cap trimming,
startup pruning, anchored physical cleanup and actor-only SQL routing. It
withheld approval because the supplied logs predated the owned-thread executor
change. New checks subsequently passed: 17 local-GC tests, 4.10 seconds; strict
all-target Clippy with warnings denied, 4.62 seconds; native binaries, 9.65 seconds.

A useful substantive finding was altered Release requests for one revision:
NeverSubmitted could follow Attempted, or Attempted could downgrade Established.
The implementation is adding an exact final-disposition marker and lifecycle
state guards so identical release retries remain idempotent and changed releases
refuse. The retry-backoff exponent is also being corrected. Followup tests and
actual review remain required. Missing frontend barrier and broader fault tests
remain visible; unit checks alone do not prove client death/cancel behavior.

## Test-body round 1

**REQUEST_CHANGES.** Actual Claude Code Sonnet 5.5 auto, session
`377a05f4-f084-495c-bf59-8e0c8ffc0506`, 116.907 seconds, 18 turns.
It read all requested test bodies and ran Python compile validation only.
The helper integration uses actual native subprocesses and byte/hash assertions,
and filesystem tests inspect real descriptor/inode identities. However, the
pre-cancel fixture accepted an incidental error, resume did not prove retained
prefix reuse, helper-path mtime assertions were missing, and commit-loss and
session edge coverage was incomplete. Tests are being strengthened rather than
counting their earlier pass as evidence of those behaviors. The relay harness
will verify the negotiated codec diagnostic and exercise compressible as well as
random binary input. Pending actual fault/installed qualification is distinct
from local function tests.

## Relay harness followup

**APPROVE, bounded test-tooling scope.** Actual Claude Code Sonnet 5.5 auto,
session `76f1decc-4a9a-41d7-abf2-3e07a274f95a`. Round 2 took 61.137 seconds,
10 turns; round 3 took 15.630 seconds, three turns. The reviewer inspected the
copy harness and backward-compatible shared `default_bytes` parameter, ran
Python compile and offline argument-default checks, and checked the negotiated
codec diagnostic against validated Open. It did not run live transfers or Cargo.

Useful findings incorporated: negative cases require empty stdout and no success
marker; every transfer retains stderr only in private diagnostic state, including
exit-0 assertion failures; reports explicitly label `small_smoke` versus
`default_window_crossing`. Both the default 2 MiB+8192 payload and explicit
smaller values are reported honestly. Byte equality and Python SHA256 are
validated against a disk oracle; the commit marker carries a transfer ID, not
an independently reported Rust checksum. Codec negotiation does not establish
wire compression ratio. Leading-dash relative path, default metadata behavior,
files larger than 65 MiB, and all listed live/platform gaps remain unclaimed.

## Lifecycle round 2

**APPROVE, bounded lifecycle source and test scope.** Actual Claude Code
Sonnet 5.5 auto, session `ed93abb6-2038-42ea-813a-4a5872960607`,
53.025 seconds, nine turns. It read the previously unread local-store tests,
verified exact per-revision release dispositions, release state guards, corrected
retry backoff, worker join before Release, owned-thread timeout behavior, and
current-state replay checks. The 19-test local-GC log passed; final-tree strict
Clippy was still pending and was not inferred from older logs.

This approval predates the production command-room lookup correction discovered
by the live Get failure; that change needs its own review and actor regression.
Positive Activate and signal-through-frontend ordering were not proved by these
unit tests. Live/installed tests remain separate. Conservative unknown ownership
and manual cleanup behavior remains intentional.

## Test-body round 2

**APPROVE for source, pending execution.** Actual Claude Code Sonnet 5.5 auto,
session `377a05f4-f084-495c-bf59-8e0c8ffc0506`, 53.079 seconds, seven turns.
The reviewer confirmed the exact Get pre-cancel category and natural helper exit,
production-frame resume offsets and byte counts, corrupted retained-prefix
HashMismatch, nanosecond metadata assertions, real Put checkpoint disconnect and
lost-Commit reply recovery, forced production residual credit, new session retry/
ACK/EOF/denial tests, and lost-transport abort uncertainty. It did not execute
Cargo. At review time the large four-direction helper test was still running;
it subsequently completed: six passed, zero failed, one expected helper-entry
ignore, 102.67 seconds.

The session/abort cohorts still require their final execution logs. Put network
fault coverage does not establish Get disconnect behavior. Cancellation under
full buffers, during sync or commit and actual client death remain acceptance
work for installed testing. The approval does not cover unexamined helper,
server admission or GC implementations; those have separate bounded reviews.

## Command-room route review

**APPROVE product correctness, execution initially pending.** Actual Claude Code
Sonnet 5.5 auto, same lifecycle session, 48.252 seconds, eight turns. The review
checked the binary command-room session schema, Client direction, current peer
and revocation filter, local principal binding, persisted group checks, and
role-independent daemon retention. The initial test fixture failed config schema;
the corrected fixture subsequently passed with the actual OpenCopy→Activate
extension in the 20-test local receiver cohort (12.24 seconds).

The review also identified that current room revocation needed checking again
at fresh and cached Activate: revoking a command room retains its stream records.
That guard and regression are being added and require followup review. Wrong-peer,
server-direction and unknown-session fixture coverage remains a separate useful
negative-test suggestion. No stale stream is assumed safe solely because a
matching durable Open record exists.

## Final activation and actor-test followups

Activation guard **APPROVE, source/test logic**: actual Claude Code Sonnet 5.5
auto, lifecycle session above, 24.490 seconds, four turns. Fresh and cached
activation both require the current exact Client command room, group, session,
peer, local principal and non-revocation. The final 20-test receiver cohort
subsequently passed in 19.20 seconds, including both revoke windows. Final
strict all-target Clippy also passed.

Authenticated replay/withdrawal test review used actual Claude Code Sonnet 5.5
auto, session `4faf8d60-5f12-4f1c-9672-692c8afa544c`:
round 1 REQUEST_CHANGES (94.582 seconds, 12 turns), round 2 REQUEST_CHANGES
(58.884 seconds, five turns), round 3 **APPROVE, static scope** (30.499 seconds,
three turns). Useful findings made the assertions discriminate: sentinel response
slots prove both duplicate Opens were processed and revoked Status did not
rewrite a cached reply; fresh signed outer events avoid event-ID dedup; positive
Status and reset budgets exclude rate-limit confounding; administrative Cancel
asserts Cancelled terminal state and physical runner completion. The test
subsequently passed in 6.73 seconds. Strict Clippy passed afterward (12.323
seconds command time).

The actor test explicitly installs its real owned child at the preparation seam.
It proves authenticated replay projection, immutable admission/runner retention,
revocation and administrative cleanup; it excludes actual engine preparation
and must not be presented as an independent end-to-end duplicate-spawn test.

## Final runtime coherence review

**APPROVE, source only, conditional on relay/installed qualification.** Actual
Claude Code Sonnet 5.5 auto, integrated session above, 122.688 seconds, 11 turns.
It read all runtime `copy_io`, `copy_helper`, `copy_worker`, and the CLI ownership/
cancellation/teardown scope, using its earlier framing/filesystem/session/engine
context reads. It checked distinct disconnect versus Cancel, physical worker
join before local Release, bounded frame/hash/credit/ACK handling, post-commit
uncertainty and hidden-helper context/dispatch. It inspected validation summaries
but ran no Cargo or network tests. No source blockers remained.

It did not reread every helper/worker test body or other scopes already covered
by independent reviews. A nonblocking helper post-receipt error-mapping edge is
tracked in Beads `marmish-5i4`: a faulty post-commit peer or Commit-send failure
can return a finite error although the destination is committed. Resume still
recovers the durable receipt. Cancelled worker futures are not reusable; current
CLI shuts their worker down. Long helper qualification used debug builds.
Live transport, actual cancellation/resume waves, installed binaries and macOS
qualification remain separate evidence gates.

## Large-file filesystem test followup

Actual Claude Code Sonnet 5.5 auto review, session
`1723b6b0-0be0-4280-ac86-323851cac8ff`: round 1 REQUEST_CHANGES
(88.156 seconds, 11 turns), round 2 **APPROVE** (20.462 seconds, three turns).
Scope: the two added `copy_files.rs` test bodies and actual Source/Receiver,
Manifest validation and error-conversion contracts. Product source is unchanged.
The reviewer ran no tests or network operations.

Findings corrected an invalid 0400-mode assumption, required actual suffix
truncation after a halfway drop, isolated invalid UTF-8 filenames to Linux,
removed a cosmetic direction toggle, and strengthened exact checkpoint and
sibling-entry assertions. Unsupported modes now have a separate pre-staging
refusal test. Both tests ran in the 28-test filesystem cohort: PASS, 21.46 seconds;
strict native all-target Clippy PASS, 12.47 seconds. This closes the source-review
execution condition for these tests, without implying codec/helper/relay or
macOS execution. The receiver preserves the manifest's 0640; this test does not
independently prove source metadata-to-manifest mapping.
