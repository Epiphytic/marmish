# Owned macOS loader-denial diagnostic

Detailed design for Beads `marmish-cjy.3`. Independent actual Claude opus5.5
**APPROVED with condition C1**, now incorporated; no further design review was
required for that doc-only correction. Earlier B1–B5 and R1/R2 are resolved.
See [the review record](../reviews/macos-owned-denial-design.md).
This document authorizes no implementation or native execution by itself.
The user's phased workflow requires independent Claude design review before
implementation and integrated code review before publication/native diagnosis.

## Evidence and purpose

Run 41132685757 matched an owned synthetic probe PID and non-nil Mach-O UUID,
including the reviewed redacted executable path, then recorded a confined
SIGABRT in dyld/libignition startup (namespace 0x23, code 2). The unconfined
control passed. See [the established boundary](macos-dyld-startup.md) and
[owned crash attribution](macos-probe-crash-context.md). The exact failed
operation remains unknown. There is no evidence for a particular sandbox grant.

The next discriminator is a denial record attributable to that exact synthetic
execution. This is a diagnostic extension to the prior explicit no-log-query
contract. It does not change the production profile, probe, compiler recipe,
allowed/denied preflight matrix, identity setup, Keychain access, installer,
managed-runtime acceptance or transport. macOS managed packaging validation and
this configured runtime abort are separate gates; neither waives the other.

## Existing interfaces and proposed ownership

`tools/marmish_macos_crash_diagnostic.py` already provides `read_owned_uuid`,
`run_child`, `snapshot`, `wait_context`, and strict PID/path/UUID attribution.
`tools/marmish_macos_probe_diagnostic.py` provides the exact fixture, build,
production command builder, fixed environment, hashing and exclusive report
writer. `run_child` currently captures its Popen PID internally and removes it
before export. `collect` compiles one short unique `mp-` basename, runs the read
control, snapshots crash candidates, runs the confined read and waits up to
20 seconds for owned crash context. Keep those existing contracts unchanged.

Implement, after approval, a separate
`tools/marmish_macos_denial_diagnostic.py` with only `--output PATH`, plus
`tools/test_marmish_macos_denial_diagnostic.py`. Reuse public fixture/build,
UUID and crash-selection helpers. A new local child wrapper may retain private
PID and timestamps; do not extend prior public reports with raw identities.
A workflow agent owns only a separately reviewed addition to the existing
manual `macos-probe-diagnostic.yml`: independent private preparation/run steps
with `if: always()`, and a distinct always-upload artifact containing exactly
one report JSON. Preserve hosted macos-15, readonly contents, pinned actions,
checkout credentials disabled and ten-minute job limit. No sudo, entitlement,
log-configuration change, secrets, caches, elevated permissions or raw artifacts.
The extra driver budget is at most 45 seconds after compilation; account for
all existing/new diagnostics, including the two-second preflight before
compilation, against the unchanged ten-minute workflow deadline.

## Private execution and attribution

Require Python 3.11+, Darwin and ordinary non-root UID before compiling or
querying. Before compilation, require os.waitid/WNOWAIT and confirm non-reaping
exit observation with an owned new-session `/usr/bin/true` preflight (two-second
limit, fixed environment, group kill before reap). Missing symbols or unsupported
observation, preflight timeout or exception emits outer
`failure.category=query_observation_unsupported` with log
category `not_queried`; no compilation/query or alternate cleanup method follows. Use a canonical owned 0700 `/private/tmp` fixture, the exact existing
probe source/flags/profile and production command builder. Only read `/dev/null`;
use the existing owned loopback listener to satisfy the profile's broker
parameter, with no external traffic. Run one unconfined control and one confined
read sequentially; DEVNULL input/output, closed descriptors and fixed environment.
Capture only bounded private stderr, preserving the prior five-second child
limit and kill/reap behavior. Do not create/import identities or touch Keychain.

Read the owned Mach-O identity before either child. Record private UTC wall-clock
instants immediately before Popen and immediately after wait/kill/reap, alongside
monotonic duration and owned PID. Refuse a negative wall interval or one longer
than seven seconds; record fixed `clock_window_invalid`, never a widened query.
Require the generated basename is at most 15 ASCII characters (currently 11);
keep it and the identity private. Snapshot crash files before spawn
and use existing 20-second collection and attribution without loosening any
exact/alias/redacted path or UUID rule.

