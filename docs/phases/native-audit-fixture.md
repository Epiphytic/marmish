# Deterministic native audit fixtures (marmish-oah)

The Linux x64 CI run 40980988449 found missing `/proc/PID/exe` while the PID
entry remained in three audit tests. Existing fixtures copy `/bin/sleep` under
role basenames and assume it remains alive for ten seconds. Early exit is
observed; multicall dispatch is only a hypothesis. Production audit behavior
must remain unchanged.

Replace only test fixtures in `tools/test_marmish_native_validation.py` with a
minimal locally compiled C executable. Require a discoverable `cc` compiler on
Linux: missing compiler or failed compilation fails these tests explicitly,
never skips them. Existing native CI already requires a native compiler. Compile
with a 30-second subprocess timeout into a private temporary directory, without
shell evaluation or downloading dependencies. No production changes or workflow
changes are required.

The executable writes one readiness byte on an inherited dedicated pipe, closes
that descriptor, then blocks indefinitely in `pause()`. It never reads fd0 or
writes fd1, changes discovery environment, dispatches by argv0, forks, or expires
on a timer. Its pathname has the exact audited role basename (`wnd` or
`marmish-engine-bridge`). The pipe descriptor is passed as a decimal argument;
it contains no credentials. Parent closes its write end and waits at most five
seconds for readiness, verifies the byte and that the child remains unreaped
and running. Register cleanup immediately after Popen, before readiness checks.
Every success/failure path closes readiness descriptors; child cleanup uses the
owned unreaped Popen PID, kills then waits at most five seconds, and closes
stdin/stdout pipes before removing private fixture files. No broad process
matching, service calls, provider calls, or relay activity.

Reuse the same fixture for daemon and bridge audits. Preserve all real negative
FD/environment checks, identity-change checks, required-role checks and private
bridge pipe requirements. Add explicit readiness/real executable/liveness
checks, plus a readiness failure case proving cleanup occurs. The fixture stays
alive after its readiness pipe closes and through repeated audits until explicit
cleanup. Only Linux process tests require the compiler; macOS skips these tests
as before.

Before implementation obtain independent Claude design approval. Then run the
focused native-validation suite, combined native E2E offline suite, and full
`tools/test_*.py` discovery with bounded command timeouts; inspect failures
honestly. Obtain Claude Code review in `--permission-mode auto`, including tests.
Record review JSON under `docs/reviews/native-audit-fixture-*.json`. Do not alter
historical K3 reports/digests or claim these fixtures ran in earlier CI. Root
handles integration, specification, commit/push, and subsequent native CI.

## Review refinement

Compile once per Linux test class, using `shutil.which('cc')`, with explicit argv
`cc -std=c11 -D_POSIX_C_SOURCE=200809L -Wall -Werror -O0 -o OUT SOURCE`.
Use the existing bounded subprocess utility (30 seconds, stdout cap 65536,
stderr discarded, no shell; environment only PATH). Missing compiler raises
`fixture_compiler_missing`; build failure raises `fixture_compile_failed`.
Use one class-owned 0700 directory and copies under exact role basenames.
The C program parses its numeric descriptor with checked strtol, full
consumption/range checks, and exits nonzero on readiness write/close failures.

Readiness uses select with a monotonic five-second deadline, reading at most
two bytes until EOF. Exactly one `R` followed by EOF proves the child closed
its readiness descriptor. Timeout, EOF without R, and wrong bytes have fixed
failure categories. Then require child.poll() is None, non-zombie proc state,
and exact resolved /proc/PID/exe. pass_fds is explicit; close_fds remains true.
Close both pipe ends if Popen raises. Register idempotent child cleanup before
waiting, and invoke cleanup immediately on any readiness failure. Test that
path deterministically by patching the readiness wait to fail, then assert the
child is reaped, readiness descriptor is EBADF, and stdin/stdout pipes closed.
Cleanup closes pipes even if wait times out; propagate that timeout. Remove the
class directory only after all spawned children are reaped; uncertain cleanup
retains it. Assert child stays live through repeated real audits, including
negative fd/environment checks. Tests also prove missing compiler fails closed.

## Implementation validation

Design approval: [Claude sonnet review](../reviews/native-audit-fixture-plan-final.json).
The first explicit `claude-opus-4-6` invocation was unavailable (404); it did
not supply approval. The default initial reviewer resolved to claude-opus-5-5;
the approved followup used the available `sonnet` alias.

Local offline validation of the test-only replacement:

- Native support plus native E2E offline tests: 49 passed.
- Full tools discovery: 258 tests, 5 expected skips, passed.
- Ten repeated audit-fixture groups: 120 tests passed in 1.411 seconds.
- Compiler: Ubuntu cc 13.3.0; no compiler/package installation performed.
- Quality delta found one remaining test-class size increase (61 to 128 lines),
  accepted for explicit readiness, cleanup and compiler-failure regression
  coverage. A reported duplicate capture wrapper was removed. New unittest
  hooks/test methods are reported as statically unreachable by the analyzer;
  actual unittest runs execute them.

These checks do not establish success on the failing native CI runner. A fresh
native CI run is still required. Production auditing and historical K3 reports,
file digests and live validation records are unchanged.

Completed-code Claude review approved the implementation and independently ran
focused33 tests three times, combined59 tests, and fulltools256/5skip. Its useful
nonblocking descriptor-cleanup finding was fixed by moving fd assertions inside
the cleanup try. Added explicit invalid-descriptor and wait-timeout cleanup tests;
the final local counts above include these two followups. The bounded Claude
[followup](../reviews/native-audit-fixture-code-followup.json) approved those
changes and independently passed the full35-test native support module. Queue pickup/human design approval does not
apply: this is the user's directly authorized phased work, delegated by root.
