# Installed interoperability: bounded execution and reconciliation

Detailed design for Beads `marmish-9sa` (B) and `marmish-0po` (C), extending
[the reviewed overall design](installed-nine-pair-interop.md) and consuming
[phase A](installed-nine-pair-a.md). Independent Claude approval is recorded in
[the final design review](../reviews/installed-nine-pair-bc-design-final.json).
This implementation contract is not evidence of passing native tests.

## Scope and existing contracts

Exercise the actual installed `marmish` and `marmishd` on all nine ordered pairs
of `linux-x64`, `linux-arm64`, `macos-arm64`. Three concurrent native jobs each
own one server and one separate client identity/profile. No application runtime
changes, archive patching, new production flags or release publication are
required. Default keychain onboarding and uv installation remain separate work.

The following inspected contracts constrain the implementation:

* `tools/marmish_app.py:780` accepts `--invocation HEX16`, explicit `--relay`,
  file key storage, `--timeout 300`, raw argv after `--`, stdin and terminal
  stdout/stderr. `--status HEX16` establishes a session without executing a
  command. Remote `not_found` becomes exit 125 and `marmish: not_found\n`.
* `Client.send_input` sends 4096-byte stdin frames before `Client.wait` starts
  polling. `wait` checks gap-free cursors and waits for terminal state plus both
  closed, completely retrieved streams. There is no remote output receipt ACK.
* `marmishd` requires an allowlist, cwd and optional additive command policy;
  startup emits `{"ready":true,"account":...}`. It has no command-count limit.
  SIGTERM closes its frontend/bridge, while detached commands can survive.
* `MdkRpc.history` returns at most the latest 100 rows, oldest first. Each row
  validates `group_id`, `message_id`, `from`, `direction`, `kind`, `tags`,
  `plaintext`, `recorded_at`, `received_at`. It provides no pagination wrapper.
* `validate_frame` binds protocol, session, group, client, server, request ID,
  type, operation and challenge. A `poll` request names the invocation and both
  cursors. Its response uses the same request ID and contains status and both
  output chunks; the response does not repeat the invocation.
* `Profile` saves non-authoritative room discovery metadata at
  `server/profile.json`, `server_groups[group]={peer,session}`. Discover there,
  then independently verify room purpose/membership and actual MDK rows. Do not
  instantiate source `Profile` or `Client` to perform any acceptance operation.
* `running_rpc` in `marmish_interop.py` demonstrates the current same-user,
  owned-socket/native-peer checks. `remove_engine_state` requires workers fenced
  and SQLCipher closed before deletion. Preserve that contract.

## Boundaries and file ownership

Keep the existing two-host harness unchanged unless extracting a demonstrably
shared helper is necessary. The new implementation is CI-only.

| Owner | Files | Interface |
| --- | --- | --- |
| A installation/rendezvous agent | Phase-A modules and tests | Pinned provenance, owned state, sanitized child environment, three identity-ready records and server-ready publication/reading |
| B execution agent | `tools/marmish_installed_interop.py`, corresponding test file | `serve`, `client`, `cleanup`; installed subprocess execution, policy, observer, reports |
| B fixture agent | `crates/marmish-interop-fixture/`, workspace member and lock changes if necessary | Fixed-role native fixture plus native byte/duplicate tests; no application integration |
| C reconciliation agent | `tools/marmish_nine_pair.py`, corresponding test file | Strict terminal-report validation and nine-pair reconciliation, Markdown/JSON statistics |
| Integrator | New installed workflow, docs/spec/review/validation | Wire A/B/C, offline validation, native execution, reviewed publication |

The small Rust fixture uses existing locked `sha2`, `serde_json` and `libc`
dependencies. A separate unpublished CI-only workspace member avoids adding
cryptographic dependencies or special fixture modes to shipped application
binaries. Set `publish = false` explicitly and keep it out of release packaging.
Use `option_env!` for every compile-time constant: ordinary workspace test/clippy
builds without configuration must compile successfully, and running that binary
must exit 125 with no output or receipt before any filesystem operation. Partial
or malformed configuration also fails closed. Cargo builds each server's three
configured variants with compile-time environment constants; tracked compile-time
environment dependencies ensure changed constants rebuild.
Copy each completed artifact to its advertised role path before building the
next. Record harness SHA, fixture source SHA256, compiler and final binary digest.
Prebuild locked dependencies during preparation, then use `--locked --offline`
after the barrier. Fixture rebuilds and policy checks count against the
five-minute local readiness budget; record their durations rather than extending
that budget silently.

