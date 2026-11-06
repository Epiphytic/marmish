# Native stream qualification harness review

Scope: `tools/marmish_stream_runtime.py` and its stdlib unit tests in
`tools/test_marmish_stream_runtime.py`. This review covers qualification tooling,
not production runtime code or live relay validation. Root coordinates those
independently. Python is test tooling only; the product remains native Rust.

## Round 1

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Reviewer ran six local hermetic tests, py_compile
and help successfully. Mutation checks on a disposable copy independently proved
the daemon-lifetime, exit-status, forced-kill and ESRCH checks detect regressions;
the copy was removed. No production binaries, Cargo, network or live relay ran.

Dispositions:

- Added a main wiring regression: unconfirmed cleanup yields exit 1, failed report,
  retained private state, and no rmtree. Intentional client exits 7/130 remain valid.
- Declined the suggested post-reap numeric PGID probe/kill and cwd/environment
  process sweep. A reaped leader no longer anchors process-group identity; a group
  can disappear and its number be reused between probe and signal. A marker also
  does not establish process ownership. Remote execution uses independently owned
  groups. The report explicitly scopes cleanup to owned daemon shutdown plus the
  separately tested protocol-confirmed cancellation, not proof that arbitrary
  escaped descendants are absent.
- Relay reporting now contains origin only and a path-redaction flag; arbitrary
  token paths remain exclusively in private configuration. A regression checks it.
- Daemon environment is an allowlist with private HOME/XDG/TMPDIR. Ambient tokens
  and preload variables are excluded. A regression checks it.
- Main-thread spawn temporarily defers raising INT/TERM handlers until Popen is
  registered, restores the originals and replays pending signals. No blocked mask
  or pre-exec callback enters the child. A regression injects TERM during Popen.
- Default report passed=False prevents incomplete error handling claiming success.
- Daemon role is an explicit spawn argument. Group permission failure attempts
  owned-child kill/reap, always fails qualification, and has a regression.
- Final report is written before private state removal; report-write failure
  preserves diagnostics and returns nonzero, covered by another regression.
  A filesystem rejecting writes cannot be made to replace an old report; consumers
  must require successful process exit, not trust a preexisting artifact alone.

Author validation after these changes: 11 hermetic unittest methods pass;
py_compile and help pass. The tests execute no real subprocesses/keys/network.

## Round 2

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **APPROVE**, no blocking findings. Reviewer agreed declining unsafe
post-reap group signals is correct and the explicit cleanup scope is honest.
Main wiring and report-retention regressions are meaningful; additional disposable
mutation checks caught ignored cleanup failure, unconditional rmtree, omitted
signal replay and omitted owned-child kill fallback.

Reviewer ran 11 unittest methods, AST parsing, help and an independent NIP-19
reference-vector check successfully, using no production binaries, Cargo, network
or live relay. Temporary mutation copies were removed. Useful low follow-up:
if private-state removal fails and rewriting the failure report also fails, remove
the newly written contradictory passing report best-effort. Implemented with a
12th regression test; author unittest/py_compile pass.

## Round 3

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **APPROVE**; round-2 approval stands, with no blocking findings.
The report-unlink fix and real-main regression correctly prevent a freshly written
passing artifact surviving a failed cleanup rewrite whenever unlink succeeds.
Residual filesystem failure still requires consumers to check process status.

Reviewer ran py_compile, all 12 unittest methods via discovery, and help
successfully. No production binaries, Cargo, network or live relay ran, and
reviewer made no edits or commits. Direct dotted-module unittest invocation is
not supported by this tools-directory layout; use the verified command:

```sh
python3 -m unittest discover -s tools -p 'test_marmish_stream_runtime.py' -v
```

Final author-owned changes are restricted to the harness and its tests; this
record is sanitized review evidence. Production runtime approval and actual live
qualification remain separately scoped evidence.

## Round 4: early stdin closure qualification

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **APPROVE**, no blockers. Independently reviewed the new
`early_stdin_closure`, main wiring, and hermetic regressions. Reviewer ran all
14 unittest methods successfully, py_compile for both files, and help. No Cargo,
builds, production binaries, keys, live relay or network tests ran.

The live harness now offers at least 270336 all-byte binary bytes to both
`/usr/bin/true` and `/usr/bin/head -c 1`, using none and zlib compression. It requires
natural exit 0 and exact empty stdout or one NUL byte, respectively. Payloads
exceed the 256 KiB logical stdin window even with `--bytes 1`; compressed wire size
is deliberately separate from logical flow-control credit. Existing bounded
communicate waits, owned subprocess registration and private diagnostics apply.
Regressions check the four-case matrix and compression placement, binary input,
exit/output mismatches, and real-main stage wiring.

Nonblocking suggestions were more granular assertions for exact argv, failure
stage, larger configured payload and main stage ordering. The source uses literal
argv, existing validated byte bounds and established stage ordering; these were
not blockers and no further source changes were made after approval. Fixed
`/usr/bin/true` and `/usr/bin/head` paths target the Linux/macOS qualification
matrix, matching existing `/usr/bin/printf` fixtures. This records harness
readiness only: actual early-closure live results must be obtained separately.

## Round 5: readiness status regression

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **APPROVE**, no blockers. Independently inspected the exact Rust status
response contracts and the harness readiness fix, then ran all 18 hermetic tests,
py_compile for both files, and help successfully. No Cargo, builds, production
binaries, network or live tests ran. Reviewer made no edits or commits.

The preceding live attempt failed before stream traffic: status returned exit 0
while its JSON state was still `activating`. Exit success alone no longer implies
readiness. The harness requires the exact successful status envelope, `active`
state, bounded nonempty identity and three nonnegative integer counters. Known
`activating` and `transport_degraded` snapshots remain not ready; malformed
successful status fails with the finite `status_schema` category. Four new tests
cover activating/degraded versus active, actual startup polling for both owned
daemons, malformed schemas/counters, and the existing bounded readiness deadline.
The failed live run is evidence of the discovered bug, not successful validation;
a new actual client/server run remains required.

Nonblocking dispositions: the existing readiness deadline remains 90 seconds,
shorter than the daemon's 150-second activation budget; slow activation can fail
qualification and the limitation is explicit. Each status probe is capped at
20 seconds; a hung probe fails immediately rather than consuming further retries.
Additional schema-edge assertions and a less call-count-sensitive fake clock
could extend coverage, but the independent review found no blocking gap.

## Limits

Cleanup checks owned daemon shutdown and actual protocol-confirmed cancellation;
it does not prove arbitrary escaped descendants absent. A signal during private
root construction (before children/keys exist), SIGKILL, or a narrow Python signal
window may leave local private state. All-report-filesystem failures may leave an
older artifact; consumers must require process exit 0. Per-command timeouts bound
individual stages rather than promising a single whole-run timeout. Hermetic
Python tests complement, and never replace, the separately recorded live runtime
qualification and production Rust review.
