# Bounded macOS streaming diagnostic

This increment diagnoses Beads `marmish-8ka.5.8`; it does not qualify a release or
replace the three-platform candidate and eight installed cohorts. Implementation
and dispatch require the independent design approval recorded by the coordinator.
Design approval was recorded after three actual Claude Code auto review rounds;
see [review evidence](../reviews/streaming-macos-diagnostic.md). Integrated code
approval and its bounded budget amendment are recorded there. Hosted
[run 42367008260](https://github.com/Epiphytic/marmish/actions/runs/42367008260)
passed all six requested observations and cleanup proofs; see
[sanitized evidence](../validation/streaming-macos-diagnostic.md). This same-host,
single-client-identity diagnostic does not qualify the candidate, explain the
original cross-host failure, or establish a product fix. Full all-platform
qualification remains pending.

## Evidence and question

Installed stream-matrix run 42363063498 used product source
`571d98b77780` (the full immutable source comes from its downloaded manifest).
All three clients completed byte-exact duplex execution against both Linux
servers; all failed against the macOS server after readiness. Cleanup and daemon
stop proofs succeeded. The public failure category does not distinguish command
refusal, fixture failure, stream terminal mismatch, or output mismatch.
`installed_streaming::collect_receipts` runs only after successful client exchange,
so absent uploaded receipts do not establish that fixtures never ran.

`installed_streaming_cases::duplex` chooses compression when `client <= server`.
Every macOS-server cell consequently uses zlib, although some successful Linux
cells also use zlib. The diagnostic must separate fixture execution, raw transport,
zlib transport, and concurrent siblings rather than assume a cause.

## Smallest implementation boundary

Add one `MacosDiagnostic` subcommand to the existing **build-only**, feature-gated
`marmish-installed-check` binary and one sibling module
`installed_macos_diagnostic.rs`. Register it in `installed_check.rs`; add a manual
`.github/workflows/streaming-macos-diagnostic.yml`. No product binary entry point,
wire schema, executor, configuration format, dependency, version, or release
workflow changes. Do not call the existing Context-based prepare/run entry points:
those intentionally require tooling source and product source to agree.

The new probe is built only for macOS ARM64 from the reviewed diagnostic source.
It runs the immutable **old candidate** installers, launchers, daemon and fixture.
New harness code must not be passed off as candidate-571 tooling. In particular,
`records::verify_tools`, `Context`, `IdentityReady`, and candidate cohort rollups
must not be forged or weakened to admit mixed source provenance.

The diagnostic policy is frozen to candidate build run **42358105270**, attempt
**1**, source **571d98b77780d265a2fb29d84ae160fec72ba37b**. These are constants
in the JS guard and Rust pins validator, not workflow inputs.

Subcommand inputs are absolute owned `--assets`, `--bootstrap`, `--fixture`,
`--workspace`, `--output`, and `--pins` paths. Pins is a small closed JSON record
constructed by the workflow from verified original candidate artifact manifests:
full candidate source SHA, candidate run/attempt, archive/manifest/bootstrap/old
fixture SHA256 and byte sizes, target, and version. The probe also records its own
full harness SHA, binary SHA256, and target separately. The closed pins schema is `schema=1`, `kind=macos-stream-diagnostic-pins`,
`qualification=false`, `repository=Epiphytic/marmish`, string candidate run/attempt,
full candidate/harness commits, target, version, `old_harness_sha256`,
`artifacts={product,tooling,bootstrap}`, and extracted `manifest`, `tooling_record`,
`bootstrap`, `fixture` file pins. Each artifact pin contains exactly numeric ID,
name, SHA256 and ZIP byte size; each extracted file pin exactly SHA256 and byte
size. Rust rejects unknown fields, checks its compiled harness commit and fixed
candidate constants, and verifies all executable file pins before execution. The old fixture comes from that candidate's
macOS tooling artifact, **not** the newly built probe's sibling fixture.

## Existing helpers to reuse

- `native_manifest::verify_directory` verifies original candidate downloads and
  installed fixed-name contents; the actual pinned bootstrap is invoked with
  `--asset-dir`, private prefix/bin/state roots and `--key-storage file`, as in
  `installed_prepare::run`. Check installed launchers resolve inside that prefix.
- `installed_process::{Environment,utilities,command}` supplies credential-free
  environments and child process-group creation. **Do not use `run_input`**: it
  discards capture/status on EPIPE, timeout, overflow or pipe errors. Implement one
  diagnostic-owned bounded runner described below, without changing ordinary
  cohort process helpers. Reuse reviewed
  `installed_edge_rsync::{leader_exited,reap_owned_group}` for all child finalizing;
  never signal a group after reaping its leader.
- `matrix::base`, `matrix::Daemon::{start,stop_streaming}`, `matrix::ready` and
  `autoexit::socket` provide installed CLI arguments, authenticated readiness,
  actual child-TMPDIR sockets and existing stop proof. Resolve both sockets before
  spawn and reject equality. Keep public targets as `records::public_destination`.
- `streaming_fixture::{arguments,bytes,receipt,receipt_path}` supplies the exact
  six arguments, binary vectors and closed receipt expectations. Copy the pinned
  old fixture to a fresh private `fixture/main` inode, mode0700, with private
  `fixture/receipts`, exactly as the original cohort does.
- `streaming_cleanup::{entries,status,cancel_all,StreamStatus::validate_idle}`
  supplies same-UID, owned-child observations and explicit administrative cleanup.
  `records::{read_bytes,canonical,write}` preserves bounded private reads and
  immutable public records.

Extract only a small reusable helper if required; do not broaden ordinary cohort
behavior. Setup configuration is assembled through the **installed public CLI**:
create server/client file-backed identities; update the server grant with the
client npub and client trust with server npub. Use their setup JSON to validate
roles/backend/cleanup. Set server cwd to the owned workspace and its sole exec
allow rule to the canonical copied fixture image. Final config grants **only this one client**;
server role enabled, client role client-only, `active_per_principal=4`, no
`max_commands` field, and `limits.max_uptime=1200`. Pass `--max-uptime 1200` to
both foreground daemons. This accommodates five total stream admissions and
three simultaneous streams; do not call Context-bound `matrix::configure`, which
sets a cumulative four-command limit. Validate serialized CommandConfig before
starting either daemon. Do not mutate a leased config,
read identity secrets, or open live private databases to bypass IPC.

## Execution and fixed budgets

One run has an absolute900-second work cutoff plus180 seconds reserved for
cleanup/reporting. Workflow timeout is48 minutes, build capped at23 minutes and
the outer probe step capped at19 minutes. Its inner work/cleanup budget remains
1080 seconds, leaving one minute for record publication and step teardown. The
outer build/probe caps total42 minutes, leaving6 minutes for toolchain setup,
artifact acquisition, independent validation and upload. This administrative
margin corrects the code review finding that an18-minute outer step exactly
equalled its inner900+180-second budgets; it adds no live-work allowance. Daemon uptime1200 exceeds the complete probe
execution budget. Cleanup uses its own fixed deadline, never the expired work
deadline. Setup/install each cap at120 seconds, readiness at180,
each direct case at15, each sequential stream at180, and the optional concurrent
batch at240. Every operation is clipped by the original work cutoff. These are
diagnostic ceilings, not latency acceptance or throughput promises. Their sum
exceeds900 seconds: they are maxima, never reservations or renewed allowances.
Late stages may receive less than their nominal ceiling or be recorded
`budget_exhausted` without starting. The concurrent batch is optional, not required
for a valid report of earlier observations.

1. Install the old candidate via its exact bootstrap and local asset directory;
   verify installation manifest and executable hash.
2. Run the copied **old fixture directly** with exact six arguments and96KiB
   stdin. Capture up to1MiB on each output; require exit7,96KiB stdout,8KiB
   stderr, exact hashes and canonical closed receipt with the actual old fixture
   image hash. Use a fresh nonce for every operation. A failed direct test is
   reported but does not suppress the independent transport attempts if safe.
3. Start one installed server and one installed client using fresh identities,
   explicit `wss://relay.us.whitenoise.chat`, Tor off and file storage. Wait for
   PID/UID-authenticated active status.
4. Run installed public `marmish ... stream NPUB -- ./fixture/main ...` once raw
   and once with `--compress`. Preserve output and journal observations **before**
   evaluating expected hashes/terminal. Each case gets a distinct stream/nonce.
5. When `--concurrent` is enabled, run exactly three additional zlib commands
   concurrently against this same owned server with fresh nonces. This isolates
   sibling load, not three independently authenticated client identities; report
   that limitation explicitly. No active work from another job may share it.
6. Observe physical idle, then stop both daemons and validate their actual stop
   reports. On failures explicitly admin-cancel owned server streams, wait for
   physical idle and perform the same stops. Unconfirmed cleanup is a failure,
   never a reason to delete state or claim absent children.

Use an explicit async finalize-on-all-results wrapper, not a claim that Drop can
await proof. Register SIGINT and SIGTERM before any async child work. Whether work
succeeds, errors, expires, or receives either signal, retain runtime ownership and
execute one bounded finalizer: stop/join local client attachments using unreaped
child ownership; request client stream cancellation via owned client IPC; invoke
`cancel_all` on the owned server; poll physical idle on both daemons; stop client
with `stop_streaming(false)` and server with `stop_streaming(true)`. Attempt the
second cleanup even if the first fails; retain separate proof/error fields. A
signal during finalization is latched but does not skip cleanup. No process PID
from an artifact grants authority. `OwnedCleanupGuard` is not a generic async
daemon guard and will not be used as such. SIGKILL/runner loss cannot promise
async cleanup; record this residual limitation.

Private state removal happens only after confirmed physical cleanup; otherwise
retain it only on the runner, never in artifacts, and report cleanup unconfirmed.
Foreground diagnostics and raw child captures remain private.

### Diagnostic-owned runner

`capture(executable,args,cwd,environment,input,deadline)` returns a closed outcome
with normal exit/signal if observed, partial stdout/stderr (private, at most1MiB
each), per-pipe overflow/error flags, input bytes actually written, input-EPIPE,
timed-out, and owned-group-cleanup result. Concurrently pump stdin and drain both
outputs. BrokenPipe closes the input writer and records partial input, but does
not discard captures or bypass child completion. Overflow keeps draining while
discarding the excess, records a flag, and cannot grow memory. Pipe errors remain
separate from validation failures.

Observe exit with WNOWAIT (`leader_exited`), retain the exact Child unreaped, and
allow at most two seconds of drain grace, clipped by the original deadline,
before cleaning descendants that retain pipes. On exit or deadline, use
`reap_owned_group` exactly once on the still-owned, unreaped leader; its existing
bounded cleanup and fresh-ESRCH policy applies. Do not call try_wait/wait first.
On runner cancellation the outer finalizer retains the runner ownership/task and
runs this same cleanup path; dropping a future must not silently drop its Child.
A failed group cleanup is reported, never converted into successful execution.

Before each sequential command record bound client/server List snapshots. If the
stderr prefix is absent/malformed, diff before/after validated List entries by
exact account principals and direction, then match session/group/Open binding
across endpoints. A unique new matching entry permits journal/receipt evidence;
zero is `list_absent`, multiple is `list_ambiguous`. In the concurrent batch there
may be multiple new entries: only an independently parsed stream prefix provides
per-case binding, otherwise mark per-case journal unavailable/ambiguous and
preserve batch-level counts. Never guess from order or raw stderr.

## Sanitized evidence contract

A new closed `kind=macos-stream-diagnostic`, `qualification=false` JSON report
records separate product/harness provenance, target, explicit relay, timings,
case names and fixed stages: `pins`, `install`, `fixture_direct`, `setup`,
`server_ready`, `client_ready`, `stream_spawn`, `stream_capture`, `stream_parse`,
`client_journal`, `server_journal`, `receipt`, `idle`, `stop`, `report`.

Record child normal exit or signal, stdout/stderr lengths and SHA256, input length
and SHA256, parsed-stream-present, fixture-receipt-present/valid, and expected
match booleans. Preserve typed finite error categories from a closed allowlist;
unknown stderr is represented only by length/hash. Use the fixed category enum
`pins_invalid`, `install_failed`, `setup_failed`, `readiness_timeout`,
`input_epipe`, `capture_io`, `capture_overflow`, `child_nonzero`, `child_signal`,
`stream_id_absent`, `stream_id_malformed`, `list_absent`, `list_ambiguous`,
`binding_mismatch`, `terminal_mismatch`, `hash_mismatch`, `length_mismatch`,
`receipt_absent`, `receipt_malformed`, `budget_exhausted`, `interrupted`,
`cleanup_unconfirmed`, `report_failed`; no arbitrary Error::Invalid text.
“Journal” always means authenticated IPC List output, never database access. Never upload raw stderr,
arguments containing private paths, keys, profile JSON, daemon journals, environment,
absolute paths, PID/UID values, raw stream/session/account identifiers or databases.
Bindings may be compared privately and represented by digests/equality booleans.

Capture both endpoint List entries before assertions: direction, state, terminal,
active, cancel_requested, Open presence and exact binding equality. The old public
List API **does not expose transport sequence/accepted/delivered/EOF offsets**.
Report these fields as unavailable, not zero or inferred from captured lengths.
Captured stdout/stderr byte counts are observation cursors only; stdin fed to the
local process is not proof of remote delivery. Do not add a product IPC change or
send a competing Frame request merely to obtain counters. Receipt validity and
final exact output remain independent evidence.

Every safe case writes an observation even on validation failure. Final success
means only the requested diagnostic cases passed and cleanup was confirmed.
Report-publication errors fail the job. The Context-bound installed rollup
publisher is private and is not reused. This diagnostic owns three create-new
records in its fresh owned output directory: report.json, complete.json, and
failure.json. Publish full validated report.json first, complete.json last;
consumers require both, and any failure.json takes precedence. On any write,
post-link removal or directory-sync error attempt failure.json with a fixed
report_failed category and return nonzero. Workflow success also requires probe
exit0 and independent closed-record validation; marker absence or inability to
write failure is still failure, never green. Unknown existing files are not
overwritten or deleted. Artifact upload uses explicit sanitized file names only.

## Manual workflow and trust boundary

Inputs: reviewed full `harness_commit` equal to workflow SHA and `concurrent`
boolean defaultfalse. Product run/attempt/source are the frozen constants above.
Only one macos-15 ARM64 job; contents/actions read permissions only, no secrets,
release upload, cargo publish, tags or Radicle actions.

Add standalone `tools/v02-actions/macos-diagnostic.mjs` and adjacent test. Import
`checkRun`, `sourceDigest`, `checkTooling`, `verifyTools` directly from workflow.mjs;
do not modify its provider diagnostic mode or call `candidateInputs` (that
function requires ordinary candidate source==workflow SHA). `checkRun` itself
does **not** impose equality with GITHUB_SHA: construct its candidate descriptor
from the frozen constants and pass only streaming-candidate workflow path, then
explicitly require the API head_sha equals the frozen source. Require completed
success/master/dispatch and exact repository/run/attempt. Fetch original run
artifacts through the API, require exactly one unexpired match per expected name,
record artifact ID plus API-reported sha256 ZIP digest, download ZIP and verify its
bytes before extraction. Reject absent/non-sha256 digest; no silent trust downgrade.

Only these three original artifacts may be downloaded:

- `v02-candidate-aarch64-apple-darwin-42358105270-1`
- `v02-installed-tools-aarch64-apple-darwin-42358105270-1`
- `v02-pinned-bootstrap-42358105270-1`

Freeze these additional immutable API ZIP anchors; recheck the API metadata and
actual downloaded bytes against them, with no fallback:

| Artifact | ID | ZIP bytes | SHA256 |
| --- | --- | --- | --- |
| product | 13570516008 | 47988691 | `f6c27bfab5b3dd2370eb38fa93f99f1c48ae906a7f04c6f45a59bf2bc758f970` |
| tooling | 13570605471 | 9460623 | `6cf4a04d8ee549bd86d9f7a256fefd7cfdcdb68eaccc189ba5c03b2c1315895c` |
| bootstrap | 13570810425 | 3543 | `974f86f238a8798ad2e565f300930070073cc5097748df07c30a9626c871d2ba` |

Use bounded extraction validation to reject duplicate/unexpected members,
symlinks and traversal. Derive original `sourceDigest(frozen_source)` from the
checked-out ancestor Git object, and pass it to `verifyTools` for macos-arm64;
record the original tooling JSON digest and separately verify fixture bytes/size
against it. The original tools record and manifests are anchored by the verified
API ZIP digests, not merely internally consistent pins. The bootstrap is rendered,
not the source scripts/install.sh. Its verified API ZIP digest is the primary
byte anchor. Additionally require extracted install.sh to equal the frozen-source
scripts/install-native.sh template with exactly its four placeholders replaced:
version equals the verified manifest version, sha_macos equals the verified
macOS manifest SHA256, and sha_amd64/sha_arm64 are each64 lowercase hex characters
(their manifests are not downloaded). No @MARMISH_ string may remain. Record the
bootstrap digest and test this structural check, including wrong template,
version, macOS digest and unresolved placeholders.
Record product archive/manifest digests plus all API artifact digests, source
fingerprint, original tooling record digest, harness source/fingerprint and newly
built probe hash in the sanitized envelope. No GitHub token reaches the probe's
child environment or report.

Require frozen candidate source is an ancestor of harness_commit, harness_commit
is an ancestor of fetched origin/master, and HEAD==harness_commit==workflow SHA.
Name-status diff frozen source..harness_commit must contain A/M only, mode100644,
with the **entire** delta restricted to this closed list:

- `.github/workflows/streaming-macos-diagnostic.yml`
- `tools/v02-actions/macos-diagnostic.mjs`
- `tools/v02-actions/macos-diagnostic.test.mjs`
- `crates/marmish-native/src/installed_check.rs`
- `crates/marmish-native/src/installed_macos_diagnostic.rs`
- `crates/marmish-native/src/installed_macos_diagnostic_tests.rs`
- `tools/v02-actions/macos-diagnostic-policy.test.mjs`
- `docs/agent-spec.md`
- `docs/validation/streaming-candidate-571d98b.json`
- `docs/validation/streaming-installed-macos-failure.json`
- `docs/phases/streaming-macos-diagnostic.md`
- `docs/reviews/streaming-macos-diagnostic.md`
- `docs/validation/streaming-macos-diagnostic.json`

Only the explicitly listed subordinate test module may be added; no shared fixture,
vector, receipt, process, product CLI or workflow helper changes are permitted.
No deletions/renames/executable modes/symlinks. Reject unexpected source changes
rather than expand the allowlist automatically. Build locked installed-checks
probe only; installation always uses original candidate assets/bootstrap. This
same-host macOS-client/macOS-server diagnostic cannot clear Linux→macOS or
cross-host interoperability and says so in every report.

Concurrency group prevents overlap with installed relay qualification; coordinator
also retains the existing exclusive live-test lane. No automatic dispatch or
public release is part of implementation. Upload only the sanitized closed report
and small provenance records on success or failure; retain no private workspaces.
After a diagnosed fix, build fresh candidates on all three platforms and run the
full eight installed cohorts. This probe cannot turn candidate571 green.

## Required checks before dispatch

Hermetic tests cover strict pins/input parsing, source separation, exact old-fixture
selection, stage/error redaction, raw/zlib argv and npub roundtrip, binary/special
arguments, receipt validation, invalid exit/hash/length preservation, absent vs
malformed journals, unavailable-offset semantics and failure report precedence.
Use synthetic Output/journal values for classification tests, not fabricated
transport passes. Add a real local copied-fixture test using an isolated test
fixture image; separately label it as harness verification rather than old-candidate
macOS evidence. Test child early exit, timeout cleanup and sibling child fencing. Deterministic runner tests cover
child exit before stdin, partial EPIPE with retained status/captures, overflow,
missing/malformed prefix with unique/missing/ambiguous List diff, descendant-held
pipes and exact once-only unreaped cleanup. Test ancestry/allowlist denials, wrong
run/attempt/source/artifact API digest, original tooling source-fingerprint drift,
no Context construction, active>=3/no max_commands/uptime config, interruption
finalizer ordering, and forced second/post-hardlink publication failures.
Workflow policy tests prove Mac-only runner, immutable candidate download, read-only
permissions, bounded deadlines, sanitized upload and no publication. Run focused
installed feature tests, strict feature all-target Clippy/fmt and independent
Claude integrated code review. Actual Mac direct/relay observations occur only
after source approval and authorized coordinator dispatch.