Use phase-A common context `{run,attempt,sha,pin_sha256}`, the exact role set,
identity-ready digest map and provenance fields. Do not invent alternate fields
for their existing meanings. Suggested execution interfaces are:

```text
python tools/marmish_installed_interop.py serve --state POINTER --peers-json PEERS
python tools/marmish_installed_interop.py client --state POINTER --servers-json SERVERS
python tools/marmish_installed_interop.py cleanup --state POINTER
python tools/marmish_nine_pair.py --records DIRECTORY --json RESULT --markdown SUMMARY
```

`serve` remains alive in the background; private status files expose bounded
server-ready/server-result records for the token-bearing adapter to publish.
The adapter writes validated terminal client records into an atomic private
inbox; the supervisor consumes them without possessing Actions credentials.
`client` writes its terminal report before waiting for its own server to stop.
Never pass API credentials to the installer, supervisor, fixture or application.
Use A's scrubbed environment and absolute installed launchers/bundled defaults.

## Fixed invocation and fixture

After the all-installed barrier, derive pair context deterministically from
canonical JSON containing the run/attempt/pin, both identity-ready record
digests, both platform roles and their public identities/nonces. SHA256 of a
domain-separated context supplies the public pair challenge; a separately
domain-separated digest truncated to 16 bytes supplies its command invocation.
Use a third domain for the cold probe's guaranteed-distinct unused invocation.
These are correlation labels, not authorization credentials.

Each server builds three fixed-role executables at canonical owned paths
`/tmp/mci-TOKEN/f/CLIENT_ROLE/main` (`/private/tmp/...` on macOS). Compile in the
client role, pair context digest/challenge/invocation and an absolute receipt
path under the server's private root. Only these paths may be written. No shell,
program spawning, runtime role selector or arbitrary pathname argument exists.
The compile-time role does not itself assert a principal: policy below supplies
that binding.

The sole accepted command argv is the executable followed by the expected pair
challenge/invocation and a fixed edge-case vector. Include empty, spaces,
single/double quotes, `$HOME`, `$(id)`, backticks, newline, backslash, `**/*`,
Unicode and a non-UTF8 byte `0xff`. Construct byte argv arrays with subprocess;
never interpolate a shell command. Unix argv cannot contain NUL; test NUL via
stdin instead. Native tests prove Python filesystem encoding round-trips this
vector through the actual installed launcher on each host; the harness
interpreter's encoding alone is insufficient. A platform encoding failure fails
that pair explicitly.

Use 8192 deterministic stdin bytes cycling through all 256 byte values, with a
pair-specific prefix included inside that size. Expected stdout is a versioned
magic plus u32-big-endian length-prefixed argv bytes, followed by a fixed
65536-byte deterministic pair-specific binary block. Expected stderr is the
reversed 8192-byte stdin. The fixture returns exit 7 deliberately, so success is
exact bytes plus expected nonzero status, not subprocess `check=True`. Hash and
length formulas are shared as documented test vectors, not by importing the
fixture implementation into expected-output tests.

At entry the fixture opens its fixed receipt using `create_new(true)`/O_EXCL,
0600, before producing output. Existing receipt means fail 125 with no success
output. Bound total argv to 16 KiB and stdin to 8192+1, reject overlong/short or
incorrect bytes/arguments. Alarm after 120 seconds, including blocked stdin or
output. Receipt is bounded (<8 KiB), contains version, fixed role/context,
challenge/invocation, length-prefixed argv SHA256, stdin SHA256, expected output
lengths/digests and outcome 7. Write a completion record only after all output
writes/flushes succeed, fsync and close. Missing/truncated/incomplete receipt is
failure. Supervisor uses no-follow regular-file/owner/mode/size checks and strict
schema; it never upgrades an incomplete receipt to success.

