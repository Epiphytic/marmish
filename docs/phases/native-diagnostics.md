# Native prerequisite diagnostics

Beads `marmish-5lw`. This is a bounded correction to the existing native
validation phase. The previous macOS spike collapsed unrelated native failures to
`native_spike_failed`; that result does not establish their cause. The disposable
explicit-Keychain provider test has passed independently. This change exposes
which prerequisite failed without changing any prerequisite or claiming native
frontend support. Independent Claude design approval is recorded in
`docs/reviews/native-diagnostics-plan-approved.json`; the initial review and its
resolved primary-failure capture finding are retained alongside it.

## Boundaries and interface

Implementation owns `tools/marmish_native_spike.py`, a new focused diagnostic
test module, a narrow optional diagnostic hook in
`DarwinConfinedDaemon._preflight`, and the native validation workflow plus its
focused workflow assertions. Reuse the existing native lifetime test unchanged.
Do not change MDK patches, source guard, provider behavior, relay selection,
process ownership, confinement policy, filesystem requirements or cleanup
ownership checks. Coordinate workflow ownership with the main agent before edits.
No live/provider calls are needed to implement or review this correction.

The spike emits one JSON failure record with exactly `outcome` (`failed`),
`category`, `stage`, `errno` and `exit_code`. Stage and category are constants
from a closed implementation-defined table. Categories distinguish native I/O,
subprocess exit, timeout, invalid native result and existing fixed prerequisite
refusals. Numeric errno is null unless obtained from the failing OSError or
immediately from the failing native call; exit_code is null unless an integer
subprocess return status is available. Bound errno to 1..4095 and return status
to -255..255; require exact int (excluding bool), using null outside those ranges. Never serialize exception text,
args, commands, paths, stdout/stderr, environment, identities or credentials.
Unknown exceptions receive a fixed unexpected-failure category and nonzero exit,
without a traceback. Parser usage errors remain ordinary CLI errors before any
native probe, with no private state created.

Record a fixed current stage immediately before each operation: relay argument
validation; native compiler availability; private workspace
and control setup; each named helper compile/sign; profile publication; process
identity; continuous clock; diskutil execution/plist validation; storage create,
file fsync, F_FULLFSYNC, exclusive publication, collision refusal and directory
fsync; socket budget; broker creation; confinement; owned cleanup. Compile/sign
output is discarded rather than printed on failure. Keep existing timeouts and
success payload; fixed diagnostic progress records may identify completed stages
without leaking dynamic values; send progress and failure records to stderr,
leaving stdout as the single success JSON. Clear ctypes errno immediately before each
renameatx_np call and snapshot it immediately afterwards. A logical mismatch in
an otherwise successful native call has null errno; do not reuse stale errno.

`_preflight` gains an optional diagnostic observer, default None. The observer
receives only a fixed event (`begin`, `failed` or `cleanup_failed`), fixed stage,
and optionally the exception object and exact subprocess return code; exceptions
are classified internally, never serialized. Before each operation, notify
`begin` outside the try which catches the operation itself; catch the operation's
exception at its boundary, notify `failed` and
re-raise the original exception before cleanup runs. A probe return-code refusal
notifies `failed` with that exact bounded code. Split every `check` into control,
optional reset and confined stages, and name fixture/socket/interface/DNS setup
and daemon construction separately. Ignore observer return values, never use
notification results to permit a probe or suppress an exception; observer errors
propagate as failures. Default None preserves normal startup behavior.

The spike observer freezes an immutable first-failure record immediately on the
`failed` event, including the failing operation's stage/category/numeric status.
Subsequent stage changes or successful cleanup cannot overwrite it. Apply the
same capture-before-cleanup boundary to the spike's operations. Explicitly run
broker close and temporary-directory removal, and handle the preflight's
probe-file unlink and outside-fixture temporary-directory removal cleanup the
same way, each with its own fixed cleanup stage. If cleanup fails after a primary error,
retain and emit that primary record first, followed by a separate fixed
cleanup-failure record. If only cleanup fails, it becomes the primary failure.
All paths exit nonzero on failure. Do not rely on `__context__` or a mutable
current-stage field at the outermost exception handler to recover the primary.

## Independent native evidence and workflow cleanup

