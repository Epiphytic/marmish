# Observe the original installed stream-matrix failure

Approved design and implemented build-only observation for Beads `marmish-8ka.5.8`;
see the [actual design/code review](../reviews/streaming-matrix-failure-observation.md).
Original runs `42363063498` and
`42371657352` passed all six Linux-server cells and failed all three Mac-server
cells with validation_or_transport, not timeout. The latter is recorded in
`docs/validation/streaming-installed-e9686c0-failure.json`. Cold multiprincipal
same-host diagnostic `42374246025` passed; no product cause is established.
`installed_streaming::failure_category` maps only Timeout and Io separately;
`streaming_cases::duplex` merges prefix, entry/binding and Pair failures. Observe
this exact matrix path without another workload or replacement capture runner.

## Exact scope and narrow interfaces

Own build-only `installed_process.rs`, `installed_streaming_cases.rs`,
`installed_streaming_records.rs`, their adjacent tests, and one small
`installed_streaming_observation.rs` formatter registered in `installed_check.rs`.
No product/common execution, dependencies, CLI/environment/workflow knobs,
Context/Pins, report schemas, acceptance criteria or budget changes.

Keep `run_input` and all callers unchanged. Its existing body becomes a private
inner function with `Option<&Trace>`; a sibling observed entry is used only by
duplex. `Trace<'a>` holds closed Role enums, monotonic start, a Cell event counter
and a borrowed fallible sink `&dyn Fn(&[u8]) -> io::Result<()>`. Production sink
locks stderr and calls write_all on one preassembled line, ignores its Result,
and never uses eprintln!, error Display/Debug or unwrap. Tests pass a private
recording/failing sink; no global logging state or arbitrary formatting callbacks.
The fixed Event enum carries only allowlisted variants and bounded scalar data.

Prefix parsing/diagnostic classification, entry selection/binding, Pair assembly
and Pair validation are small pure steps in streaming_cases/records. Each takes
already captured Output/Entry slices and optional Trace, returning the original
Result. The async duplex wrapper retains its exact original awaits, command and
ownership. Pure steps let tests reach each assertion without a daemon/socket.
List acquisition remains async; a tiny classify-list-result wrapper over the
existing Result lets tests exercise its error marker without fabricating IPC.

## Stage, failure and evidence contract

Driver STDERR uses `marmish installed stream observation: ` plus compact JSON:
schema=1, case=duplex, client/server Role, elapsed_ms, fixed stage/outcome and an
optional fixed-shape evidence object. elapsed uses monotonic duration with
saturating u128-to-u64 conversion. At most 48 lines per cell, each <=1536 bytes.
This covers the full begin/result path (including Pair checks) without truncating
its decisive late stages. Allow at most 46 ordinary events; reserve slots47/48 for
one truncated marker and
one final complete/error event. On first excess ordinary event emit truncated
once; a later terminal event still emits. No repeated retry/poll events. Outer
traffic timeout may drop the cell: an absent terminal plus last begin event is
explicit incomplete evidence, never success; do not introduce Drop cleanup/logging.

Emit begin BEFORE each existing await: run_input, input write/shutdown, child
wait, stdout/stderr joins, List lock and List request. This distinguishes waiting
for ep.daemon.lock() from the IPC request. Emit synchronous prefix, binding,
Pair-construction and Pair-validation begin/result too. Record entry_absent
separately from List failure. No added await or timeout around the existing lock.
Inside run_input also record limit preflight, spawn, overflow and existing
terminate begin/result; preserve cleanup ordering/error precedence exactly.

Retain write_all then shutdown inside the SAME existing timeout future. Update a
local phase enum before shutdown, allowing write/shutdown error or timeout to be
identified without splitting the future or renewing its deadline. All still
return installed_child_input. Wait and drain expiry still return Error::Timeout,
with distinct observation stages. A pure classifier distinguishes stdout/stderr
join failure, drain io failure and overflow at existing return branches; no
synthetic read/join injection is claimed through real child pipes.

Closed io-kind tokens: broken_pipe, connection_reset, permission_denied, not_found,
interrupted, would_block, timed_out, unexpected_eof, invalid_data, invalid_input,
other. Unknown kinds become other. Never include error text/errno, raw output,
argv, paths, environment, account/session/stream/nonce values or their hashes.
Only already available stdout/stderr lengths and SHA256 may be logged at complete
capture; after prefix success also payload-stderr length/hash. Failed capture
counts/hashes discarded by the existing helper stay null. A failed write_all has
unknown written count; successful write_all permits local_pipe_written=input
length, expressly not remote accepted/delivered. Do not recover discarded bytes.

