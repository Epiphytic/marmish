# Installed qualification failure observability

Status: design and integrated implementation independently approved by actual Claude reviews on 2026-11-08. Local validation passes; fresh hosted qualification remains required. Baseline source: `46c9cb46e382735a8678b17eca5e416f30b0c264`. See [review evidence](../reviews/streaming-failure-observability.md).

## Evidence and smallest useful increment

Run 42501629829 Linux AMD64 failed after 31.637 seconds. Native cause is unknown. Empty fallback can follow pre-cell failure, complete result validation before write, or external termination; it does not prove no execution. Actual own-ready upload finalized 16:48:18Z, before failure 16:48:47.342Z, contradicting an initial upload timeout. Normal supervisor-uploaded terminal client/server artifacts match the fallback records. Source checks remoteError before scanning, then kills the child in finally without further uploads. Thus these normal uploaded fallback records contradict the simple hypothesis that a supervisor polling error initiated this child kill. They fit child failure followed by supervisor scan/upload/driver_failed. This is evidence-and-source inference, not proof of native cause or SIGTERM origin. A second poll delivering an invalid rendezvous record is possible, not established.

Make native finite failure attribution primary. Add only minimal supervisor finite stage/category reporting as general CI diagnostic hardening. Defer transport HTTP-status attachment, redirect/blob stage splitting and any retry semantic change. Add finite peer-role/read-versus-validate precision only to incident-relevant servers() rendezvous; do not instrument every transport/cell operation. No transport.mjs behavior changes in this increment. No product, protocol, installer, release-pin or version edits. Qualification evidence schemas and acceptance predicates remain unchanged.

## Public-log security contract

GitHub repository and Actions logs are PUBLIC. Diagnostics must contain only closed finite vocabulary, never raw error.message, Display/Debug, Invalid payload, DaemonStartup string, stack, URL, HTTP body/header, token, path, argv, payload, npub, or untrusted timestamp. Define schema1 fixed-prefix JSON with component enum driver/supervisor, event failed, closed stage/category and optional closed origin/role/peer_role (role fields are only linux-amd64/linux-arm64/macos-arm64 or null). Unknown variants map to unknown_error; do not reflect arbitrary Invalid strings. Each line has a fixed bound (1024 bytes maximum including prefix/newline). Serialization and sink failure never throw or replace the result. No arbitrary strings enter the serializer. Existing generic terminal CLI messages may remain; exactly one structured diagnostic per owner. Public publication retains finite diagnostic categories and evidence hashes, not raw private runtime files.

Tests must assert whole-line fixed-vocabulary schema, required fields, enum membership, no extra fields, exact size bound, no sentinels, and no emission on success. Mutation test locally changes classifier to echo a sentinel error message/Display, confirms the schema/security test goes RED, then restores the change. Mutation source never committed. This is mandatory leakage validation, not just checking a few absent strings.

## Native interface and placement

Use a small installed_streaming_diagnostic.rs module with path-based tests, following existing streaming inspection module patterns. Reuse existing bounded stderr sink only if its interface fits without enlarging unrelated APIs. Provide pure classify(origin, stage, &Error) -> Diagnostic and nonthrowing write_diagnostic(sink). Category maps by Error variant to fixed tokens, never dynamic payload. Keep a small tracker of closed stage, origin and emitted flag; unknown role may be null before trusted role selection. No broad refactor or nested helper extraction of run semantics.

Wrap existing run body in a thin internal async function accepting mutable tracker. Outer run calls it, then on Err emits if not already emitted; this catches setup and post-work returns. In work path assign stage immediately before the operation or coherent group in the table below. After select record work_error, work_deadline or external_signal explicitly; for work Err emit before fallback/cleanup, then preserve original Result and current cleanup/fallback/return ordering exactly. Outer handler emits post-work failures only if no work failure was already emitted. Cleanup and evidence write errors do not overwrite an already emitted initiating diagnostic. Existing returned Error behavior remains unchanged, including existing cleanup/evidence precedence; diagnostics report the earliest observed work failure separately. No new early returns or skipped cleanup.