Only query logs when the unconfined control exited zero, confined child is
SIGABRT, and existing crash attribution returned `matched` for the exact PID and
probe identity (not merely `sandbox_exec_exact`). Explicitly require
`context.path_shape.uuid_image_match is True` for **every** probe attribution,
including exact and alias paths: existing helpers do not enforce this on those
branches. Reuse that shape fact without weakening prior helpers. Otherwise emit
`control_invalid`, `not_sigabrt`, or `owned_crash_unavailable` and do not query.
UUID binds the executable, not the execution; private Popen PID, unique basename,
pre-spawn snapshot and measured window remain required. No old report fallback.
A missing or redacted record is inconclusive, not permission evidence.
The crash UUID authenticates neither unified-log contents nor the logger's
process metadata. PID reuse, timestamp precision, logger delay/redaction and
unknown record schemas can undermine correspondence; the unique name and short
window reduce ambiguity but do not prove causal ownership. A selected denial is
a candidate hint to investigate, never authenticated causal denial proof or
sufficient permission allowance. A matching message may also be spoofed; no
production policy decision may rely on it alone.

## Fixed log query and bounded capture

Use absolute `/usr/bin/log`, fixed argv and no shell. Proposed interface:
`log show --style json --start START --end END --predicate PREDICATE`.
Generate START/END literally as `YYYY-MM-DD HH:MM:SS+0000`, rounding start down
and end up to the second. Compare wall elapsed with monotonic elapsed: difference
must be at most one second, or `clock_window_invalid`. Revalidate parsed records
against this same rounded inclusive interval, at most nine seconds wide, never
an accidentally stricter precise interval. Record timestamps must have an
explicit numeric offset (`+HHMM` or `+HH:MM`, similarly minus), optional 1–6
fractional digits, fixed date/time with space or T separator, no zone-name/Z or
leap-second fallback; parse strictly and convert to UTC.
PREDICATE is generated from validated `mp-[0-9a-f]{8}` and a positive bounded
Popen PID (1 through 2^31−1), and requires the exact message token `BASENAME(PID)` plus a denial
marker. Restrict the predicate to `process == "kernel"`; accept a returned
record only when its exact `processImagePath` is `/kernel`. Drop sandboxd entirely. No unfiltered fallback, log stream, entire-host dump, reverse
search, prior boot, broader name-only query or privileged retry.

CLI syntax, `processImagePath=/kernel`, timestamp shapes, cryptex aliases and
operation spellings suggested by the reviewer are **unverified memories**, not
facts established against this OS build or sourced public documentation here.
No public-source lookup or native query was performed for this amendment.
They are fail-closed contracts for the reviewed native format-validation run.
Use a fixed internally generated UTC format; if this OS cannot interpret it,
return `query_failed` rather than relaxing the window. The account uses only
its existing log-read authorization; permission denial is `query_unavailable`
with no OS error text, escalation, auth prompt or OS configuration change. Do not interpret the
emitter's numeric processID as the denied child's PID: attribution comes from
the exact name/PID pair parsed from the denial message in the same record.

Run the query once after crash attribution, timeout five seconds, with fixed
PATH and no inherited loader/Python variables. Start an owned process group;
use `start_new_session=True`, monitor capture descriptor sizes at intervals of at most 50 ms while running,
and enforce stdout 256 KiB and stderr 4096 bytes. Kill the group **before** reaping
the leader, including normal completion, then wait; never signal a possibly
reused PGID after `poll`/`wait` has reaped it. A bounded non-reaping exit observation
(e.g. supported `waitid(...WNOWAIT)`) must precede this sequence; unsupported
observation support failure uses the preflight outer
`query_observation_unsupported`/log `not_queried` outcome before compilation.
If observation fails at query time after successful preflight, use `query_failed`.
Neither case permits signaling a group after its leader has been reaped or a
racy cleanup fallback.
Timeout, overflow, exception and cancellation use the same kill-before-reap order. Capture
only in a private 0600 descriptor under the fixture; read at most cap+1 bytes,
reject overflow, and delete on owned fixture cleanup. Never print raw subprocess
output, exception messages, predicate, time, basename or PID. Bounds are enforced
while the query runs, not merely on a later read. JSON accepts at most 128 records
and at most 8192 encoded bytes for any message, with bounded parser depth/errors;
field overflow (message >8192, timestamp >64, image path >4096 bytes) rejects
the whole collection as `format_unknown`, empty denials/null count. Capture
overflow remains `query_output_limit`. No partial result is promoted to a match.
Do not upload or leave behind raw JSON, stderr, `.ips`, directory listings or logs.

## Record validation and finite export

