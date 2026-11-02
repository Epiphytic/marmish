# Native production-profile startup check

[Run 41399133227](https://github.com/Epiphytic/marmish/actions/runs/41399133227)
tested source commit `6f6d42c60c73bc5ef2f8e2f6e61b17216c14b0a5` on macOS ARM64.
The workflow conclusion is **failure**. The `loader-matrix` job succeeded; the
`diagnostic` job failed only at its owned-denial-candidate step. These statements
are distinct from the successful startup observations below.

| Uploaded report | Observation | Report ok |
| --- | --- | --- |
| [Confined probe](macos-confined-probe-diagnostic.json) | All five cases exit 0, without signals or timeouts | true |
| [Crash context](macos-owned-probe-crash-context.json) | Control and confined read exit 0; context is `not_sigabrt` | true |
| [Denial candidate](macos-owned-probe-denial-candidate.json) | Both reads exit 0; `not_sigabrt`, with no denial query attempted | false |
| [Historical loader matrix](macos-loader-matrix.json) | Old baseline aborts; all and only root-containing variants pass | true |

The normal confined-probe cases are unconfined `/dev/null` read, confined read,
unconfined selected-loopback TCP, confined selected-loopback TCP, and confined
`/usr/bin/true`. Each completed with exit 0 and empty stderr using the current
production profile, SHA256
`28cec5d90d4610b27c11f9c064978ab92cba051631c7a991bb27f74b335ccc68`.
This confirms that the earlier startup SIGABRT no longer occurs for those
synthetic workloads on this runner. The crash diagnostic also observed a successful
read rather than an abort. It listed the crash-report directory for its initial
snapshot but did not open crash-file contents.

The denial-candidate diagnostic was built to attribute a SIGABRT. It deliberately
keeps `ok=false` on all paths, including when the confined read does not abort;
its `not_sigabrt` result
caused the job and workflow to fail despite successful probe execution. Its
`failure` is null, `query_exit_code` is null, and the denial list is empty because
no query was attempted. This is neither an actual denial nor evidence that a
performed denial query found nothing. The diagnostic behavior is recorded as-is;
this evidence increment does not change its pass criteria.

The matrix uses its explicitly pinned historical profile
`14b1ab31a589f3f52b7b2ff93fadb35d26139c1a5cb154a0fdb50ed90a310d9a`,
not the current production profile as its baseline. It reproduces five SIGABRTs
for each non-root combination. All root-containing combinations complete exact
exits `[0,0,2,2,2]`; its root-only variant is byte-identical to the current
production profile. These exits mean allowed read and selected TCP, denied outside-file read/write
and unselected TCP. Thus it independently repeats the
[earlier minimal-factor result](../macos-loader-41397862711/README.md).

The [run summary](run-summary.json) contains public run/job/step metadata from
`gh run view 41399133227 --json headSha,conclusion,url,jobs`, with per-job
`startedAt`, `completedAt` and `status` fields omitted. The four artifact JSONs
are byte-for-byte copies of the bounded, redacted artifact reports.
No identities, secrets, runtime databases or raw process logs are included.
This run did **not** execute the production daemon's full `_preflight`, MDK
startup, identity setup, installed server/client command execution, or all nine
cross-platform pairs. It supplies no command latency or throughput statistics.
Those acceptance gates remain pending. The existing
[native-validation workflow](../../../.github/workflows/native-validation.yml)
runs the full native prerequisite spike before the MDK build; whether its current
run reaches and passes that gate must be recorded separately.
