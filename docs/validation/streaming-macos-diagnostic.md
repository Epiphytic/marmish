# macOS same-host diagnostic observation

[Run 42367008260, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42367008260)
passed on macOS ARM64 on 2026-11-07. All six requested observations passed, both
endpoint terminals were exit 7 for every stream, each fixture receipt validated,
and all six cancellation/physical-idle/stop cleanup proofs were true. The
credential-free workflow validator passed. The downloaded records also passed
the independent closed Pins/report/complete validation locally.

This is **diagnostic PASS, qualification=false**. It uses one macOS host and one
authenticated client identity. It does not reproduce the earlier three-identity,
cross-host topology, establish a product defect or fix, or clear the failed
[installed matrix 42363063498](https://github.com/Epiphytic/marmish/actions/runs/42363063498).

## Provenance

The installed product, rendered installer and copied fixture are immutable
candidate 42358105270/attempt 1 source
`571d98b77780d265a2fb29d84ae160fec72ba37b`, version 0.2.3,
`aarch64-apple-darwin`. The separately built diagnostic harness is source
`e9686c0799e78010c350d4990406c93ddb8bc474`; its signed probe SHA256 is
`c075b05f7235c4dcb63247b8fea5bb035f3a4e51b9df726b7acaec34442800e4`.
No mixed-source candidate Context or qualification rollup was constructed.

Artifact 13572088229, `macos-stream-diagnostic-42367008260-1`, is 3381 bytes;
its ZIP SHA256 is
`4b5ec5d6202da949b13b1c118b18af409dcc31cbd786a94f07e1d3601ab73783`.
It contains only provenance.json, output/report.json and output/complete.json;
no failure marker is present. The complete marker binds report SHA256
`6f4982991473b7e3f199103a565c15d43cb9aa0f49dd632552c39f8014c1afe5`.
[Machine-readable evidence](streaming-macos-diagnostic.json) retains all three
sanitized records, exact source-artifact pins, file hashes and API provenance.

## Observed timing and payload

Every case fed 98304 bytes, returned 98304 bytes stdout and 8192 bytes fixture stderr,
and exited 7. Stream stderr capture is 8241 bytes because it also contains the
49-byte stream-ID prefix. Hashes and receipts matched independently.

| Case | Wall time | Payload rate, both directions combined |
| --- | ---: | ---: |
| Direct old fixture, no transport | 0.048s | Local execution only |
| Raw stream | 14.761s | 13.55KiB/s |
| Zlib stream | 14.765s | 13.55KiB/s |
| Concurrent zlib 1 | 44.148s | 4.53KiB/s |
| Concurrent zlib 2 | 44.174s | 4.53KiB/s |
| Concurrent zlib 3 | 44.249s | 4.52KiB/s |

Rates divide 204800 payload bytes (stdin+stdout+fixture stderr) by each case's
recorded wall time. They include local process and transport work, are **not
RTT or pure wire throughput**, and exclude the stream prefix. Concurrent timings
overlap and must not be summed. There are no wire-byte, compression-ratio or
transport-offset measurements; unavailable fields remain unavailable.

The run used explicit `wss://relay.us.whitenoise.chat`, Tor off, disposable
file-backed identities, a 900-second work budget, 180 seconds for cleanup and
1 MiB capture limits per output. It exercised the installed bootstrap and old
fixture directly, then raw, zlib and three concurrent zlib streams with the same
client identity. It did not test copying, rsync, other platforms or the full
installed qualification suite.

Fresh candidate 42367702241 at e968 had started qualification when this evidence
was recorded; no result is asserted here. All-platform candidate validation and
all eight installed cohorts remain required. Public release 0.2.3 remains without
these streaming features.
