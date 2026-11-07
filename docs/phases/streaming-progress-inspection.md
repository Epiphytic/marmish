# Read-only local progress inspection for the installed matrix

Design proposal for the original stream-matrix investigation. Implementation
requires coordinator authorization after an actual Claude auto design approval.
This author has not run Claude, Cargo, a workflow or relay traffic for this proposal.
The failed original attempt remains failed; observability does not qualify cells.

## Problem and scope

Original matrix run 42383713647 reached CLI spawn and then timed out in the
harness stdin write_all at 330 seconds for all three Mac-server cells. This proves
incomplete local writing, not zero reads, absent launch or a remote timeout.
CLI startup performs room preparation, Hello and Open before the first read;
subsequent reads are separated by awaited Exchanges. Each remote call can poll
Pending up to the default 660-second idle limit. Cold same-host diagnostics passed
but do not establish the cross-platform matrix or identify its cause.

Add one authenticated SAME-UID LOCAL operation to the existing daemon IPC and
one original-runner failure-finalization observation. No remote request, retry,
publication, reconciliation, cancellation, acceptance change, traffic extension,
new CLI mode, database migration or persisted diagnostic record is introduced.
This explicitly authorizes a product observability change; the next candidate is
not represented as identical frozen product bytes. Keep existing List, its DTO,
all existing CLI output and remote wire protocol unchanged. No version bump,
release publication or installer change is part of this increment.

## Grounded contracts

* stream_service.rs defines closed request/result enums and StreamEntry; the
  latter exposes neither slot request/response state nor byte counters.
* ipc.rs verifies Unix peer credentials before dispatch (verify_peer and accept
  loop), caps frames at 65,536 bytes and local exchanges at 15 seconds.
* stream_runtime.rs::stream_request_local executes on the leased identity actor.
  List reads only the journal; Inspect must use that same dispatch/access path.
* stream_store.rs stores StreamRecord, at most five retained request slots per
  stream, and globally bounded publications. Slots have request bytes, sequence
  and nullable response. Publications have state/native_id/intent/evidence.
  Existing stream_publications loads payloads: do not reuse that loader for Inspect.
* stream_runtime.rs initializes state=hello with hello=false. Client Hello
  responses do not set hello; a client Open request sets opening/open_digest,
  and projected Opened sets running. Server Open admits preparing before actual
  preparation/spawn. Identity persistence later sets server running.
* installed_streaming.rs returns on failed cells before collect_receipts. Every
  job owns its own client and server daemon, not any remote daemon. Inspection
  therefore belongs before runtime.close in unconditional failure finalization.

## New closed local interface

Add StreamLocalRequest::Inspect {} and
StreamLocalResult::Inspection { snapshot: Box<StreamInspection> }. Both use the
existing tagged deny_unknown_fields style; add no fields to old variants. No
caller-selected SQL, path, arbitrary filter, remote action or payload is accepted.
The response carries schema=1, entries and records_truncated. Select newest eight
live_streams by rowid, fetch a ninth key only to indicate truncation. Deterministic
rowid order is observational, not priority or completeness proof. Each returned
record includes private local binding stream/session/client/server and direction
for exact harness association, plus the following closed progress fields:

* phase: hello, opening, preparing, launching, running, cancelling, terminal,
  authority_revoked, lost, unknown_launch, transport_uncertain,
  reconciled_possible_live, cleanup_uncertain, server_stopping, server_stopped,
  projection_invalidated, other. Map unknown stored text to other, never echo it.
* server_hello_validated: nullable bool (record.hello only for server direction;
  null for client), open_present, active, cancel_requested, projection_invalidated,
  record_sequence, record_stdin_accepted, record_stdin_delivered,
  stdout_eof/stderr_eof nullable offsets and existing finite terminal projection.
* Up to five retained slot descriptors in insertion order, plus slots_truncated.
  Each contains closed kind {hello,open,exchange,status,cancel,copy_hello,
  open_copy,other,invalid}, sequence and response_cached. Exchange adds nullable
  stdin_offset/end, stdin_eof, stdout_ack, stderr_ack; response adds nullable
  stdin_accepted/delivered and output block ends/eof from validated wire fields.
  Inapplicable values are null. No argv, codec bytes, raw frame, request digest,
  open/group/event hash or error text is returned. Do not invent one pending
  operation: several retained controls/data slots may coexist.
