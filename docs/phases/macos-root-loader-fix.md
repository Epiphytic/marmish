# Literal root read for Darwin loader startup

The diagnostic-only [matrix design](macos-loader-matrix.md) was implemented and
independently approved before [hosted run 41397862711](https://github.com/Epiphytic/marmish/actions/runs/41397862711)
on commit `d0a76ce`. Its [closed native report](../validation/macos-loader-41397862711/report.json)
records all five unconfined controls exiting zero. Every combination without
`root` ends in SIGABRT for all five confined cases. Every combination with `root`
passes exact exits `[0,0,2,2,2]`: read `/dev/null` and selected loopback TCP succeed;
synthetic outside-file reads and writes and unselected TCP are denied. No passing
case has a signal or timeout. The smallest passing set is only `root`; the other
factors are neither necessary nor proposed for production.

Change production `tools/marmish_transport_macos.py` PROFILE by appending exactly
one line, including the trailing newline:

```
(allow file-read-data (literal "/"))
```

This is the exact tested variant, SHA256
`28cec5d90d4610b27c11f9c064978ab92cba051631c7a991bb27f74b335ccc68`,
from historical base
`14b1ab31a589f3f52b7b2ff93fadb35d26139c1a5cb154a0fdb50ed90a310d9a`.
It permits reading the root directory itself (including directory entries), not
file contents under it. It does not grant root subpath reads, writes, executable
mapping, Mach services or network capabilities. Add a source comment explaining
the empirical loader requirement and link this evidence. Do not import diagnostic
permissions into production, add environment overrides, bypass startup probes, or
change MDK, native C, state, command generation, storage or cleanup contracts.
All existing positive and negative preflight checks remain mandatory and unchanged.

The diagnostic matrix must continue testing its historical A/B hypothesis after
production changes. Its fixed base is the historical profile above, not the new
production profile. Derive it from the current PROFILE by removing only the exact
trailing root line when present; accept it only if its SHA256 equals the historical
base. Otherwise raise finite `ValueError`, reported as fixture_failed, rather than
silently compare an unknown profile. No arbitrary baseline input or broad copy.
The historical base is diagnostic-only and can never select production behavior.
Report `base_profile_sha256` from this validated historical base. Default diagnostic
fixtures continue using the exact current production PROFILE. Crash and denial
diagnostics also use that current profile through their shared make_fixture. This keeps the
baseline SIGABRT reproduction gate meaningful and the passing root variant byte
identical to the production fix. Keep the fixed rules only in the diagnostic module.

Offline tests must pin exact old/new hashes and equality of production bytes to
historical base plus this one line. Verify the narrow literal rule and absence of
broad root read/write/map grants, preserve all existing startup preflight assertions,
and update matrix tests to compare against the explicit historical bytes rather
than live PROFILE. Cover historical derivation from both accepted old and new
profiles, rejection of any other changed profile, reported base digest, and the
unchanged default fixture. Run the macOS unit suites and targeted diagnostic tests;
these tests validate contracts and are not native acceptance.

Obtain actual Claude design approval before code, then actual Claude auto code
review after offline tests. Record sanitized evidence. Parent publishes the reviewed
increment to both mirrors and runs the native spike with the full existing startup
positive/negative gate plus installed-interop. The previous matrix proves only the
five synthetic capabilities, not full native preflight, MDK startup, identity setup,
macOS server execution or nine-pair transport acceptance. Those remain explicit
pending gates; do not mark the production blocker or platform acceptance resolved
merely because the profile bytes match a passing synthetic variant. Further native
failures require evidence and separately reviewed changes, not broader permissions.