Server allowlist contains exactly the three expected client keys. Default
`allow_exec` lists all three exact fixture paths; each key's `allow_exec` lists
only its role path; shell is disabled. These layers intersect. Never set a
default deny-all and expect per-key exceptions. The installed bridge has no
policy-only operation. Before starting `marmishd`, start the actual installed
`marmish-engine-bridge` with the identical generated allowlist/command restrictions,
but a separate private scratch state root and cwd. For each of the three allowed
principals, request a token, then submit each of the other two role paths with
fresh invocation IDs and require `command_denied` (six denied submissions).
Require an unlisted test principal's token request to fail authorization. These
are direct same-user trusted bridge calls, not Marmot commands or claims of
remote authentication. Require no worker/fixture receipt, then call
`cleanup_ready` and close/remove this scratch engine state under its guard. Any
unexpected accepted submission, worker, receipt, uncertain outcome or cleanup
failure aborts readiness; do not run successful live commands afterward. Offline
tests cover this concrete operation sequence and denied-only side effects.

## Authenticated local observer

Open the running daemon socket using existing ownership/native-peer checks;
never read encrypted SQLite or expose RPC to fixtures. Read only bounded owned
profile discovery metadata; fail on any `server_quarantine` entry,
`server_quarantine_full`, or pending binding for an unexpected principal, so a
room quarantined between samples cannot disappear silently. Authenticate each
discovered group via `show_group`,
`group_session`, exact two-member `members` and the expected peer. Reject
unexpected accepted groups/principals rather than silently excluding them.

Poll each group at least once per second while serving; serialize RPC operations
with bounded deadlines and reserve execution time for the daemon itself. Start
observation before advertising server readiness. For newly discovered groups,
process every current row, including the first window (do not use `History`'s
initial-baseline discard). Remember message IDs and canonical row digests;
duplicate IDs with different immutable contents fail. Digest the row's group,
message ID, sender, direction, kind, tags and plaintext; exclude mutable
`recorded_at`/`received_at` timestamps. On a full 100-row window require
overlap with the previous window; a first full window or lost overlap fails
`history_gap`. An empty/new short window can be processed safely because the
profile is fresh and the observer predates clients. Bound seen rows/groups;
overflow is failure, not silent eviction of evidence needed for acceptance.
Verify purpose/membership at discovery and every 15 seconds rather than every
history tick. After all terminal client reports arrive, take one final history
sample before stopping; a final under-100-row history is complete primary
evidence for a fresh room. A full final window still needs retained overlap
coverage, and any earlier gap remains a failure.

For every relevant incoming row require `direction=received`, actual
`from=expected client`, kind 30100, correct Marmish tag and group. Validate the
frame against verified session/group/client/server and exact operation schema.
For outgoing rows require `direction=sent`, actual `from=server`, correct
kind/tag/group and matching response frame. Correlate by
`(group,client,request_id)` and require matching operation/challenge. Response
may appear before request in a fetched batch; buffer within fixed bounds.
Do not treat plaintext `client`, argv labels or fixture receipt alone as proof.

Allowed protocol operations are `hello`, `token`, `submit`, `status`, `cancel`,
`begin_input`, `write_stdin`, `stdin_eof`, `poll`. Expected traffic includes
unanswered early hello (server enrollment baselines existing rows), identical
8-second retransmits, hello renewal and `stale_challenge` replies. Unanswered
non-admitting hello/token requests and their benign errors do not fail by
themselves; the probe, eventual successful submit, acknowledged stdin and terminal
poll must still meet all strict checks. Cancellation is permitted only during
failure cleanup and cannot produce a successful pair. Unknown operations,
conflicting duplicate requests or unrelated invocation traffic fail.

Maintain these pair checks:

1. Cold probe: authenticated hello request/response and `status` request for the
   probe ID, with matched response `{ok:false,error:"not_found"}`. Require no
   submit for this ID and no receipt. Protocol challenges stay private.