* Fixed outbox buckets {unsubmitted,prepared,queued,published,uncertain,failed,
  acked,retired_failed,other}, each count plus native_id_present/intent_present/
  evidence_present counts. These are row-presence counts, not actual native
  fanout delivery, remote receipt or remote execution. No native ID/value escapes.

All offsets/sequences use checked u64 conversions with existing MAX_OFFSET bounds;
terminal codes/signals and wire error enums remain finite. Malformed record/slot
binding, decode or counter relationships produce a closed per-entry
integrity={valid,invalid}, with affected progress fields null, never plausible
fabricated zeros. Decode only existing bounded wire frames; verify slot request
binding matches its owning record before attributing it. A decoded cached response
must match binding, request digest and sequence before its scalars are exposed.
Unknown kind/state is observationally other, not an authorization fallback.

## Read bounds and absence of side effects

Compose existing wire decoders and finite projections of StreamRecord fields in
narrow journal queries in stream_store.rs or an adjacent stream_inspect_store.rs;
no generic inspector framework, new index or persistent progress cache. Existing
caps justify five descriptors: stream_request_slot admits at most four ordinary
slots, five including cancellation; global slot/publication limits are 992
ordinary and 1,024 with cancellation. Eight records is a diagnostic selection
budget, not the server concurrency limit (32), and may omit the relevant stream.

Use these bounded query shapes in one deferred read transaction:

1. `live_streams ORDER BY rowid DESC LIMIT 9`: ordinary rowid reverse traversal,
   not a full record scan or key sort. The ninth row is a truncation sentinel.
   Fetch `length(record)` and use SQL CASE to return record only when <=524,288
   bytes. This inspection-only allocation ceiling is conservative, not a new
   persisted-record validity contract. Over-cap/decode-invalid selected rows
   become invalid entries with no usable binding; never load an arbitrary blob.
2. Per selected key, `stream_slots WHERE k=? ORDER BY rowid LIMIT 6`: existing
   primary key `(k,digest)` supports key selection; ordering may sort that key's
   small retained set. Read five plus sentinel, check request/response lengths
   against MAX_CONTENT (32,768) with SQL CASE BEFORE loading blobs, then reuse
   canonical bounded decoders. Normal bound is eight small lookups, not a global
   slot scan. Unexpected excess produces slots_truncated, not silent completeness.
3. Read publication HEADERS once globally, in deterministic rowid DESC order, with a
   1,025-row sentinel limit; aggregate only the selected eight keys. There is no
   existing publication index on k, so eight per-key full scans are forbidden.
   A CTE may select k, closed CASE bucket and presence booleans; aggregate at most
   its first 1,024 rows, never loading payload/evidence/intent/native_id blobs.
   CASE maps unknown state to other before grouping, limiting output to nine
   buckets per key. If the sentinel exists, set outbox_truncated globally; these
   counts then describe only the deterministic prefix. No total/full-history or
   absent-outbox inference is valid. Normal cap bounds header work; database
   corruption is not assumed to preserve that cap.

Record BLOB input is at most 8 * 524,288 bytes and slot input at most
8 * 5 * 2 * MAX_CONTENT, independently of response size; these are input bounds,
not a claim about exact serde heap usage or RSS. Validate fixed-size stored slot
digests without loading oversize values. Release
per-record decoded payload/argv after projecting its scalars; never return them.
Fixed output fields, eight entries and five slot descriptors must serialize to
<=49,152 bytes INCLUDING the existing IPC response envelope, below FRAME_LIMIT.
Test the worst-case populated DTO, not only normal records. Over-cap serialization
returns finite inspection_capacity, never partial JSON. No full journal loader,
stream_records or payload-heavy stream_publications call is appropriate.

No stream_init, touch, cleanup, write, outbox repair or MDK call. No runner
snapshot: it can copy payload blocks and mixes physical/journal times. Journal
counters can lag the runner. Output ACK cursors are retained Exchange descriptors,
not invented record high-water marks; EOF is length, not proof of drain/ACK.
No work/query/cache/timer is added to normal stream processing when Inspect is
absent, beyond routing the new explicit local request variant. A harness-side
direct database read is rejected: the journal is daemon-owned, and bypassing the
authenticated actor would weaken ownership and snapshot consistency.

