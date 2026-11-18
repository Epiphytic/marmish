# Peer host sampling calibration

This opt-in native proof evaluates local sources for an eventual encrypted
Peer heartbeat. It has no daemon, wire, storage, scheduling or publication
behavior. Source coverage and native measurements are reported separately.
The full heartbeat design must independently establish the authenticated
Peer audience, policy and relay enforcement.

- Read the effective account using `geteuid` and bounded `getpwuid_r`
  lookups. Validate the returned username and discard invalid, incomplete
  or overflowing results. Check that environment variables cannot stand
  in for a native result.
- Use Linux `CLOCK_BOOTTIME`. On macOS compare the continuous and
  wall-derived boot clocks across operator-assisted suspend. Report the
  comparison only when observations permit a qualified claim.
- Collect three finite nonnegative load averages. Convert to bounded
  fixed point. Measure disk use on only the selected filesystem with
  checked arithmetic and no path in output.
- Select exact interface names and IP families before reading address
  bodies. Enforce the native address extent and result cap and always
  release the list. The report prints no addresses or username.
- Give the collector's disposable child a deadline independent of its
  worker. A synthetic stall exercises process termination and reaping.
  Production IPC responsiveness and worker recovery require separate
  qualification.
- Report the sample count, p50 and p95 for each source and the combined
  loop. Keep timing thresholds and the heavy proof out of routine CI.

See the [validation results](../validation/peer-host-sampling-proof.md).
