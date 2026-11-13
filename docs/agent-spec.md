# Marmish native v0.3 agent specification

**[v0.3.0 is published](https://github.com/Epiphytic/marmish/releases/tag/v0.3.0).**
The reviewed [publication amendment](phases/v0.3-caveated-publication.md) permits
this explicitly caveated release of the existing streaming, rsync and native
copy behavior. Public installation selects v0.3.0. Both peers need v0.3 for the
new commands, and native copy requires an exact peer version match.
Release source is `72d730f0f99e5029ffcf6e4d83086da2c598fc0c`; annotated tag object
is `f68b659443e9bbfa5dc3c012599add4e6811b838`. Caveated candidate
[42555204478](https://github.com/Epiphytic/marmish/actions/runs/42555204478),
attempt 1, built all three platforms: five jobs passed and two acceptance jobs
were skipped. Native tests/Clippy, installed-feature tools and provider/quickstart
prequalification were skipped; this is packaging evidence, not full qualification.
The earlier macOS cleanup test failed with `RecoveryUncertain` on first TERM;
its cause remains unresolved. Public HTTPS installer run
[42557032770](https://github.com/Epiphytic/marmish/actions/runs/42557032770),
attempt 1, passed all six GitHub/Iris × platform installations and the combined
validation using file-backed test identities. The [release evidence](validation/v0.3-release.md)
records the frozen pre-release source, five passing installed cohorts, failed
copy edge cohort and two undispatched interruption cohorts. These are version
0.2.3 source-build observations, not measurements of released v0.3 binaries.
Full eight-cohort qualification is incomplete; the user authorized publishing
with this disclosed boundary before a separate reviewed correction/benchmark phase.
Tor remains experimental; unattended tests do not establish interactive Keychain consent.

The reviewed [post-release test phase](phases/v0.3-test-cleanup.md) addresses copy
refusal evidence and fresh qualification. Its [benchmark guide](validation/v0.3-operation-benchmarks.md)
defines existing operation clocks and byte numerators; it does not claim new results.

## Pending v0.3.1 patch

The `4fcb107` qualification chain failed at a listed identity artifact ZIP API404
in [edge-stream 42744796757](https://github.com/Epiphytic/marmish/actions/runs/42744796757),
attempt 1, before server enable on AMD. The backend/visibility cause remains
unknown. The [listed-artifact retry phase](phases/v0.3.1-listed-artifact-retry.md)
adds supervisor-only bounded retries for that narrow HTTP outcome; gather,
listing and blob failures retain hard behavior. Fresh source-bound qualification
is pending; this correction does not qualify the failed source or publish 0.3.1.

The subsequent [CI report collection increment](phases/ci-report-gather.md)
extends bounded transient retries to the rollup collector, within its original
120-second deadline and persisted API budget. Listed ZIP API404 can retry;
authentication, metadata, digest, archive and blob404 errors remain hard. CLI
diagnostics retain a closed stage/category instead of hiding every error.
Original streaming run 42885236657/1 passed its three platform jobs and failed
before native rollup; its historical exception is unknown. Exact retained
reports subsequently passed local collection and native validation (9/9 pairs).
The [hosted replay 43173887015](https://github.com/Epiphytic/marmish/actions/runs/43173887015)
passed collection and native validation over those original executions; it does
not relabel the failed run or claim new product tests. At CI-only source
`05bec8b`, [fresh candidate 43174030584](https://github.com/Epiphytic/marmish/actions/runs/43174030584)
passed all eleven jobs and full fifteen-artifact inspection. The ordinary
[streaming workflow 43182899512](https://github.com/Epiphytic/marmish/actions/runs/43182899512)
then passed all five jobs and all nine directed platform pairings. See the
[CI validation and timing measurements](validation/ci-report-gather.md). This
completes the CI increment, not the remaining release qualification cohorts.

The [native CI optimization](phases/native-ci-optimization.md) separates default
production and installed-feature build/test lanes onto independent hosted VMs,
bounded to six build jobs including two macOS jobs. Dependency-only caches have
separate lane/target keys; installed tooling caches downloads only after the
initial six full caches exceeded the 7 GB budget. All tests, strict Clippy and
package/installer/provider
checks still execute. Candidate inspection selects exact legacy eleven-job or
new fourteen-job inventories from the authenticated source workflow; historical
replay explicitly retains its legacy contract. Hosted cold/warm measurements and
cross-source cache/stamping validation are pending; see [capacity and baseline
evidence](validation/native-ci-optimization.md). This changes CI, not runtime
behavior, public installer pins or release qualification requirements.

Candidate eligibility also runs the stdlib workflow trust-policy tests before
version evaluation; drift in the native or provider workflow can fail this gate.

Beads `marmish-d97` tracks the [reviewed patch plan](phases/v0.3.1-release.md).
The native package version changes to 0.3.1; there is no protocol or stored
format change. The patch also includes the bounded live cleanup correction
described below. Product-source cleanup diagnostics are retained,
while ordinary release binaries omit their debug emission. The copy refusal
validator and legacy uninstall changes correct test evidence and fixtures.

The public installer remains pinned to 0.3.0 until trusted publication creates
its installer-only child. Earlier `11e93a1` candidate/preflight evidence is not
qualification of future 0.3.1 bytes. [macOS preflight](validation/v0.3.1-preflight.md)
passed the original combined suites, 80 repetitions and strict release Clippy;
the historical first-TERM cause remains unknown. Final source-bound candidate,
macOS regression, eight cohorts, finite benchmarks and independent audits remain
required. Hold automatic publication with an explicit restoration duty before
pushing the version bump, because the publisher does not gate these cohorts.
No new rollback or lifecycle claim is made. Keep published 0.3.0 immutable.

Candidate [42614762310](https://github.com/Epiphytic/marmish/actions/runs/42614762310),
attempt 1 at `d23fce6`, failed the macOS installed-feature authority-withdrawal
cleanup test: 902 passed, one failed and three ignored. Its combined group/status
diagnostic cannot identify the rejected condition. The
[diagnostic increment](phases/v0.3.1-macos-authority-diagnostics.md) preserves
all cleanup proof predicates while adding precise debug-only observations and
100 targeted repetitions. It is not a runtime correction or release qualification;
the cause remains unknown and publication stays held.

Candidate [42653492263/1](https://github.com/Epiphytic/marmish/actions/runs/42653492263)
at `33138eb` failed the macOS debug installed-feature
`final_handle_drop_releases_full_unacked_output_race` test: 906 passed, one
failed and three ignored. Both Linux package jobs passed, but the downstream
cohorts were skipped; the candidate remains unqualified. Separate exact-source
stress [42653573013/1](https://github.com/Epiphytic/marmish/actions/runs/42653573013)
passed 100 installed-feature authority iterations and 80 default-feature cleanup
repetitions. Those different fixtures and features do not substitute for the
failed full installed-feature suite.

The reviewed [held-status diagnostic increment](phases/macos-held-status-diagnostics.md)
adds failure-only closed wait categories without changing cleanup predicates,
syscalls, signals, grace or fixtures. Its first hosted run failed the worker
metadata guard before tests; the corrected guard at `e461754` then admitted
[run42666220530/1](https://github.com/Epiphytic/marmish/actions/runs/42666220530).
The full macOS installed-feature suite passed 907/0/3 (passed/failed/ignored),
but repetition 22 of the exact race failed after 21 passes; 78 were not run.
The initial TERM proof observed `wait_pid=zero wait_code=zero`: no exit status
was available from that successful nonblocking wait. The kernel cause remains
unknown. See the [sanitized evidence](validation/macos-initial-no-status.md).
The observed diagnostic source `e461754` contains no runtime correction.
The subsequent [owned-waiting correction](phases/macos-initial-no-status-pending.md)
under `marmish-d97.3` adds a live-only one-attempt TERM cleanup API. After the
existing fresh owner validation, only an initial successful zero/zero wait on
proof attempt one may wait within the original cancellation grace when no exit
has already been observed. Waiting claims neither TERM delivery nor terminal
cleanup. The first TERM attempt anchors that grace once; the original one KILL,
drain deadline, complete-proof and reap requirements remain. Every KILL no-status,
attempt-two loss, final-held loss, metadata/identity error and prior-exit loss
remains hard; strict signal APIs, finite workers and Drop retain their contracts.
Portable lifecycle and state tests pass locally in debug and release, alongside
the affected stream-runner family. The new Darwin tests, real macOS execution,
full exact-source candidate and stress/transport qualification remain pending.
See the [review and local gates](reviews/macos-initial-no-status-pending.md).
Diagnostic execution is not release qualification; publication and operation
benchmarks remain pending their required gates.

At subsequent source `86119f1`, ordinary candidate 42594063209/1 and exact-source
macOS regression 42594082226/1 passed. Installed qualification stopped at
edge-copy 42611543208/1 after five passing workflows: the CI coordinator rejected
a GitHub artifact listing before copy execution. Its original response was not
retained, so the failing list predicate remains unknown. Final native semantic
replay, interruption cohorts and finite benchmarks did not execute. The
[artifact-list amendment](phases/v0.3.1-release.md#artifact-list-coordination-amendment)
records bounded CI transport handling and diagnostic changes; it requires fresh
source-bound qualification rather than reusing those partial results. Version
0.3.1 remains unpublished and the public installer still selects 0.3.0.

The retained Python reference and installed-test harness support Linux managed
interpreters lacking Python's pidfd APIs through checked libc bindings. Each
Python API is preferred independently; OS errors never trigger a fallback, and
process identity checks and descriptor-only signalling remain required. The
[focused validation](validation/legacy-managed-pidfd.md) passed 24 tests,
including real libc and setsid operations. Full hosted reference coverage and
global process-cleanup observation remain unqualified; this changes no native
runtime behavior.

The optional reference harness now prepares a fresh, pinned test-only dependency
cache before its existing locked offline fixture builds. A local managed-Python
matched control reproduced an empty-cache setup failure, then passed the same
test and all six fixture tests after preparation. All 27 cache contracts also
passed on managed Python. See [the phase](phases/legacy-offline-fixture-cache.md),
[reviews](reviews/legacy-offline-fixture-cache.md) and
[validation limits](validation/legacy-offline-fixture-cache.md). This adds no
Python, Cargo or namespace prerequisite to installed Rust Marmish; full hosted
reference-suite and platform validation remain pending.

## Historical v0.2.3 publication

**Native v0.2.3 was released.** Source is
`0d849fe4dd92b447aa16591b168fff70abcd732c`; bootstrap-only commit is
`84af2ac715b6af3dcf2e553359200c8834b6bf99`. Candidate
[41926462226](https://github.com/Epiphytic/marmish/actions/runs/41926462226)
passed all 11 jobs: three-platform builds, 388 default and 483 installed-feature
native tests per platform, provider checks and installed quickstarts. Publisher
[41936735554](https://github.com/Epiphytic/marmish/actions/runs/41936735554)
passed all four jobs and published the
[v0.2.3 assets](https://github.com/Epiphytic/marmish/releases/tag/v0.2.3).

At that historical publication, both public scripts installed v0.2.3 and matched SHA256
`164a59a2672b1f2a74afbcd6556d4eed87df80170cabc00bd5638ffad2c88a30`.
The two-source, three-platform HTTPS installer run
[41937495649](https://github.com/Epiphytic/marmish/actions/runs/41937495649)
passed all eight jobs, including six install/setup cells and completed cleanup.
Source and tag were manually mirrored to Radicle and synchronized with Iris;
binary releases are hosted only on GitHub. See
[current release evidence](validation/native-automatic-release.md).

Interactive macOS Keychain consent and recovery of the reported user denial,
the full nine-pair remote command matrix and version-lifecycle qualification
remain unverified. Tor command transport remains experimental. The
[v0.2.2](validation/v0.2.2/corrective-publication.md),
[v0.2.1](validation/v0.2/corrective-release-0.2.1.md) and
[v0.2.0](validation/v0.2/initial-release.md) evidence remains historical.

## Scope and navigation

Marmish executes explicitly authorized remote commands through authenticated,
encrypted Marmot/MLS rooms. The native Rust CLI, daemon and dedicated worker
replace the historical Python application for v0.2. Python sources remain a
legacy reference; Python, uv, Node, Cargo, a browser and WASM are not required
by installed product operation. Some repository qualification tools use Python
or Node outside the product environment.

The architectural plan is [v0.2-plan.md](v0.2-plan.md). Detailed contracts live in
[Phase 2](phases/v0.2-phase2.md), its
[interfaces](phases/v0.2-phase2-interfaces.md),
[retention/ACK amendment](phases/v0.2-phase2-retention-and-ack.md),
[server limits](phases/v0.2-phase2-server-limits.md), and
[native installation](phases/v0.2-phase3-release-entrypoints.md).
Beads tracks work and review dispositions; this document describes behavior.

The supported release targets are Linux AMD64, Linux ARM64 and macOS ARM64.
Current manifests declare glibc 2.39 for GNU/Linux and macOS 15.0 for Darwin.
Other operating systems, architectures and older runtime platforms are not
covered by those assets. Target availability is separate from full qualification. Darwin ad-hoc signing is not Developer ID signing or
notarization; the actual manifest records the signing state.

Direct relays are the route exercised by the passing streaming matrices below.
Full finite-command nine-pair and version-lifecycle qualification remain separate
from those results. Embedded Arti and its foundation evidence remain
experimental command capabilities, with no direct-network fallback after a
Tor failure. No PTY, tmux, interactive shell, Unix-login mapping, root execution
service or general process sandbox is promised by this release.

## v0.3 streaming and file transfer

Current usage is in the [README](../README.md). Use rsync 3.x on both ends with
`rsync -as -e 'marmish rsh' ./source/ SERVER_NPUB:destination/`; `-s` is mandatory.
Global identity/relay options precede `rsh`. `stream`, `rsh` and native copy use
660-second idle and 300-second keepalive defaults; configurable values are
1–86400 seconds with keepalive strictly below idle. Compression is opt-in via
`--compress`, with 8192-byte blocks and a 256 KiB byte window, not a file-size cap.
Copy paths are literal regular-file paths, at most 4096 bytes with no NUL and
subject to filesystem limits. Existing targets require `--overwrite`.
`put/get --resume TRANSFER_ID` must repeat every original path and option,
including overwrite and metadata preservation; resume creates a fresh stream.
A lost commit acknowledgement is uncertainty and can leave a committed target.
Ctrl-C requests confirmed remote cancellation; disconnection/idle timeout does
not terminate execution. Cancellation preserves durable partial data, and
already committed destinations remain committed. `stream-reconcile` acknowledges
possible orphan execution and releases credit; it does not kill a process.
Executable resolution uses the running server daemon's inherited PATH; restart
the daemon after changing it. Executable policies remain additive and do not
confine rsync or copy to a directory. Child processes receive minimal PATH
`/usr/bin:/bin`. Both peers must upgrade together for exact-version native copy.

The [C2 validation report](validation/v0.3.1-c2-validation.md) records exact-source
`4e76ec5` package/macOS checks, six accepted live cohorts and an interrupted-PUT
failure; the chain is not qualified. Its [45 operation measurements](validation/v0.3.1-c2-benchmarks.md)
retain whole-operation timing and compression-request limitations. The exact
hosted interruption subcall remains unknown. A separate real Journal test confirms
the production Busy refusal contract; source inspection establishes that the
fixture's admitted-helper expectation contradicts that contract. The [admission-proof correction](phases/installed-put-admission-proof.md)
changes installed-test evidence only and needs fresh source-bound qualification.
No C2 measurement qualifies rebuilt later-source artifacts. Public v0.3.0 and
the installer default remain unchanged; v0.3.1 remains unpublished.

The following increment history preserves each result's original scope; the
[current release summary](validation/v0.3-release.md) supersedes statements
that installed positive matrices are still pending, without erasing failures.

The [streaming plan](phases/streaming-transfer-plan.md) adds a separate bounded
full-duplex execution path, rsync remote-shell mode, native regular-file copy
and optional block compression in reviewed increments. Phase 1 introduces the
pure Rust `marmish-stream` wire, zlib codec and byte windows. Phase 2a connects
those primitives to the native daemon and `stream` CLI with explicit remote
cancellation; its integrated implementation has independent Claude code approval and passed
Linux AMD64 live-relay qualification. Existing finite commands retain their limits.
The [phase 1 design review](reviews/streaming-primitives-design.md) records the
approved bounds and provisional protocol, and the
[runtime design](phases/streaming-runtime.md) specifies the authenticated launch,
backpressure, cancellation and recovery contract. The reviewed `rsh` adapter
now supports modern rsync with secluded arguments (`-s`); Linux same-host native
relay tests pass for exact trees, deltas, compression, confirmed process-group
cancellation and partial restart. See [rsync evidence](validation/rsync-runtime-live.md)
for rates and uncovered cases. It requires rsync3.x on both ends and uses the
same executable authorization as other commands; granting rsync permits broad
same-UID file access and execution. Native copy has independent source approval and passed the 12-case Linux relay
small smoke and [2 MiB + 8192-byte functional suite](validation/copy-runtime-live-r4.md).
The [66 MiB local helper suite](validation/copy-large-helper-local.md) also passed all four direction/codec combinations.
The [selected corrective relay deadline-margin checks](validation/copy-runtime-live-r5-corrective.md)
passed; installed cross-platform qualification remains pending. None of these streaming
additions is in v0.2.3.

The [native copy design](phases/streaming-copy.md) adds literal regular-file
`put`/`get`, verified commits, explicit overwrite, metadata options, bounded block
compression and transfer-ID resume across fresh stream IDs. A privileged helper
protocol is not introduced: the authenticated server authorizes its canonical
native executable using the ordinary additive execution policy. Allowing that
image also permits its other subcommands with the server UID's authority; there
is no directory confinement or copy-only grant. Helper image revalidation is a
same-UID consistency check with a retained pathname-to-exec race.

The helper requires EOF on its bounded context pipe before decoding or opening
profile state. The source reader allows one absolute second for a transient
writer inherited by a concurrent fork to close; it retains the 4097-byte sentinel
cap and descriptor/build checks. A writer retained longer still causes safe
refusal. See the [portability correction design](phases/streaming-portability-fixtures.md)
for regression requirements; this change does not establish installed platform
qualification.

Candidate [42332308304](https://github.com/Epiphytic/marmish/actions/runs/42332308304)
passed both Linux packages and native macOS verification, signing and packaging,
but failed one isolated installed-tool cleanup test (751 passed, one failed,
three ignored). It is ineligible for installed relay cohorts. The bounded
[owned-group cleanup correction](phases/streaming-owned-group-cleanup.md) retains
the exact Child fence for the final group signal; after EPERM, only bounded
exact-child reap and a fresh zero-signal ESRCH proof permit cleanup success.
Unknown cleanup remains a failure. Candidate
[42346198320](https://github.com/Epiphytic/marmish/actions/runs/42346198320)
then passed all three complete native/package jobs and rendering, including the
macOS cleanup fixture. See [native evidence](validation/streaming-candidate-78e39ee.json).
Its first installed stream matrix failed on all three hosts before server-ready:
the qualifier polled caller-selected sockets while the daemon binds its standard
identity socket under the child's TMPDIR. A real installed Linux reproduction
answered Status and reached relay-active at that standard address. The bounded
[socket correction](phases/streaming-installed-socket.md) also fixes the reused
resolver's latent sixteen-versus-thirty-two-character profile-ID check. Failed
hosted cleanup proofs remain incomplete; separate local stop reports cannot
qualify them. Fresh candidate
[42352121220](https://github.com/Epiphytic/marmish/actions/runs/42352121220)
passed all three native/package/tool gates and rendering, including all four
new socket regressions. See [native evidence](validation/streaming-candidate-86b40cb.json).
Its installed [stream matrix](https://github.com/Epiphytic/marmish/actions/runs/42356736438)
now published all three server-ready records, then failed all nine duplex cells
before any validated receipt. The qualifier passed internal hexadecimal accounts
to public CLI destinations, which require npubs or aliases. The bounded
[target correction design](phases/streaming-installed-target.md) converts only
public argv boundaries; internal account and evidence formats stay hexadecimal.
An installed Linux same-host npub duplex reproduction passed exact binary output,
special arguments and clean stop in about 22 seconds; this does not qualify the
hosted matrix. See [failure and reproduction evidence](validation/streaming-installed-target-failure.json).
The corrected candidate
[42358105270](https://github.com/Epiphytic/marmish/actions/runs/42358105270)
passed all three complete native/package/tool gates and rendering, including
the public destination contract tests. See [native evidence](validation/streaming-candidate-571d98b.json).
Its installed [stream matrix](https://github.com/Epiphytic/marmish/actions/runs/42363063498)
published all three server-ready records. All three clients observed exact
duplex output and exit7 against both Linux servers; every macOS-server cell
failed with an unresolved generic category. All macOS-server cells requested
zlib, so platform and compression are not isolated by this run. Receipt
collection was skipped after local failures; zero uploaded receipts does not
prove fixtures never ran. The final rollup qualifies zero cells. See
[failure evidence](validation/streaming-installed-macos-failure.json).
The separately identified [macOS diagnostic 42367008260](https://github.com/Epiphytic/marmish/actions/runs/42367008260)
passed direct-fixture, raw, zlib and three concurrent zlib observations on one
macOS ARM64 host using one client identity; all cleanup proofs passed. It ran
immutable candidate 571 product/fixture artifacts with a separate e968 harness.
See [diagnostic evidence](validation/streaming-macos-diagnostic.md). This is not
qualification: the original three-identity cross-host macOS-server failure
remains unexplained, and no product fix has been established. Fresh all-platform
candidate [42367702241](https://github.com/Epiphytic/marmish/actions/runs/42367702241)
at e968 passed all three native/package jobs and rendering; see
[native candidate evidence](validation/streaming-candidate-e9686c0.md). Its original
[installed stream matrix 42371657352](https://github.com/Epiphytic/marmish/actions/runs/42371657352)
repeated the same three macOS-server failures while all six Linux-target client
observations passed exact bytes and exit7. The failed rollup qualifies zero cells,
and the sequence stopped before the other seven cohorts. See
[repeat failure evidence](validation/streaming-installed-e9686c0-failure.json).
The independently reviewed [cold three-principal diagnostic](phases/streaming-macos-cold-multiprincipal.md)
passed on macOS ARM64 in [run 42374246025](https://github.com/Epiphytic/marmish/actions/runs/42374246025).
All three concurrent identities preserved exact bytes and exit7 in 42.392–43.915
seconds; all cleanup proofs passed. Its [verified evidence](validation/streaming-macos-cold-multiprincipal.json)
is diagnostic only: same-host traffic does not replace the original cross-host
matrix. No product cause or fix is established; full installed qualification remains open.
The reviewed [original-matrix observation](phases/streaming-matrix-failure-observation.md)
now emits bounded sanitized stage records from that exact duplex test path,
including input/process completion, prefix parsing, List acquisition, terminal
binding and ordered Pair predicates. Its local203-test installed-driver suite
and strict Clippy passed. Fresh candidate
[42377010307](https://github.com/Epiphytic/marmish/actions/runs/42377010307)
at d5addee passed all three native/package/tool jobs and rendering; see
[reviewed candidate evidence](validation/streaming-candidate-d5addee.md).
Its original [installed matrix 42383713647](https://github.com/Epiphytic/marmish/actions/runs/42383713647)
again failed every macOS-target cell. The new observations locate all three
failures at the local CLI stdin `write_all` deadline of 330 seconds, followed
by successful child termination. Partial written bytes and remote execution
state remain unknown: the original failure path discards the drain results.
The CLI can await room setup, Hello, Open or an Exchange before consuming more
input, so this does not establish a remote rejection or a product cause.
All six Linux-target observations passed exact bytes, exit7 and Pair predicates;
the failed rollup still qualifies zero cells. The other seven cohorts were not
dispatched. See [failure evidence](validation/streaming-installed-d5addee-failure.md).
The observation change alters neither the product nor matrix acceptance;
full installed qualification remains open.

The approved [local progress inspection](phases/streaming-progress-inspection.md)
adds a read-only same-UID IPC request and bounded original-matrix failure
snapshots before cleanup. It projects retained journal, slot and publication
state without command content or native identifiers in public summaries. Missing,
truncated or ambiguous records remain unknown; cached answers do not prove CLI
consumption, and recorded publication state does not prove relay delivery.
Inspection changes product observability, so a new complete candidate and the
original installed matrix are required. The [integrated code review](reviews/streaming-progress-inspection.md)
approves the implementation. Fresh candidate 42387811714 at 987d71d passed all
three platform jobs and rendering; see [candidate evidence](validation/streaming-candidate-987d71d.md).
The subsequent original matrix 42395261837 failed: six Linux-target observations
passed, but all three macOS-server records remained Preparing with Hello/Open
cached and their first input Exchange unanswered. Six bounded inspections
succeeded; admission is not process-launch proof. See [reviewed failure evidence](validation/streaming-installed-987d71d-failure.md).
Zero cells qualified, and the other seven cohorts were not dispatched. The
[bounded inherited-limit diagnostic](phases/macos-engine-inherited-limits.md)
compares shell and Node 24 launch environments without product execution or a
qualification waiver. Its [actual hosted observation](validation/macos-inherited-limits-fa71302.md)
measured soft limits of 10240 under the shell and 1048575 in the Node24 action's
child, both with infinite hard limits, on the same recorded image version as
the failed matrix. The latter exceeds the engine's fixed 65536 startup ceiling.
This reproduces a gate incompatibility, without capturing the original engine
error. The independently approved [native startup correction](phases/macos-native-startup-limits.md)
normalizes only an excessive macOS soft limit before threads, preserves hard
and lower soft limits, and retains the engine's existing high-descriptor fence.
The source also implements definitive bootstrap-failure preparation/cleanup
handling, with current authority checks and durable finish before local retirement.
The [integrated code review](reviews/macos-native-startup-limits.md) approves
the correction. macOS child commands inherit a soft limit at most 65536 while
the original hard limit is preserved. Fresh installed qualification remains
required; release readiness is unestablished.

The [local receiver cleanup contract](phases/streaming-copy-local-gc.md) uses an
encrypted actor-owned index, explicit admission/activation, owned worker joining,
current durable GC claims and anchored physical checkpoint validation. Unknown
or active ownership is retained. `copy-receipts` supports inspection and explicit
index-only forgetting with revision and retained-state acknowledgement. The
`limits.copy_receipt_retention` setting is distinct from room expiry and command
payload retention; it defaults to and normalizes to at least 604860 seconds,
with a maximum of 365 days. Positive installed platform matrices passed, but
edge-copy and interruption qualification remain incomplete. See [earlier copy
evidence](validation/copy-runtime-live.md), including
the failed early relay attempt, and [integrated review](reviews/streaming-copy-integrated-code.md).

Cancellation must be explicit and independent of blocked stream data. The runtime
implements Ctrl-C and cancellation by stable stream ID from another terminal,
confirmed versus uncertain outcomes, and safe process-group cleanup. Disconnect
alone must not cancel a command. Cross-platform and live-relay qualification are
required before claiming the new capabilities.

The v0.3 stream snapshot has a mandatory `stdin_closed` flag. Closing
stdin early does not fail the command: accepted/delivered cursors remain truthful,
valid output ACKs still apply, and the client stops sending unaccepted input while
draining output and the natural exit status. A client fingerprints Open before
publication and retains nonterminal Open/running state beyond the seven-day fresh
admission window. Lost, UnknownLaunch and uncertain publication holds do not age
out; room invalidation cannot convert them into confirmed cleanup or release their
quota. Explicit local reconciliation acknowledges possible surviving children.

Known charged streams receive typed Capacity/Closed admission refusals. A fresh
Hello refused at retained capacity or while stopping is dropped without a durable
reply and may time out. This bounds storage instead of creating uncharged retry
or publication rows. These are reviewed and Linux-qualified source contracts, separate
from released v0.2.3 behavior and platform acceptance.

## Native 0.2.1 launcher upgrades

The native 0.2.1 installer automatically replaces recognized v0.1
`0.1.0-experimental.N` launchers using the strict legacy marker, owned selector
and exact regular wrapper bytes. It never executes an existing launcher.
Receipt-owned native updates retain their existing behavior. Foreign files,
symlinks and ambiguous ownership refuse without replacement. The old runtime,
marker, private state and same-directory launcher backups remain; this is not
profile migration or automatic disk cleanup. Receipt generation determines
rollback versus roll-forward after interruption, with recovery on the next
installer invocation. Native uninstall removes native links, not the retained
legacy backups. See the [reviewed design](phases/v0.2.1-native-legacy-upgrade.md).
At that publication, the public bootstrap selected 0.2.3 with this behavior. Native suites and
provider qualification passed on all three release targets; actual user upgrade
and interactive macOS consent remain separate observations.

## Identity, ownership and local trust

One named profile owns one Nostr identity, encrypted native state and one
serialized MDK account owner. The same identity can be client, server or both.
Separate named profiles have separate ownership leases and state. The local
CLI talks to that owner; it does not concurrently open its MLS database.

Private profiles, sockets, configuration and key material are owned by the
current Unix UID. Socket authentication uses SO_PEERCRED on Linux and
getpeereid on macOS, alongside an owner-only socket directory. Local IPC is a
same-UID interface, not a new remote authentication mechanism. Root operation
is refused. Installation and identity state use separate directory trees.

`-i NAME` selects the local named identity. `-u ACCOUNT` selects a remote public
Nostr identity or configured alias; it never selects a Unix account or imports
a private key. Public destinations require the npub spelling; nsec and secret
material are not destination syntax. Configuration accepts its reviewed public
key forms without claiming that parsing alone authenticates a peer.

Roles control capability, not identity creation. Client mode can initiate
commands to trusted remotes; server mode can accept execution from granted
clients; both permits each independently. A default both profile with no
execution grants cannot accept commands. No engine is constructed when neither
current nor retained historical principals require one.

`trusted_remotes` authorizes outbound command-room relationships.
`execution_clients` authorizes inbound command execution. Setup's `--trust` and
`--grant` change these different sets; neither implies the other. Updates require
explicit setup update semantics. Removing authority invalidates current cached
responses/publications as required; it does not silently cancel a running job.
A fresh unauthenticated message, ordinary DM, relay connection or relay receipt
cannot grant command execution.

## Keys and storage

`--key-storage auto|keychain|file` selects native custody. Auto may choose file
storage for a genuinely unavailable provider when creating a fresh identity.
A configured keychain profile does not downgrade, regenerate keys or fall back
to plaintext/file custody after locked, denied or erroneous provider access.
Explicit keychain mode fails when the provider cannot satisfy its contract.

macOS uses Keychain and Linux uses Secret Service through its real provider
session; Linux hosts without an accessible provider can explicitly use file
custody. Keychain-backed profiles retain references, not serialized secret
copies. Named identities use distinct entries, including independent Nostr and
SQLCipher material. Both product frontends embed the private vault-helper
entrypoint; no separately installed helper executable is required.

Secret-bearing engine, journal and MDK storage uses SQLCipher. One process-wide
initialization barrier precedes all linked SQLite consumers, including Arti's
cache. This addresses the reviewed parallel initialization race without
serializing the test suite or weakening encryption. Private files, ownership,
link/type checks and canonical path requirements remain enforced.

Released v0.2.3 format constants are profile 2, journal 6, engine 1, config 1
and commands 1. The v0.3 native-copy implementation advances the journal to 7,
recognizing formats 0, 3, 4, 5, 6 and 7; checked initialization creates the copy
index/replay state and stamps 7. Do not downgrade a format-7 profile into the
released format-6 owner. Command tables share the journal's persisted compatibility gate; commands=1
is not an independent database version check. Journal opening recognizes the
implemented older native formats 0, 3, 4 and 5 before checked initialization and
stamping 6 in the released v0.2.3 implementation. This does not promise migration
of historical Python profiles.
Newer unsupported formats fail preflight before ordinary owner opening.

Read-only preflight can cause the pinned SQLite stack to create/open bounded
WAL/SHM sidecars; it does not promise a byte-for-byte unchanged directory.
Main encrypted database preservation and the exact permitted sidecar inventory
are separately specified in the
[version lifecycle design](phases/v0.2-installed-version-lifecycle.md).
Never delete databases, change format markers or bypass checks to recover access.

## Relay selection, destinations and routing

v0.3 defaults new setup profiles to
`wss://relay.eu.whitenoise.chat/` and `wss://relay.us.whitenoise.chat/` when no
relay is supplied. Setup writes this concrete pair to disk. Existing profiles
keep their relays, and missing/empty/null config fields still fail validation.
Published v0.2.3 requires explicit setup relays. This supersedes the earlier
prohibition on public defaults following the user’s White Noise authorization.
See [default-relay validation](validation/whitenoise-default-relays/README.md)
for live commands and installer checks.
A named profile freezes a
canonical relay set and route policy. Selected command destinations must equal
that exact set, not merely be a subset. CLI options cannot broaden or replace a
live owner's connector policy. Aliases bind an exact remote npub and exact relay
set, not a hostname-based authorization rule.

Commands require the separator before argv, for example:

```
marmish -i client npub1...@relay.example -- /canonical/program literal-argument
marmish -i client -u npub1... --relay wss://relay.example/ -- /canonical/program
marmish -i client ALIAS -- /canonical/program
```

Unreleased source accepts a bare valid npub, using the profile’s relay set when
no host or relay selector is given; explicit `--relay`/`--srv` follow the existing
`-u` rules. Ordinary aliases keep their complete configured set. Npub/nsec/nprofile
prefixes and raw 64-hex names never resolve as aliases; a valid npub wins over a
same-name alias. No invalid input or network failure invokes a default fallback.
Default Tor auto connects directly to the White Noise endpoints; `--tor always`
requests Tor. Config-less daemon startup still requires setup.

Bare hosts normalize to secure WebSocket URLs. Full URLs, IPv4 and bracketed
IPv6 use the native parser. Conflicting inline, alias, --relay, --srv and -u
sources fail rather than choosing one by precedence. Relay endpoints never
choose or replace the authenticated remote Nostr identity.

Explicit `srv:_SERVICE._tcp.DOMAIN` or repeated `--srv NAME` opts into bounded
system-recursive DNS discovery in the CLI before IPC. The raw-query resolver
accepts only matching SRV answers; it does not follow response-directed CNAME
or additional-record target lookups. No search domains, public resolver default,
implicit service guessing or periodic refresh is used. Resolution happens once
under its original five-second budget, then its snapshot is discarded after
selection. The resulting canonical relay set must still equal the profile set.
See [destinations](phases/v0.2-phase2-destinations.md) and the overriding
[raw-query correction](phases/v0.2-phase2-srv-raw-query.md).

SRV is rejected before DNS for always-Tor mode and onion selectors. In auto mode,
ordinary explicit SRV selection permits direct DNS as documented; it does not
authorize onion name leakage. `.onion` names are never sent to system DNS.
Tor auto routes ordinary relays directly and onion relays through Arti; always
uses Arti for every relay, and off refuses onion endpoints. Route failure never
changes the selected route. Mixed auto sets intentionally permit direct traffic.

Relay publication confirms relay acceptance, not peer decryption, command
admission, output consumption or process completion. Native source ACKs and
frozen outgoing-event obligations are tracked independently of application TTL.

## Command authentication and sessions

Native `marmish-command/1` intentionally breaks legacy Python wire compatibility.
Closed canonical request/response schemas bind protocol purpose, direction,
client, server, session, operation and request identity. Actual MLS group bytes
are authenticated out-of-band, not trusted from payload fields or confused
with a transport identifier. Command rooms and foundation-test rooms have
separate purpose records and quotas; cross-purpose reuse is rejected.

The server checks current room authority and inbound grants before admission.
Engine endpoint/principal ownership derives from the sealed authenticated
context. Peer-supplied fields cannot supply a token, UID, arbitrary engine root
or execution authority. Command replay uses timestamp-bound IDs, a durable
seven-day replay floor and immutable invocation digests, not fresh-key intuition.

Ordinary commands reuse a separate default command room. `--new-session` on a
new submission requests an independent room and leaves default reuse unchanged.
Handle operations, setup and administration reject that flag. The CLI freezes
one preparation token across bounded retries; the daemon chooses the actual
independent session and authenticates the room.

An atomic encrypted-journal creation fence binds token, peer, exact normalized
relay-set digest and session before one native create attempt. An interrupted
attempt without an accepted room returns command_room_uncertain; retry does not
create again or adopt an unproven orphan. A successful fresh room consumes an
active slot. There are 64 active command rooms, 4096 lifetime room metadata rows
and 4096 permanent preparation markers per profile, with no user room-retirement
operation. Total metadata exhaustion refuses and terminally forgets the new
unaccepted native group; it does not erase old obligations. Existing default
reuse remains possible at capacity. See [new sessions](phases/v0.2-new-session.md).

## Execution policy and process behavior

A native command carries direct byte argv and one immutable finite stdin value.
The server accepts the whole canonical argv/stdin digest durably before engine
side effects. Argv is not shell-parsed. Shell requests are denied in native v0.2;
authorized interpreters can nevertheless execute arbitrary code as the local
Unix user. Execution policy is not a sandbox or resource-isolation promise.

Current source resolves bare command names using the running daemon's PATH,
with no fallback if PATH is unset. Empty or relative PATH entries and relative
paths containing `/` use the configured command cwd (or passwd home). Symlinks
are canonicalized before policy. PATH is captured when starting the engine;
restart the daemon after changing it. Lookup runs on the blocking engine owner,
never the MDK convergence actor. The original argv, including argv[0], is
preserved for multi-call executables; a separate canonical program path is
persisted. The trusted Marmish worker helper never uses PATH search.

Allow layers match the canonical executable. Deny layers match both its canonical
target and the selected absolute alias spelling (lexically normalized). A denied
first executable in PATH never causes a search for a later permitted candidate.
All policy/lookup checks precede token issuance. The original native request
digest remains unchanged; established invocations bypass resolution on retries
and recovery. An initial Accepted can be followed asynchronously by a refusal.

The child still receives its existing minimal environment, including
PATH=/usr/bin:/bin, rather than inheriting the daemon environment. There is no
implicit shell, tilde or environment expansion. Launch uses an explicit execve
image prepared before fork, so executable text without a valid header is not
silently retried through /bin/sh. Legacy raw Exec bare names retain the fixed
child PATH lookup; current native requests use their resolved executable.
Filesystem checks have ordinary
path replacement races and can block on slow filesystems; canonicalization is
not executable-inode pinning. For bare-name search, PATH is bounded to 64KiB and 1024 entries.

Missing executables, non-executable targets, resolution errors and policy errors
have distinct finite codes/details. `command_denied` means an effective command
policy refusal. The client explains validated reasons using fixed local text;
debug logs contain finite stage/category/reason without argv, PATH, matched
rules, payloads or OS error strings. Known process-start failures have persisted
`launch_failed` outcomes, separate from cancellation-before-start and uncertain
launch. Retrying a refused invocation returns its recorded refusal; after repair,
submit a new command. Authorized clients can distinguish missing/type failures
from policy denial; this limited filesystem existence disclosure is accepted.

These v0.2.3 changes require coordinated client/server upgrades: old closed wire
decoders cannot read new error/outcome values. New binaries read legacy engine
requests/outcomes, but downgrading a profile after it records ResolvedExec or
LaunchFailed is unsupported. Published 0.2.2 assets predate this increment and
retain their canonical absolute-path requirement, including possible refusal of
symlink spellings such as /bin. See
[the reviewed lookup design](phases/native-command-path-errors.md) and
[validation evidence](validation/native-command-path-errors.md).

Default and every matching per-key restriction accumulate. Allows intersect,
denies union, omitted allow adds no constraint, and an empty allow denies all.
Repeated matching key entries add restrictions. Slash-aware globs match argv0:
`*` excludes slash and `**` includes it. An effective /** baseline for omitted
allow does not override explicit layers, grants or canonical-path checks.
Configuration reload requires restart; no hot-reload authorization is promised.

Engine calls run on a bounded blocking owner thread, not in MDK convergence.
The actor registers operations durably, projects completion facts/outbox updates
and acknowledges commits before advancing recovery barriers. An ambiguous
Submit is reconciled by status before any possible retry; an engine-known job
that later disappears is never redispatched as a new command.

Stdin is at most 16 KiB, immutable and local EOF-delivered only after actual
input progress reaches its length. There is no network streaming stdin or PTY.
With a TTY, default stdin is empty; --stdin explicitly reads finite input, while
--stdin-none forces empty. Non-TTY input defaults to reading through EOF under
the finite bound. Withholding EOF does not make interpreter side effects atomic.
An early exit with unconsumed nonempty input produces an input-failed diagnostic;
a zero-input command need not receive fabricated EOF to have a valid outcome.

## Handles, output and failure reporting

Default submission waits for authenticated admission, then copies stdout and
stderr bytes independently. --detach returns after durable server acceptance,
prints the private canonical handle on stderr and leaves stdout empty. A normal
successful attached command need not print a handle. `commands`, `command-status`,
`wait`, `output` and `cancel` operate through bounded same-UID IPC and explicit
selectors, including --handle and --handle-file.

A handle binds profile, remote, session, invocation and original request SHA.
It is not remote authority. Its whole-request SHA differs from the group-bound
Submit digest used by the engine ledger and Finish. Read-only client invocation
metadata exposes that latter digest separately; no raw MLS group is exposed.

Output polling uses one outstanding combined bounded Poll and durable stdout/
stderr cursors. CLI writes bytes before committing local response consumption.
A crash in that interval may repeat bytes after restart; terminal output writes
are not exactly once. Unread responses are not age-evicted simply to free quota.
Explicit gaps, invalid chunks and uncertain outcomes are reported, not hidden.

Known remote exit codes pass through; signal N maps to min(255,128+N).
Local/transport/policy/input/output failures use 255; unknown launch/lost or
ambiguous admission uses 254. Remote 254 and 255 remain valid remote outcomes;
finite stderr diagnostics distinguish them. Timeout preserves the handle and
reports pending rather than claiming remote termination. Ctrl-C detaches; only
explicit cancel requests cancellation. Cancellation itself is not proof of exit.

Ordinary client disconnect, daemon Stop and uptime exit do not cancel admitted
workers. Restart resumes ledger/native reconciliation and output retrieval.
Accepted-but-undispatched work may remain for recovery; Lost/UnknownLaunch is
never converted to successful completion or safely rerunnable work by guesswork.

## Limits, retention and automatic server exit

Defaults are four active jobs per principal, 32 globally, 1024 retained
invocations per principal and 10000 globally. Config may lower these within its
validated bounds. Uncertain/unreconciled jobs retain active credit. Retained
rows, pending requests, publications, rooms and native obligations have distinct
finite limits; overload is explicit busy/quota backpressure, not dropped work.

The final encoded command envelope is always at most 32768 bytes, including
argv/stdin/metadata. Local IPC frames are at most 64 KiB. Output uses combined
bounded pages; the default retained stream cap is 8 MiB each and aggregate
engine spool cap is 512 MiB. OS pipes continue draining when storage caps cause
gaps. A full output hash is unavailable when retention/gaps prevent proving it.

Payload retention defaults to one day; the command replay horizon is seven days.
Engine tombstone retention is fixed at two days. Expiring payload does not expire
running/uncertain ownership, immutable replay facts or protected native bytes.
Retirement requires the appropriate consumed-slot, source, publication and
post-ACK native-absence proofs. Invalidations protect the affected command groups;
no fabricated ACK or unfiltered-obligation deletion may satisfy cleanup.

`--max-commands N` counts unique engine admissions, not retries, refused requests
or provisional ledger reservations. Durable credits prevent overshoot. Reaching
N closes new admission, while existing responses continue serving. Automatic
exit additionally requires the authenticated client's durable Finish assertion
for every counted admission and complete local/native drain predicates.

Finish binds the original group Submit digest, authoritative terminal fingerprint
and exact final gap-aware cursors. It cannot launch work, feed input, cancel or
supply execution authority. Last-output consumption and frozen Finish creation
are atomic. Busy fingerprints permit bounded fresh-Poll/Finish retries, at most
four Finish attempts per invocation; replay reuses bytes and does not recount.
Finish is a peer consumption assertion, not proof that a person read or fsynced
output, and its response is not inferred from relay ACKs.

`--max-uptime` accepts a positive duration through 30 minutes. If max-commands is
set without an uptime, 30 minutes supplies the escape bound. The suspend-inclusive
monotonic budget begins before identity/config/network startup and is checked
inside admission/dispatch as well as the owner loop. Process restart does not
reset durable admission counters; qualification restarts also preserve the
original overall workload deadline. Revocation, missing Finish or offline native
obligations may force uptime exit rather than count exit. Neither exit cancels
workers or proves their absence. See [server limits](phases/v0.2-phase2-server-limits.md).

## Installation, updates and operational cleanup

The native bootstrap verifies a release-pinned manifest and all three product
binaries plus licenses before calling Rust install. It uses fixed asset names,
exact hashes/sizes/provenance and bounded HTTPS downloads; no archive extraction,
manifest shell evaluation, unpinned latest lookup or legacy-version fallback.
Native setup is offline and uses installed product entrypoints for identity,
role, trust and grant configuration. Installation does not initialize identities.

Versioned installations use owned receipts and checked filesystem transactions;
selecting a retained version reverifies it. Uninstall removes owned launchers
while preserving identity/config/database state and installed version contents.
Rollback selection does not imply older binaries can read newer state. Unsupported
formats refuse before profile opening; no compatibility marker is rewritten to
force acceptance. Runtime requires the installed sibling marmish-worker.

The release bootstrap is served byte-for-byte from Radicle/Iris and GitHub,
with GitHub the independent script backup. Both routes download GitHub-hosted
release assets. The current v0.3.0 bootstrap SHA256 is
`d766bf1078de5f721c97e79ee7af675bd2d8b2cbcdedb88195087050c288d16b`;
both public routes matched it and passed all six install/setup checks in
run 42557032770 attempt 1. The historical v0.2.3 results remain in
[earlier evidence](validation/native-automatic-release.md). Source replication,
HTTPS delivery and installed behavior remain separately recorded observations.

Ordinary shutdown preserves jobs and durable native work. Hidden same-UID fixture
cleanup is a separate irreversible draining lifecycle: close admission, refresh
all ledger rows under an allocated fence, commit terminal/presence facts, consume
the actual engine into its cleanup guard, retain the live owner/profile lease and
original inode receipt through protected removal. Unknown, revoked or incomplete
proofs fail cleanup. No-engine clients use honest engine-absence/empty-server-ledger
and retained-directory proofs, never a fabricated engine guard or quiesced flag.
No caller-supplied PID or filesystem path authorizes process-tree deletion.

## Historical initial release and evidence status

The user authorized the initial native 0.2.0 release with a prominent macOS
full-testing-in-progress caveat; corrections are planned for v0.2.1. This
changes publication timing, not the truth of pending acceptance results.

Baseline candidate [41655210565](https://github.com/Epiphytic/marmish/actions/runs/41655210565),
attempt 1, used source `347cf799ae5baa9e577ded1a00566c7a6b99b933`.
Product verification, builds and ABI inspection passed on all three targets;
macOS ad-hoc signing passed. The macOS isolated installed-tool suite reported
426 passed and two failed local benchmark child-process fixtures. No actual
cause is proven, and no claim of a corrected fixture or full macOS success is
made. The uncommitted diagnostic work is excluded from initial release source.

Release source `73ed2b41b64ecf614d245668e3ab3b2e5b753895` changes only release-workflow
and initial-release documentation against baseline `347cf799ae5baa9e577ded1a00566c7a6b99b933`.
Runtime, build, dependency and scripts trees are unchanged. Asset-build run
41664164076 attempt 1 completed successfully. Keep actual manifests and build outcomes bound to their source. Do not
relabel baseline tests as new-source tests or claim identical executable bytes:
compiled metadata and build provenance differ.

Earlier evidence remains useful within its recorded scope: foundation acceptance
[hosted-41577006224](validation/v0.2/hosted-41577006224.md),
[public CLI regression](validation/v0.2/public-cli-admission-regression.md),
[internal direct recovery](validation/v0.2/local-command-recovery-direct-2026-11-03.md),
and the [lifecycle component review](reviews/v0.2-version-lifecycle-integrated-code-r3.md).
Component review is not hosted release qualification.

At the initial release, the following qualification was pending. The current
v0.2.3 provider results supersede the historical provider status below; the
remaining full matrix, benchmark and lifecycle gates still need independently
bound results:

- Installed matrix: nine directed platform pairs and exact bytes/exits.
- Local installed cohorts: count/uptime exit, policy, recovery and new-session.
- Benchmark: cold plus three warm samples on each target, with real latency and
  goodput. No final-platform values or percentiles may be invented.
- Version lifecycle: actual A/B/A loading, live-worker preservation, C refusal,
  state/sidecar proofs and owned keychain lifecycle.
- Providers: actual default-auto Linux and macOS native-provider qualification.

HTTPS installer validation passed for v0.2.0 within its tested scope for both script sources across all three
targets in run 41667214179 attempt 1; this is historical v0.2.0 qualification. Current v0.2.3 HTTPS and provider
results are recorded separately above.

Actual release manifests bind source and immutable asset hashes. Record source
mirror and installer-byte verification as their own evidence; do not infer
installed qualification from publication.


Every qualification report binds its own source, run/attempt, target,
manifests/tooling and original deadlines. Partial results retain their failure
and cleanup facts. Synthetic lifecycle variants are explicitly synthetic.
Process exit or relay publication cannot substitute for exact-byte execution,
authenticated origin, engine admission, terminal validity and owned cleanup.

## Initial asset verification

Asset-build run [41664164076](https://github.com/Epiphytic/marmish/actions/runs/41664164076),
attempt 1, successfully built, inspected and packaged all three targets and
rendered the native bootstrap from this release source. All 15 product assets
were individually checked against their manifests. This caveated packaging run
skipped product test reruns, installed-test tooling, provider qualification and
quickstart jobs; its success is not full qualification. The exact rendered shell
also installed Linux AMD64 from the artifact asset directory: release metadata
matched source/version/formats `[2,6,1,1,1]`, and all four installed product files
matched their expected hashes and modes. Public release and both installer URL cutovers have been verified. The local installed native protocol check passed, as described below.

The installed Linux AMD64 direct-relay command check passed (exit 0), using the
installed production owner, engine and worker with disposable identities. Total
elapsed time was 19444 ms. Its running-command owner-restart/recovery case took
6295 ms and verified exact recovered output, execution count one, identical-submit
attachment and remote conflict refusal. Literal argv completed in 1590 ms;
16 KiB binary stdin/output in 2778 ms (5896 output bytes/s); stderr plus exit 7
in 1466 ms. Joined engine/MDK stops, engine quiescence, runtime cleanup and state
cleanup all passed, with no retained state. This is one local installed native
protocol check, not the public-CLI platform matrix, an OS restart test, or full
benchmark/provider/lifecycle qualification.

The [initial v0.2.0 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.0)
is public with all 15 product assets; server sizes and digests match the verified
manifests. GitHub and Radicle tags both identify the release source. Bootstrap-only
commit `7e3e74411bed8657e8b8f5b599b36c2b702fe5bd` changes only `scripts/install.sh`.
Both public installer URLs were independently fetched and matched the rendered
bootstrap SHA256 `46b4f369792f4f4ea36726c24414c5cca2ef977fec61712d821a1b81f985367a`.
The two-source/three-platform HTTPS run [41667214179](https://github.com/Epiphytic/marmish/actions/runs/41667214179), attempt 1, passed its resolver, all six install/setup cells and rollup. This validates both script sources on all three targets within that test scope; it does not establish full command-matrix, provider or version-lifecycle qualification.

A separate Linux AMD64 install downloaded the public release assets without an
asset-directory override and passed (exit 0), selecting active generation 2
with the same version, source and format tuple. See the
[initial release evidence](validation/v0.2/initial-release.md) for exact reports and boundaries.

The installed lifecycle checker derives synthetic A/C versions from the frozen
stable `0.2.N` candidate and matching native lock entry, rather than pinning
`0.2.0`. See the [patch-version amendment](phases/v0.2-lifecycle-patch-versions.md).
This changes qualification tooling only; hosted lifecycle qualification remains
pending. At that time, the public installer pinned 0.2.3.

The canonical credential-helper correction (`marmish-far`) makes managed and ordinary sibling `marmish`, `marmishd` and worker entrypoints select validated same-version `marmish` for provider operations. Exact downloaded standalone CLI behavior is preserved. This fixes an inconsistent helper identity; reported macOS Keychain denial causation and interactive recovery remain unverified. Current v0.2.3 automated provider checks passed separately; they do not validate user consent dialogs. No ACL or file-fallback policy changes. See [design](phases/v0.2-canonical-credential-helper.md).

The published 0.2.2 increment adds explicit foreground macOS
`marmish [-i NAME] [--state-dir DIR] keychain-authorize`; it is not in the
published 0.2.1 assets. See the independently reviewed
[authorization design](phases/v0.2.2-keychain-authorize.md). It requires an
existing ready Keychain profile and its exclusive ownership lease. It reads
only that profile's exact two v2 credential references through the canonical
helper, requests native consent, and then verifies access in a fresh helper
with prompts disabled. It creates no profile or keys, changes no backend,
rewrites no ACL, opens no database and contacts no relay. Persistent consent is
the user's OS choice. The ordinary helper and daemon remain noninteractive.
Success proves current credential access for this helper, not database health,
identity binding, another binary version's access or full daemon operation.
INT/TERM/HUP cancel the owned helper; a shared 120-second active-time deadline
reserves the final 15 seconds for verification. SIGKILL cleanup relies on the
helper watchdog. Interactive macOS consent and recovery of the user's denial
remain qualification requirements, separate from offline test success.

### Historical macOS provider supervisor correction

Candidate 41698628787 macOS stopped in offline supervisor tests; real Keychain qualification was skipped, so it supplies no provider or helper-custody verdict. The bounded correction canonicalizes lifecycle fixture paths and handles an initial owned-child group EPERM only by exact-child reap plus a fresh ESRCH proof; unknown or denied cleanup stays red. Linux provider23/HTTPS24 offline checks and independent [code review](reviews/v0.2.1-macos-provider-supervisor-code-r1.md) pass. At that evidence point native macOS offline tests and actual disposable Keychain qualification remained pending; see the newer observation below. See [sanitized evidence](validation/v0.2.1/macos-provider-supervisor.md) and [approved design](phases/v0.2.1-macos-provider-supervisor.md).

Candidate 41726568264 passed all 23 macOS offline supervisor tests, but its
outer real-provider qualifier failed after 676 ms with `operation_failed` and
unchanged runner Keychain snapshots. No inner report was imported; whether
provider execution began is unknown. `owned_account_removed=false` does not
prove an account existed or leaked. Bounded failure-stage diagnostics are
tracked as `marmish-wtg`; interactive macOS acceptance remains pending.
Current v0.2.3 provider checks passed on all three platforms; these historical
failures remain separately recorded and do not establish interactive consent.
The installed Linux command check also emitted a background sink panic despite
passing result/cleanup flags. Its source/dependencies are unchanged from 0.2.1;
independent triage permits this caveated correction, while `marmish-xvd` tracks
the defect. Remote final-write/closure effects remain unproven.

## Background daemon (v0.2.3)

Version 0.2.3 makes `marmishd` and `marmish daemon` detach by default.
Explicit `--foreground` operation preserves owned-child
supervision, inherited output and shutdown receipts. Initialization and other
operations retain their existing behavior. This is not login-service setup.

Background startup is quiet on success only after credentials, storage, IPC,
transport activation and the first active snapshot. Finite startup failures
remain visible to the caller. Normal output appends to the private profile's
`daemon.log`, mode 0600, without automatic rotation; stop before rotation.
Private command output and credential-helper responses retain their dedicated
pipes rather than inheriting the log. Installed supervisors explicitly select
foreground operation to retain their exact child handles and receipt drains.

Client autostart has a shared 300-second startup budget covering launch and
failed-start cleanup, independent of command wait time. A launching parent or
client that disappears before the readiness acknowledgement cannot leave an
unacknowledged daemon running. An acknowledged daemon survives its caller.
The released v0.2.3 candidate passed the three-platform native suites, provider
checks and quickstarts. These results do not establish the full nine-pair command
matrix or version lifecycle.
See the [approved design](phases/native-background-daemon.md).

The background-daemon source increment passes 471 local Linux all-feature native
tests and strict all-target/all-feature Clippy. Its final live Linux test verifies
default detachment, readiness, duplicate-start errors, simultaneous cold-client
autostart, exact command bytes, private logs and joined cleanup. See
[validation](validation/native-background-daemon.md); platform qualification remains
separate from these local results.


## Gated native release automation

The released v0.2.3 increment adds GitHub-only binary release automation.
A master push changing the native Cargo manifest runs version eligibility first;
an explicitly bumped native version must match Cargo.lock and advance existing
native tags. CI never chooses a semantic version automatically. An unchanged
version is a no-op. Candidate packaging, installed tests, quickstart installation
and real provider checks must all pass on Linux AMD64/ARM64 and macOS ARM64.

A separate publisher checks the completed whole attempt, source, reports and
artifact hashes. It reproduces the Rust-rendered bootstrap without repository
credentials, publishes exactly 15 immutable GitHub assets, then fast-forwards
master with a direct-child change to scripts/install.sh. Publication succeeded
for v0.2.3, and the bootstrap then selected its verified assets. Recovery cannot replace
assets or move tags. Source and tag mirroring to Radicle/Iris remains manual;
GitHub Actions has no Radicle credentials or release-publication responsibility.
The independent six-cell public HTTPS installer run remains a separate gate;
it passed for v0.2.3 in run 41937495649.
See [the release design](phases/native-automatic-release.md) and
[publication plan](phases/v0.2.3-publication.md), with actual hosted candidate,
publisher and public-install results in
[release evidence](validation/native-automatic-release.md).

The separate cold-three-principal macOS diagnostic harness now has actual
independent Claude code approval and passing local checks (191 Rust installed
checks and 29 JavaScript tests). See [the review](reviews/streaming-macos-cold-multiprincipal.md)
for explicit coverage limitations. Hosted run `42374246025` passed all three
cold principals (42.392–43.915s), direct-after, receipts, journal bindings and
cleanup; [structured evidence](validation/streaming-macos-cold-multiprincipal.json)
retains the exact immutable pins. This same-host diagnostic omits the original
macOS outgoing traffic, multi-server trust and cross-host timing. It changes no
product behavior and does not resolve the failing installed matrix or qualify a
release.

### Development worker isolation and cleanup diagnostics (source increment)

The engine crate's standalone development target is `marmish-engine-worker`;
the native distributed target remains `marmish-worker`. Engine Cargo fixtures
assert the unique source target and may stage its bytes under their private
`marmish-worker` destination. Both historical Python bundle builders preserve
that destination while selecting the unique engine source. Generic negative
worker fixtures still accept deliberate custom artifacts.

The [failed 53f94f9 candidate](validation/streaming-candidate-53f94f9-failure.md)
reported the old output-name collision and a separate unclassified cleanup
uncertainty. Removal of a longstanding collision is a build-contract correction;
the failed run's winning bytes were not captured. The native runner increment
adds only test-side failure context, observed-exit synchronization and bounded
panic cleanup; it does not change production cleanup or waive uncertainty.
The original full-window race remains covered alongside deterministic after-exit
and live-cancellation cases. Test diagnostics identify the operation, finite
engine error and lifecycle/window state; an internal Darwin reason can still
require another investigation.

The manual `macos-native-regression.yml` uses an exact workflow/source pin,
records distinct worker hashes, runs the original combined native suites and
repeats four focused cleanup cases. It uses no relay and is diagnostic only.
See the [approved design](phases/macos-candidate-worker-and-cleanup.md) and
[actual reviews](reviews/macos-candidate-worker-and-cleanup.md). All-platform
candidate and installed qualification remain required; the public installer
selected v0.2.3 at that time, with no new release implied by that source increment.

The first local combined rerun passed the engine integrations but failed the
closed-stdin fixture (delivered13, expected0). A controlled inherited-reader
check demonstrates a missing fixture premise, without establishing the failed
run's exact cause; its inherited test-thread setting was not captured. A narrow
exact selfexec isolation correction has independent design and final code/result
approval. The final unchanged Linux combined suite passed in693.03s; delivery,
retry, EOF and natural-exit assertions and production behavior are preserved.
A recurrence in the isolated child remains a product defect to investigate.
See [local and hosted evidence](validation/macos-worker-cleanup-local.md).

At source2ebc8b6, focused macOS run42406498129/1 passed the combined suites and
80 exact cleanup repetitions under serial libtest. Historical failure causality
and parallel macOS behavior remain unproven. Subsequent candidate42408127994/1
passed macOS and Linux ARM64, but Linux AMD64 hit its90-minute job cap during
release compilation of installed-test tools after its tests passed. The
[candidate is unqualified](validation/streaming-candidate-2ebc8b6-timeout.json),
with zero installed cohorts dispatched. These diagnostic/partial results do not
qualify the streaming release; a fresh exact-source candidate and all eight
original installed cohorts remain required.

The [reviewed candidate build-budget correction](phases/streaming-candidate-build-budget.md)
changes only the three-platform package-job cap from90 to120minutes, after the
actual AMD timeout. The installed65-minute cap and all runtime/traffic deadlines,
workloads and acceptance remain unchanged. Its
[local policy checks](validation/streaming-candidate-build-budget.md) do not
qualify a release; a fresh exact-source candidate and all eight original
installed cohorts remain required. A second budget expiry must be preserved
and investigated rather than silently increasing the cap again.


Candidate [42416184935/1](https://github.com/Epiphytic/marmish/actions/runs/42416184935)
at `50ad3d12abaa7698737bf23c68ffd7002c07112d` passed all three complete native,
package and installed-tool jobs plus rendering under the reviewed build cap.
The subsequent original stream, rsync and copy matrices passed all nine directed
platform pairs (9 duplex, 18 rsync and 18 copy cases). Their retained receipt,
manifest and checkpoint evidence and application timing/goodput measurements
are recorded in [the installed report](validation/streaming-installed-50ad3d1.md);
see also [native candidate evidence](validation/streaming-candidate-50ad3d1.md).
The fourth cohort, edge-stream, failed on all three hosts without preserving its
failing case or stage. The chain stopped before the remaining four cohorts;
complete installed qualification remains open. The separate legacy managed
validation workflow is also red, with some native-tool failures unexplained.
Passing candidate and partial matrix results do not mean all CI is green.

The [reviewed harness correction](phases/streaming-edge-harness-correction.md)
prepares the fixture's required private gate directory, distinguishes legitimate
inactive Hello startup from established-running proof, and adds bounded closed
failure-stage diagnostics. Its discovered source defects do not uniquely prove
the historical hosted cause. Marker identity, physical charged-runner checks,
explicit confirmed cancellation, existing acceptance, deadlines and workloads
remain required. A corrected source requires a new complete three-platform
candidate and all eight original serial cohorts; historical passes cannot be
combined into a new qualification. No public release, version or installer pin
change is part of this correction. At that time, public installation selected v0.2.3.

Fresh candidate [42424780330/1](https://github.com/Epiphytic/marmish/actions/runs/42424780330)
at `a7c3db4b30a953c3f3413226566e7650b282d33f` passed all three native/package/tool
jobs and rendering. Its original stream, rsync, copy and edge-stream cohorts
passed, with 72 retained observations. The subsequent edge-rsync cohort failed
on all three platforms; its records retain no individual case/stage and report
server-stop incomplete. The remaining three cohorts were not dispatched.
See [partial evidence and measured rates](validation/streaming-installed-a7c3db4.md).
The rsync startup observer independently contains the same premature rejection
of valid Hello without Open; this does not establish historical failure causality.
The proposed [bounded rsync correction](phases/streaming-rsync-edge-correction.md)
requires independent design and integrated code approval, then a fresh complete
candidate and all eight cohorts. Release qualification remains incomplete.


Candidate [42435626121/1](https://github.com/Epiphytic/marmish/actions/runs/42435626121)
at `2f98802d25e2a6ec9ebd995adfef3504169175b7` passed all three native/package/tool
jobs and rendering. Five original cohorts passed, retaining 90 rollup observations
(stream, rsync, copy, edge-stream and edge-rsync). Edge-copy then failed on all
three platforms: all 12 canonical records remain, but no case/stage proof was
retained and every server-stop flag is false. Both interruption cohorts and the
final auditor were not run; complete qualification remains false. See
[partial results, pins and application rates](validation/streaming-installed-2f98802.md).
A separate source audit identifies the same workspace being used for two exclusive
nonce-directory creates before the first copy edge case; historical runtime errno
was not retained. The [approved correction](phases/streaming-copy-edge-correction.md)
separates those fixture roots and adds bounded failure observations. Independent integrated
code/evidence review approved the correction; a fresh complete exact-source chain remains required.
Legacy managed CI remains red: 10 of 19 unique failed identities locally reproduce
obsolete assumptions; nine locally pass with hosted causes unresolved. No failure
is waived. GitHub exact source bytes matched, but Iris HTTP timed out despite
verified remote refs/sync. This source-only increment changes no public version,
release tag or installer pin; public installation selected v0.2.3 at that time.

The subsequent f96 candidate failed the macOS installed-feature native suite before
any installed cohort could run: source control flow shows the test reached its final cancellation assertion, but
cancellation cleanup returned `Failed(Uncertain)` instead of `Cancelled`.
[Retained candidate evidence](validation/streaming-candidate-f96b692-failure.md)
distinguishes this failure from the earlier copy-root defect. The existing test
now uses the existing physical-cleanup guard and emits its finite failure context;
it preserves the command, limits and every original assertion. This is a fixture
lifetime/diagnostic correction, not proof of the historical cleanup cause.
See the [diagnostic phase](phases/streaming-runner-cleanup-diagnostics.md) and
[independent reviews](reviews/streaming-runner-cleanup-diagnostics.md).
A separately demonstrated stale-observation lifecycle hazard requires its own
review and tests. Fresh full qualification remains required; no new public
version, release tag or installer pin is created. Source publication is separate from test qualification.


The [runner exit-observation correction](phases/streaming-runner-exit-observation.md)
starts existing grace budgets at successful signal completion and refreshes owned
exit observation once at the existing expired post-KILL failure boundary. A real
exited-child stale-state regression fails old code and passes the correction;
real still-running and already-reaped controls remain uncertain. Cancellation
meaning and budget constants are preserved. This independently demonstrated
defect is not established as the historical f96 macOS cleanup cause. Local checks
do not replace the required fresh native candidate, all eight installed cohorts
and actual final auditor qualification.


Candidate [42453390792/1](https://github.com/Epiphytic/marmish/actions/runs/42453390792)
at `46c9cb46e382735a8678b17eca5e416f30b0c264` passed all three native/package/tool
jobs and rendering. Its original stream and rsync cohorts passed, retaining 27
rollup observations. The original copy cohort
[42501629829/1](https://github.com/Epiphytic/marmish/actions/runs/42501629829)
then failed; the remaining five cohorts and final auditor were not run.
The initial Linux AMD64 driver failed after 31.637 seconds with only a generic
error. Its normal ready upload completed before failure, and normal terminal
artifact uploads contradict the simple supervisor-poll-error-initiated kill
theory. Neither empty fallback records nor cleanup flags establish whether a
command ran or the physical state of the failed hosted processes. The initiating
native cause remains unknown. See [retained results and application rates](validation/streaming-installed-46c9cb4.md)
and the later [independent evidence approval](reviews/streaming-installed-46c9cb4-evidence-review.md),
which reviews those frozen report bytes without claiming release qualification.
The independently approved [failure-observability design](phases/streaming-failure-observability.md)
adds bounded finite native and supervisor diagnostics while preserving existing
workloads, deadlines, acceptance and cleanup behavior. The [integrated code review](reviews/streaming-failure-observability.md) approves
the implementation and local validation; fresh complete hosted qualification
remains required. Historical passes cannot qualify changed source. No new public version, release tag or installer pin is
part of this increment. At that time, public installation selected v0.2.3.

Candidate [42504599079/1](https://github.com/Epiphytic/marmish/actions/runs/42504599079)
at `e31b8afae452bb30a041916fbf6d2bac4e92f042` passed all three native/package/tool
jobs and rendering. The original stream cohort retained nine accepted observations;
rsync [42510560482/1](https://github.com/Epiphytic/marmish/actions/runs/42510560482)
failed with zero accepted cells despite eighteen raw successful client pairs.
The macOS driver reported `remote_client_barrier`, origin `work_error`, category
`invalid`; its server had no receipts. Six later cohorts were not dispatched and
the final auditor did not run. The automatic legacy workflow also remains red
with twenty failure identities versus nineteen previously retained; the new
macOS identity and the historical nine unexplained failures remain unresolved.
See [the stopped-chain evidence](validation/streaming-installed-e31b8af.md) and
[its independent evidence review](reviews/streaming-installed-e31b8af-evidence-review.md).

The [record-publication design](phases/streaming-record-publication.md) addresses
a confirmed source contract defect: immutable publication exposes a target with
two hard links until staging cleanup, while strict readers require one. The
actual native validator passed retained results for three clients, six pairs
each; this does not recover failure-time inbox metadata or establish the hosted
initiating cause. [Three actual Claude design reviews](reviews/streaming-record-publication-design.md)
resolved missing live marker readers and approved a narrowly bounded pending
state at seven live read sites, preserving strict ordinary reads and original
deadlines. [Integrated implementation and test-only followup reviews](reviews/streaming-record-publication-code.md) approved the correction;
fresh exact-source hosted qualification remains pending. Earlier failed-chain facts and public v0.2.3
installation remain unchanged; no new release qualification is claimed.


The [legacy namespace selection phase](phases/legacy-namespace-coverage.md)
adds an explicit Linux-only reduced reference-test lane and a separate manual
lane for the two preserved namespace integration tests. Only literal
`MARMISH_LEGACY_SKIP_NAMESPACE_TESTS=1` selects the two configured skips;
invalid values and any supplied value on macOS are refused. Reports distinguish
reduced coverage from default full selection. Native Rust requires no Linux
namespace privileges. [Integrated review](reviews/legacy-namespace-coverage.md)
and 62 focused contracts in each flag mode pass. At accepted source `2b951ae`,
both hosted Linux reduced suites passed; the capable manual two-test lane still
has not executed and would not alone qualify full legacy packaging. These are
reference results, not native release qualification or an explanation of
historical failures.


The legacy fixture-cache report now retains bounded compiler-admission metadata
(tool, stage, exit code, byte count and exact version/host comparison results).
Raw output remains confined to the existing explicitly requested private log,
and successful probes clear stale capture before later operations. The
[reviewed diagnostics](reviews/legacy-compiler-admission-diagnostics.md) pass34
focused mock tests. Later hosted compiler/cache preparation passed on all three
platforms at `2b951ae`; the historical rejected probe's OS cause is not inferred
from that success. Version admission remains unchanged.


The [reviewed macOS proof restart](reviews/macos-proof-snapshot-restart.md)
permits one fresh complete cleanup proof only after a first-snapshot nonleader
BSD-read ESRCH. It never accepts a missing member or weakens terminality,
ownership or identity checks. Debug/release share the same classification;
final debug failures identify member role and attempt using fixed vocabulary.
Portable checks pass; Darwin execution and full release qualification remain
pending. Historical group/status and role-unknown BSD-read failures remain
unexplained. No extra signal, reap or timing budget is introduced.

The [exiting-member cleanup phase](phases/macos-exiting-member-pending.md)
addresses a later observed macOS failure: a validated nonleader was still
reported as running with the exit-in-progress flag. That observation is never
accepted as terminal. Only the live runner may keep observing the complete
owned-group proof within one existing drain allowance, anchored at its first
KILL attempt. Hard ownership, identity and membership failures remain errors;
pending or failed cleanup cannot authorize reaping or a new KILL attempt.
Ordinary signal completion still requires the existing exit/drain conditions.
Strict finite-worker and destructor signal paths retain their prior behavior.
Local Linux tests cover portable state and deadline behavior; Darwin execution
and fresh complete release qualification remain required. This does not explain
every historical macOS failure or qualify unpublished version 0.3.1.

The temporary [confidential legacy diagnostic](phases/legacy-confidential-trace.md)
is retired after accepted source `2b951ae` passed all six ordinary legacy jobs
in run42649925720/attempt1. Its manual workflow, public recipient certificate
and nine associated mock tests are removed; historical documentation and private
evidence/key material remain preserved. The existing explicitly requested private
failure-log mechanism remains available. GitHub's registered workflow history
remains, but the deleted workflow cannot be dispatched on the new source. See
the [retirement evidence and review](reviews/legacy-diagnostic-retirement.md).
This changes no native runtime or acceptance predicate.

The [managed test-environment correction](phases/legacy-managed-environment.md)
uses an executable Rustup sibling of the selected compiler, or the existing
proxy match, to request the pinned fixture toolchain explicitly. Standalone
direct compilers retain their existing selection path and version checks.
Two workflow test modules now use scoped standard-library assertions instead
of requiring PyYAML in the isolated interpreter; unsupported scalar forms
are refused, with mutation tests covering the retained guards. The
[independent review](reviews/legacy-managed-environment.md) approves the source
and 102 focused tests in four configurations. Hosted reference suites subsequently
passed on all three platforms at `2b951ae`, with the explicit Linux reduced scope
and existing macOS offline limitation described below. This adds no installed
product dependency.

The [explicit reduced-reference amendment](phases/legacy-namespace-coverage.md#amendment-explicit-hosted-linux-reference-scope)
requires both `--reduced-linux-reference-only` and the existing Linux namespace
selection for automatic Linux reference checks. It completes through the full
tools suite, then explicitly leaves independent offline retention unselected;
reports never mark full legacy packaging complete for that scope. The default
full acceptance path and macOS remain unchanged. The
[independent review](reviews/legacy-reduced-reference-scope.md) passed all 126
focused contracts in both flag modes. Earlier hosted Linux full tools suites
completed 1,011 tests with 11 configured skips before their separate namespace-bound
packaging check failed. Accepted run42649925720/attempt1 at `2b951ae` passed the
explicit reduced scope on both Linux architectures: 1,024 total tests including
11 skips each, exit0, full packaging incomplete and independent offline retention
explicitly unselected. macOS passed its default full legacy packaging scope with
1,016 total tests including45 skips and final completion=true; its guarantee
remains `uv_offline_only_not_independently_enforced`. These results qualify only
that source and those legacy scopes. The retirement source R (`ae309a5`)
subsequently passed all six jobs in
[run42652134004, attempt1](https://github.com/Epiphytic/marmish/actions/runs/42652134004).
Both Linux architectures completed 1,015 total tests including11 skips, exit0,
with explicit reduced-reference scope, full legacy packaging complete=false,
and independent offline retention unselected and unmet. macOS completed 1,007
total tests including45 skips, exit0, with default full legacy packaging
complete=true and the same `uv_offline_only_not_independently_enforced`
limitation. These actual totals are G's totals minus the nine retired mock
tests, with skip counts unchanged. The capable manual two-test lane has not
executed and would not alone qualify full legacy packaging. Full Linux
namespace-capable packaging remains unqualified on hosted infrastructure.
Installed Rust Marmish needs no namespace privileges.

The [legacy HTTP worker cleanup correction](phases/legacy-http-watchdog.md)
handles Darwin EPERM only after an existing double-observation proof confirms
the unreaped direct child has exited. That proof authorizes reaping, never a
claim that its group is absent: a separate bounded observation must reach ESRCH.
There are no real signals after reap, and failed or uncertain cleanup remains
an error. Pipe cleanup and bounded wait are attempted even after signal failure.
The [independent review](reviews/legacy-http-watchdog.md) passed 29 focused tests,
including a simulated-Darwin own-child EPERM regression that failed before the
fix. Hosted Darwin full-tools and packaging passed at `2b951ae` in the accepted
run above; the underlying kernel reason for the historical EPERM is not
established. This changes reference and
installed-test tooling, not native Rust command execution.

Diagnostic retirement R is a separate source increment; its successful legacy
checks do not qualify a later source. A local native test at accepted G
(`2b951ae`) failed with `AlreadyOpen` in
`store::tests::denial_precheck_does_not_exhaust_execute_tokens`; the actual
competing holder remains unknown. The
[fixture ownership correction](phases/native-fixture-frontend.md), independently
[reviewed under `marmish-d97.1`](reviews/native-fixture-frontend.md), is now
implemented in N, source A (`33138eb`). It retains and transfers the same
frontend guard from fixture initialization into the engine, removing the
fixture handoff gap while preserving the wrong-master test's deliberate early
release. The regression and descriptor-alias control establish fixture
ownership behavior, not the historical holder. This changes test fixtures
only; production locking, retries, timeouts and thread counts are unchanged.
The later exact-A candidate failed the separate macOS cleanup race described
above. G's and R's legacy results cannot substitute for fresh native/release
qualification of a corrected source. No native gate is waived, and no operation
benchmark or release qualification result is claimed here.
