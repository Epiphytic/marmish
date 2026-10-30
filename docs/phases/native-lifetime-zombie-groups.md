# Proving zombie-only native groups (marmish-zuv)

Run40980988449 (76e487c) passed disposable explicit-Keychain acceptance and
72-byte bootstrap delivery. The unconfined shim then emitted exactly
`native_lifetime_failed/kill_group/errno=1` and returned70. Seatbelt and live
steps were skipped. Do not infer broader platform support from this evidence.
This increment changes only the classification of that specific native group
EPERM after proving the group has no live members. Root owns specification and
aggregate CI evidence; no provider/default-keychain/live calls belong here.

## Actual Apple contract

Research is pinned to Apple XNU commit
`f6217f891ac0bb64f3d375211650a4c1ff8ca1ea`, not assumed to be the runner's exact
kernel revision. Primary files/functions:

- [kern_sig.c killpg1](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_sig.c):
  the group iterator excludes `SZOMB`; zero signalable members returns EPERM in
  POSIX mode. `WNOWAIT` intentionally leaves our exited leader as a zombie.
  This is consistent with the observed result; it does not establish the exact
  runner kernel path or make every EPERM harmless.
- [proc_info.c proc_listpids/proc_pidinfo/proc_pidbsdinfo](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/proc_info.c):
  group enumeration filters `p_pgrpid` under `proc_list_lock`, visiting both
  `allproc` and `zombproc`. Returns byte count; a full caller buffer can silently
  truncate. `PROC_PIDTBSDINFO` with nonzero arg explicitly enables zombie lookup;
  arg0 can miss them. PID info reports PID/PPID, effective/real UID, PGID,
  status and start seconds/microseconds.
- [libproc.h public declarations](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/wrappers/libproc/libproc.h)
  expose `proc_listpids` and `proc_pidinfo` since macOS10.5, below this project's
  deployment target15. The [libproc.c wrappers](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/wrappers/libproc/libproc.c)
  return0 for PID-info syscall failure; exact-size validation is mandatory.
  Existing platform code uses arg0 for live identities; it does not establish
  zombie arg1 behavior. Native fixture success is required evidence for arg1,
  zombie group inclusion and retained credential fields. Refusal retains70
  with group_inspection; do not bypass unsupported native behavior.

## Proof and failure behavior

Implement an internal Darwin-only `lifetime_zombie_group(pid_t leader, int *error)` helper
returning true only for the following complete bounded proof. It is invoked only
when the existing final `kill(-leader,SIGKILL)` fails with EPERM. All other signal
errors and ESRCH behavior remain unchanged. No signals after final reaping.

First call waitid for this exact direct child with
`WEXITED|WNOHANG|WNOWAIT`: exact success, matching si_pid and a terminal
CLD_EXITED/CLD_KILLED/CLD_DUMPED code are required. This positively holds the
original forked child unreaped; no earlier code reaps it, and no PID/PGID reuse is
possible while held. Failure, no event, EINTR or unexpected code refuses this
exception rather than guessing. Check native BSD leader metadata with arg1:
exact struct-size return, PID and PGID equal leader, PPID equals this shim,
effective/real UID match current nonroot equal real/effective supervisor UID,
nonzero start seconds and microseconds below1000000, state SZOMB. Save its full
native identity `(pid,start_sec,start_usec,ppid,pgid,uid,ruid)` for revalidation.
Do not accept a merely same-UID unrelated group.

Use system libproc struct declarations with a Darwin-only static size assertion
of136 bytes; portable fake declarations do not establish native layout parity.
Use fixed stack buffers: two arrays of257 PID slots and256 normalized identity
slots (no dynamic allocation). `proc_listpids(PROC_PGRP_ONLY,leader,...bytes)`
must return a positive aligned byte count strictly below buffer capacity, with
at most256 members. Reject negative/zero/full/misaligned/oversized results,
nonpositive/duplicate PIDs, missing leader or multiple leader entries. Never
use the null-buffer size hint as a membership count. Sort PIDs for comparison.

For every returned PID, read BSD info with arg1 and require exact size, matching
PID and PGID, same expected effective/real UIDs, valid nonzero start identity,
and SZOMB. Only the leader requires PPID equal this shim; other owned zombie members
may have another parent. Save only the normalized identity fields, never names or command
fields. A vanished/reaped member, short result, access refusal, live member or
any mismatch refuses the exception. Do not ignore errors or treat disappearance
as proof. Then enumerate the group a second time, requiring the identical PID
set, and re-read every member, requiring identical saved native identities and
SZOMB. Finally repeat the leader waitid and BSD identity checks. No retries:
fixed maximum514 PID-info reads, two list reads and two waitid calls.

This proof is deliberately strict: unrelated zombie reaping between snapshots
can cause safe refusal. Zombie processes cannot return to execution or fork.
The unreaped private group leader prevents PGID reuse. A live process already
outside the original group but in its session could join concurrently; escaped
groups/sessions were already outside the shim's original group-signal guarantee.
This change does not claim atomic cleanup of all descendants or escaped groups,
and adds no authority to signal an unrelated group. Same-UID hostile inspection
or process manipulation is outside the existing supervisor trust boundary.
All group members visible at both complete snapshots must be terminal.

Only proven zombie-only EPERM is classified like the existing ESRCH case; still
attempt the leader SIGKILL and final waitpid, and preserve child status mapping.
Never accept live/uncertain members, any other errno, incomplete enumeration,
wrong UID/PPID/identity, or waitid failure. Those retain exit70. Group proof
failure adds one fixed `group_inspection` diagnostic before the original
`kill_group` EPERM record; native errno is snapshotted immediately for failed
calls, otherwise null for a logical refusal. Increase the existing bounded record
cap from5 to6 and insert this stage between waitid and kill_group. Records remain
buffered until all final signals/waitpid attempts finish; no process identifiers,
member counts, native struct bytes, names or paths are logged. Proven success
emits no error record. No change to child stdin, grant or parent-death protocol.

