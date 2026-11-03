# Finite native owner outcome validation

This fixture-only increment changes no native Rust transport, deadline, route or
readiness requirement. It preserves a strictly bounded finite outcome before
private state is deleted. Phase 1 hosted acceptance remains open.

The implementation agent and actual independent Claude Code Opus 5.5 reviewer
each passed the final full `bash scripts/test-v02-owner-diagnostics.sh` suite and
a mutation check: removing only the reader's stderr redirection makes the suite
exit 1 at its leak assertion. The real fixture exits 0. An earlier root suite
run also passed, before the final assertion correction; it does not establish
that those previously unenforced assertions worked. See
[r2's finding](../../reviews/v0.2-native-outcome-code-r2.md) and
[the final r3 approval](../../reviews/v0.2-native-outcome-code-r3.md).

Bash syntax, actionlint for `v02-native.yml`, and diff whitespace checks pass.
Shellcheck is unavailable. No Cargo, live transport or provider test was repeated
for this shell/jq-only change. Actual off-route serde writer bytes are embedded
in the suite: they are rejected as-is, then accepted with only the route literal
normalized. This verifies framing, not a real Tor outcome.

Final `ripwire scripts --quality-delta` exits 2: one 155-token overlap between
the existing log extractor and new native-report extractor, plus one minor
one-line increase in `serve`. The helpers intentionally retain different parsing
contracts: bounded independent log lines versus a bounded exact whole report.
The reviewer accepted that separation. Repeated test jq invocation was
consolidated. No baseline or suppression was added.

The exact staged source and ignore rules were audited: only source, design,
review and sanitized finite evidence are included. Identity keys, provider data,
private profiles, SQLCipher databases, Tor state, logs and build products remain
excluded. Earlier hosted reports keep their original schema and failed status;
the new field cannot reconstruct outcomes that were already discarded.
