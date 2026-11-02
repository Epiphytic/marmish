# Installed matrix attempt and setup measurements

[Run 41397651278, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41397651278/attempts/1)
failed before any remote command. All three platform runners were assigned
within five minutes of dispatch, as recorded in the closed public
[run metadata](run-summary.json). The [generated matrix](nine-pair.md) and
[machine-readable rollup](nine-pair.json) preserve nine unverified cells with
null metrics and zero transport samples. These are missing command evidence,
not nine observed command failures or zero-latency observations.

The actual installer used experimental.2, application source
`f609b6ccdc9302aeec661a2ae3692d3ef68b86fa`, under harness
`9a694963c253071900532d3d1fc9f5489d82d878`. Later master fixes are not validated
by this archive. The role directories preserve the original sanitized native
installation and final cleanup reports.

| Successful setup operation | Platform | Elapsed seconds | Samples |
| --- | --- | ---: | ---: |
| Install | Linux AMD64 | 3.729040216 | 1 |
| Verify installed tree | Linux AMD64 | 0.075301633 | 1 |
| Initialize server identity | Linux AMD64 | 2.365546353 | 1 |
| Initialize client identity | Linux AMD64 | 2.307373194 | 1 |
| Install | Linux ARM64 | 6.434030637 | 1 |
| Verify installed tree | Linux ARM64 | 0.061442219 | 1 |

These are local suspend-inclusive elapsed durations from the report's nanosecond
fields. Stage-local child teardown is included; final cleanup is separate:
0.269534471 seconds on AMD64 and 0.254234278 seconds on ARM64. Each final Linux
cleanup report proves all seven checks. These single samples do not establish
percentiles, relay latency or a platform speed comparison.

Linux AMD64 initialized both identities and timed out waiting for its peers.
Linux ARM64 failed server identity initialization after 0.265866240 seconds;
the finite job diagnostic reports `namespace_start_failed`, exit 125, without
timeout or output truncation. That category does not identify the denied
operation or prove that a namespace was entered. Its failed duration is excluded
from the successful table.

macOS failed even earlier while bootstrapping Rust: the job log reports
`No space left on device` during the standard-library copy. No installation
report exists. Its cleanup record is `not-started`, with null checks and zero
cleanup duration; this does not prove cleanup of the pre-state compiler
bootstrap. The separate reviewed disk-footprint increment addresses that
boundary. No macOS setup or transport sample is reported here.

The relays were explicitly selected:
`wss://relay.eu.whitenoise.chat/` and `wss://relay.us.whitenoise.chat/`.
Successful command measurements will require exact client output, authenticated
server evidence and complete cleanup under the strict rollup, with the
definitions in [the agent specification](../../agent-spec.md).
