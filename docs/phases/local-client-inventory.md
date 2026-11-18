# Local server client inventory

This increment implements the local policy and access-history portion of
[the interactive and peer roadmap](interactive-peers-roadmap.md). It uses existing
Command rooms and same-user IPC. Local inventory has no dependency on Peer
invitation admission or the later network heartbeat. Obtain Claude design
approval before coding and integrated Claude review after validation.

## CLI and ownership

- `marmishd clients list [--after <npub>] [--limit <1–64>]` returns an explicit
  page of configured client npubs and their recorded access state. Default
  to 32 rows. Order the canonical 32-byte keys lexicographically and use an
  exclusive cursor. Return a next cursor only when another page exists.
- `marmishd clients show <npub> [--policy-page <n> --policy-generation <id>]`
  returns that client's local timestamps and one page of effective rules.
  Default to page zero and require a generation for every continuation.
  Include page number, generation and optional next page. Bind the
  generation to the normalized scoped policy and principal with a versioned
  domain and digest. Reject a stale generation when that policy changes.
- Use existing state directory and named identity options to address the
  running daemon. Handle `Clients` in `cli::run` before `daemon_operation`
  and leave `cli::is_daemon` false for this command. Add a closed
  `Request::ClientInventory` variant and exclude it from the negative
  match in `Request::is_command`. Define `uses_short_exchange` as the
  existing `is_command` cases plus `ClientInventory`. Use it for the IPC
  server's 15-second absolute deadline and the owner's expired/closed
  queue check. Dispatch inventory before `command_acceptance` and the
  Command classifier. Preserve the existing Command and Stream timeouts.
- Use a dedicated `ipc::client_inventory_request` with one 15-second
  budget for connect, peer verification, send and typed response. Carry
  a closed, versioned `ClientInventoryReply` in `Response.result`. Accept a
  success only with `ok`, that result, and all other carriers absent.
  Decode a closed set of explicit `Response.error` categories. Map `busy`
  to a retryable local busy error and `operation_uncertain` to the local
  deadline error because queries have no side effects. Preserve two-way
  real peer credentials, socket mode `0600` and the profile lease. The CLI
  needs no provider initialization, relay request or second owner. A
  stopped daemon reports the local socket error. Pre-serialize or size the
  complete response envelope against 65,536 bytes before handing it to
  the socket writer. Return a typed `inventory_response_too_large` error
  if it cannot fit, including when a single row cannot fit. Keep existing
  readiness fields free of inventory data.
- Use the live command service's `CommandConfig::execution_clients` as
  the sole key set, including clients with no activity. The underlying
  `BTreeSet<Account>` is already canonicalized and capped at 4,096.
  Require a `Server` or `Both` profile. Reject an unconfigured key with
  a static category and emit canonical lowercase npub for display. Client
  trust, aliases, historical command principals, known rooms and received
  metadata confer no inventory membership. List reads at most 65 config
  keys and their corresponding history, using journal primary-key lookups.
  A server with no configured clients returns an empty list and
  `inventory_client_unknown` for every show, even if historical principals
  keep the engine running. A client-only or unconfigured profile returns
  `inventory_server_unavailable`.

## Effective policy

Add `CommandPolicy::describe_for_principal` and a versioned read-only
`CommandPolicyDescriptionV1` in `marmish-engine`. Share the existing
`policy.rs` effective layer iterator with `check_command`. Derive the
server description from the validated engine command policy.
In `command_runtime::enable_commands`, clone only the normalized
`CommandPolicy` from the result of `CommandConfig::engine_policy` into
`CommandService` before moving the full policy into the engine owner.
This keeps the synthesized `/**` default used by admission when the raw
`allow_exec` is absent. No separate parser or validation pass is needed.
If the builder returns `None`, there can be no configured client to show.
Missing policy for a configured key is an internal typed error. Keep layer order, explicit empty
allows and deny rules. Any match inside a nonempty allow layer suffices
for that layer, and every present allow layer must match. Denies are
additive. Show that allow patterns match the canonical executable, deny
patterns also match the invoked path, `*` stops at `/`, and `**` can cross
`/`. Describe PATH resolution and the existence of the match budget
without publishing PATH or evaluating a supplied command. The description
is a policy snapshot. Actual admission also depends on paths, listener
state and resource limits. Report shell commands disabled and interactive
shell support unavailable under the current admission contract.

Page only rule data for the selected principal. Cap the complete lossless
encoding at 64 pages of 8 KiB including page metadata, and split at
whole-rule boundaries with layer identifiers. Explicit empty allows must
be representable. Return `description_too_large` if the complete policy
cannot fit and keep command execution unchanged. Bound construction by
existing `MAX_PATTERNS`, `MAX_PATTERN_BYTES` and `MAX_KEY_ENTRIES`. Emit
only this client's npub, access fields, and effective rules. Exclude other
keys' policies, full config, cwd, PATH, arguments, output, environment,
Nostr secrets and group, invocation or native source identifiers from
responses, diagnostics and ordinary readiness. Print policy text with
JSON escaping or equivalent safe rendering.

