# Conditional cold macOS diagnostic with three client principals

This is a design proposal for Beads `marmish-8ka.5.8`, not implementation approval
or an additional qualification requirement. Implement and dispatch it only if the
fresh, complete installed stream matrix for source `e9686c0799e78010c350d4990406c93ddb8bc474`
fails again against macOS. The coordinator must first record that actual run and
its failure evidence, obtain independent Claude design approval, and authorize
implementation. A passing matrix makes this diagnostic unnecessary. No product
fix or cause is established by this proposal.

## Evidence and precise question

Original matrix run `42363063498` completed all six client-to-Linux duplex cells
and failed all three client-to-macOS cells. Its `FailedCell` retains only
`validation_or_transport`. `installed_process::run_input` discards child status
and partial output after stdin write/shutdown failure. Other failures from
`installed_streaming_cases::duplex` collapse prefix parsing, journal assertions
and byte validation into that same public category. Missing uploaded receipts
cannot distinguish these cases: receipt collection runs only after complete
client-result exchange.

Diagnostic run `42367008260` passed direct fixture execution, raw, zlib, and three
concurrent zlib streams using the identical frozen source571 product and fixture.
It executed the copied fixture directly first, then warmed both raw and zlib
paths before concurrency, and used one client identity. The original matrix
executes no direct fixture warmup, admits three distinct client principals to
three rooms, and concurrently operates its macOS client against other servers.
Every original macOS-server cell requests zlib. These are distinctions to test,
not proposed explanations.

The inspected matrix source remains unchanged from571 for configuration, fixture,
case execution and result validation. Streaming configuration replaces the finite
helper's entire limits object with `max_uptime=1800`, so the helper's temporary
`max_commands=4` does not remain in the daemon configuration. Argument construction
and `Pair::validate_duplex` have no macOS-only validation branch. Do not change
these product or ordinary cohort paths on the basis of this investigation.

## Smallest implementation boundary and compatibility

Add a separate build-only installed-check subcommand
`macos-cold-multiprincipal-diagnostic`, registered in `installed_check.rs`, with a
small child `installed_macos_cold_multiprincipal.rs` orchestrator and adjacent
tests, declared with `#[path] mod cold_multiprincipal` inside
`installed_macos_diagnostic.rs`. Preserve the existing `macos-diagnostic` command, option defaults, execution
order, closed report schema, independent validator and historical evidence.
Generalizing its single-client `State`, positional account array and all reports
would create unnecessary regression scope. Conversely, copying its full runner,
provenance and finalization implementation would duplicate reviewed ownership
logic. Neither is the proposed approach.

The child module can access its parent's private Pins, Capture, Pipe, Observation,
`capture`, `read_pipe`, `choose` and `bound`; do not widen those types into a global
API. Register the child through one parent entry point. Make these concrete small
extractions rather than assuming the current State-bound signatures generalize:

- Split `install_and_setup` into installation/image verification and the existing
  two-profile setup wrapper. The installer helper takes owned paths, Environment,
  Pins, fixed deadline and signal receiver; it returns the verified fixture path.
  The old wrapper still creates client then server and retains accounts in exactly
  `[client, server]` order.
- Extract typed `ProfileSpec { identity_name, role }` setup argv and validation
  helpers. Validate the name with the existing identity validator and restrict role
  to client/server. Separate immutable desired peer lists from identity names;
  config validation requires exact sorted/deduplicated accounts and expected role.
  Keep old `setup_valid`/`configure_value` wrappers passing the original literals
  and single peer. No untyped arbitrary setup/config callback is introduced.
- Extract pure named stream/cancel argv builders from `stream_args` and the
  ClientCancel branch. Pass identity name and fixture role explicitly, including
  `client-1` etc. The old stream wrapper passes exactly `client` and `macos-arm64`;
  its cancellation wrapper passes exactly `client`. No fallback profile selection.
- Extract receipt checking over verified fixture path, Pins, nonce and fixture
  role; retain the old State wrapper supplying `macos-arm64`. Extract observation
  inspection over explicit before/client/server List slices plus the exact
  `[client_i,server]` pair. Keep the old `entries` and `inspect` wrappers using
  the original two-daemon State. Share pure `bound`/`choose`, not State ownership.