Closed origin values: setup_error, work_error, work_deadline, external_signal, post_work_error. External_signal means this process received SIGTERM, never implies which process sent it. Stage discriminates where Error::Invalid(installed_streaming) is otherwise opaque. Pass the same mutable tracker into servers() (lines 110–135). For each existing ROLES member set peer_role to its finite enum before try_exists/read, stage server_record_read for existence/read/JSON parse, then server_record_validate immediately before record.validate. Before the missing-record sleep/timeout set stage server_rendezvous_wait and peer_role null, so timeout does not blame the last scanned peer. After servers returns clear peer_role. Preserve every read, iteration, validation, 100 ms sleep and deadline. clients() stays coarse remote_client_barrier; no internal read/validate precision claimed there.

Every fallible site in run must be accounted for by this source-site table, checked against the actual implementation before review:

| Frozen source site | Closed stage / handling |
| --- | --- |
| 394–441 context read/validate, cases, identities/target, own identity/canonical comparison, private dirs, fixture copy, preparation deadline/configure, time/budget/deadlines, sockets/collision | setup; outer wrapper catches every `?` and explicit Err |
| 451–452 signal handler registration | setup |
| 454–469 daemon starts; 470–471 readiness | daemon_startup; daemon_readiness |
| 472–490 own-ready construction/digest/pins/time/rsync, validate/hash/write | own_ready_publish (construction/validation and publication grouped) |
| 491 servers(), internally 110–135 | server_record_read, server_record_validate, server_rendezvous_wait; finite peer_role as above |
| 492–498 common_budget/time/deadlines/client handle | common_budget; peer_role null |
| 505–524 edge endpoint/deadline and edges::run | edge_execute |
| 525–538 edge result construction/digest/pins/validate | completed_result_validation |
| 539–542 edge write; 543 clients(); 544–556 confirmation | client_result_publish; remote_client_barrier; server_confirmation |
| 560–582 interruption endpoint/deadline and cases::run | interruption_execute |
| 583–596 interruption result construction/pins/validate | completed_result_validation |
| 597–600 interruption write; 601 clients(); 602–609 confirmation | client_result_publish; remote_client_barrier; server_confirmation |
| 613–659 cell deadline/join/outcome collection/cases lookup | local_cells; cell errors still retained in failed_cells unchanged |
| 660–676 normal result construction/pins/validate when complete | completed_result_validation |
| 677–680 normal result write | client_result_publish |
| 681–683 incomplete explicit Err | cells_incomplete (set after publication before Err) |
| 684 clients(); 685–694 collect_receipts incl server handle | remote_client_barrier; server_receipts |
| 697 select | preserve active work stage; origin explicitly work_error/work_deadline/external_signal |
| 699–729 fallback try_exists/digest/pins/write | post_work_fallback, unless initiating work diagnostic already emitted |
| 730–743 optional failure inspection and Runtime::close | no Result propagation currently; do not change cleanup semantics or introduce claims from best-effort inspection |
| 744 fallback propagation | post_work_fallback |
| 745–755 completeness including cases lookup | post_work_completion |
| 756–774 server-result construction/digest/pins/write | server_result_publish |
| 775 final !complete Err | post_work_completion unless already emitted |

Line numbers are frozen-source orientation, not an assertion about shifted implementation lines. Reviewer must reconcile table to actual operations; every `?` and explicit Err must have a stage. Internal called functions remain represented by the caller's finite stage; finer cause attribution is out of scope.

Native test feasibility: no existing run-level fixtures; run pins current_exe, starts real daemons and relays. Do NOT claim deterministic execution of its three failure paths. First add a compiling pure seam stub that intentionally emits no diagnostic, run new assertions and record RED for missing diagnostic (not missing symbol/compile failure), then implement classifier/sink and GREEN. Unit tests exercise every stage/origin and Error variant, finite peer-role/read-versus-validation distinction and reset, synthetic pre-rendezvous / completed-validation / external-signal classifications, once-only behavior, work failure followed by cleanup failure, sink errors, public-schema security and mutation leakage. This verifies pure attribution/output contracts. Actual stage placement and preservation of run behavior are verified by complete site-table source review; end-to-end run coverage remains a declared limitation until a new candidate qualification. Do not build test-only real-daemon injection infrastructure for this diagnostic increment.

