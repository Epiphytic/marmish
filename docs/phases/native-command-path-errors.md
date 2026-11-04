# Native command lookup and failure diagnostics

Beads: `marmish-8r4`. This increment follows the background daemon increment.
User authorization: resolve commands using the server daemon PATH and distinguish
missing executables and other execution failures from command-policy refusals.

## Scope and implementation boundaries

The engine adds bounded executable resolution and finite launch-failure outcomes;
the native owner integrates them with its existing durable admission, CLI and
finite debug diagnostics. No shell interpretation, sandbox, worker-helper PATH
search, arbitrary remote environment, automatic launch retry, or release tag is
introduced. Agents implement engine and wire/CLI components with disjoint files;
the integration owner changes command_service, command_engine and command_store.

## Resolution contract

Capture the daemon process's `PATH` once when starting its command-engine worker.
Tests inject this value instead of changing process-global environment. An engine
`resolve_exec(request, search_path: Option<&OsStr>)` method uses the command
account's configured cwd and returns a persisted ResolvedExec request carrying canonical executable, absolute
invoked_path and the original complete argv. Preserve all argument bytes, including
empty arguments, whitespace, Unicode and shell metacharacters. Validate request
limits before lookup and after substitution (256 args, 64KiB total, existing
executable limit). Reject NUL/empty argv and Shell requests at this API.

Absolute paths and relative paths containing '/' are resolved directly; relative
paths use the engine account cwd. Bare names search daemon PATH in order. Unset
PATH means no search and command_not_found. Empty PATH and empty components mean
command cwd, following Unix PATH semantics. Relative entries also use command
cwd. Bound PATH to 64KiB and 1024 components; oversized configuration returns
command_resolution_failed/invalid_search_path. No implicit /usr/bin:/bin search.

Canonicalize aliases, including explicit /bin symlinks. Require a regular file
and effective-user execute access. Skip missing or non-executable PATH candidates
and use the first executable candidate; remember inaccessible/non-executable
candidates to return that explanation if the search finds no executable. A
selected executable denied by policy stops admission: never search later entries
to evade its restriction. Other filesystem errors stop lookup with a finite
resolution reason. Direct paths do not search alternatives. Preserve the fixed,
trusted worker helper selection and the worker's existing sanitized child
environment (including PATH=/usr/bin:/bin); daemon PATH selects argv[0], it does
not add arbitrary environment inheritance to remote commands.

## Policy and durable ordering

Remove the pure raw-argv executable-policy precheck in command_service; retain
peer authentication, role/grants, shape, time, quota, authority and journal digest
validation. Filesystem work runs only on the dedicated command-engine thread,
never the serialized MDK/journal convergence actor.

Retain execute_operation's initial engine status lookup and engine-known guards.
Only a genuinely new Dispatch resolves argv[0], checks engine execution policy
before issuing any execute token, then calls Engine.submit with the resolved
request. Engine.submit independently repeats this check after its existing-job
lookup. Enforce every additive default and per-key allow layer against the
canonical target; deny if any deny layer matches either canonical target or
absolute invoked_path. Preserve alias-path deny rules. Its existing repeat lookup
stays before policy checks. Native journal identity/digest continues to use the
original authenticated argv; engine identity/digest uses its resolved request.
An existing invocation always attaches to recorded facts without re-resolution,
even after PATH, executable or policy changes. A changed original request still
conflicts, even if both spellings resolve to the same target. Refusals are
persisted via the existing pre-engine refusal mechanism and replay identically.
Known missing recovery never dispatches.

CommandDenied is reserved for actual allow/deny mismatch. Malformed executable
inputs return InvalidRequest; pattern validation/evaluation exhaustion returns a
finite policy/configuration failure, not a fabricated permissions denial.

## Finite error and wire contract

Add engine lookup errors and native wire codes:

| Code | Finite optional detail |
| --- | --- |
| command_not_found | path_search_exhausted, executable_missing |
| command_not_executable | filesystem_permission, not_regular_file, execute_bit_missing |
| command_resolution_failed | symlink_loop, filesystem_io, invalid_search_path |
| command_denied | policy_restriction |

Use typed engine reason enums or equally closed variants; native mappings are
exhaustive. Wire detail validation allows only exact code/reason pairs; retain
the historical noncanonical_executable variant and its bounded canonical-path
validation for decoding old records. Never include arbitrary OS strings, PATH,
argv, matched rules, payloads, private keys or command output in diagnostic
metadata. Debug emits finite stage/category/reason at resolution and committed
refusal boundaries. CLI retains WireError detail and selects fixed local human
explanations; it never prints arbitrary remote details verbatim. Submission,
status/poll and resumed wait paths all expose the same explanation.

Preflight is advisory and has filesystem races. Add persisted
ExitOutcome::LaunchFailed(LaunchFailure) alongside legacy NotStarted. Finite
LaunchFailure reasons classify filesystem permission, missing interpreter or
loader/target disappeared (ENOENT/ENOTDIR at spawn), invalid executable format,
invalid cwd, resource unavailable and other startup failure. Do not confidently
label spawn ENOENT as the original executable missing. Keep cancellation-before-
start as NotStarted and any uncertain post-spawn observation as UnknownLaunch.
Store transition guards allow LaunchFailed only where NotStarted was legal;
Running never transitions to a startup failure. Wire adds finite launch outcome
kinds (or one launch_failed kind with an allowlisted reason field) and native
CLI emits command_launch_failed with a fixed reason instead of silently returning
255. Retain compatibility reading existing stored NotStarted rows.

