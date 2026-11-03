# Post-review native direct command repeat — 2026-11-02

**PASS.** Fresh client/server keys and explicit `wss://relay.us.whitenoise.chat`, `--tor off`; final rebuilt native `marmish command-check` plus its Rust worker. This repeat includes the server `command_poll_busy` retry classification fix and the client fixture's bounded same-request-ID admission retry. The [failed intermediate attempt](local-command-poll-pressure-2026-11-02.json) remains recorded.

| Case | Result | Command completion | Stdout goodput |
| --- | --- | --- | --- |
| Literal/special/non-UTF8 argv | Exact53 bytes; exit0 | 1571ms | 33B/s |
| Binary stdin/EOF through cat | Exact16384 bytes; exit0 | 2973ms | 5509B/s |
| Stderr/nonzero exit | Exact12 stderr bytes; exit7 | 1623ms | Not applicable |

Total13103ms including setup and cleanup. Engine quiescence, joined engine and MDK stops, runtime cleanup and private-state removal all passed. The [sanitized JSON](local-command-direct-final-2026-11-02.json) includes full source hashes for the main changed command files. These are single Linux AMD64 observations, not latency distributions, RTT measurements or sustained bandwidth figures.

No public command CLI/IPC, running-job restart, authenticated remote conflict/replay, successful Arti command execution, installed cross-platform command acceptance or release is claimed. Phase2 remains open.