- Reuse publication through a narrow `PublicationContext { output, report_kind }`
  with pre-serialized validated report bytes and a typed outcome supplying fixed
  stage/category/passed fields. It computes report digest and emits the existing
  immutable complete/failure markers. Unknown report kinds are refused; shared
  marker kinds bind to the exact report hash and independently validated kind.
  Keep old `finish_report` and category selection wrappers with old case rules;
  the new report has its own completeness predicate. Do not turn Report into a
  generic framework or change any existing JSON fields/markers.
- Extract pure common path/pin/platform/nonroot/environment preflight and one
  small signal-watcher scope from `run`/`run_inner`. A small deadline holder fixes
  work900/cleanup180. The two explicit orchestrators still own work, cleanup,
  removal-after-proof and report assembly; no resource-owning generic async
  runner or user callback receives authority. Preserve original error precedence
  (`work_result.and(removal_result)`), final signal latch check, and publication
  failure semantics. Roughly90 lines of truly common preflight need not be copied.

The existing `capture` remains sole unreaped-child owner, retaining finite drain
grace and owned-group cleanup. New four-service finalization is explicit and
separate from the old sequential two-service `DaemonCleanup`; it must not reuse
that sequential loop four times. Old-path literal/serialized regression tests
pin setup ordering/account ordering, identity and role arguments, single grants,
config, stream/cancel argv, receipt role, old report kind/fields/markers and default
execution order. This is regression protection, not a claim that helper extraction
itself qualifies transport.

Use the existing verified acquisition helper in a new manual
`.github/workflows/streaming-macos-cold-multiprincipal.yml`; add a distinct JS
report validator entry point in `macos-diagnostic.mjs`. Share acquisition and its
fixed policy, not Context-based candidate tooling. The new validator must reject
the old report kind and vice versa. No product module, dependency, wire, manifest,
executor, command policy meaning, release workflow or qualification rollup changes.
No second fixture implementation or alternate payload generator is introduced.

## Frozen product and independent harness provenance

Use exactly candidate run `42358105270`, attempt `1`, source
`571d98b77780d265a2fb29d84ae160fec72ba37b`, target `aarch64-apple-darwin`.
Reuse the existing fixed three artifact IDs, ZIP sizes and SHA256 anchors in
`macos-diagnostic.mjs::ARTIFACTS`, the original bootstrap, manifest and fixture
file pins, and the exact closed Pins schema. Crosscheck current artifact API
metadata and ZIP bytes; an expired/unavailable/different artifact is a blocker,
never permission to use a newer product or a newly compiled fixture.

The new debug probe records and checks its own compiled full harness commit and
binary digest separately. Preserve old tooling source digest and record pins;
never substitute the new harness commit for the original tooling source. Do not
construct Context or forge a candidate qualification identity record.

The source guard must verify clean committed checkout, full ancestry from571 and
publication on origin/master. Create a **separate new-mode allowlist**, leaving the
old diagnostic guard's acceptance unchanged. Parameterize the internal guard by a
closed mode enum; the existing entry point always selects its existing old list.
The new list is the existing diagnostic's exact `ALLOWED_PATHS` plus exactly:

- `docs/validation/streaming-macos-diagnostic.md`
- `docs/validation/streaming-installed-e9686c0-failure.json`
- `docs/validation/streaming-candidate-e9686c0.json`
- `docs/validation/streaming-candidate-e9686c0.md`
- `docs/phases/streaming-macos-cold-multiprincipal.md`
- `docs/reviews/streaming-macos-cold-multiprincipal.md`
- `docs/validation/streaming-macos-cold-multiprincipal.json`
- `crates/marmish-native/src/installed_macos_cold_multiprincipal.rs`
- `crates/marmish-native/src/installed_macos_cold_multiprincipal_tests.rs`
- `.github/workflows/streaming-macos-cold-multiprincipal.yml`