Decode strict UTF-8; reject duplicate keys (`object_pairs_hook`), NaN/Infinity
(`parse_constant`), malformed JSON and RecursionError. Initially accept only a
normal JSON array. Each record's `expected` keyset means required `timestamp`,
`processImagePath`, `eventMessage` keys are present with bounded string values;
extra keys are allowed, ignored after parse and never exported. Timestamp is at
most 64 bytes, image path 4096, message 8192; the total capture bound still applies.
A zero-exit query returning `[]` is valid and produces `no_owned_denial`, count0.
Zero-byte stdout is `format_unknown`, never a valid empty-result assumption. Unknown representations remain `format_unknown`. Export only a
finite `format_shape`: top_level=array/object/empty/other, leading_text=boolean,
trailing_summary=boolean, keysets=unique enums expected/summary/other (maximum 3),
timestamp_parse=not_observed/valid/invalid/mixed. No actual key names or values.
A `{count, finished}` trailing summary or NDJSON is shape evidence but remains
`format_unknown`; do not silently accept another schema. Query failure may still
record bounded shape facts without any content. Invalid UTF-8 has other/false/
false/empty/not_observed defaults and a fixed format failure.

Revalidate each record's timestamp in the rounded window and exact kernel image
path. Parse only anchored `Sandbox: BASENAME(PID) deny(N) OPERATION[ TARGET]`.
N is a positive integer of 1–9 decimal digits; spaces separating fields are ASCII. Exact full basename
and decimal PID equality are required, never substring or mixed-record matches.
TARGET is the remainder of the line, including internal spaces, compared only by
exact classifier rules; absent target maps to `none`. Reject NUL, controls,
non-ASCII, escapes that introduce controls, multiline text and invalid fields.
A `sandbox-exec(PID)` denial before exec is deliberately excluded, even with the
same PID. Such exclusions and format mismatches cannot establish absence of a
denial. Any returned record rejected for image path, window/timestamp, grammar,
exact name/PID token, multiline/control text, pre-exec sandbox-exec or out-of-range
deny counter makes the entire parsed collection `format_unknown`, with empty
denials and null matched_count, even if another record matched. This precedence
is above matching_limit as listed below. Never call such a collection
no_owned_denial. No rejected content enters the report.

Operation allowlist (finite diagnostic subset, **not** a complete macOS vocabulary):
`file-read-data`, `file-read-metadata`, `file-map-executable`, `process-exec`,
`mach-lookup`, `sysctl-read`, `system-fsctl`, `file-ioctl`, `file-test-existence`,
`process-info-codesignature`, `iokit-open`, `system-info`, `ipc-posix-shm-open`,
`ipc-posix-shm-read-data`, `ipc-posix-shm-write-data`, `ipc-posix-shm-write-create`,
`ipc-posix-shm-unlink`. Unknown spellings are never approximated. The actual
PROFILE already allows file-read* under /System and /usr/lib, metadata globally,
sysctl-read, process-exec for EXEC and process-info* targeting self. A corresponding
candidate may reflect aliases or another constraint; it is not proof of a missing
grant. Operations not obviously allowed are hypotheses, not established causes.
Map to those fixed enums only. TARGET is never copied into the report. Initial
resource classification is intentionally narrow:

- Exact `/usr/lib/dyld` -> `system_dyld`.
- Exact `/usr/lib/libSystem.B.dylib` -> `system_libsystem`.
- Exact `/dev/null` -> `fixture_dev_null`.
- Exact `/System/Library/dyld` -> `system_dyld_directory`.
- Exact `/System/Library/dyld/dyld_shared_cache_arm64e` or
  `/System/Library/dyld/dyld_shared_cache_x86_64` -> `system_shared_cache`.
- Exact `/System/Volumes/Preboot/Cryptexes/OS` -> `system_cryptex_os`.
- Exact cache paths under each of `/System/Volumes/Preboot/Cryptexes/OS`,
  `/System/Cryptexes/OS`, `/private/preboot/Cryptexes/OS`, followed by
  `/System/Library/dyld/dyld_shared_cache_arm64e` or `_x86_64`, optionally followed
  by a literal dot and 1–3 decimal digits -> `system_cryptex_shared_cache`.
  No other suffix or path normalization; these proposed aliases are unverified.
- No target -> `none`; do not interpret it as a filesystem path.
- Every other target -> `unclassified` with **no text, basename, path hash,
  length, UUID, Mach name, host identifier or preview**.

These entries classify evidence; they do not assert that those resources were
accessed or justify granting them. No unrestricted wildcard target, prefix export or arbitrary
Mach service/sysctl name is accepted. An owned, grammar-valid denial with unknown
operation uses fixed enum `unknown`; no raw spelling. Known and unknown operations
may coexist as pairs in `denials`. Any unknown makes the category
`operation_unknown` even when useful known pairs also exist. Unknown target
retains the known operation and `unclassified`, but is insufficient to select a narrow resource permission.
Before any future fix requiring an unknown target, design/review a separate
finite classifier extension. Prefer another inconclusive run over exporting
private host state. Conflicting matching records export up to eight unique
(operation, resource_class) pairs sorted deterministically; excess is
`matching_limit`, not silent truncation.

## Report and failure semantics

