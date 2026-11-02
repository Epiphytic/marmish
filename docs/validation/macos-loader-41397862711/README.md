# Native root-directory loader matrix

[Run 41397862711](https://github.com/Epiphytic/marmish/actions/runs/41397862711),
job `125123015117`, tested commit `d0a76ce` on macOS ARM64. The bounded
[report](report.json) is copied exactly from artifact `macos-loader-matrix`.
All five unconfined controls exited zero. No successful variant timed out or
received a signal.

| Factors | Confined exit codes | Result |
| --- | --- | --- |
| baseline | -6, -6, -6, -6, -6 | SIGABRT |
| root | 0, 0, 2, 2, 2 | passed |
| map | -6, -6, -6, -6, -6 | SIGABRT |
| root + map | 0, 0, 2, 2, 2 | passed |
| cryptex | -6, -6, -6, -6, -6 | SIGABRT |
| root + cryptex | 0, 0, 2, 2, 2 | passed |
| map + cryptex | -6, -6, -6, -6, -6 | SIGABRT |
| root + map + cryptex | 0, 0, 2, 2, 2 | passed |

The ordered capabilities are allowed `/dev/null` read, allowed selected loopback
TCP, denied synthetic outside-file read, denied outside-file write, and denied
unselected loopback TCP. Negative results require the probe's exact exit 2;
a loader crash never counts as denial. The minimal passing set is the literal
root-directory read alone. Executable mapping and cryptex grants are unnecessary
for this synthetic workload and are not proposed for production.

The [production correction design](../../phases/macos-root-loader-fix.md) selects
that exact tested profile. Full native preflight, MDK startup, disposable identity
initialization, installed server/client operation and all nine cross-platform
pairs remain separate acceptance gates. This sample supplies no transport latency
or throughput measurement.