2. Possible command admission is observed from the incoming request, before
   checking replies. Any authenticated, schema-valid `submit` naming another
   invocation fails the run even if its response is missing or denied: dispatch
   may have executed before reply publication failed. The expected incoming
   `submit` names the derived invocation and exact
   mode/argv, its reply is successful accepted admission (or a byte-identical
   idempotent retransmission). An expected submit with no matching successful
   reply also fails. Record actual principal and room binding.
3. Input: correlate acknowledged writes by invocation/generation/offset, decode
   canonical base64, reject conflicting overlaps and verify the stdin hash and
   EOF offset. Repeated identical requests do not increase execution count.
4. Output: correlate each `poll` response back to its request's invocation and
   cursors. Decode both byte chunks, require `next_cursor=cursor+len`, no gaps,
   consistent overlapping data and monotonic produced lengths. Reconstruct the
   bounded byte streams and match fixture/client lengths and digests.
5. Completion: require matched successful poll with status `state=exited`,
   `outcome={kind:"code",value:7}`, both streams closed, cursors through all
   produced bytes and no gaps. Require the complete native receipt to agree.

Count unique `(principal,group,invocation)` completions only. Exactly one command
invocation per expected principal, no other possible incoming submission and no extra
fixture receipt are allowed. Retransmitted frames never become extra commands;
conflicting invocation/request reuse fails. Public evidence retains validated
bindings, row IDs/digests, counts, stream digests and verdicts, never raw protocol
frames, submit tokens, protocol challenges, keys, journals or databases. This is
same-user local evidence, not remote cryptographic attestation.

This interface gate is blocking: offline tests establish exact serialized status
fields, and the first successful native run must demonstrate actual incoming
submit plus matching terminal outgoing rows. If the installed archive differs
or history loses essential evidence, report failure and review a diagnostic or
candidate-release change; do not fabricate evidence or modify the archive.

## Lifecycle, clocks and delivery

Consume A's all-installed barrier before launch. The supervisor's live-phase
entry immediately after that barrier begins a hard 1800-second suspend-inclusive
deadline, including the five-minute fixture-build/policy-check/daemon-readiness
budget and output/artifact drain. Record actual daemon launch separately; neither
it nor a slow rebuild resets either deadline. Keep macOS awake and reject existing clock drift or
suspend inconsistency checks. A normal local frontend termination can return
zero; the terminal report carries `reason`, deadline and exact completion counts.
Matrix success is decided only by reconciliation.

Each client waits for all three server-ready records, then processes servers in
fixed platform order, using one client profile serially. Per pair perform the
non-executing cold status probe then exactly one command. Share a 300-second
pair budget between probe and command (CLI timeout rounded down to remaining
seconds); external watchdog includes a bounded termination margin. Total three
pair budget, including margins, is at most 18 minutes. A probe failure records
that pair failed and does not run its command. Subsequent pairs run only while
global/server budgets permit.

The readiness wait ends no later than the local server deadline minus the
18-minute client allowance and a two-minute artifact-delivery reserve. Missing
readiness at that cutoff fails pending pairs. Remote boot-clock deadlines are
never compared to local clocks; a remote server reaching its own cap is an
honest pair failure.

Every client publishes one terminal report with all three slots, including
failed/skipped/timed-out slots, before waiting for its own server. The adapter
delivers all three terminal reports to each server. Stop the server when all
three arrive or the lifetime cap expires. Success additionally requires three
received slots for this server plus three matching authenticated completions
and native receipts. Receiving failure artifacts permits early shutdown, never
acceptance. Missing/delayed output keeps serving until retrieval or deadline;
do not stop just because a native process has exited.