Exclusive 0600 report outside fixture, schema 1, with fixed driver/version tag,
platform/architecture, profile/probe-source hashes, sanitized existing control
and confined statuses, crash attribution enum and finite log object:

`{category, query_exit_code, query_signal, matched_count, denials, format_shape}`.

Total precedence, highest first: `control_invalid`, `not_sigabrt`,
`owned_crash_unavailable`, `clock_window_invalid`, `query_unavailable`,
`query_output_limit`, `query_timeout`, `query_failed`, `record_limit`,
`format_unknown`, `matching_limit`, `operation_unknown`, `owned_denial_found`,
`no_owned_denial`, `not_queried`. The first applicable category wins; not_queried
is the initial state when fixture/build failure prevented later gates.
All limit/format/query/gate failures export empty denials and null matched_count;
`matching_limit` also exports none, never a truncated list. Valid no-match exports
empty denials/count0; valid known/unknown pairs export their count1–8.
`matched_count` is always `len(denials)` for a valid collection, the number of
unique operation/resource pairs, never the number of matching source records.
Export no raw time/PID/UUID/name/path/message/predicate, command or OS error text.
No arbitrary exception fields or tracebacks in JSON/stdout/stderr. Explicit
cleanup/collection failure uses outer `failure.category=cleanup_failed` or
`fixture_failed`, never changes the finite log category and remains failure.

Confined SIGABRT keeps report `ok=false` and exit nonzero even if a denial is
found. Lack of evidence never becomes a passing runtime gate. If the confined
control passes, record that observed status but do not infer the complete native
allow/deny matrix or run a broader log query. Diagnostic success means useful
candidate evidence was collected; runtime acceptance is evaluated separately.

## Tests and exit criteria

Offline tests cover fixed argv/no-shell/no input expansion, exact PID/name token,
wrong/substr PID/name/emitter/time, timestamp boundary/rollback, duplicate keys,
unknown shape/operation/target, escaped/control/multiline inputs, multiple records
with name and PID split across them, same PID with wrong unique basename, nil or
wrong UUID and sandbox-exec-only crash attribution, oversized capture/message,
record/pair caps, deterministic finite output and rejection without raw leaks.
Expected rejection outcomes are fixed across driver and workflow tests:
wrong/substr PID or name, wrong kernel image, out-of-window/invalid timestamp,
malformed grammar, multiline/control text, sandbox-exec token and zero/overlong
deny N each => format_unknown/empty denials/null count. Missing/wrong-type
required fields or oversized fields, duplicate keys, invalid UTF-8,
NaN/Infinity, RecursionError,
NDJSON, trailing summary or zero stdout => format_unknown/empty/null. Oversized
capture => query_output_limit; >128 records => record_limit; >8 unique pairs =>
matching_limit; each exports empty/null. Extra keys with valid required fields
remain accepted/ignored. [] => no_owned_denial/empty/0; valid unknown operation =>
operation_unknown with unknown enum pair(s). Nil/wrong UUID or sandbox-exec-only
crash attribution => owned_crash_unavailable and no query. Clock inconsistency =>
clock_window_invalid and no query. Missing non-reaping support => outer
query_observation_unsupported and no compile/query. Include secret/path/hostname/UUID sentinels in every rejected and unclassified
fixture; assert absence from serialized reports and diagnostic console output.

Use bounded real synthetic query children to prove timeout/output-cap cleanup,
group reaping, asynchronous interruption, exclusive report behavior and private
fixture removal, without invoking actual unified logs in unit tests. Native CLI
format validation is a separate reviewed manual run; no unit-test permission
fallback. Test that root/non-Darwin/old Python refuse before log access, that
unattributed crashes never query, and that matching a denial preserves SIGABRT
failure. Workflow tests must allow only the report upload, preserve the existing
action pins/permissions/timeouts and never include wildcard fixture paths.

After design approval, implement with disjoint ownership: driver/test agent owns the two new diagnostic files;
workflow agent owns the YAML and new
`tools/test_marmish_macos_denial_workflow.py`;
review integrated code with Claude auto mode and run offline checks. Publish the
reviewed diagnostic increment to both source remotes, then perform one authorized
manual hosted native run and preserve only sanitized candidate evidence. If inconclusive,
record the precise fixed category rather than broadening scope in place.
A corroborated candidate may motivate a separately reviewed minimal production fix,
which must pass the full native allowed/denied matrix and actual Marmot command
execution. No such fix is part of this phase.

## Questions for independent design review

Validate whether log record timestamp/denial syntax can support this exact
attribution without a broader query; whether requiring prior UUID-attributed
crash evidence is appropriately conservative; whether the initial target enums
are useful while sufficiently private; and whether 256 KiB/128 records/five
seconds fit the ten-minute workflow. Any incompatible CLI/JSON contract must
fail closed or receive a reviewed design amendment, not an implementation guess.
