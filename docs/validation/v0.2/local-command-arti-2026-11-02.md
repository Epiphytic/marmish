# Native Arti command attempts — 2026-11-02

These are **failed observations**, not command acceptance. Both used the internal Rust `command-check` fixture with fresh keys, the explicit relay `wss://relay.us.whitenoise.chat`, and `--tor always`. Source was based on `5a37bdc886aaa1d3e48f776ca9a25749882e1ebb` plus the uncommitted command increment. Neither attempt produced a command result.

| Attempt | Result | Elapsed | Shutdown and private-state cleanup |
| --- | --- | --- | --- |
| Initial | `transport` / `arti_configuration` | 6435 ms | All reported checks passed |
| Corrected local user environment | `timeout` | 125752 ms | All reported checks passed |

A separate configuration-only Rust probe reproduced the first failure in upstream filesystem security checks: the harness USER value differed from the process UID, and the corresponding user database lookup returned ENOENT. Removing the incorrect USER value, or setting it to the actual local account, made the same configuration builder succeed. The retry used the correct USER value. No permission checks were disabled and no production code was changed for this environment correction.

The retry passed the earlier configuration failure but timed out before collecting command samples. Its report does not identify a narrower network or recovery stage; no such cause is claimed. Prior Phase1 Arti exchanges do not establish Phase2 command success. See the [initial report](local-command-arti-configuration-2026-11-02.json), [retry report](local-command-arti-retry-2026-11-02.json), and the separately [passing direct command evidence](local-command-direct-2026-11-02.md).

A read-only comparison found a 570-second overall command execution budget and a separate shared 120-second Arti bootstrap cap. The observed duration is compatible with bootstrap timeout but does not identify it. Foundation activation is sequential and its owned onion destination differs from this attempt’s clearnet WSS destination through Tor. Further testing should first record a finite failure stage and owner role; these observations do not justify increasing timeouts.