Finally stop a serving frontend using TERM and bounded wait, then kill only if
its exact
recorded identity still matches. Do not indiscriminately kill process groups or
change generic detached-command behavior. Fixture alarm caps command lifetime;
use authenticated cancellation where available, then installed bridge
`cleanup_ready` and owned worker identity checks before deleting engine state.
On any uncertainty retain private owned state and report `cleanup_incomplete`.
For timed-out client CLI processes, send SIGINT first so KeyboardInterrupt can
run `Profile.__exit__` and close its separately sessioned MDK daemon. Then use
bounded TERM/KILL escalation only with owned identity/leader reservation. After
watchdog escalation skip remaining pairs unless native cleanup proves the shared
client profile-home lock free; an orphaned/uncertain daemon means incomplete
cleanup. Keep leaders unreaped through the last owned process-group signal.
Stop caffeinate, perform actual installer uninstall, and remove owned roots only
after all owned processes/state are safe. Preserve sanitized report artifacts
even when installation, runtime confinement, client work or cleanup fails.
Earlier installation/server reports carry provisional cleanup information only.
The final proof is a separate `cleanup-result`, written into an independently
owned output directory outside the removed root after cleanup finishes, and
uploaded using `if: always()`. Never write success before uninstall/root removal.

## Measurements and public report contract

All durations use local suspend-inclusive time; wall clocks are provenance only.
No cross-host timestamp subtraction. Reader pipes drain concurrently and record
first/last nonempty stdout/stderr observations without console decoding.
Phase A records integer nanoseconds. Convert elapsed nanoseconds by division by
1e9 for the explicitly named seconds fields below, and multiply those fields by
1000 in the human-readable latency table headed milliseconds. Never relabel
seconds as milliseconds. Use finite nonnegative numbers and explicit definitions:

| Measurement | Definition and limitation |
| --- | --- |
| `cold_session_status_seconds` | Spawn of fresh pair's installed CLI status probe through expected not-found exit; includes CLI/Python startup, confinement, relay/MLS establishment and one status roundtrip, not isolated MLS handshake latency |
| `command_wall_seconds` | Spawn of installed command CLI through verified exit and complete local output; room already established, includes repeated process/transport startup and stdin/output protocol overhead |
| `command_first_output_seconds` | Command spawn to first observed stream byte; includes execution/input transfer and local scheduling, not remote execution-start timestamp |
| `output_drain_seconds` | First through last nonempty output observation; missing/zero resolution yields null rate, not infinity |
| `effective_download_bytes_per_second` | Verified stdout+stderr bytes divided by command wall time; user-visible end-to-end goodput, not link capacity |
| `output_drain_bytes_per_second` | Verified bytes divided by positive output-drain duration; local observation window, potentially affected by buffering |
| `stdin_bytes` / `stdout_bytes` / `stderr_bytes` | Exact fixture stdin/stdout/stderr bytes; includes the fixture's stdout magic/length-prefix argv encoding, excludes Marmish JSON/base64/MLS/relay framing |

There is one cold control probe and one command sample per ordered pair, not a
statistical latency distribution. The fixture's fixed binary block is one
transfer workload, not many independent latency samples. No p95/p99 or network
bandwidth claim. Report cold and established-room measurements separately;
aggregate only same metric/workload/release/relay set, with explicit n. Optional
min/median/max across distinct platform pairs must be labeled heterogeneous
observations, never platform performance estimates. Failed pairs have null
performance results and a category; never include them as zero latency.

Extend A's envelope with strict versioned terminal schemas (closed key sets,
booleans distinguished from integers, bounded sizes, finite numeric types):

* Client report: local platform/client identity, server-ready record digest map,
  three exact server-role slots; each contains pair context/invocation, both
  public identities, status (`received`, `failed`, `timed_out`, `skipped`), category,
  exact byte lengths/SHA256, observed exit, verification booleans and metrics.
* Server report: local platform/server identity, policy digest, fixture digest
  map, expected principal map, exactly three slots with authenticated
  principal/group/session/invocation, probe proof, request/terminal row IDs and
  digests, reconstructed stream lengths/digests, complete receipt digest/outcome,
  unexpected accepted command count and cleanup/lifetime verdict.
* Records retain A's provenance, pin/source/release/attempt/nonce bindings.
  Client artifacts <=64 KiB; server artifacts <=128 KiB; no raw output files.
