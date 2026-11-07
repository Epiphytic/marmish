# Installed streaming socket correction review

Baseline `78e39ee0301a58bbe2ed0da1187e58ef74f01ff0`, Beads
`marmish-8ka.5.5`. Scope is the [bounded design](../phases/streaming-installed-socket.md),
the installed qualifier and its local regressions. Product behavior and release
pins are outside this correction.

## Independent design review

Actual Claude Code, Claude Sonnet 5.5, `--permission-mode auto`, session
`23b91b9e-0e94-4f92-a337-7055a4a8947b`.

- Round 1: **REQUEST_CHANGES**, 84.813 seconds, 16 turns. The reviewer inspected
  the relevant product and qualifier source without edits or builds. It found
  that the proposed reused helper incorrectly accepts sixteen-character IDs,
  while real product IDs contain thirty-two hex characters. It also required
  the stricter product runtime-path bound and a mandatory subprocess regression
  comparing real product socket identity and authenticated Status under a child
  temporary root distinct from the parent's.
- Round 2: **APPROVE**, 10.981 seconds, two turns, after those requirements were
  added explicitly. No edits or builds. The older finite qualifier endpoint
  defect is separately tracked as `marmish-8ka.5.6`, without a pass claim.

## Evidence before correction

[Candidate 42346198320](https://github.com/Epiphytic/marmish/actions/runs/42346198320)
passed all three complete native, package and installed-tool jobs plus rendering.
The [sanitized native evidence](../validation/streaming-candidate-78e39ee.json)
records suite totals, job links and log hashes. This confirms execution of the
macOS owned-group fixture; it does not identify its natural errno branch.

The first [installed matrix](https://github.com/Epiphytic/marmish/actions/runs/42350381517)
failed on all three platforms at the readiness deadline. The
[failure evidence](../validation/streaming-installed-socket-failure.json)
preserves incomplete server/stream stop proofs. Those flags are not proof that
a process remained alive or that physical cleanup completed. No remote test
command was reached by the driver before readiness failed.

Two Linux AMD64 reproductions used the actual downloaded candidate and its
pinned installer, disposable file-backed identities and explicit US Whitenoise
relay selection. The first observed an owned live daemon answering `activating`
at its standard child-TMPDIR socket while the requested decoy was absent. The
second observed `activating` followed by `active` at that same standard address.
Both stopped through SIGTERM with exit zero, empty stderr and true
engine/MDK/runtime stop reports; their private profiles were removed. They do
not qualify cross-platform command execution or the repaired qualifier.

## Independent integrated code review

Actual Claude Code, Claude Sonnet 5.5, `--permission-mode auto`, session
`e06ae313-1663-4420-8a57-16fa6b2dea14`: **APPROVE**, 144.119 seconds,
22 turns. The reviewer inspected the complete diff and new files, made no edits,
and independently ran formatting, strict installed-feature all-target Clippy,
the four socket regressions six times without a failure, existing autoexit
(16 passed), streaming (13 passed), and configuration (27 passed) tests. Clippy
used artifacts already fresh for these sources. Both evidence files parsed.
No blocking findings remained. Optional suggestions concerned temporary-path
error detail, a harmless product-mirroring redundant bound, and watching the
held-handshake fixture for future hosted flakiness; no weakening was adopted.

The author first demonstrated RED against a real product-generated profile ID:
the old sixteen-character resolver rejected it, after which the owned child
stopped successfully. With the correction, all four focused regressions passed
in 2.60 seconds and all 153 installed-driver tests passed in 6.12 seconds.
Strict Clippy, formatting and diff checks passed. The real-daemon test checks
an authenticated activating Status, not remote command execution.

Ripwire quality delta reported test-fixture construction clones, test functions
as statically unused, the subprocess test's size, and churn in the corrected
resolver. These were inspected: fixture setup is local test code, Rust's test
harness invokes the tests, and the production change remains bounded. No broad
fixture refactor was introduced for this correction. The change-context pass
confirmed the reused helper's autoexit callers; the focused suites cover them.

Fresh hosted candidate and installed cohorts are still required. No release
version, assets, daemon security checks, deadlines or cleanup proofs changed.
