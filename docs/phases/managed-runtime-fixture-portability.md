# Managed-suite fixture portability and shutdown cleanup

Bounded test-only amendment for the managed-runtime proof. Independent Claude design review (`claude-opus-5-5`) APPROVED test-only
implementation with the nine conditions below; native acceptance is not
claimed. See [review evidence](../reviews/managed-runtime-fixture-portability-design.md). Parent Beads work tracks execution and review dispositions.

## Evidence and cause

The macOS report from run `41143960720` records `full_tools` / `child_timeout`,
494 tests, 30 skips, null exit status and 508 matched progress prefixes. Cleanup
completed and home remained unchanged. The final matched identifier belongs to
`EntrypointTests.test_main_success_finally_restores_handlers_umask_and_state`.
Prefixes can also match failure headings; neither this identifier nor the count
establishes the location of a hang. The captured summary suggests unittest
finished before interpreter shutdown stalled.

`_run_process()` in `tools/marmish_managed_runtime_spike.py` sends combined
output to a regular temporary file and polls the direct interpreter. It does
not wait for pipe EOF. Descendants retaining stdout cannot alone explain this
timeout; the direct interpreter remained alive at the deadline.

`NamespaceTests.test_approved_bridge_decoy_denial_capabilities_and_cleanup`
in `tools/test_marmish_transport_sandbox.py` starts two non-daemon accept-loop
threads, then constructs a daemon and reads
`/proc/sys/net/ipv4/ip_unprivileged_port_start` before entering its cleanup
`try/finally`. Darwin lacks that file. An exception there bypasses stop, socket
closure and thread joins. The class currently has no Linux-only admission.

A bounded local single-test subprocess reproduced the mechanism without running
namespaces: patch only that exact `/proc` read to raise FileNotFoundError. The
runner printed `Ran 1 test` / `FAILED (errors=1)` and returned while both serve
threads remained non-daemon and alive. The interpreter failed to exit within
three seconds; its owned process group was killed and reaped. This establishes
a fixture defect and a strong causal candidate, not a measured native rerun.

Separately, `tools/test_mdk_smoke.py` cleanup fixtures expect Linux main-entry
behavior but do not simulate its admission check. On Darwin, the unchanged
`mdk_smoke.main()` rejects before process/state creation; their cleanup assertions
therefore fail. The invalid-relay fixture similarly encounters the platform
refusal before relay validation. These are quick failures, not evidence that
that final progress hint identifies the shutdown stall.

The exact bounded reproduction invocation below was run from the repository
root. Only the identified `/proc` read was simulated; no source was edited.

```sh
python3 - <<'PY_REPRO'
import os, signal, subprocess, sys
source = r'''
import sys,unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,'tools')
import test_marmish_transport_sandbox as t
original=Path.read_text
def missing(self,*args,**kwargs):
    if str(self)=='/proc/sys/net/ipv4/ip_unprivileged_port_start':
        raise FileNotFoundError('simulated Darwin missing /proc')
    return original(self,*args,**kwargs)
with patch.object(Path,'read_text',missing):
    suite=unittest.TestSuite([t.NamespaceTests('test_approved_bridge_decoy_denial_capabilities_and_cleanup')])
    result=unittest.TextTestRunner(verbosity=2).run(suite)
    print('RUNNER_RETURNED threads=' + str([(x.name,x.daemon) for x in __import__('threading').enumerate()]),flush=True)
'''
p=subprocess.Popen([sys.executable,'-I','-B','-c',source],stdout=subprocess.PIPE,
                   stderr=subprocess.STDOUT,start_new_session=True)
try:
    output,_=p.communicate(timeout=3)
    timed_out=False
except subprocess.TimeoutExpired:
    timed_out=True
    os.killpg(p.pid,signal.SIGKILL)
    output,_=p.communicate(timeout=3)
finally:
    if p.poll() is None:
        os.killpg(p.pid,signal.SIGKILL)
        p.wait(timeout=3)
print('simulation_timeout=',timed_out,'reaped=',p.poll() is not None)
print(output.decode('utf-8',errors='replace'))
PY_REPRO
```

Sanitized observed result: `simulation_timeout=True`, `reaped=True`, one test
finished with one error, and the returned runner listed `MainThread` plus two
`serve` threads, all non-daemon. No raw traceback is stored here.

## Scope and implementation contract

Change only `tools/test_marmish_transport_sandbox.py` and
`tools/test_mdk_smoke.py`. Production behavior, managed harness, workflow,
timeouts, output parser, sandbox admission and acceptance remain unchanged.

Make the actual Linux `NamespaceTests` class explicitly Linux-only, as its
capability, `/proc`, unshare and teardown assertions exercise the Linux backend.
On Linux these tests remain mandatory: missing namespaces, helper failures or
capability failures must fail, never skip. Existing actual macOS confinement
tests and artifact-bound native guards remain required and untouched. Record
this platform classification honestly in resulting skip counts.

Protect every acquired socket and every started thread immediately, including
bind/listen, fake executable creation, daemon construction and `/proc` reads.
Register socket closure from acquisition and stop/join cleanup before starting
threads; track only successfully started threads. Use an outer cleanup guard or
ExitStack so an earlier setup failure cannot bypass cleanup and failure in one
cleanup action cannot prevent attempts on remaining owned resources. Close the
daemon when constructed, signal stop, close listeners and boundedly join workers;
assert workers terminate. Do not hide the problem by making workers daemon
threads. Keep the Linux namespace/capability assertions intact. The separate startup-
failure case also registers `self.addCleanup(daemon.close)` immediately after
daemon construction, ensuring an unexpected assertion or successful startup
cannot leave its owned daemon alive; repeated close remains idempotent.

