# Additive command-policy validation

The increment adds cumulative default and per-key executable restrictions. It
remains a Linux, single-device command prototype; macOS work is a separate phase.

## Results

- Real Marmot transport: **14/14 assertions passed in 207.696 seconds** using
  fresh disposable server/client identities and explicitly selected White Noise
  EU and US relays. No public relay default was added. Private state was cleaned.
- Defaults deny `/usr/bin/false`, the client-specific rule denies
  `/usr/bin/true`, bare PATH execution is refused, and explicit shell execution
  is refused despite `--allow-shell`. Every denied submission leaves no job.
  The same token and invocation ID then successfully execute allowed Python.
- Literal spaces, Unicode, quotes, newline, dollar signs, backticks, leading
  dash, empty and non-UTF-8 argv; binary stdin/stdout/stderr; nonzero exit;
  cancellation; duplicate/conflicting submissions; unauthorized rooms;
  disconnect, restart and replay checks all passed.
- Offline checks: **109 Rust tests**, **87 Python tests**, workspace clippy with
  warnings denied, workspace formatting, Python compilation and workspace build
  passed. SQLCipher negative tests intentionally emit wrong-key diagnostics.
- The live run preceded extraction of its unchanged policy assertions into
  `Harness.check_command_policy`; the extraction received a follow-up review.

Machine-readable evidence: [live report](command-policy-e2e.json). Independent
[code approval](../reviews/command-policy-code-final.json) includes reviewer-run
local tests. [Design approval](../reviews/command-policy-plan-final.json) follows
resolution of the [initial findings](../reviews/command-policy-plan-initial.json).

## Quality check disposition

`ripwire . --quality-delta` reports four gating structural findings, rather than
an all-green quality result. The normalized duplication of two unrelated small
boolean helpers is not a useful abstraction; repeated unittest temporary-file
setup is intentional. The server constructor adds one optional keyword policy
argument to preserve existing callers. The E2E harness grows to exercise the new
contract; its policy checks have a dedicated method. Other new-symbol findings
include test-discovery and serde false positives, matcher complexity and explicit
persistence/recovery test cases. No behavior was removed to reduce a score.

## Limits

Patterns select canonical executable paths, not arguments or program behavior.
Allowing interpreters grants their execution capabilities. Canonicalization does
not pin an inode or eliminate replacement races; launch checks refuse observed
aliases and symlink replacement. Large policies can exhaust the bounded matcher
and deny valid requests. Existing accepted authority survives policy tightening.
The tested MDK artifact's exact source provenance is still unverified. Relay
retention is best effort. No macOS or cross-platform success is claimed here.
