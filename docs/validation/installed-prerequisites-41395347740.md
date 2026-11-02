# Native installed setup measurements

[Run 41395347740](https://github.com/Epiphytic/marmish/actions/runs/41395347740)
used the real installer and published `0.1.0-experimental.2` distribution on
three native runners. All three started within five minutes. Overall acceptance
failed before any remote command; transport latency and throughput have **zero
samples**.

| Successful operation | Platform | Elapsed time | Samples |
| --- | --- | ---: | ---: |
| Install | Linux AMD64 | 3.985 s | 1 |
| Verify installed tree | Linux AMD64 | 0.074 s | 1 |
| Server identity initialization | Linux AMD64 | 2.362 s | 1 |
| Client identity initialization | Linux AMD64 | 2.316 s | 1 |
| Install | Linux ARM64 | 5.820 s | 1 |
| Verify installed tree | Linux ARM64 | 0.063 s | 1 |

These are individual setup-stage measurements. Bounded child-process teardown
inside a stage is included; final installation/state cleanup is timed separately
(219.304 ms on Linux AMD64 and 219.778 ms on Linux ARM64). They do not
measure relay latency or establish platform performance differences. Each stage
uses local suspend-inclusive elapsed time, recorded in nanoseconds alongside
monotonic-clock checks. No percentile can be inferred from one sample.

Linux AMD64 reached the identity barrier, then timed out because the other peers
were unavailable. Linux ARM64 failed server identity initialization after 254 ms;
the underlying CLI error was discarded by that harness revision, so its cause
is unknown. Both Linux jobs proved all seven final cleanup checks. macOS failed
during installation after 5.183 s; owned process absence checks passed, but
uninstall and root removal were not proven. Failed-operation durations are not
included in the successful setup table.

The [machine-readable evidence](installed-prerequisites-41395347740.json) records
exact harness/application commits, installer/archive/manifest digests, interpreter
versions, explicit relays, raw timing fields and final cleanup outcomes. The
subsequent [Darwin cleanup fix](../reviews/macos-zombie-group-code-final.json)
is independently reviewed; this earlier run does not validate that fix.