Within the smoke fixture's simulated main-entry branch only, supply Linux
`sys.platform` admission and an `os.WNOWAIT` attribute as needed. Prefer a
module-local sys replacement to changing interpreter-wide platform state.
Processes/exchange stay mocked; no native waitid call is permitted by this
simulation. Apply the same admission simulation to the invalid-relay fixture.
Keep mock lifetimes bounded and restore all patched attributes. Add a separate
Darwin-admission refusal test with valid arguments: production main must return
`unsupported_platform`, spawn nothing and allocate no private state. Do not
change production platform admission or claim live smoke support on macOS.

## Required checks and exit criteria

Add a portable bounded regression outside the Linux-only class that injects an
early setup failure after thread start, before namespace execution. It must
observe the intended exception, verify both workers stop and sockets close,
and verify no daemon spawn. Also cover failure while acquiring/setup of the
second listener so the first acquired resource is reclaimed. A subprocess
regression must show unittest returns and the interpreter exits within a bounded
interval; on regression failure its owner kills/reaps its process group. Avoid
unbounded joins and arbitrary executable/test-selector inputs.

Run affected modules from normal and symlinked repository paths, including
existing main success, interruption and cleanup-failure assertions. Add an
explicit classification regression proving the namespace class is not marked
skipped when loaded with Linux admission; an unavailable Linux namespace must
produce a failure rather than a skipped success. Linux runs
must execute the real namespace tests under the existing trusted helper
contract; a local environment lacking that prerequisite is a reported failure,
not a waiver. Run the full tools suite and managed proof with unchanged budgets,
then require native Linux AMD64, Linux ARM64 and macOS ARM64 results. Publish
sanitized actual counts, skips, exit codes, cleanup/home checks and unresolved
failures; do not assume historical counts remain fixed after new regressions.

Obtain independent Claude code review against this amendment and incorporate
useful findings before publication. A successful fixture repair does not prove
production bootstrap readiness, unified-daemon support or resolution of the
separate historical macOS native sandbox SIGABRT. All existing native acceptance
gates remain in force.

## Required independent review conditions

1. One real shared fixture owns listener/thread/daemon/proc setup for actual
   namespace execution and portable injected-failure regressions.
2. Inject second-thread start failure; first worker terminates, both sockets
   close, no daemon starts.
3. Inject second listener acquisition/setup failure; first socket closes and
   no worker starts.
4. Register cleanup from acquisition, stop/join before worker start and before
   socket closure. Attempt every cleanup even if one fails; preserve original
   exceptions. Keep workers non-daemon and joins bounded with liveness checks.
5. The subprocess runs the portable injected path, using fixed executable flags,
   code and test id, absolute tools path, and checks expected error plus summary.
   Keep it in the owner's group; always kill/reap its PID in finally.
6. Fixed-reason `skipUnless(sys.platform.startswith('linux'), ...)` class
   admission; regression checks both classifications; no Linux runtime skip.
7. Module-local smoke.sys only; add WNOWAIT only if missing and restore it; mock
   Processes/exchange/Popen and assert no real waitid call.
8. Portable simulated-Darwin and actual Darwin-only refusal cases preserve
   production admission, handlers and umask, complete cleanup, and spawn or
   allocate nothing.
9. Report actual changed counts/skips and unmet prerequisites; no Linux waiver.

## Implementation checkpoint before independent code review

Only the two scoped test files implement this amendment. The shared real fixture
owns acquisition and teardown; portable regressions exercise that helper rather
than a copy. A fixed same-group subprocess verifies the injected failure case
finishes unittest and exits. Linux admission is required and separately checked
against the actual class definition; production Darwin refusal remains intact.

Local Linux checks on the integrated source: affected whole modules passed from
both normal and symlinked repository paths, **48 tests / 1 skip** each; the real
Linux namespace tests executed successfully. Full tools discovery passed
**505 tests / 8 skips** in 21.414 seconds. `git diff --check` passed. The extra
Linux skip is the actual-Darwin refusal case, not a namespace waiver. Native
macOS/ARM64 checks and coordinated managed packaging proof remain pending.

Ripwire quality delta reports required fixture-assertion verbosity and historical
short-horizon churn as gating findings, plus new test/helper structural findings.
The post-refinement run exits 2 with 25 rows (3 gating, 19 new-symbol, 3 minor);
these are reported for independent review rather than suppressed or treated as
a passing quality check. This initial checkpoint preceded the final narrow
code approval recorded below and grants no native acceptance.

The initial independent integrated code review (`claude-opus-5-5`) approved
publication with no blockers and independently repeated 47/1 normal/symlink and
504/8 full-suite checks. Its temporary-copy mutation removing worker cleanup
made four regressions fail. Before publication, its useful finding was adopted:
the fixture now tracks its own exception-in-flight explicitly, so an unrelated
enclosing `except` cannot suppress a cleanup failure after normal yield. A new
regression checks propagation plus worker/listener cleanup in that situation.
Narrow independent re-review of this refinement APPROVED publication.

Local [Trial18 managed proof](../validation/managed-runtime-selfhosted-trial18.md)
passed all ten assertions, 505 tools tests with 8 skips, enforced offline network
isolation, cleanup and monitored home checks. Native three-platform acceptance
remains pending; this completed reviewed increment does not close that gate.