Prefix failure token is exactly missing_prefix, short, newline, utf8 or hex,
selected with the current parser's order; returned bad() remains unchanged.
Scan retained stderr for EXACT complete diagnostic lines produced by
stream_cli: `marmish stream: remote error TOKEN` or `remote failure TOKEN`, where
TOKEN is one of Unsupported, Invalid, Denied, Conflict, Gap, Expired, Capacity,
Uncertain, NotFound, Closed. Map those to the closed WireErrorCode enum. Emit at most one
closed code (a closed ambiguous enum variant if multiple differ); scan retained
stderr whether prefix parsing succeeds or fails, and never echo unmatched text. No new
parsing acceptance, framing fallback or output modification follows that token.

Binding evidence includes direction_ok/client_ok/server_ok, active and the full
closed terminal projection: none, exited(code:u8), signaled(signal:u8), cancelled,
failed(WireErrorCode). This separates terminal absence/recording races from exit
mismatch. It confers no authority and does not change existing binding checks.

## Shared Pair checks, preserving validation semantics

In installed_streaming_records extract DuplexChecks with nullable booleans for
roles, transfer_absent, case, complete, failure_absent, nonce, each of the four
hex fields, each stdin/stdout/stderr count, exit7, wall_ns and each content hash.
A private checked outcome contains these checks plus the ORIGINAL Result<()>;
only checks serialize, never Error. validate_duplex returns that same result;
the observed caller emits checks before returning it. Evaluate predicates in
original short-circuit order: later checks remain null after failure. Preserve
errors from nonce()/bytes(), and do not hash/generate untrusted-sized buffers
after counts fail. This is a single validator, not a duplicated expected oracle.
Pair-construction evidence separately records open_digest_present,
exit_representable, wall_ns_representable; missing digest/failed conversion still
returns original bad(). Emit no raw values used to construct the Pair.

## Tests and actual qualification

Pure event tests pin closed fields/enums, saturation, line/event bounds, reserved
terminal/truncated slots, io fallback and absence of sentinel secrets. A failing
per-context sink must preserve success and failure. Pure duplex tests cover each
prefix branch, List error/entry absent, binding/terminal variants, construction
conversion failures and each Pair check. Pin staged nulls/short-circuit error
precedence and excessive count refusal before byte generation; serialized Pair
and existing validator results must remain identical. Test read/join classification
with real io errors and a failed JoinHandle classifier, not invented pipe coverage.

Hermetic observed/unobserved run_input parity tests compare exact Output bytes
and status or exact Error variant/static category for exit7, early input close,
wait expiry, drain expiry and overflow. Assert the corresponding last stage and
existing cleanup behavior. Test interleaved role contexts and an outer-cancelled
pending List lock: begin exists, no invented result/terminal event. No global sinks.

Run focused/all installed_check tests, strict feature Clippy and formatting;
obtain actual integrated Claude auto review and publish reviewed source normally.
Rebuild the existing three-platform candidate with native/installer gates and
rerun original stream-matrix before later cohorts. Actions already inherits
STDERR; retain sanitized event extraction with actual run/source provenance.
Original eight-cohort artifacts/cleanup/acceptance remain binding. Log volume is
bounded in SIZE, not time: synchronous stderr can block on a stalled log consumer.

## Observed outcome

Candidate 42377010307 at d5addee passed all three complete native/package/tool
jobs and final rendering. The original installed stream matrix 42383713647
failed again: every client targeting macOS timed out during the local stdin
write at 330 seconds. Each emitted an explicit error after successful owned
child termination. All six Linux-target observations reached successful ordered
Pair validation; the failed rollup qualifies zero cells and the remaining seven
cohorts were not dispatched. See the [candidate evidence](../validation/streaming-candidate-d5addee.md)
and [original failure observations](../validation/streaming-installed-d5addee-failure.md).

This establishes the failing harness stage, not the remote cause. The write
may have partially completed, and the original failure path discards stdout
and stderr drain results. The CLI can await setup, Hello, Open or subsequent
Exchanges before consuming more stdin. No remote output count, remote error,
absent execution or absent receipt is inferred. Additional read-only progress
inspection needs its own approved design; the current acceptance is unchanged.
