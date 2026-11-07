# Native copy live harness review

Actual Claude Code Sonnet 5.5, `--permission-mode auto`, round 1 **APPROVE**,
session `ca961e4a-93de-4d15-882d-b28c2fb2fc86`, 114.384 seconds, 14 turns.
Read-only review; no tests or live commands run by the reviewer.

Scope: `tools/marmish_copy_runtime.py` and reused stream harness ownership,
cleanup and sanitization, as a same-host native/Marmot baseline. This approval
expressly excludes live interrupted resume, cancellation, GC, installer and
cross-platform qualification, and is not complete phase approval.

The reviewer checked exact parser invocation, independent byte/hash/metadata
oracles, literal path arguments, failure assertions, bounded workloads, owned
process cleanup and finite sanitized reporting. No blocking findings.

Useful recommendations incorporated after review: capture expected source bytes
before executing the transfer; reject mutations of source and intermediate
files; compare timestamp to the fixed fixture constant; require exit 1 for the
expected file failures; verify missing-source cases create no destination;
report actual payload size including the 2 MiB floor; categorize a missing
successful destination; remove the unused import. Runtime validation remains
pending and is recorded separately when executed.

Later rounds are recorded in the [integrated review record](streaming-copy-integrated-code.md#relay-harness-followup):
codec observation, compressible payloads, stronger failure assertions and private
diagnostics were reviewed and approved. The old forced-size floor was replaced
by a 2 MiB+8192 default with explicit smaller smoke sizes, which are labelled in
evidence. Neither approval substitutes for actual relay or installed tests.