Existing allowed paths `installed_check.rs`, `installed_macos_diagnostic.rs`, its
tests, the acquisition/validator JS and tests, policy tests, and agent specification
are the only shared files permitted to change. Guard tests must fail on product,
ordinary fixture/vector/case/process-helper changes, renamed files and unexpected
file modes. The repeated matrix failure is recorded only at the named
`docs/validation/streaming-installed-e9686c0-failure.json` path, including its actual
run identifier. If it requires another path, amend and review this exact list
before implementation; no wildcard documentation allowance. Parameterize workflow
policy tests by explicit mode/workflow path and expected step names; retain the
old workflow's current assertions and add independent new-workflow assertions.

The frozen candidate artifacts currently expire on **2026-11-21 around10:05UTC**.
Recheck exact API expiry before dispatch. Their unavailability/expiry is a hard
blocker for this frozen experiment; extending deadlines, replacing IDs or
rebuilding571 is outside this design.

## Four owned services and immutable configuration

Install once in a fresh short canonical `/private/tmp/mm.XXXXXX` root. Create
one fresh file-backed server identity and three independently generated client
identities, named `server`, `client-1`, `client-2`, `client-3`. Their CLI identity
names are distinct from setup role: each client uses `--role client`, the server
uses `--role server`. Do not reuse the existing helper's assumption that identity
name equals role. Validate setup JSON against the expected name and role,
backend, create/update result, account, and cleanup proofs.

Using only the installed public setup CLI, grant exactly those three distinct
npubs to the server and trust exactly that server npub on each client. Privately
verify all four accounts differ and each serialized configuration has the exact
expected sorted grants/trust list. All use explicit
`wss://relay.us.whitenoise.chat`, Tor off, file key storage and fresh profiles.
Only the server has fixture execution authority: sole canonical `fixture/main`
allow rule, cwd equal to owned workspace, no per-key exceptions, no
`max_commands`, active-per-principal4, and max-uptime1200. Clients are client-only
and cannot accept remote commands. No running configuration mutation or secret
inspection is permitted.

Resolve all four sockets from their actual child TMPDIR/profile IDs before any
spawn, reject duplicates and existing unsafe paths, then start four foreground
installed daemons. Keep their Child ownership through authenticated PID/UID
readiness and finalization. Copy the original fixture into one fresh0700 inode
and create its private receipt directory; never execute that copy, inspect it by
running it, or sign/rewrite it before the cold batch. Hashing and manifest checks
are permitted and must not be described as execution warmup.

## Cold-first execution and evidence

Start no local outgoing fixture traffic besides the three selected client
attachments. All client daemons are ready before a shared barrier releases
exactly three concurrent public CLI commands, one from each client profile,
against the same server npub. Use `--compress` for all three, the original exact
six `streaming_fixture::arguments` operands, and independently generated fresh
nonces. Label fixture roles with the three original ROLES values, one per client;
these labels are fixture arguments, not a claim of cross-platform execution.
Require original96KiB stdin/stdout,8KiB stderr and exit7.

Capture before asserting. Reuse the diagnostic concurrent stdin pump/output
drains, bounded1MiB retained output per pipe, overflow/error flags, actual local
input-written count, child exit/signal and unreaped-group cleanup. Never return a
generic error that discards successful earlier capture. The existing original
`run_input` remains unchanged and is not used by this diagnostic.

For each case retain closed stage outcomes equivalent to the original client
path, plus the stronger existing diagnostic proofs:

1. Child spawn/capture: status, elapsed time, timeout/signal/input-EPIPE/pipe errors,
   input/output lengths and digests, group-cleanup outcome.
2. Exact stderr stream prefix parsing, before interpreting remaining binary stderr.
3. PID/UID-authenticated local client List: direction client, exact expected
   client/server accounts, terminal Exited7, session/group/Open binding.
4. Server List counterpart: direction server and identical principals, stream,
   session, group and Open digest; terminal Exited7, inactive and not cancelled.
5. Original Pair-equivalent exact byte lengths/hashes, exit7 and330-second bound.
6. Closed fixture receipt: matching nonce, role, exact argv digest, original image
   digest and expected byte hashes. A receipt absent before this stage is an
   observation, not a statement that execution never began.

