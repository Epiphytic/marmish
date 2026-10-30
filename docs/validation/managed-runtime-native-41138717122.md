# Native packaging run 41138717122

[GitHub run](https://github.com/Epiphytic/marmish/actions/runs/41138717122),
source `8e19cdf8757b415845905fa47ef5d04e5f8da2ab`; [sanitized reports](managed-runtime-native-41138717122.json).
All six hosted jobs started within twelve seconds. Installer archive and
controlling-terminal tests passed on Linux AMD64, Linux ARM64 and macOS ARM64.
All three managed-packaging jobs failed; no acceptance gate is waived.

Both Linux targets reached the full suite: 445 tests, 7 reported skips, exit 1.
Two required tests failed on both: actual unshare/Bubblewrap handoff and the
approved bridge/decoy/capability/cleanup test. Bare unshare, direct Bubblewrap
and combined preparation probes all returned permission denial. `ip`/`sysctl`
were present. The read-only AppArmor userns-restriction value was 1; that
observation does not establish exclusive causality. Both native-help probes
on each target exited 0. Cleanup completed and monitored home metadata stayed
unchanged.

On macOS both slots returned the exact artifact-bound startup guard marker,
exit 1, 32 output bytes, and no signal. Installed help, doctor, argv fidelity,
runtime audit and signature checks passed. The full tools suite exceeded its
900-second child deadline and was terminated: `failure=child_timeout`,
`stage=full_tools`, no completed suite count. This report does not identify the
slow test or establish a confinement-crash cause. Cleanup completed and
monitored home metadata stayed unchanged. The configured macOS SIGABRT remains
an independent unresolved issue.

These reports predate the three new executable completion cases and retain
those unmet fields. Historical earlier failures remain unchanged. The approved
self-hosted path and newer source require their own actual runs. Existing
self-hosted jobs remain queued/unassigned; organization runner policy was not
observable with the available token (403), and no isolation claim follows from
workflow guards or runner labels.
