# Descriptor-based native storage gate (marmish-3cq)

CI 40983389092 on 84882c7 passed both unconfined lifetime tests and disposable
explicit-Keychain acceptance. Its broad prerequisite failed at storage_diskutil
with exit 1 before plist parsing or any durability operation. The exact reason
is unproven because stderr was deliberately discarded. The current gate passes
a fresh nested private directory to diskutil, whose documented device/volume
interface is not an appropriate dependency for this question.

## Contract and boundary

Reuse the implemented engine contract at
crates/marmish-engine/src/platform/macos.rs:validate_storage: fstatfs on an open
root descriptor, exact filesystem type `apfs`, MNT_LOCAL required. Apple's
[statfs manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/statfs.2.html)
explicitly supports any file within a mounted filesystem and fstatfs on an open
FD. System ABI comes from sys/mount.h, pinned research to Apple XNU commit
f6217f891ac0bb64f3d375211650a4c1ff8ca1ea
[header](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/mount.h):
f_fstypename16, exported f_flags and MNT_LOCAL 0x1000. Use the installed system
header, never a copied ctypes struct. These contracts justify the replacement;
they do not prove the exact prior runner exit 1 cause.

Add one mode `storage` to the existing compiled/signed marmish-probe helper.
It opens the supplied private directory O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC,
fstats it and requires directory, current nonroot effective UID ownership and
mode 0700 (including no privilege/sticky bits). Then fstatfs obtains the exact
opened object's filesystem. Require a NUL within the 16-byte type field, exact
apfs, and MNT_LOCAL. Unknown/nonlocal/other filesystems refuse. The spike already
creates its root/data with mode 0700; no parent climbing, hardcoded root volume, mountpoint
lookup, DiskArbitration service, dynamic library or ctypes ABI is needed.

The mode has a closed result contract, no names/paths/native struct bytes:
exit 0 and empty stdout on success; exit 2 and empty stdout only on unsupported filesystem (non-apfs, missing NUL,
or non-local); exit 7 and empty stdout for directory metadata refusal (not
S_ISDIR, wrong owner/root EUID, or S_IMODE not exactly 0700); exits 3/4/5/6 for open/fstat/fstatfs/close
failure and stdout containing only the snapshotted positive errno decimal and
newline. The stdout result is at most 4 digits plus newline; only 1..4095 is valid.
Storage mode requires exactly argc 3; other argument counts return 64.
Invalid errno emits `0\n`, which the Python adapter rejects as malformed.
Snapshot errno immediately; after a successful open always attempt close, keep
an earlier refusal/error if close also fails, otherwise report close failure.
Never retry unknown failures or fall back to diskutil.

`storage_probe(home, diagnostic, probe)` invokes the existing runtime helper
with timeout 5/check False, discards stderr and validates exact result/exitcode.
The passed probe is the owned compiled/signed runtime executable. Diagnostics
use fixed stages storage_filesystem (launch/timeout), storage_filesystem_result
(malformed output), storage_filesystem_open/fstat/fstatfs/close (native errors),
storage_filesystem_type (unsupported filesystem), and
storage_filesystem_directory (exit 7, new native_storage_directory_unsafe refusal). Propagate errno only after strict
ASCII decimal validation; never log stdout or command/exception strings.
Require exact empty stdout for 0/2/7; exits 3..6 require exact
ASCII `[1-9][0-9]{0,3}\n` and numeric value <=4095, raising OSError with
that errno and a fixed message at the exit-derived stage. Any other returncode
(including 64, signals or bool), extra/nonASCII/oversized output or invalid errno
is ValueError at storage_filesystem_result, category native_result_invalid and
null exit_code. Launch with stdin DEVNULL, close_fds True, fixed PATH /usr/bin:/bin,
stdout PIPE and stderr DEVNULL. Remove obsolete diskutil/plist stages and plistlib dependency. Keep every
existing fsync, F_FULLFSYNC, exclusive rename/collision and directory fsync
operation, ordering, cleanup and diagnostics unchanged.

## Implementation and validation