Each client has exactly one new diagnostic stream, so a missing/malformed prefix
can use before/after List diff for that exact client account/direction, crossbound
to the server entries. A unique candidate yields binding evidence without turning
prefix failure into success; zero/multiple candidates remain absent/ambiguous.
Never infer order from the server's three-entry list. No direct database reads.
Transport sequence/remote-delivery/EOF counters unavailable from old List remain
explicitly unavailable; local written/captured counts are not such counters.

After all three cold captures are inspected, run the identical copied fixture
DIRECTLY with a fourth nonce and exact arguments, even if cold validation failed,
only when **all three capture tasks returned and each reports confirmed owned-group
cleanup**, no signal is latched, and time remains before the fixed work cutoff. This is
`direct-after`, not a precondition or warmup. If a live unknown child prevents
safe execution, retain the cold outcome and record direct-after not run.
No warm retry is required. A later separately approved optional retry must retain
the failed cold observations and cannot set overall passed=true.

This batch isolates cold multiprincipal incoming load while excluding the
original macOS client's simultaneous outgoing traffic to Linux. It does not
reproduce that outgoing load or cross-host timing. If cold multiprincipal passes
but the full matrix still fails, the next evidence step is stage-preserving
observation in the actual matrix or a separately reviewed outgoing-load case,
not an assertion that independent identities were the cause. There is also a trust-topology difference: original
matrix clients trust all three servers, while this diagnostic trusts only the one
owned server; both grant all three diagnostic client principals. That omitted
multi-server trust topology remains an explicit limitation alongside outbound
traffic and cross-host timing.

## Bounds, finalization and publication

Preserve the current absolute900-second work cutoff and180-second cleanup reserve;
max-uptime1200 exceeds the entire1080-second probe. Install120, setup shared240,
all-four readiness shared180, cold batch330, direct-after15 seconds are ceilings
clipped to the original work deadline, not summed reservations or renewed timers.
The old batch's240-second nominal ceiling is not used to claim original330-second
Pair equivalence. A stage without remaining time records budget-exhausted rather
than starting new work. Keep48-minute workflow,23-minute build cap and19-minute
probe cap; no live deadline increase is part of this design.

Register SIGINT/SIGTERM before async work. Retain all attachment tasks until each
has completed its owned-group finalizer. On every success/error/timeout/signal,
use one explicit async finalization wrapper with a fixed180-second reserve. Do
not apply `cutoff(now, N)` sequentially in a way that renews or consumes another
service's allowance. Define phase deadlines relative to cleanup start T:

1. **Cancel, T..T+75:** concurrently run all three named client cancel operations
   and server admin-cancel (`tokio::join!` over disjoint owned daemon references).
   Each client List is capped at15 seconds, then its one owned Open cancellation
   at45 seconds; the server operation is capped at60. All clip to T+75. A failed
   client operation does not suppress server cancel or another client.
2. **Idle, after cancel join..T+120:** concurrently poll all four physical-idle
   statuses, each with the same absolute T+120 deadline. Even a hung cancel phase
   leaves45 seconds available. Retain four separate proof results.
3. **Stop, after idle join..T+165:** concurrently stop all four exact owned daemons,
   client `stop_streaming(false)` and server `stop_streaming(true)`, clipping every
   future to the same absolute T+165. An idle failure does not skip a stop. Reserve
   T+165..T+180 for completed-task collection, safe removal and reporting setup.

The phase deadlines clip ordinary work; existing bounded reap and termination
may finish a few seconds beyond them. The final reserve absorbs the existing
three-second owned-group reap/poll and five-second daemon termination grace.
These are not hard preemptive deadlines, and missing cleanup proof remains a
failure. Do not add new overshoot or renew the180-second reserve.

At most one cancellation candidate exists per client in this experiment. If
unexpected additional active entries appear, report topology violation and do
not serially issue an unbounded per-entry loop; server administrative cancellation
and all subsequent cleanup phases still run. Cleanup phase timeout handling must
retain each daemon Child for the existing stop/reap operation rather than drop a
future owning it. Attachment cleanup is collected concurrently inside the first
phase and its outcome remains independent; unsafe unreaped attachments cannot be
claimed cleaned merely because daemon idle succeeded.