## Qualified access and migration

Keep one SQLCipher inventory row per configured canonical account, capped
at 4,096. Assign a grant generation when a row is first created and
preserve it on unchanged restarts. Add a nullable generation marker to
existing invocation rows, without allocating an independent event history.
Stamp it only when the qualifying submit is first recorded. Completion
must match the current account generation, so old job recovery cannot
write access history after removal and readdition.
Expose `last_authenticated_submit_at` and
`last_engine_admitted_submit_at` as UTC Unix receipt times or null, with a
typed state for invalidated or never-recorded submit evidence. No inferred
historical timestamps. The second field is the receipt time of the newest
submit known durably admitted to the engine. It does not promise that a
process started or exited successfully. Retain these values while the key
is configured. Interactive, streaming or future Peer activity can each
add a typed evidence source in its own increment.

In `command_service::project_request`, reuse authenticated server-direction
and `can_accept` checks. Inside the `command_store::command_project_request`
transaction update receipt only for a fresh `RequestOp::Submit` with a
new invocation row and valid request/source binding, after source binding
succeeds and before commit. Count `AdmissionDecision::New` and the only
current projection-time durable refusal for a new Submit,
`QuotaExceeded`. Keep this an explicit decision allowlist. `InvocationConflict`,
`NotFound`, `Existing`, duplicates, alternate sources and non-Submit
operations leave history unchanged. The same applies when projection
returns before commit, including stale or future input, closed admission,
capacity or storage failure and transient `Busy`, `FutureDeferred`,
`ClockBehindFloor`, `StorageFailure` or `TransportDegraded` refusals.
Authentication and source checks still apply before quota counts.
The later engine refusals `CommandDenied`, `CommandNotFound`,
`CommandNotExecutable`, `CommandResolutionFailed`, `CommandPolicyError`,
`InvalidRequest`, `QuotaExceeded` and `Unauthorized` neither erase the
already-qualified access nor create admission evidence. Invalid input
refused before authentication or projection never advances history.
Persist the local receipt and its private exact `(group_id, native_id)`.
Update admission in `command_commit_completion` in the transaction where
`record.engine_known` first becomes true, reusing `command_invocations.created`
so restart recovery or the timing of a poll cannot make an old access look
newer. Require server direction and a current grant. A pre-upgrade
invocation without the new receipt evidence cannot seed either field.
Use the server invocation's existing SQLite AUTOINCREMENT cursor to
order the newest submits, with receipt time kept as a separate field.
Compare and update each history value independently, so a later engine
completion for an earlier submission cannot replace newer evidence.
Retain a per-grant high-water receipt time and expose a typed
`wall_clock_regressed` flag when the latest recorded receipt predates
that high water or the current wall clock is behind it. Preserve the
observed receipt time and use the cursor for recency, even while flagged.
Existing wire freshness checks still reject time outside the replay
window. Prove cursor nonreuse through garbage collection and reopening.

Inside `record_invalidation`, retract inventory receipt evidence only
by exact `(group_id, native_id)` in the same transaction as native
invalidation. Match the stored tuple directly, independently of source
GC. Existing command invalidation of an alternate source may revoke the
invocation through the request-ID join. Preserve that command behavior.
It neither retracts inventory receipt for the original source nor
restores a retracted receipt. Run the same exact check on the early
invalidation path before writing or consuming a matching
`command_early_invalidations` tombstone. The existing table covers early
outgoing publications. Preserve its scope and limit, and additionally
suppress inventory updates if an exact tombstone is still present.
Reuse the inbound preprojection check in
`command_adapter::decode_command_event` for a pending
`AppMessageInvalidated` with the same group and native ID. An invalidated
pending original never creates history. Capture these orderings in the
native fixture.
Inside `command_store::revoke_group_in`, retract current evidence by
group in the room-revocation transaction. A room revocation never deletes
an account row or changes its grant generation. Other rooms for that
account remain distinct. A retracted latest receipt shows `invalidated`
without guessing an older time. Preserve the cursor fence and any
independently recorded durable admission.

Under the exclusive profile lease at every `daemon::open`, reconcile the
inventory table in one transaction against the actual
`execution_clients` set. Treat a missing config or role without server
access as an empty set. Delete rows absent from the effective set and
create a fresh generation for additions. Complete reconciliation before
command engine recovery or IPC dispatch. Config edits made while the
daemon runs take effect after restart. If a key is removed and readded
while stopped and no daemon open observes its absence, its previous
history and generation survive. After observed absence, the key appears
with no history on readdition. Test this across three launches with a
held engine completion. Old job recovery must preserve command
semantics and cannot reseed access under the new grant generation.

