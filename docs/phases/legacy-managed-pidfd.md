# Retained Linux pidfd compatibility

Beads `marmish-d93.1`, under retained-reference cleanup `marmish-d93`.
This increment changes the historical Python reference and installed-test
harness. The native Rust runtime and its frozen v0.3.1 candidate are separate.

## Evidence and review

Two selected process-cleanup tests under the pinned managed CPython 3.13.13
both raised `AttributeError` because `os.pidfd_open` was absent. They ran two
tests, with two errors and no skips. This establishes the local API absence;
it does not establish libc or kernel support, or the cause of hosted failures.
The separate offline fixture dependency-cache failure remains out of scope.

Actual Claude design review used session
`5e4ce260-7d2f-4f23-bf98-3fc66b4c50e1`, with `--permission-mode auto`.
The original design SHA-256 is
`8acfb45c74bfa036cc327dce0d136361d8823af4f8a3b96665c91fce820f0fef`.
Its signal-enum amendment is
`3f36e2dd8b1f9828770763f96fb489b5c55f114725f2b818a7f65b4fc74e184d`.
The actual follow-up design approval SHA-256 is
`1edff59a1099a5b71036debcc8cbebaccbd82363f635950ca893599ea1ae4795`.
Implementation and validation use an isolated worktree. The failed native
candidate remains failed; integrating this reference-only increment requires
fresh qualification of the eventual release source.

## Interface and trust boundary

Add `linux_pidfd_open(pid)` and `linux_pidfd_send_signal(fd, number)` to the
existing platform module. Prefer each Python API independently. Only a missing
API permits a lazy cached `ctypes.CDLL(None, use_errno=True)` binding to the
corresponding libc export. Bind exact C signatures, flags zero and null signal
information. Never fall back after an OS error, retry, select raw syscall
numbers, or signal by numeric PID. No library is loaded at import or on Darwin.

Validate PID and descriptor as bounded plain integers, rejecting bool. Accept
integer signal enums, reject bool, normalize to plain int and range-check before
calling either backend. Missing exports return ENOSYS; syscall errors preserve
errno and OSError subclasses. A negative return without errno becomes EIO.
The open caller owns the descriptor, including descriptor zero; send never
closes its argument. Unsupported hosts fail closed.

The existing state helper retains its identity check before open, second
identity check before signaling, descriptor-only signaling and exactly-once
finally close. Preserve its current open/exit race behavior. Replace the direct
Python API in the real cleanup test with the same platform wrapper, retaining
all ownership, cross-session refusal and descriptor-readability assertions.
There is no protocol, persistence or recovery-format change.

## Validation and acceptance

Cover independent native and libc selection, ABI signatures, enum conversion,
invalid values, non-Linux rejection, lazy loading and cache restoration. Exercise
ESRCH, EPERM, ENOSYS, EINVAL, EINTR, missing exports and zero-errno failure, with
no fallback after errors. Test identity ordering and descriptor closure on
success, disappearance, mismatch and exceptions; numeric-PID signaling must
remain unused.

Completed-code review session `3027f7c4-95ac-409a-aad3-4553f317258e`
requested a real noninheritance check independent of descendant visibility.
Add a Linux-only own-process test that forces both libc bindings, opens
`os.getpid()`, checks the descriptor is non-inheritable, checks signal zero
without delivering a signal, and closes it in `finally`. Clear the cached libc
handle and restore errno and patched APIs after the test. Missing exports or
kernel support fail the test on Linux; they are not skipped.

The separately reviewed focused validation ran 21 mocked tests, one real
own-process libc test and the two original real setsid tests under managed
CPython 3.13.13. All 24 passed with zero skips, errors or failures. Whole affected
modules and hosted tests were not run in this increment. Record real backend
availability only when measured. Keep original failing evidence and distinguish test success
from process observation or cleanup uncertainty. Never delete or signal
unproven retained state. Validate relevant macOS tests without loading Linux
bindings; hosted Linux results remain separate from local observations.

Obtain Claude review of the completed code and actual validation before
committing. Record useful findings and limitations, keep source/private state
out of staged files, and publish source to both configured mirrors only after
release-freeze sequencing permits. A plan or mock PASS is not real-process
acceptance.

See [validation](../validation/legacy-managed-pidfd.md) and the
[completed-code review](../reviews/legacy-managed-pidfd.md). Host-wide process
observation remained incomplete; retained state was not deleted or treated as
proof of global cleanup.
