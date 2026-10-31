# Managed fixture portability: integrated code review

Independent Claude Code, actual model `claude-opus-5-5`, auto permission mode,
**APPROVED publication** of the test-only repair against HEAD `20c76f1`.
Scope: the two test modules, phase/design-review documents and agent-spec status.
The Beads export was tracker-only and will not be staged. Production, helpers,
harness, parser, deadlines, workflow and Linux acceptance gates are unchanged.
Initial-review source SHA256 values:

- sandbox tests: `b6eff716704b61dc40f78c22bd12e3b97b5d46b84b08b91fcac51a3fe966eb50`.
- smoke tests: `c635d8642e2a77b94099a54e255f646b934802a90db724760d95ffb5491081c7`.

The reviewer confirmed all nine design conditions: shared actual fixture and
injected regressions, immediate resource ownership, partial listener/thread
startup cleanup, bounded non-daemon joins, original-exception preservation,
same-group bounded regression child, Linux-only classification with required
Linux namespace checks, module-local smoke admission simulation and unchanged
simulated/actual Darwin refusal contracts.

Actual independent validation on Linux/Python 3.12.3:

- Both affected modules normally and through a symlink: 47 tests, 1 skip each;
  both actual Linux namespace cases executed and passed.
- Full tools suite: 504 tests, 8 skips, passed in 21.4 seconds.
- Temporary-copy mutation removing stop/join registration made four relevant
  regressions fail; the pre-thread listener-failure case correctly still passed.
- A forced-Darwin-on-Linux probe exited promptly with namespace cases skipped,
  but had four errors because continuous_time tried to load Darwin libSystem.
  That simulation is neither a native pass nor evidence of another fixture bug.

No source edits, commits, network, CI, native macOS or administrative actions
were performed by the reviewer. Raw local evidence is
`/tmp/marmish-managed-fixture-code.raw.json`; it is not a committed raw log.
This code approval supplies no native acceptance or implementation authority.
The reviewer separately retained its human-approval caveat; governing task
instructions determine authorization, not this record.

## Useful follow-up before final review

The fixture used sys.exc_info in finally to detect an in-flight error. Entering
it from an unrelated except block can falsely suppress cleanup errors after a
successful yield. Replace that inference with explicit exception state and add
a regression for the enclosing-except case. The required narrow post-fix review
is recorded below; the initial approval does not cover later source edits.

The phase also needs to name the added idempotent daemon.close cleanup in the
startup-failure test. The indirect child success marker is supported by an
in-child identity assertion. The AST classification check is unusual but
functional; it is not a native execution claim.

Root quality check against `20c76f1` exited 2: 23 findings, three gating findings
(two short-horizon churn findings in the repaired namespace class/method and
existing EntrypointTests growth from 167 to 178 lines), three minor and 17
new-symbol findings. The latter include helper complexity/length, fixture-class
length and unittest/callback discovery flags. These are disclosed, not suppressed
or erased by resetting a baseline; no clean-quality-gate claim is made.

## Review mutation supervision

During the narrow follow-up, a temporary-copy mutation batch intentionally
stopped cleanup at the first error. Its unittest process remained alive for
about three minutes after the summary because the mutation leaked non-daemon
workers. Root verified ownership and sent SIGTERM only to that mutation Python
process. The review CLI and integrated source were untouched. This termination
was supervisor-enforced, not a naturally bounded mutation result or a source-test
pass. Final review must distinguish it from ordinary successful source tests.

## Final narrow review: APPROVED

Actual independent `claude-opus-5-5`, auto permission mode, explicitly APPROVED
publication of the final test code, with no blockers. Final sandbox SHA256 is
`bfd8da1b74c006b92f3061423f93d385346f2fd1c250bf28c0444caaf89ae61f`;
smoke remains `c635d8642e2a77b94099a54e255f646b934802a90db724760d95ffb5491081c7`.
The reviewer verified both hashes unchanged before/after its checks and made no
source edits. Raw local evidence: `/tmp/marmish-managed-fixture-code2.raw.json`.

The explicit fixture-local exception flag resolves the useful initial finding.
Cleanup attempts every action, preserves the original exception object, and
raises cleanup failure after clean yield even inside an unrelated except. The
added regression covers that case. The phase now documents the extra idempotent
startup-failure daemon cleanup.

Actual independent checks: affected modules normally and through a symlink each
passed 48 tests with 1 skip; full tools discovery passed 505 tests with 8 skips
in 21.8 seconds; diff check passed. Temporary-copy mutations reverting ambient
exception inference, always raising cleanup error, never raising it, and stopping
cleanup early were detected by the intended regressions. The last mutation's
process required the root intervention recorded above: detection by assertions
is distinct from natural process termination. No claim of bounded mutation
ownership follows from that batch.

The reviewer did not run managed packaging, native macOS/ARM64, CI, network,
administrative or keychain checks and made no commits. Approval is test-code
publication only; native acceptance and the historical configured SIGABRT remain
unresolved. Final source quality counts increased to 25 findings, three gating,
three minor and 19 new-symbol findings; the same churn/class-growth disclosures
apply without suppression.

Final root structural checks against `20c76f1` confirmed quality exit 2 with the
25/3-gating/3-minor/19-new counts above. PR-context exited 0 and mapped 26 sandbox
and 12 smoke affected tests. Its broad name-based impact was capped for sandbox
(611 dependent symbols across 52 files); that is structural signal, not a claim
of runtime propagation. Actual 48-test affected and 505-test full runs provide
the executable validation.

Final independent evidence review, actual `claude-opus-5-5` in auto mode,
APPROVED publication of the eight staged paths. It verified Trial18's exact
bytes/hash and report facts, current source hashes, both code approvals, quality
and structural disclosures, prior native facts, privacy and the mutation
intervention. It ran no tests, network or native commands. The report itself
contains no source hashes; the companion records the tested working-tree hashes.
Raw local evidence: `/tmp/marmish-managed-fixture-evidence.raw.json`.
