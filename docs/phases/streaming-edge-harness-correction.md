# Correct the installed edge-stream harness

Approved for implementation by actual independent Claude AUTO design review;
see [review evidence](../reviews/streaming-edge-harness-correction.md), recorded
before source changes. Existing human phased implementation authorization
persists; this document records no source qualification or public release authorization. Beads is currently
inaccessible; do not edit its database or migrate it to bypass that condition.

## Evidence and bounded purpose

Source `50ad3d1` candidate `42416184935` passed all three platform packages. The
subsequent original stream, rsync and copy matrices each passed all nine pairs.
The edge-stream run `42422474341` failed on all three platforms with the generic
`invalid_or_failed` category; final records contained `edges: null` and reported
cleanup flags true. Those flags are reported observations, not independent proof
of physical cleanup. No failing family/case was retained. The private exact-
candidate admission reproduction produced its expected public CLI outcomes;
that does not establish the hosted post-command journal or idle checks passed.

Two source defects weaken this harness: edge-stream does not prepare the private
`edge-gates` directory required for fixture marker writes, and its initial
running checks reject a legitimate inactive Hello observed before Open admission.
These defects justify correction; neither uniquely establishes which assertion
failed in the historical hosted run. Add only the breadcrumbs needed to locate a
remaining failure. No product behavior, protocol, evidence schema, workload,
assertion outcome, deadline, retry, IPC count or cleanup policy changes.

## Prepare the existing gate prerequisite

Extract the existing edge-rsync create-0700-then-validate prerequisite into a
small `edge_fixture::prepare_gates(workspace) -> Result<()>` helper in
`installed_edge_fixture.rs`. Use it before edge-stream families in
`installed_edges::run`, and replace the duplicated edge-rsync setup with the same
helper. The helper creates only the direct `edge-gates` child of the already
owned validated server workspace. It does not recursively create ancestors.

Accept a missing directory by creating it privately, or an existing directory
only after the existing identity directory checks establish a real directory,
current-UID ownership and private permissions. Reject symlinks, regular files,
unsafe ownership/permissions and unexpected I/O errors. Do not repair permissions,
follow a symlink, remove existing contents, or truncate marker/config files.
Existing immutable marker writes and nonce collision checks remain unchanged.
This prerequisite runs before any fixture can attempt to write its marker.

## Classify startup without weakening established-running proof

Add a small pure `StartupPhase::{Wait, Running}` classifier alongside `Entry` in
`installed_streaming_cleanup.rs`. It accepts `Option<&Entry>`, expected client
and server principals and an `established: bool`, and returns `Result<StartupPhase>`.
Callers already select the exact stream; preserve that selection. For every
present entry require direction `server`, exact principals, no cancel request,
no terminal and no `child_disposition`. Apply this table:

| Selected entry | Initial startup | Established running/detached dwell |
| --- | --- | --- |
| Absent | Wait | Reject |
| `hello`, inactive, Open absent | Wait | Reject |
| `preparing` or `launching`, active, Open present | Wait | Reject |
| `running`, active, Open present | Running | Running |
| Any contradictory flags, other phase, terminal, cancel or restart disposition | Reject | Reject |

Entry deserialization does not validate its plain-string Open digest. The
classifier itself must check every present digest with
`records::hexadecimal(digest, 64)` (lowercase-only); reject malformed presence
rather than treating it as absent.
Do not infer child absence physically from `child_disposition: None`. The product
initializes that field to None in `stream_runtime::stream_record`, preserves it
through Open admission, launch intent and identity-commit Running, and populates
it for restart recovery in `stream_store::stream_init`. The real child identity
is stored separately and is not exposed by this Entry projection.

Use the classifier in `installed_edge_signals::running` and
`installed_edge_quota::started`. Wait means
continue the existing poll under the original absolute deadline and sleep
interval; it never means success or a new deadline. Running only permits the
existing marker identity/argv/nonce checks and physical charged-status checks to
execute. Preserve all those checks and exact binding comparisons.

Detached dwell must pass `established=true` after its first proven Running
snapshot. Disappearance, Hello, Preparing, Launching or any contradictory state
must fail immediately; do not consume a startup wait and later count it as a
continuous detached-running observation. Retain the existing fixed two-second
minimum, at least three actual charged/unfinished Running polls, marker/binding
stability and explicit second-client confirmed cancellation. Initial startup may
wait for the first marker; established dwell must reject disappearance of its
previously verified marker immediately, rather than waiting for it to reappear.
No new polling.

Make the signals split explicit: `running_snapshot(..., established)` is a
one-shot async observation using the existing entry, marker and charged-status
requests, returning `Result<Option<(Entry, String, StreamStatus)>>`. None means
Wait only during startup; Some is the existing proven Running tuple.
`running` is the startup loop around that one-shot path and the existing 50ms
sleep. `detached_dwell` calls `running_snapshot(..., true)` directly on each
existing dwell poll; it never calls the startup loop. Established mode converts
any Wait, absent/regressed entry or missing marker into an immediate error.
Only startup may wait for a first marker. Quota `started` keeps its startup loop
and existing marker reads, using the same classifier.