The current native journal is format 7, with supported source formats
0, 3, 4, 5, 6 and 7. Use format 8. The candidate inspector reads
`MARMISH_CANDIDATE_JOURNAL: journal-v8` from the authenticated source workflow
and requires the exact tuple in each artifact manifest. Historical workflows
without the marker retain their original rules. Today
`copy_store::copy_init` stamps the current version inside its own
transaction and `Journal::open` stamps it again after initialization.
Remove both stamps in favor of one final schema transaction owned by
`Journal::open`. Check identity and run the existing prerequisite
initializers first, including `command_init`, `stream_schema_init`,
`copy_local_init`, `command_rooms_init` and
`command_preparations_init`. Split out the existing copy schema/data
step to accept this transaction. Run it first within the transaction,
then add the inventory table, provenance index and nullable
`command_invocations` marker, then stamp the new version once and commit.
Keep the older-format copy rewrite in this same transaction, so a
failed migration cannot leave rewritten payloads under an old stamp.
Only then run `stream_init` recovery, which uses the migrated copy
schema. A recovery failure after commit leaves a fully migrated
schema. A migration failure rolls back the new schema, copy rewrite
and stamp, produces a typed startup error and leaves the journal
available for retry. The current-format path must check the expected
inventory schema and fail closed on missing components.

Update the format support asserted by `native_preflight.rs`, the
installed manifest in `native_manifest.rs`, version reporting in
`native_install.rs` and the expectations in
`installed_version_lifecycle_tests.rs`. Earlier binaries reject the
new format. Update the format-6 copy component fixture to exercise
its transactional DDL and data rewrite separately from the version
stamp. Document taking a coherent stopped-profile backup before upgrade.

## Validation and exit

- Add `policy::inventory_core_tests` to `marmish-engine` for parity with
  `check_command`, including multiple entries for one key, no allow versus
  empty allow, deny aliases, `*`/`**` and exhausted match budget. Add
  `client_inventory::core_tests` to `marmish-native` for normalized
  pagination, the synthesized default, no configured principals, typed
  policy overflow and empty history. Use `journal::client_inventory_store::core_tests`
  for encrypted schema-7 upgrade, wrong key, empty history,
  a fresh submit, denied submit, admission and a duplicate. Add these
  filters to the existing checks in `.github/workflows/interactive-core.yml`
  on the `epiphytic-ci` self-hosted Linux X64 `bel-ci` job.
  Run `cargo test --locked -p marmish-engine --lib policy::inventory_core_tests`
  and the two native filters with `cargo test --locked -p marmish-native --lib`.
  Add `crates/marmish-engine/**` to its trigger paths. Use no relay, native
  package matrix or benchmark in this job.
- Run full native/engine tests locally, then the disposable SQLCipher and
  real Unix-socket fixture on Linux amd64, Linux arm64 and macOS arm64.
  Prove real daemon CLI dispatch, named-profile isolation, wrong-UID
  refusal through the credential predicate and a second UID on a
  qualified host, oversize and cross-generation pagination, timeout and
  transport-degraded reads. Inject failure between the copy migration
  step and inventory DDL and reopen. Verify the old stamp, old copy rows
  and absence of half-migrated inventory, then retry successfully.
  Test format-6 and format-7 inputs plus v8 rejection by the prior
  reader. Exercise each named refusal and non-Submit boundary, exact
  original versus alternate source invalidation, early and pending
  invalidation, room revocation, post-restart replay, GC and rollback of
  wall time. Test an unfinished invocation across the three-launch
  removal and readdition sequence and an unobserved config edit.
  Use native signed Command delivery to demonstrate an authorized command
  and an authenticated denied command, with stable receipt values after
  replay and restart. Verify a different key and a wrong room cannot
  affect the view. Seed private arguments and identifiers and verify they
  never appear in inventory output, logs or readiness.
- Provide an ignored local `client_inventory_benchmark` fixture. Measure
  cold and warm list and show latency through the socket at 0, 64 and
  4,096 configured keys and with near-limit policy pages. Measure p50/p95/p99
  actor round trip, query throughput and SQLCipher submit projection cost
  against a control without inventory writes. Measure command latency
  concurrently with repeated inventory reads to catch actor starvation.
  Publish sanitized sample counts, versions, targets and results under
  `docs/validation/`. Keep the benchmark and long native fixtures out of
  routine CI.

Exit requires passing the two CI filters, native socket and journal
validation, signed Command evidence, published benchmark results and
Claude approval of the integrated diff.
