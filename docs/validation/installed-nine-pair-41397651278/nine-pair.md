# Installed Marmot interoperability measurements

Outcome: **failed**. [Actions run](https://github.com/Epiphytic/marmish/actions/runs/41397651278/attempts/1).

Application: `f609b6ccdc9302aeec661a2ae3692d3ef68b86fa`; harness: `9a694963c253071900532d3d1fc9f5489d82d878`; release: `0.1.0-experimental.2`.

Client → server | Linux AMD64 | Linux ARM64 | macOS ARM64
--- | --- | --- | ---
linux-x64 | failed | failed | failed
linux-arm64 | failed | failed | failed
macos-arm64 | failed | failed | failed

Client → server | Cold status (ms) | Command (ms) | First output (ms) | Effective download (B/s) | Output drain (B/s) | Input/output bytes
--- | ---: | ---: | ---: | ---: | ---: | ---:
linux-x64 → linux-x64 | — | — | — | — | — | —
linux-x64 → linux-arm64 | — | — | — | — | — | —
linux-x64 → macos-arm64 | — | — | — | — | — | —
linux-arm64 → linux-x64 | — | — | — | — | — | —
linux-arm64 → linux-arm64 | — | — | — | — | — | —
linux-arm64 → macos-arm64 | — | — | — | — | — | —
macos-arm64 → linux-x64 | — | — | — | — | — | —
macos-arm64 → linux-arm64 | — | — | — | — | — | —
macos-arm64 → macos-arm64 | — | — | — | — | — | —

Each successful cell contains one cold nonexecuting status probe and one command sample. Durations use local suspend-inclusive clocks; no cross-host clock subtraction. Cold status includes startup, confinement, relay and MLS setup. Command time includes startup, stdin transfer, complete output retrieval in an established room and harness teardown. These timings include process cleanup and are conservative user-operation measurements. Effective download is verified stdout plus stderr divided by command time, not link capacity. Payload byte counts exclude protocol, base64 and MLS framing. Failed cells have no performance sample.

Explicit relays: `wss://relay.eu.whitenoise.chat/`, `wss://relay.us.whitenoise.chat/`.