## Interfaces and tests

Own `tools/native/marmish-lifetime.c`, cleanup header, a new internal native group
header, `tools/test_marmish_lifetime.py`, `tools/test_marmish_transport_macos.py`,
a new native group-test fixture source, and this phase/review/validation evidence.
No MDK patch/sourceguard change: helpers compile in place, are signed and hashed
into the existing manifest. Root may add the new native test name to the existing
pre-spike workflow step after design approval; workflow remains root-owned.

The production C file includes the group implementation before the cleanup
header. The portable cleanup harness defines a fake function of the same
internal signature before including only the cleanup header. The group harness
substitutes native function names with macros after system/test declarations
and before including the group header, avoiding duplicate definitions.
The existing portable cleanup harness supplies a fake group-proof function to
test EPERM+prooftrue vs proof-false, other-errno-never-probed, both signals still
attempted, subsequent waitpid failure, and record order/cap. A separate portable
harness compiles the actual group-proof header with test-only libproc declarations
and fake calls. Cover every structural/ownership/status/native-identity refusal,
missing/duplicate/truncated group, changed member set, reused PID start value,
vanished member, both waitid checkpoints, and exact arg1/list-size contracts.
Mock later calls alter errno to verify original snapshots. Never signal real
processes in portable tests. Compile with Wall/Wextra/Werror; explicitly skip
only if no compiler exists (Linux CI provides one).

Add a Darwin-only fixture executable used only by tests. It owns child processes
and process groups, and uses waitid WNOWAIT to hold a zombie leader. Cases:
zombie-only single-leader and leader-plus-zombie-peer groups succeed; live leader refuses; zombie leader plus live same-group
peer refuses; unrelated/nonchild leader refuses. Use same-session child-owned
process groups so the fixture parent can own and reap both leader and peer;
production still uses its existing setsid boundary. Pipes synchronize readiness;
no sleeps as proof. Cleanup always attempts owned group/individual termination
before reaping, handles expected zombie-group EPERM only with the same proof,
and waits for all owned children. Never leave fixture descendants orphaned.
The original actual shim/bootstrap test must now return0 and no records, while
retaining exact72-byte/frame checks and fd0 /dev/null assertion. Root adds both
native tests to the existing unconfined pre-spike step so positive and negative
proof behavior executes before a separate Seatbelt failure can skip offline tests.
Neither test touches any keychain or relay.

Run focused portable and full Python checks, py_compile and diff checks locally;
record six Darwin skips honestly. Obtain independent Claude Sonnet design
approval before code and auto-mode completed-code review afterward, incorporating
useful findings. Actual native CI must then pass both unconfined tests or expose
sanitized refusal stages. This is required runtime acceptance, not implied by
portable contracts. Do not commit/push until parent integrates and authorizes it.

Independent Sonnet design review approved in native-lifetime-zombie-plan-initial.json.
Useful ABI/arg1 caveats, a two-zombie positive fixture, nonleader PPID intent and
explicit test-hook placement are incorporated above. The new targeted native
test name is NativeLifetimeTests.test_native_zombie_group_proof.

## Implementation and offline validation

The bounded group proof, guarded cleanup integration, six-record diagnostic
parser, portable native-call harness and Darwin fixture are implemented. The
fixture's two positive cases and three refusal cases run independently of the
confined daemon. The workflow invokes both this fixture and the exact-frame
bootstrap shim test before the broad Seatbelt prerequisite.

Local Linux validation passes 260 Python tests with six Darwin-only skips
(5.288 seconds), including the portable C harness compiled with
`-Wall -Wextra -Werror`. That harness exercises 50 group/identity/native-call
states; the cleanup harness separately checks proven and refused EPERM, other
errors, signal/reap ordering, errno preservation and bounded diagnostics.
`py_compile` and `git diff --check` pass. No provider, keychain or relay calls
were made for this increment. The system Darwin ABI, zombie lookup and actual
shim success remain pending native CI acceptance.

The quality-delta report exits 2: `finish` complexity grows from 12 to 20 for
the explicit EPERM-only proof and refusal diagnostics, the native test class
adds a fixture method and repeats compile/sign/run scaffolding, and test setup
contains larger bounded C fixtures. These are contained security branches and
independent executable test fixtures. Static internal C helpers are not a new
external API; unittest callbacks reported as dead are discovered by unittest.
No metric baseline or suppression was added. PR-context inspection found the
expected cleanup/parser/test/workflow boundary; the rest of the transport and
secret handoff stay unchanged.

Independent Sonnet completed-code review approved without blockers. Its useful
coverage suggestions are incorporated: duplicate leader, exact 256-member
acceptance/257 refusal, second-pass disappearance, both additional terminal
CLD statuses, initial EINTR and nonzero child status after accepted proof.
The nonchild native refusal intentionally uses the fixture PID; portable tests
cover uncertain native identities without creating orphan zombies. Native
compiler output remains discarded to preserve the fixed diagnostic boundary;
nonzero compile status fails the gate. The existing two-minute pre-spike budget
remains bounded and unchanged; each fixture mode has its own five-second limit.

Followup completed-code review: Sonnet APPROVE, no blockers, recorded in
`docs/reviews/native-lifetime-zombie-code-approved.json`; reviewer independently
ran the full 260-test suite with six native skips and verified all added cases.
Sanitized offline evidence is in `docs/validation/native-zombie-groups.md`.
Actual Darwin acceptance remains outstanding.