Keep explicit disposable-Keychain provider acceptance first. Before the broad
prerequisite spike, run the single existing Darwin test
`NativeLifetimeTests.test_bootstrap_child_reads_exact_frame_parent_uses_devnull`
in a separately named workflow step gated explicitly to macOS, using
`PYTHONPATH=tools python3 -m unittest
test_marmish_transport_macos.NativeLifetimeTests.test_bootstrap_child_reads_exact_frame_parent_uses_devnull`,
with a two-minute bound. Keep ordinary workflow fail-fast sequencing; a failed
shim step prevents the spike, and does not become a passing skip. It uses a known
nonsecret 72-byte fixture and verifies child receipt and the shim parent's
/dev/null check. Call this **unconfined shim handoff** evidence. It does not prove
Seatbelt permits inherited-pipe reads. The later offline suite may repeat it;
actual confined patched-MDK bootstrap and command execution remain mandatory
native acceptance gates after build.

Give checkout an explicit step ID in both jobs. Gate all three always-cleanup
call sites: native prefix removal, live interop cleanup, and live prefix removal.
These steps invoke repo scripts only if this attempt's checkout succeeded. A missing checkout emits a
fixed `cleanup_not_initialized` status and does not invoke a missing or stale
workspace script. Both `cleanup_not_initialized` and
`cleanup_retained_unverified` exit nonzero. If checkout did not succeed but a state pointer exists (`-e` or `-L`, including a dangling symlink),
report `cleanup_retained_unverified` and do not read/delete it or claim cleanup.
Do not change existing prefix retention after offline failure or cancellation:
report fixed `cleanup_retained_test_failure` or `cleanup_retained_cancellation`
and retain the owned prefix with a nonzero cleanup-step status, preserving current
behavior. The cancellation label applies only when the offline step was itself
cancelled; earlier cancellation leaving offline skipped retains the existing
behavior rather than silently broadening the retention policy. Successful-path cleanup still uses the existing
validated pointer/backlink/inode checks. Live cleanup still requires verified
process/private-state cleanup before removing its prefix; uncertainty retains
state and reports failure. Cleanup output never changes an earlier failed step
to success; a cleanup-only failure fails the job. No host lifecycle service or
new deletion policy is introduced.

## Verification and acceptance

Hermetic unit tests inject each operation's OSError, subprocess exit/timeout,
malformed XML, non-dict plist roots and native-call errno results. Exceptions deliberately contain
private path, command, output and credential-shaped sentinels; assert none appear
in emitted JSON. Verify exact keys, closed stages/categories, bounded numeric
fields, errno clearing/snapshot, nonzero failure, unchanged success payload and
preservation of the primary preflight stage when cleanup succeeds. Separately
inject broker-close, temp-directory removal and probe-file unlink failures after
a primary error; assert the unchanged primary record precedes a fixed cleanup
record. Also test cleanup-only failure and reject bool numeric values. A return-code
refusal may notify twice (once with the code, then when re-raised); assert the
first record wins and keeps the original exit_code. Test the preflight
hook's fixed control/probe labels and default no-op behavior without executing
Seatbelt or network calls. Workflow assertions cover missing checkout, stale
state pointer, normal cleanup, offline failure/cancellation retention and live
cleanup prerequisites; failed checkout must never invoke repo Python.

Run affected offline Python checks and whitespace checks, then independent Claude
code review in auto mode. Publish only reviewed changes through the main agent.
A subsequent authorized native workflow must expose the exact fixed failing
stage/status, or pass the unchanged gates. Until then report diagnostics tested
offline and native prerequisite cause pending; do not infer a platform fix.

Never change the runner user's default keychain or search list. Full macOS native
frontend acceptance still requires a dedicated owned default-keychain account.
Explicit-Keychain provider success and unconfined shim success remain separate
facts, and neither closes the confined runtime/default-keychain acceptance gates.

## Implemented offline result

The spike now emits fixed operation-stage failure JSON and preserves primary
failures through owned cleanup. The workflow separates the macOS unconfined shim
check and guards all three cleanup call sites against a missing checkout while
preserving deliberate offline-failure/cancellation retention. Focused diagnostic
checks pass (12 tests), and full Python discovery passes (246 tests, five
Darwin-only skips, 5.005 seconds). Whitespace checks pass. Initial independent
Claude code review approved; the bounded useful fixes and final review are
recorded under `docs/reviews/native-diagnostics-code-*.json`.

These are offline results only. No native prerequisite run, provider operation
or keychain setting was executed for this correction. The original macOS cause,
confined inherited-pipe execution and dedicated-default-keychain acceptance
remain pending until their native gates execute.

The final quality-delta check reports increased `_preflight` complexity (16 to
22), length (89 to 138 lines) and containing-class length (386 to 435 lines).
These changes are the independently reviewed operation/cleanup instrumentation
needed to capture the original failure before cleanup; probe behavior and
confinement remain unchanged. Callback and test-fixture reachability reports and
recent-edit churn do not identify additional runtime defects. The measured debt
is accepted for this bounded correction; no suppressions or baseline changes
were added.
