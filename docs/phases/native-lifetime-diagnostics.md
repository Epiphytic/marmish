# Native lifetime cleanup diagnostics (marmish-zuv)

Native run40978430684 at f1f3616 passed the disposable explicit macOS Keychain
provider check. Its independent unconfined shim test published the exact 72-byte
bootstrap report, then the shim returned 70 instead of 0. The broad Seatbelt spike
was skipped. Linux x64 passed, including all three pinned MDK bootstrap tests;
Linux ARM refused its nested user-namespace prerequisite. The sanitized monitor
report is [native run40978430684](../validation/native-run-40978430684.json);
it contains no private paths or raw logs. This increment addresses only the shim's lost error
information, without claiming the native cause is already known.

## Actual boundary and decisions

`tools/native/marmish-lifetime.c:exited` calls `waitid` with
`WEXITED|WNOHANG|WNOWAIT`, intending to retain the leader PID/group until final
signals. No other wait operation precedes `finish`. `finish` sends group and
leader SIGKILL before `waitpid`, accepts ESRCH for either signal, retries waitpid
EINTR, and otherwise returns 70 for either signal refusal or wait failure.
Currently those distinct causes, and earlier waitid errno, are discarded.
The stdin fixture returns only 0,64,65,67,68; observed 70 therefore belongs to the
shim cleanup path. Unexpected reaping/ECHILD, a waitid platform error, or signal
refusal remain hypotheses, not diagnoses. Do not change WNOWAIT, drop group
cleanup, accept ECHILD, or ignore non-ESRCH failures speculatively.

Emit at most one fixed JSON record per failed native operation to stderr:
`{"outcome":"failed","category":"native_lifetime_failed","stage":S,"errno":N}`.
The only stages are `waitid`, `kill_group`, `kill_leader`, and `waitpid`.
Errno is snapshotted immediately after the failed operation, before diagnostics
or another syscall. Encode only 1..4095 as an integer, otherwise null. Never emit
PID, PGID, paths, argv, environment, exception strings, output, or secret bytes.
Collect stage/errno pairs in a fixed five-entry array: up to two waitid records
(main and deadline loops), then kill_group, kill_leader and waitpid, in operation
order. Emit zero to five newline-terminated records in one bounded write only
after final waitpid completes or fails, immediately before returning from finish.
Ignore diagnostic-write failure; diagnostics must never delay remaining signals
or reaping. Do not change fd2 flags shared with inherited open descriptions.
The final reporting write may delay shim exit if a caller supplies a full blocking
fd2; owned process cleanup has already completed or attempted all operations. Existing ignored SIGPIPE prevents
a broken diagnostics pipe from terminating the shim. No stdout changes: its
sole initial PID line remains the durable-grant protocol. Child stderr continues
to be redirected to /dev/null before exec.

Preserve all existing cleanup and exit behavior: waitid EINTR remains retryable;
non-EINTR failures are reported before existing cleanup fallback. ESRCH remains
normal for group/leader signals. Attempt both final signals even if one fails.
Retry waitpid EINTR; snapshot/report any other failure and retain 70. Report final
signal failures even if later waitpid succeeds, retaining 70. No secret-store,
confinement, process-ownership, timer, or source patch change belongs here.

The existing macOS bootstrap-handoff test captures the shim's stderr instead of
discarding it. On failure it reports only strictly parsed closed-schema records,
never raw stderr. Call wait(timeout=5) first, then consume at most 4097 bytes with select/os.read
under a two-second monotonic deadline, stopping at EOF. Accept zero to five
newline-terminated records in the prescribed order. Truncated, malformed,
oversized, timed-out or unknown records produce a fixed `native_lifetime_diagnostic_invalid` assertion label.
Even exit zero with a diagnostic record fails this native test with the sanitized
record, because an unexpected waitid error requires investigation. Keep exact
frame-length/content, parent's /dev/null verification, durable grant,
child cleanup and zero-exit assertions unchanged. Close the new pipe in finally.
The workflow already executes this test before Seatbelt, so no workflow changes
are required. Production continues discarding helper stderr as before.