Use an injected hung-client regression: client1 cancel consumes its full75-second
virtual phase while client2/client3/server cancel are observed starting immediately;
all four idle operations then receive the unexpired T+120 bound and all four stops
the unexpired T+165 bound. Also hang one idle and require every stop still starts
with45 seconds reserved. Check fixed deadlines and call ordering using injected
clock/actions, not75-second sleeps. Missing physical proof remains failure even
when stops run. This tests starvation avoidance, not successful cleanup of a
permanently hung service.

Signals during cleanup remain latched; SIGKILL/runner loss cannot promise async
cleanup. Remove private state only after all required physical/stop proofs; never
upload retained private state. Work errors take precedence over removal errors;
report cleanup failures separately and never erase the original cold failure.

Emit a distinct closed `kind=macos-cold-multiprincipal-diagnostic`, schema1,
qualification=false, same_host_only=true report. Keep the old Pins schema and
existing nested capture observation fields; add exact requested scenario,
cold-before-direct ordering, three principal-label/digest bindings, independent
room/session/Open digests, original Pair-stage outcomes, four service cleanup
proofs, and explicit `local_outbound_fixture_traffic=false`. Distinct rooms are
verified through group/session binding, not inferred from different keys. Overall
success requires all three cold observations, direct-after, unchanged provenance,
no late signal, and all cleanup proofs. Failed first attempts remain failed.

Use fresh create-new `report.json`, `complete.json`, `failure.json` publication
with existing post-hardlink/directory-sync failure handling and failure-marker
precedence. Independent JS validation checks exact requested cases and order,
all private comparisons' sanitized outcomes, pins/current probe binding, four
cleanup proofs and marker semantics. Upload only these explicit sanitized records
plus validated provenance. Never upload raw captures, account/stream/session IDs,
paths, process environment, grants/profile JSON, keys, PID/UID values or databases.
A failed probe/validator remains failed even when a useful report was uploaded.

## Required validation before dispatch

Hermetic tests cover identity-name/role separation, exact three sorted grants,
duplicate principals/sockets refusal, public npub argv roundtrip, original literal
arguments, unique per-principal prefix fallback, ambiguous/missing List entries,
terminal/active/cancel mismatch, exact Pair bytes and receipt mismatch, and cold
failure followed by direct success remaining failed. Inject stage ordering to
prove no copied-fixture execution precedes cold capture and no outbound load is
silently added. Exercise partial input/EPIPE capture and existing child ownership
regressions through the shared runner rather than duplicating its implementation.

Finalizer tests inject cancellation/readiness/idle/stop failures, require attempts
for all four owned services, and reject missing physical proof. Publication tests
force second-write and post-link failures; JS rejects complete+failure, wrong
scenario, warm-only evidence, missing principal/room, changed pins/probe and leaked
unknown fields. Policy tests retain fixed571 assets, closed source allowlist,
nonpublishing workflow, bounded steps and explicit upload membership. Run focused
installed-check tests, strict feature-enabled all-target Clippy and formatting;
obtain actual integrated Claude auto code approval before any dispatch. Record
Linux hermetic execution separately from the eventual Mac diagnostic result.

The expected implementation is a thin new four-service orchestrator plus modest
visibility/extraction changes to reviewed helpers and an independent closed
validator. If it requires another large runner, generic service framework or
product observability change, stop and revise this design before implementation.

## Trigger observed

Fresh full candidate `42367702241` at exact e968 completed successfully on all three platforms and rendering. Its original installed stream matrix `42371657352` repeated the same three macOS-server failures; the six Linux-target client observations passed exact bytes and exit7. The failed final rollup and actual run evidence must be retained at the named failure evidence path before implementation. This activates the conditional diagnostic after the already recorded independent design approval. The two exact e968 candidate evidence paths above are the only allowlist amendment: they contain reviewed native/package metadata and no executable or product changes.

## Implementation checkpoint

The repeated e968 matrix failure met the conditional trigger. The bounded
implementation and independent actual Claude code approval are recorded in
[the review](../reviews/streaming-macos-cold-multiprincipal.md), including local
checks and explicit nonblocking coverage limitations. Hosted execution is the
next validation step. Product binaries, ordinary installed matrix and public
installer version remain unchanged.