Existing closed wire enums reject unknown codes/outcomes. Document that richer
errors require coordinated client/server upgrades; do not claim old-reader
compatibility. Do not change existing success/exit-status or replay semantics.
Canonicalization is not inode pinning and does not prevent same-user file
replacement, copying or hardlink-based policy limitations.

## Validation and acceptance

Engine tests cover PATH order, later executable after unusable candidate, missing,
unset/empty/relative PATH entries, cwd-relative command paths, symlinks, spaces,
non-UTF8 filesystem paths, malformed argv, limits, permissions/type, loops,
missing targets, and exact argv-tail preservation. Policy tests cover additive
layers, alias canonical targets, selected-denied candidate stopping search and
budget/configuration failure. Launch tests cover invalid format, missing loader,
finite persisted outcomes, and unchanged uncertain/cancel behavior.

Native tests cover closed wire code/detail/outcome roundtrips and invalid pairs,
CLI explanations including resume/poll, durable refusal reopen/retry, original
request digest conflicts, engine-known status after PATH/target changes, and
known-missing recovery. Debug output tests exclude secret/payload sentinels.

Run appropriate engine, prototype and all-feature native suites, fmt and strict
Clippy. Perform actual Rust daemon/client execution over an explicit relay with
disposable keys: bare command, symlink/canonical policy behavior, nonexistent
bare and explicit path, non-executable target, invalid executable format, denied
command and literal special-character arguments. Record sanitized outcomes and
latencies in docs/validation. Run existing Linux AMD64/ARM64 and macOS CI after
source publication and report actual results independently of local checks.
Obtain actual Claude auto design approval before coding and integrated code
review after fixes. Update agent-spec and README, audit staged secrets/ignore,
commit and push both mirrors, verify refs and Iris before closing the increment.

## Frozen integration details

Policy validation/evaluation errors use wire `command_policy_error` with no
detail, distinct from command_denied. Engine InvalidPolicy maps there when
admitting a new command. Shell-mode disallow under an effective executable
restriction is an actual policy refusal; malformed Exec spelling is InvalidRequest.

Wire outcome `launch_failed` has value Some(1..=7), with explicit conversions:
1 filesystem_permission; 2 missing_executable_or_interpreter;
3 invalid_executable_format; 4 invalid_working_directory;
5 resource_unavailable; 6 executable_changed; 7 other_startup_failure.
Never depend on Rust enum ordinals. Missing/null/0/out-of-range values are invalid.
Check cwd when possible before classifying spawn ENOENT/ENOTDIR; retain conservative
other_startup_failure where the failure is ambiguous. Native CLI preserves normal
terminal output consumption and Finish before reporting command_launch_failed;
it must not turn a terminal launch outcome into a pre-terminal status_problem.

Typed lookup errors are CommandNotFound(NotFoundReason),
CommandNotExecutable(NotExecutableReason), CommandResolutionFailed(ResolutionReason)
with enum names corresponding exactly to the table above. Engine LaunchFailure
variants use the seven named reasons above. Prototype mappings must remain
exhaustive and compile; the historical Python application is not being extended.

## Round 1 correction and accepted limits

Claude round 1 found six blocking gaps; the following requirements resolve them.
The following integration details incorporate the accepted round 1 corrections.
Add native-only CommandRequest::ResolvedExec { executable, invoked_path,
argv }. Legacy Exec/Shell remain readable. ResolvedExec persists all original argv
bytes; launcher selects executable as program and calls CommandExt::arg0(argv[0]).
This preserves multi-call behavior. The digest includes every field. Resolver
constructs an absolute invoked_path from the selected PATH entry or direct path;
retain its alias spelling with the lexical normalization specified below for deny
matching. Do not replace it with its canonical target. Canonical target is used for all allow layers and also deny
matching. Add a symlink-app executable test that reports its original argv[0].

Add Engine::check_execute_request(ctx, request), validating context, structure,
canonical executable and complete additive policy without creating a token/job.
Native invokes it before token issuance. Engine.submit repeats equivalent checks
after its existing-job lookup. Resolution and policy refusal therefore consume
no execute token; test more than 128 consecutive denials followed by allowed work.
Map InvalidPolicy to a durable command_policy_error refusal, not Missing; all
other definite refusal mappings are explicit and never redispatched. Malformed
requests are InvalidRequest. Unexpected operational errors retain existing
uncertainty behavior and must not be mistaken for definite rejection.

