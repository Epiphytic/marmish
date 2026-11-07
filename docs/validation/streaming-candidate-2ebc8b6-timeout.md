# Candidate2ebc8b6: build-budget timeout, unqualified

Source `2ebc8b6b2489f3b63255658846b23e7fe58f855f`, manual candidate
[42408127994/1](https://github.com/Epiphytic/marmish/actions/runs/42408127994).
**FAIL / UNQUALIFIED**: the AMD64 package job exhausted its90-minute cap while
release-compiling installed-test tools. Render was skipped; no installed cohort
was dispatched. See [structured evidence](streaming-candidate-2ebc8b6-timeout.json).

| Platform | Job | GitHub result | Elapsed |
|---|---:|---|---:|
| Linux AMD64 | 128510172415 | cancelled by job budget | 5416s |
| Linux ARM64 | 128510172556 | success | 3756s |
| macOS ARM64 | 128510172583 | success | 3433s |

All runners started within six seconds of dispatch. These are job wall-clock
times including compilation/tests, not Marmot latency or transfer speed.
The AMD job started19:50:20Z and completed21:20:36Z on2026-11-07. Its failure
annotation explicitly reads “The job has exceeded the maximum execution time
of1h30m0s” (spacing normalized here). This session made no cancellation request;
no external audit log was inspected. The actual annotation and elapsed time
establish the budget expiry despite GitHub's `cancelled` conclusion label.

## Actual tests and unfinished gate

The unfiltered native/engine/stream/vault test and strict Clippy stages passed
on all three platforms. Selected top-level library results from actual logs:

| Platform | Engine | Native | Installed-feature native |
|---|---|---|---|
| Linux AMD64 | 141passed/1ignored | 630passed/3ignored,1932.94s | 843passed/3ignored,1974.74s |
| Linux ARM64 | 141passed/1ignored | 630passed/3ignored,1036.36s | 843passed/3ignored,1077.08s |
| macOS ARM64 | 148passed/1ignored | 629passed/3ignored,648.95s | 842passed/3ignored,680.41s |

These are separate, partly overlapping test invocations, not additive counts of
unique scenarios. Existing ignored tests remain ignored. The logs also contain
integration/binary/doc targets and exact-test selfexec children; child summaries
must not be added to their parents again. Platform count differences are
observations, not an independently established explanation of platform behavior.
The workflow asserts non-root execution before testing.

AMD installed-feature Clippy completed21:20:03.202Z, followed by release-tool
compilation through21:20:33.093Z. Timeout cleanup then terminated cargo/rustc/cc
children. No failed-test result or compiler-error finding was identified in the
log. This does not prove the interrupted release build would succeed. Its
package/archive stages had passed, but its tool preservation/upload gates had
not completed. Successful individual stages and the two ARM jobs cannot qualify
an incomplete immutable candidate.

The [approved build-budget correction](../phases/streaming-candidate-build-budget.md)
raises only the package-job cap to120minutes. The successful baseline's
10m40 release-tool build, added to actual progress before timeout, estimates
about100m23 total plus final copy/upload steps. This is an uncontrolled point
estimate, not a runtime or performance guarantee. The earlier94m20 projection
underestimated the subsequently observed feature-suite duration and is retained
as superseded analysis in the design. A new reviewed source SHA needs a fresh
three-platform candidate and all eight unchanged installed cohorts. No assets
or cells from this attempt transfer into that qualification tuple.

## Evidence and limits

Actual full log hashes are in the structured evidence. Private paths are under
`/tmp/marmish-release-testing-2ebc8b6/`; raw logs and API payloads are not committed.
The actual independent design review verified the timeout and baseline timing
comparison; see [the review](../reviews/streaming-candidate-build-budget.md).
Separate focused [macOS diagnostic evidence](macos-worker-cleanup-hosted.json)
passed at this same source but exercises no installer or relay. It does not
qualify this failed candidate or transfer to a new source by name.

The separate automatic legacy workflow
[42406466335](https://github.com/Epiphytic/marmish/actions/runs/42406466335)
passed its three legacy-installer jobs but failed all three legacy managed-package
jobs at `full_tools`. Its reports do not retain exception traces, so those
failures are not all classified as preexisting or explained here. The two new
legacy worker source/destination mapping checks passed locally; the obsolete
Python-installer extraction test has a separately reproduced baseline failure
tracked by `marmish-5j0`. No claim that all repository CI is green follows from
the passing native tests. This workflow's failures are distinct from the actual
native candidate budget expiry and remain visible for follow-up.

There is no installed client/server traffic, cancellation/resume validation,
network latency or goodput from this candidate. Actual installer qualification
and the original workloads/deadlines/acceptance remain required. No version,
tag, public release or installer pin was changed; public installation continues
to select v0.2.3. Beads tracking remains partially unavailable and the task open;
no database migration or bypass was used.
