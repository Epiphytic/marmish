# Finite native owner outcome validation

This fixture-only increment changes no native Rust transport, deadline, route or
readiness requirement. It preserves a strictly bounded finite outcome before
private state is deleted. Phase 1 hosted acceptance remains open.

## Actual local onion check after publication

The unmodified committed fixture at `ce0b9411b84f2b77d295cf7963d25611c318e33f`
passed one local Linux AMD64 owned-onion run with real Arti, encrypted exchanges,
receipts and restart. Its strict eleven-key cleanup validator passed and retained
the new finite outcome. The [sanitized local report](local-ce0b941-onion.json)
labels its scope explicitly; fixture binding numbers are not published as hosted
run evidence. No retry, native deadline change or scratch report parser was used.
Canonical passwd environment and explicit private disposable state were used as
in the earlier local reproduction. All native and owned-state cleanup flags
passed. `supervisor_reaped_by_this_step` is a schema constant `false`; the supervisor
writes this report about its own relay children, which were reaped. The local
run reused the hosted run number only as a fixture parameter; it is separate
execution, not evidence from GitHub Actions.

| Measurement | Time |
| --- | ---: |
| Fixture wall time | 153 s |
| Native check including native cleanup | 131.701 s |
| Cold activation | 97.294 s |
| First A→B / B→A exchange | 1.935 / 2.314 s |
| Warm activation | 17.810 s |
| Post-restart A→B / B→A exchange | 2.523 / 1.856 s |

Each exchange carries 66 bytes. These single message/receipt observations do
not measure remote-command latency, bulk goodput or percentiles. The final native
stage is `completed`, operation `none`, category null, and all native cleanup
flags are true. This validates real outcome retention, not hosted acceptance.

## Offline checks

The implementation agent and actual independent Claude Code Opus 5.5 reviewer
each passed the final full `bash scripts/test-v02-owner-diagnostics.sh` suite and
a mutation check: removing only the reader's stderr redirection makes the suite
exit 1 at its leak assertion. The real fixture exits 0. An earlier root suite
run also passed, before the final assertion correction; it does not establish
that those previously unenforced assertions worked. See
[r2's finding](../../reviews/v0.2-native-outcome-code-r2.md) and
[the final r3 approval](../../reviews/v0.2-native-outcome-code-r3.md).

Bash syntax, actionlint for `v02-native.yml`, and diff whitespace checks pass.
Shellcheck is unavailable. No full Cargo test suite or provider test was repeated
locally for this shell/jq-only change. The later live run is reported separately
above. Actual off-route serde writer bytes are embedded
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

## Hosted run 41574835843: failed warm activation

[Attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41574835843)
used the same exact `ce0b9411b84f2b77d295cf7963d25611c318e33f` source.
The [retained owner outcome](hosted-41574835843-owner-cleanup.json) is a failed
check, independently of the local pass above. Cold activation took 24.590 s,
and the first A→B and B→A 66-byte exchanges took 1.326 s and 1.284 s.
Warm activation then exhausted its original deadline; total native elapsed time
was 220.248 s. The retained diagnostic operation is `activation`, category
`deadline`, at `restart_activation_warm`.

The finite log tail shows one restarted owner connected. The other owner's
first stream acquisition hit the connector timeout, then its second startup
turn ended with the remaining activation timeout. This identifies where the
check stopped; it does not establish the underlying Tor network cause. All
native cleanup flags and owned relay reaping/private-state removal passed.
Readiness was false. No deadline, route or acceptance requirement was relaxed.

All three platforms completed offline tests, release builds and direct encrypted
message checks before their unavailable onion-endpoint waits were cancelled.
The terminal artifacts are failed/cancelled outcomes, not completed Tor cells.
The direct reports preserve their passing subset:

| Platform / report | Total native time | Cold activation | Warm activation | A→B / B→A before restart; A→B / B→A after restart |
| --- | ---: | ---: | ---: | --- |
| [Linux AMD64](hosted-41574835843-linux-amd64-direct.json) | 16.746 s | 381 ms | 654 ms | 377 / 382 ms; 832 / 391 ms |
| [Linux ARM64](hosted-41574835843-linux-arm64-direct.json) | 11.537 s | 855 ms | 995 ms | 443 / 443 ms; 866 / 443 ms |
| [macOS](hosted-41574835843-macos-direct.json) | 9.244 s | 982 ms | 1067 ms | 501 / 438 ms; 866 / 452 ms |

Each exchange carries a 66-byte payload and verifies its application receipt. All direct
reports have `ok`, native/runtime/state cleanup true, and both directions pass
after restart. These are single small-message observations, not remote commands
or a bulk-throughput benchmark. Source/run/attempt bindings were validated
against the complete downloaded artifacts.