* Final cleanup report: `format=1`, `kind="cleanup-result"`, common
  `run,attempt,sha,pin_sha256,role`, `outcome` (`complete`, `incomplete`,
  `not-started`), finite `error` category or null, `duration_ns`, and exact `checks`
  map containing `frontend_absent`, `mdk_absent`, `workers_absent`,
  `caffeinate_absent`, `engine_cleanup_ready`, `uninstall_complete`,
  `root_removed`. Values are true/false/null, with null permitted only for
  genuinely unattempted work on a failed setup; all seven must be true for live
  acceptance. Absence of an unused facility is proved absent, not null. Record
  is <=8192 bytes, has no private paths/process details and uses the same strict
  provenance/type/key rules. Cleanup callback checks workers/MDK and guards
  engine deletion; outer A cleanup owns uninstall/root removal and final report.

Adapters validate before upload and after download; distinct names per stage,
platform, run and attempt. Invalid/missing artifacts never turn into success.

## C: strict rollup and published evidence

Always-run rollup requires exactly three install records, three server reports
and three client reports plus exactly three final cleanup reports for the one
pinned attempt. Validate all schemas and
identity-ready/server-ready digest bindings. Require the exact Cartesian product
of three clients and three servers, not a total count of nine. For each cell
recompute pair context/invocation and expected payload hashes, match both local
reports, policy-role principal and fixture, terminal outcome and complete
retrieval. Require no unexpected commands/receipts, correct lifetime reason,
final cleanup outcome complete with all checks true, and source/release
consistency. Missing final cleanup records, install/runtime records,
cancelled jobs or native confinement failure make the matrix red explicitly.

Produce bounded machine-readable `nine-pair-result.json`, a 3x3 Markdown
status table, and a nine-row metrics table. JSON includes schema version,
run/attempt/commit URLs, installer/application/harness source identities,
release/asset/fixture digests, explicit relay set, storage mode, clocks,
payload definitions, sample counts, failure categories and all nine cells.
Actions summary and artifact publication occur on failure as well as success.

After native validation, commit sanitized results under
`docs/validation/installed-nine-pair-RUN-ATTEMPT.json` and `.md`, linking the
Actions run and exact tested source/release. Markdown explains inclusive timing,
one-sample limits, environment/platform pairs and measured throughput. Link the
latest result from the agent specification/README. Publish actual failed or
partial results honestly while fixing discovered bugs; do not present offline
tests or older archive results as current application acceptance. The workflow
does not receive repository write permission merely to publish measurements;
the reviewed integration commit publishes the selected sanitized evidence.

## Validation and phase exits

B offline tests cover strict pair derivation; baked role/path/challenge;
O_EXCL duplicate refusal; native argv/binary vectors and incomplete receipt;
unconfigured/partially configured workspace fixture exits without side effects;
wrong principal/spoofed label; sent/received direction confusion; missing request
ID; changed immutable duplicate rows (but tolerated timestamp updates); terminal
poll correlation; extra submit without any reply; expected submit without reply;
concrete denied-only throwaway bridge checks; overlapping output/retry;
history-gap rejection including first full window; missing/gapped output; fixed
command count versus frames; deadline/suspend/process identity; delayed artifact
delivery and cyclic shutdown avoidance. Measurement tests use a fake clock to
prove definitions/null/finite rules without claiming native performance.

C tests delete each required record, duplicate a platform/pair, swap principals,
modify invocation/fixture/policy/output/source/attempt/nonce, add a fourth command,
provide successful exit without terminal output, fail cleanup, and exercise
missing/provisional-versus-final cleanup reports and root-removal failure, plus
partial/cancelled/native-startup-denied reports. Test JSON and Markdown retain
all nine ordered cells and count only comparable successful observations.

Run affected Python tests plus locked fixture build/native tests, shell/workflow
validation and integrated Claude auto review. Then execute the actual installed
workflow, watch runner assignment within five minutes, investigate native
failures, and rerun corrected reviewed increments. B's publication can honestly
record native blockers; C's acceptance requires nine actual successful pairs in
one attempt. Commit/push reviewed source and sanitized evidence to both configured
remotes and verify publication; never close the overall task on mocked or partial
matrix evidence.
