# Keyless Linux namespace primitive localization

## Evidence and purpose

Native run41401305306, source61a13adfad02dec39c20dd12e20224ed2d9e3bc9,
job125133510631, started3seconds after dispatch, passed authenticated Bubblewrap
bootstrap, failed first combined unshare stage with rc1/reasonnonzero_exit and
passed owned cleanup. Neither exact English permission line matched. Sanitized
facts are docs/validation/linux-namespace-arm-41401305306.json. No artifact was
uploaded; finite JSON came from the job log. This proves configuration failure,
and does not prove a permission denial, syscall, sysctl or host-policy cause.

Avoid guessing additional English messages in serial increments. Localize the
first failing primitive and remove the locale ambiguity using the same bounded
keyless diagnostic. Parent authorizes this scope; publication/dispatch remain
centrally coordinated around the runtime-release freeze.

## Scope and contract

Only change diagnostic_probe in tools/marmish_ci_bwrap.py and its unit tests,
plus this design, sanitized evidence and actual reviews. Existing workflow,
run helper/ToolFailure privacy, production transport and binary release unchanged.
Use the already published manual ARM diagnostic workflow. No privilege, host
policy, sysctl, package, source or fallback change.

Build one explicit diagnostic child environment from existing build_environment,
then set LANG=C and LC_ALL=C. All other existing entries remain unchanged;
never mutate process environment. This is diagnostic-only, not a claim of exact
installed runtime environment. The inherited API-token refusal stays first.

In order, run these six fixed stages, stopping after the first failure:

1. control: /usr/bin/true
2. user_namespace: /usr/bin/unshare --user /usr/bin/true
3. root_mapping: /usr/bin/unshare --user --map-root-user /usr/bin/true
4. unshare: /usr/bin/unshare --user --map-root-user --net /usr/bin/true
5. one_bwrap: existing combined unshare prefix + existing one Bubblewrap +true
6. two_bwrap: existing combined unshare prefix + existing two Bubblewrap +true

The last three commands remain byte-for-byte the prior diagnostic commands,
and two_bwrap remains the original probe. Each uses existing private temporary
root and bounded runner, now timeout10seconds per stage. Six worst-case slots
of10seconds workload plus5seconds existing reap reserve total90seconds,
leaving30seconds within the existing two-minute step for setup/report/temp-root
teardown. The workflow's separate three-minute owned-prefix cleanup always
runs. Keep existing job bound18minutes; do not extend live workloads.

Stage rows and top-level schema/categories stay unchanged. All nonzero statuses
remain failures. Extend the existing permission_denied mapping only to the
three unshare primitive stages user_namespace/root_mapping/unshare when rc is
exactly int1 and ToolFailure.permission_denied is exactly True. These are the
same absolute unshare binary and existing source-verified whole-line matcher;
no new message classifier, substring or dynamic path matching. control and
Bubblewrap stages cannot become permission_denied. Other reasons and numeric
bounds remain unchanged; raw output never enters report/exception attributes.

A passing control proves direct execution works. A failing user_namespace
localizes the tested failure to launching/executing under user-only unshare;
without a recognized syscall diagnostic it does not prove namespace creation
itself failed. A passing user_namespace then failing root_mapping isolates the
additional mapping configuration. Passing root_mapping then failing unshare
isolates the added network namespace configuration. Each test is a fresh process,
so transient runner-state differences remain possible. Recognized permission
output proves the utility reported that failure, never a specific host policy.
A fully passing set permits later installed-stage diagnostics, not an E2E pass.

## Validation and review

Offline mocked tests assert exact six argv, order, timeout10 and explicit
C-locale environment with other fields retained and parent environment unchanged;
stop-first-failure at every position; mapping remains finite and restricted to
unshare primitive stages; existing direct probe contract unchanged. Adjust
prior diagnostic tests for new stage order/count. Test cleanup failures preserve
stage evidence. Affected helper/workflow/managed-workflow/release suites must
pass. Actual Claude design review also checks current native evidence against
private finite job facts; independent auto code review follows implementation.
No native work, network or CI during reviews; no commits/pushes by this agent.

Final validation:61tests and106subtests passed across helper/workflow/managed
workflow/release suites; py_compile and scoped diff --check passed. Actual Claude
design, code and native-evidence reviews approved. Private API metadata directly
links job125133510631 to run41401305306, attempt1/source61a13adf via run metadata.
No native execution of the new six-stage variant has happened yet. No inferred
host-policy cause and no product support/E2E claim follows from these tests.