## Implementation boundaries and offline verification

Own only the native shim, its focused tests, and this phase/review evidence.
A small internal portable cleanup header may hold the actual `exited`/`finish`
functions, used verbatim by production and a test-only C harness. The harness
substitutes kill/waitid/waitpid at compile time after system declarations;
it never signals or waits on real processes. This avoids claiming mocked Python
coverage exercises native error handling. Production uses normal native calls.
The header must be a regular committed source alongside the shim; existing native
build compiles that file in place, then signs/hashes the helper in its manifest.
No MDK patch or guarded MDK-tree digest changes are needed. Header changes affect
the actual helper digest through compilation.

Harness cases cover group ESRCH, leader ESRCH, both signal failures while still
attempting both and waitpid, waitpid EINTR then success, waitpid ECHILD, waitid
EINTR/non-EINTR, invalid errno bounds, and successful exit/signal behavior.
Assert exact JSON and preserved return codes/order, no dynamic data. Python
parser tests cover closed keys/stages/category, bool/negative/out-of-range errno,
invalid JSON/UTF8, extra fields, more than five records, invalid order, oversized
input, bounded read timeout, empty success and exit-zero-with-record failure.
Native mocks set distinct subsequent errno values and cover combined group and
waitpid failures, preserving both snapshots and record order. Compile the harness
with -Wall -Wextra -Werror. Skip only when no C compiler is available, and report
that skip explicitly; existing Linux CI installs a native compiler.
Run focused portable tests and full Python discovery offline. Native tests stay
Darwin-only and must not be represented as having passed on Linux.

Obtain independent Claude design review before implementation and auto-mode code
review afterward; incorporate useful findings and retain evidence under
`docs/reviews/native-lifetime-*.json`. Parent owns specification/validation edits
and phase commits. After publication, a new authorized native workflow must either
pass this unchanged gate or expose one of the fixed stages and numeric errno.
Until that run, the exact native cause and confined inherited-pipe behavior remain
pending. Dedicated owned default-keychain account acceptance remains separate.
No default-keychain/search-list changes, provider operations, or live tests are
part of this increment.

The initial independent design review requested explicit multi-record parsing,
deferring writes until all cleanup operations finish, and defining exit-zero
with-record handling. These are incorporated above. Zombie-only group signaling
refusal and inherited SIGCHLD ignore/automatic reaping remain unverified candidate
causes; this increment adds no signal-disposition changes or speculative fix.

## Offline implementation result

The production cleanup functions now share an internal static header with the
portable fake-syscall harness. All native operations and exit decisions are
preserved. The focused suite passes 29 tests (five Darwin-only skips), including
the compiled portable harness. Full Python discovery passes 250 tests with five
Darwin-only skips in 5.101 seconds. Whitespace checks pass. The exact macOS cause
is still pending native execution; no native/provider/live calls were made here.

The final quality-delta check against e5cb345 flags the relocation of internal static `exited` and
`finish` as an API-surface change; neither is exported and their signatures are
unchanged. The strict test-only parser (complexity 18) and native-case table test
(complexity 21) encode bounded validation and explicit syscall outcomes. These
reviewable checks are retained rather than hiding their branches. The native
test class grows four lines for bounded evidence capture. No suppressions or
quality-baseline changes were added.

Initial independent code review approved with no blockers. Its useful follow-ups
are incorporated: the portable harness separately covers a group-signal failure
followed by successful waitpid (still returns 70 with the original errno), and
the parser splits literal newline records and rejects carriage returns. JSON
whitespace remains harmless because every decoded field is closed and validated.
Malformed diagnostics deliberately retain the approved fixed invalid label;
they cannot expose raw stderr. No production behavior changed in this follow-up.

Final independent auto-mode code review approved with no blockers in
`docs/reviews/native-lifetime-code-approved.json`. Its focused checks reran the
portable harness across modes0–10 and the transport tests; the five Darwin-only
tests remained skipped. The final quality/pr-context evidence is scoped against
e5cb345; reported API movement is internal static relocation, with unchanged
signatures. The small test-only complexity increase above is accepted explicitly.