## Minimal supervisor interface

Add injectable nonthrowing diagnostic sink to supervise(options); baseline ignores extra option, so a new fake-transport hard-error test fails by empty capture rather than missing export. Keep existing real owned-child cleanup fixtures and test clocks; do not mix epoch-1970 supervisor test context with real ArtifactTransport Date.now checks.

One central main wrapper owns setup failures before supervise starts; supervise owns all failures once entered. Implement catch before finally with once-only guard so the initiating stage/category is emitted before cleanup can replace returned error; outer main emits only if not already owned/emitted. Cover pre-try identity/deadline checks and env/context/token/transport setup under setup. Sink invocation is caught and never changes failure or cleanup. Poll's role/kind and stage must be captured in finite variables accessible to its catch. artifact_retry and aborted auxiliary polls remain silent. Retain first initiating error stage while main loop changes stages. Stages: setup, remote_list, remote_download, inbox_publish, local_record_read, local_record_publish, child_terminal, bounds, cleanup. Category explicit allowlist of existing transport/supervisor error tokens; arbitrary errors map unknown_error. No HTTP status field or new transport error annotation in this revision.

Meaningful RED: fake remote download throws known artifact_http while controlled real owned child is running, pass diagnostic sink option, assert one bounded closed remote_download/artifact_http line plus unchanged failure exit/termination. Baseline capture is empty. GREEN keeps exit/failure/cleanup semantics. Extend existing race test: hard remote error after local PASS still fails and captured stage remains remote_download. Setup errors emit setup once. Cleanup failure cannot overwrite initiating diagnostic. Sink throw stays failure with cleanup intact. Whole-line public schema and mutation secret tests apply. Existing 24 transport/upload-client tests remain GREEN; retry intervals/API budget/deadlines/missing-pending/fatal classes/no fallback unchanged. HTTP-status precision is explicitly deferred; no speculative 404 change to explain this incident.

## Execution and qualification gates

Actual independent Claude auto design approval is required before implementation. Then use disjoint native and JS agents, local stub-RED/GREEN and relevant existing native/JS suites, exact diff/quality review, actual independent Claude auto code review with every run-site table entry reconciled, sanitized evidence/spec updates, explicit reviewed increment commit and push to GitHub and Radicle with verified refs/availability. No phase closes with unresolved blocking findings. Preserve original failed run and corrected analysis. New diagnostic source requires its own candidate and original serial eight cohorts; source46c9 results cannot silently qualify changed source. Only new finite evidence can justify a targeted semantic fix; no relaxed retries, schemas, workload or qualification gates to force GREEN. No live qualification before approved implementation and code review.


## Approval clarifications

The approved v2 snapshot has SHA256 `28c51afb8fb674231a0709521d307afc2905c759b277b19c67dc8b5dfd433371`. This repository copy adds approval metadata and these binding clarifications without changing scope.

- The existing completion race injects an error through `list`; its diagnostic must retain `remote_list`. A separate fake download test covers `remote_download`.
- Existing owned-child fixtures use an EventEmitter wrapped by the real `ownedChild`; they are not operating-system processes.
- The select arms return origin together with the existing result, distinguishing an outer work deadline from an inner timeout while preserving returned errors.
- Register the new native module in `installed_check.rs` and run its tests with `installed-checks`. Native error classification can exhaustively match variants; JavaScript unknown errors map to `unknown_error`.
- The fixed public diagnostic prefix is `marmish installed diagnostic: `; it must never be a GitHub workflow command.

Stage placement and all existing failure/cleanup paths must be reconciled against the integrated code at independent review. No run-level native integration coverage is inferred from pure classifier tests.