Name the small pure proof seams so coverage does not require a live daemon:
`edge_fixture::validate_marker(&serde_json::Value, expected_nonce,
expected_image, expected_argv) -> Result<()>` compares exact JSON equality with
schema 1 and those three fields. Both signals and quota call it after their
existing bounded read; this preserves quota exact equality and signals typed
`deny_unknown_fields` behavior. Do not add a second marker read or weaken identity
checks. `streaming_cleanup::validate_charged(&StreamStatus, expected: u32)`
requires exactly `active == expected`, `unfinished_runners == expected` and
`uncertain == 0`. Signals uses expected 1 at the same existing status request;
quota uses expected 2 at its existing outer before/after refusal requests. Quota
startup gains no new status request. All additional existing status predicates
remain in place; the helper does not invent a physical proof.

## Minimal closed stderr breadcrumbs

Add `installed_edge_observation.rs` with one adjacent test file, registered in
`installed_check.rs`. Reuse the existing fallible locked
`streaming_observation::stderr_sink`; do not change the duplex trace or introduce
a common logging framework. Enable only for the existing edge-stream path.
Each prefixed JSON line contains only schema 1 and closed enums for client
platform role (the existing Cohort role, not inferred server platform), family, case, stage, outcome and optional coarse error category
(`timeout`, `io`, `invalid`, `other`). No arbitrary error text, raw process output,
identifiers, paths, argv, nonces, hashes, journal snapshots or terminal projections.

Use a case-scoped trace carrying only the last stage and bounded emission count.
Emit begin, stage changes before meaningful existing operations and an explicit
success/error only after capturing the original Result. On early return or
cancellation, Drop may emit `abandoned` with the last stage; it must not claim a
root cause, a timeout or completed cleanup. Do not restructure the async owner or
replace existing cleanup. In particular preserve the existing signals/quota
abort-error precedence, preopen resume/abort order, and all `?` short circuits.
A case can finish with the final returned error after its original cleanup path.
A once-only finished latch prevents a duplicate finish or Drop completion. Trace
writes acquire no daemon mutex; the sink only locks stderr, including while
preopen owns its existing Pause guard.

Keep stages modest and static: prerequisite, baseline, attachment, startup,
local_action, attachment_finish, detached_dwell, cancel, journal_check,
validation, idle and cleanup. Family/case distinguish admission's three cases,
signals' four cases, quota and preopen. The outer coordinator emits family begin/
completion and aggregate-validation/server-reconciliation stages, so completed
earlier cases remain visible even though a later failure leaves `edges: null`.
`confirm_server` emits its reconciliation breadcrumbs inside that helper, guarded
to edge-stream, so both its caller in `edges::run` and the later caller in
`installed_streaming` are covered without instrumenting other cohorts.
Do not emit within polling loops. Cap each trace at 24 ordinary lines plus one
truncation and one final line; cap each line at 512 bytes including prefix/newline.
Validate those bounds over all enum combinations. Format/serialization/write
failure drops the diagnostic only, never replaces an operation or cleanup error.
The existing supervisor inherits driver stderr, so no workflow/upload change is
needed. Missing final breadcrumbs remain ambiguous abandonment, not diagnosis.

## Exact implementation boundary and tests

Source ownership is limited to the fixture prerequisite and its edge-rsync caller,
`installed_streaming_cleanup.rs` classifier, signals/quota consumers, the four
edge-stream family files and `installed_edges.rs` breadcrumb wiring, the new
observation module/tests, and adjacent tests/module registrations. If necessary,
the only change to the old observation module is making its existing stderr sink
`pub(super)` for this reuse; its behavior and duplex schema remain unchanged. Preopen and
admission gain diagnostics only. No shared process runner, product/runtime,
workflow, report/record, source-pin or dependency edits.

Required focused tests:

- Gates: absent creates private directory; existing safe directory and content
  remain unchanged; symlink, regular file and unsafe permission cases reject.
  Ownership checks reuse the existing validator; exercise foreign ownership only
  where the test can construct it without privileges or changing system state.
- Classifier table: absent and every legitimate phase, correct progression,
  direction/principal mismatch, malformed Open, contradictory active/Open flags,
  cancellation, every terminal class and restart disposition. Established mode
  rejects absence/regression. Assert the existing marker/charged proof is still
  required after Running, using the named pure seams rather than live IPC. Test
  marker missing/unknown keys, all incorrect fields and wrong JSON types; absence
  cannot prove Running. Test charged counts too low/high and nonzero uncertainty.
  One-shot established observation tests reject missing marker immediately; no
  startup wait is reachable from dwell.
- Breadcrumbs: exact closed serialized fields, enum/line/event bounds, failing
  sink parity, last-stage error/abandonment, earlier completed cases retained when
  a later case fails, later families uncalled, and original failure/cleanup
  precedence. Never assert breadcrumbs prove remote execution or physical cleanup.

Run focused tests, the complete `installed_check::` filter, strict installed-
feature all-target Clippy, scoped formatting and diff checks. Obtain independent
actual Claude AUTO integrated code review and resolve blockers before publication.
Record initial failures honestly; do not change expectations merely to green CI.

## Exit and qualification boundary

Update source/specification and sanitized review evidence when implementation is
approved and tested. This source change requires a fresh three-platform candidate
and all eight serial installed cohorts from that same fixed source, using the
existing installer, provenance, traffic, budgets and fail-stop coordinator.
Preserve the failed historical attempt. Local tests or manually passing cases do
not qualify the installed platforms or substitute for any cohort. No public
release, version change or release-publication claim is part of this correction.