The identity actor executes synchronous rusqlite; its existing journal busy
timeout is two seconds, and must remain unchanged. The shared ASYNC caller
cutoff below bounds waiting for responses, not an already-started SQLite query,
busy wait, storage I/O or synchronous decode. Dropping the IPC future cannot
interrupt that operation; the actor may still be occupied and delay cleanup.
Finite row/blob bounds reduce work but do not guarantee scheduling or wall time.
Do not introduce connection-wide interrupt/progress handlers or alter busy timeout
for this feature. No await occurs within the read scope; transaction completion
or failure releases it normally. Cleanup reserve is a budget, not a promise that
an occupied daemon will finish cleanup in that time. Unknown/unavailable outcomes
must stay unknown; an inspection timeout must not assert the query never ran.

## Original harness capture and public evidence

For stream-matrix only, if ordinary work failed, attempt Inspect once per locally
owned daemon BEFORE runtime.close or AdminCancel. Skip on SIGTERM or the existing
installed_interrupted outcome: cleanup takes priority over diagnostics during
termination grace, independently of stop_by. If SIGTERM arrives during the
inspection await, abandon both local awaits immediately and enter cleanup; late
queued execution still follows the existing IPC rules below. Reuse installed_cleanup::stream_request
and response, which already provide one attempt, deadline, child liveness and peer
PID checks. Do not add daemon mutex acquisition: work and its guards are already
dropped, leaving the runtime's directly owned client/server children.

Use ONE absolute inspection cutoff min(now+20 seconds, stop_by-30 seconds), and
retain each existing IPC exchange's 15-second cap. Twenty seconds accommodates
ordinary scheduling behind an existing actor operation better than two seconds;
it does not guarantee the actor becomes available. Start both requests concurrently,
not sequential 20-second allowances. Skip both if cutoff expired, recording
skipped_no_budget. No retry, new remote Status/Frame, or traffic-deadline renewal.
Missing/dead daemons get a finite unavailable outcome. Finish/abandon local awaits
before existing cleanup; cleanup always runs despite inspect/format/write errors.

At-most-once means a LOCAL REQUEST ATTEMPT per owned daemon, not a guarantee about
synchronous execution or observation time. Request::Stream is an IPC command;
after the caller drops, a queued unstarted Inspect may remain eligible until the
server's exchange deadline minus 100ms (about 14.9 seconds for the existing cap).
The actor skips it only when its response receiver is closed or deadline passed;
the server task can retain that receiver after the client stops waiting. A queued
or started read can therefore execute after caller timeout and delay cleanup List
or AdminCancel. Read-only semantics make late execution harmless to persisted
state, not free of actor scheduling cost. No cancellation guarantee is invented.

Inspect at most once per locally owned daemon at finalization, not once per failed
cell. The Mac server job captures when its OWN work ends, normally when client
result files arrive, not at a remote client's stdin timeout. All daemon snapshots
are nonsimultaneous, and may postdate CLI termination or later progress.

Expected role/account topology from existing private context/identity records
binds returned entries locally.
For each expected client/server role pair, emit match_count bucket {zero,one,
multiple}; only one correctly bound entry may supply progress. No first-row
selection. Private IDs can correlate the same stream across local response data,
but public evidence contains ONLY role enums, direction, closed phase/integrity,
booleans, finite codes, sequences, byte offsets, slot descriptors and bucket
counts. Multiple matches remain ambiguous; truncated results never prove absence.
No accounts, session/stream IDs, paths, nonces, argv, arbitrary errors, hashes of
identifiers, raw stderr or output enter public logs. Include relative elapsed_ms,
not renewed clocks/deadlines. Each endpoint outcome is ok, unavailable, timeout,
invalid, capacity or skipped_no_budget; unknown failures map invalid.

Emit a new bounded schema=1 diagnostic stderr line family, at most two endpoint
summaries per job, each <=24KiB, separately recognizable from existing observation
lines. Endpoint summaries include all three fixed role-pair projections and
snapshot truncation; no existing ClientResult/ServerResult/rollup schema changes.
Actions already retains stderr; sanitized extraction binds exact workflow/run/
attempt/source and records observation timestamps as nonsimultaneous. Never treat
this sidecar as a qualifying rollup input. Sink failure is best effort and cannot
replace work failure, affect cleanup flags or acceptance. Inspection does not
convert timeout to transport success or conceal a failed original attempt.