Own tools/native/marmish-probe.c, tools/marmish_native_spike.py,
tools/test_marmish_native_spike.py and this phase/review/validation evidence.
Root owns spec and workflow. No production engine/confinement/provider changes. Ancestors and canonical
home are already validated by _open_control; the final open rejects symlinks.
The later durability operations reopen the private path: this is a path-level
gate, not a continuous descriptor guarantee against same-UID modification.
Existing ctypes renameatx_np ABI stays; no ctypes filesystem struct is added.

Portable offline tests mock the closed helper result for each native stage,
malformed/oversized/negative/nonASCII/extra output, unsupported/failure exits,
timeout, success and primary-error preservation. Compile the actual probe with
synthetic system mount declarations and substituted open/fstat/fstatfs/close
calls to exercise syscall ordering, all refusal conditions, immediate errno
snapshot and close-error precedence. Shadow sys/mount.h using -I, rename main and
substitute syscalls around the actual source. Missing compiler fails the test;
fake declarations are not Darwin ABI proof.
Existing storage durability tests adapt only their initial gate and must still
exercise original fsync/fullfsync/publish/collision/directory fsync failures.

A new Darwin-only `NativeStorageTests.test_opened_private_directory_filesystem`
compiles/signs the actual helper with system headers and production target 15.
Use a disposable /private/tmp/mn- directory for actual local APFS success.
File/missing/symlink failures must report storage_filesystem_open with native
errno respectively ENOTDIR/ENOENT/{ELOOP,ENOTDIR}. A chmod 0755 or 01700 directory
must report exit 7/directory refusal; wrong-owner and root checks remain synthetic
because a nonroot fixture cannot create a genuinely unowned 0700 directory. No provider or
relay calls. Root adds this exact test to the unconfined pre-spike step, so it
runs before the broader prerequisite. The owned WorkflowTests assertion must
require all three exact test names in this step and verify its ordering before
the spike. Root workflow and assertion land together. Keep existing two-minute
step cap; compile/sign each bounded 60/10 seconds, fixture invocations 5 seconds. If actual storage is unsupported, fail
closed; do not skip or weaken to claim support.

Obtain independent Claude Sonnet design approval before code and auto-mode code
review after integration. Run focused/offline full Python tests, py_compile,
diffcheck and quality/pr-context review. Record native acceptance as pending
until actual runner evidence passes the new test and retained durability gate.
No commits/push or native/provider/live calls in this agent's scope.

Initial Sonnet design review requested three bounded clarifications: separate
metadata/filesystem refusals, close the complete exit/stdout/native-negative
contract, and assert native workflow placement. All are incorporated above.

## Implementation and offline evidence

The system-header helper mode and strict adapter are implemented. Existing
fsync, F_FULLFSYNC, exclusive publish/collision and directory fsync tests retain
their original operations and exact failure-stage assertions. The native test
is included in the root-owned workflow's existing pre-spike invocation.

Focused spike tests: 14 passed with one Darwin skip. Full Python discovery:
262 passed with seven Darwin-only skips (5.427 seconds). The actual probe source
compiles with synthetic syscall/header substitutions in 19 cases; this is
portable contract evidence, not Darwin ABI or filesystem acceptance. Python
compilation and diff checks pass. No keychain/provider/relay calls were made.

Quality-delta reports the new static storage function's 20 explicit validation
and cleanup branches, the expanded fixture class and unittest callback false
dead-code results. The independent result-decoder closure is invoked through
the diagnostic runner. These bounded predicates and synthetic executable tests
are retained; no metric baseline/suppression was added. The unrelated pending
build_mdk.py delta is outside this increment's ownership. PR-context review
matches the helper/adapter/test/workflow boundary. Actual macOS filesystem and
durability acceptance remain pending native CI evidence.

Independent Sonnet completed-code review approved the integrated source, tests
and workflow with no blockers and independently repeated the full offline suite.
The long refusal-set line was wrapped. The optional negative-errno fixture adds
no meaningful coverage beyond the tested zero/out-of-range clamp, so it remains
unchanged. Native compiler output stays discarded to preserve the existing fixed
diagnostic boundary; actual compile success is a mandatory native gate. The old
nonessential diskutil guide citation was removed. Root owns updating the spec's
historical failure description when native acceptance evidence arrives.