Launch preflight checks command cwd separately. Canonical mismatch maps reason 6;
invalid account/spec/preconditions map reason 7 unless a more precise fact is
known. Replace ChildSpawnError::NotStarted with a classified payload for definite
launch failures; cancellation remains persisted NotStarted. Wrap pre_exec failures
into a conservative recognizable other-startup category before spawn's errno
pipe, never mislabel them filesystem permissions. Spawn ENOENT/ENOTDIR reason 2
has conservative client wording mentioning executable, interpreter/loader or
working directory disappearance: a cwd preflight cannot eliminate races. A script
that successfully starts /usr/bin/env and whose env cannot find another program
is an ordinary process exit (often 127 with stderr), not a launcher failure.
The sanitized child PATH remains documented separately.

Persist LaunchFailed in existing encrypted JSON rows and ResolvedExec in request
rows without rewriting old terminal records. New versions read legacy outcomes;
old binaries cannot read new variants. Downgrading a profile after new commands
is unsupported. No old-reader/on-disk backward compatibility claim. Wire uses
only launch_failed plus required value 1..7, with no added outcome fields.
store.finish must use is_startup_failure (NotStarted or LaunchFailed), legal only
from Launching and never Running. Exact wire error/detail pairs are validated.

Authorized execution clients can distinguish existence/type from policy denial;
this limited filesystem-existence disclosure is accepted to provide the requested
troubleshooting. Filesystem operations can block on NFS/FUSE despite bounded
candidate counts; no hard lookup deadline or sandbox is promised. Every finite
lookup refusal, including transient filesystem_io, is durable for that invocation;
retrying after repair requires a new invocation. Denials are now asynchronous:
initial Accepted can be followed by a persisted refusal. Existing admission and
retention budgets apply to those rows; test cleanup. A restart can resolve a
previously unsubmitted invocation differently using the new daemon PATH, but an
engine-known invocation is never re-resolved or relaunched.

ResolvedExec always requires the worker canonical-executable recheck, even when
there are no effective restriction layers. Charge matcher fuel for both canonical
and invoked-path deny comparisons before short-circuiting. Validate both path
fields (absolute, NUL-free, <=4095 bytes) and original argv limits. For deny
matching, invoked_path removes redundant separators and dot/parent-dot components lexically
without following symlinks; canonical executable is computed from the original
candidate, so lexical normalization never changes the file selected for execution.
No claim that arbitrary alias patterns identify every possible alias of a file.

## Launch-format correction after integrated review round 1

Actual review and the engine invalid-format test confirmed libc execvp performs
an implicit /bin/sh fallback on ENOEXEC. Preserve the promised explicit argv
execution by preparing an owning ExecImage before fork: canonical program
CString, original argv CStrings including argv0, sanitized key=value environment
CStrings, and null-terminated argv/environment pointer arrays. All allocation,
validation and environment inspection occurs in the parent. Buffers and pointer
arrays remain immutable and alive across spawn; a narrowly documented Send/Sync
implementation is valid only because pointers refer to owned immutable CString
allocations and the callback captures the complete image.

The existing final pre_exec callback first calls platform::pre_exec, mapping its
failures to PRE_EXEC_FAILURE, then invokes libc::execve with the prepared image.
Success replaces the child without returning; failure returns the kernel errno
through std's existing spawn error pipe. No callback returns success into execvp,
so ENOEXEC surfaces as invalid_executable_format without an implicit shell.
Std Command still handles fork, stdio duplication, cwd change, the handshake,
failed-child reaping and successful Child ownership. Explicit Shell requests
prepare [account.shell,-c,command] for execve and retain their intentional shell
semantics. No header sniffing, post-fork allocation or environment lookup.

Test invalid plain text and missing interpreter, original argv0 and inherited
sanitized environment, and injected pre_exec EPERM/ENOENT remaining reason7.
This correction requires independent Claude design approval before coding.

Legacy raw Exec bare names retain lookup against the exact sanitized child PATH
from spec.environment, using the existing resolver before fork and taking only
its executable for the ephemeral ExecImage. Do not rewrite old request/digest.
For legacy Exec names containing '/', retain the original absolute path or join
its relative spelling to spec.cwd; existing restricted canonical checks run first.
Map legacy missing lookup to missing_executable_or_interpreter, non-executable
lookup to filesystem_permission, other preparation errors to other_startup_failure.
New native ResolvedExec already carries the selected canonical program and never
uses this compatibility lookup. Explicit Shell still uses its configured shell.

Independent amendment approval (Claude session
0d5f1aec-53b1-4f38-a25a-6714b10f53b2) is conditional on these recorded details:
legacy bare Exec uses the compatibility lookup above (never cwd execve of a bare
name); map macOS EBADARCH to invalid_executable_format; callback calls a method
on the whole immutable image to retain every CString owner; execve method returns
io::Error so callback cannot return Ok into execvp; keep Command's canonical
program/environment construction consistent; validate environment keys are unique,
nonempty and contain neither '=' nor NUL, with CString failures classified reason7.
Image fields are private, constructor-only, no Clone/interior mutation or custom
Drop, and local to launch preparation. Test exact sanitized envp and legacy bare
lookup as well as the previously required launch, argv0 and setup-error cases.

Definite Unauthorized and QuotaExceeded engine-admission results are also recorded
as durable refusals for that invocation; changing authorization/capacity requires
a new invocation. Operational failures retain their separate uncertainty behavior.