Partial stderr prefix retention is not required for this first design: private
Inspect bindings plus expected topology suffice for unambiguous association or
an explicit ambiguous result. Do not add another capture buffer or change failed
run_input cleanup/reader semantics merely to obtain an identifier.

## What can and cannot be proved

A client retained Hello without cached response identifies a retained Hello
without a local answer; cached means daemon has a response, not CLI has consumed
it. A retained Open
proves local Open submission after the CLI accepted Hello. Client opening plus
server preparing distinguishes durable admission from local pending projection.
Server running records identity journaling, not current liveness/input consumption.
An uncached retained Exchange and its input/ACK offsets identify a slot without
a locally cached response at snapshot; they do not prove a live CLI waiter.
Answered slots show cached progress only. Record
and cached-response counters can differ and must remain separately labelled.
Outbox prepared/queued/published/native-id-present separates recorded native
attempt states
without asserting relay delivery. Retained/pruned/cleared slots and truncated
records can leave no relevant evidence; none is not never sent. Snapshots taken
after CLI termination may show later progress and do not reconstruct the exact
330-second instant. No snapshot proves CLI read position, output-write blockage,
network cause, physical group cleanup, exhaustive history or guaranteed root cause.

## Ownership, tests and acceptance

Coordinator assigns disjoint implementation ownership after design approval.
Freeze the exact StreamInspection and nested DTO fields/serde encodings with the
IPC author BEFORE harness implementation starts; communicate the agreed interface
and keep its changes centrally coordinated:

1. IPC author: stream_service.rs DTOs, new stream_inspect_store.rs plus journal
   module registration, stream_runtime.rs read dispatch. Preserve existing variants.
   This owner also audits ALL matches on the two extended enums and owns minimal
   compile/finite-unexpected-response fallout in stream_session.rs, stream_cli.rs,
   copy_local_gc.rs and other actual match sites. Preserve wildcard behavior where
   already intentional; coordinate edits before touching another author's file.
2. Harness author: installed_streaming.rs finalization and new adjacent sanitizer/
   tests; coordinate registration only. No changes to process capture or workloads.
3. Coordinator: integrated review/evidence, candidate builds and original matrix
   rerun. Serialize shared Cargo; no overlapping live relay qualification.

Required hermetic tests: same-UID credential rejection through existing IPC path;
closed serde rejects extra fields; old List serialization byte-equivalent; mixed
Hello/Open/Exchange slots with cached/uncached responses; exact client/server
phase asymmetry; conflicting/malformed/oversize slot binding and response refusal;
EXPLAIN QUERY PLAN assertions for rowid traversal and indexed slot-key lookup
(with small per-key ordering allowed); SQL unknown-state aggregation, published
bucket and global header sentinel; record/slot input and worst-case envelope
limits/truncation; no SQL/journal/outbox/slot
mutation before/after Inspect; publication native presence distinct from queued;
secret sentinels in argv/payload/IDs/errors never appear in public projection;
zero/one/multiple association; shared absolute timeout while IPC/actor stalled;
queued Inspect survives caller drop only under the existing server deadline,
with no persisted mutation; both requests attempted at most once; SIGTERM/
installed_interrupted skips both, and no-budget skips promptly; unchanged original
error/acceptance and cleanup even with sink/inspection failure. Source tests must
not claim actual network causes or physical cleanup from booleans.

Run focused native and installed-feature checks, strict Clippy and formatting;
obtain integrated actual Claude auto code review, incorporating useful findings.
Publish reviewed source normally, build a fresh three-platform candidate with
native/installer gates. Any build or candidate gate failure STOPS before matrix
execution; diagnose it, review substantive fixes and build a new candidate. Once
those gates pass, rerun the ORIGINAL stream-matrix AT MOST ONCE per reviewed
increment. Cold, ignored, local or same-host diagnostics cannot substitute for
either the fresh three-platform candidate or that original matrix attempt.

Capture finite observations with exact provenance and preserve any first-attempt
failure. Empty, truncated, unavailable or ambiguous snapshots remain UNKNOWN;
they require bounded diagnosis and, if needed, another reviewed observability
increment, not blind retries or declaring the problem absent. If the matrix fails,
stop qualification and interpret the evidence before proposing further work.
Only actual qualifying artifacts, receipts and cleanup validation can qualify
cells. Later cohorts and release readiness retain their existing gates. No
release/public installer pin changes are authorized by this observability plan.
