# Large Linux relay copy qualification

The source-only 12-case suite passed in **4134.843382 seconds** on same-host Linux AMD64 through explicit `wss://relay.eu.whitenoise.chat`. The four main transfers each copied **2,105,344 bytes (2 MiB + 8192)**. This is functional evidence; deadline-margin and installed-platform qualification remain in progress.

[Machine-readable result](copy-runtime-live-r4.json) is the unchanged harness output. Its SHA-256 is `5f96d205251666672dc0a89d0327a1ad7f4a7cf7b980db92bb35b80e790d9fa3`.

| Case | Exit | Seconds | Payload bytes/s | Deadline headroom |
| --- | ---: | ---: | ---: | ---: |
| put_raw | 0 | 385.632204 | 5,459.5 | 2.334× |
| get_raw | 0 | 504.492509 | 4,173.2 | 1.784× |
| put_zlib | 0 | 739.814749 | 2,845.8 | 1.217× |
| get_zlib | 0 | 720.348362 | 2,922.7 | 1.249× |
| compressible_put_zlib | 0 | 41.307695 | 1,388.2 | 21.788× |
| compressible_get_zlib | 0 | 49.480216 | 1,158.9 | 18.189× |
| empty_put | 0 | 24.469141 | — | 36.781× |
| empty_get | 0 | 29.500598 | — | 30.508× |
| put_missing_source | 1 | 15.979480 | — | 56.322× |
| put_existing_destination | 1 | 796.748054 | — | 1.130× |
| get_missing_source | 1 | 20.573001 | — | 43.747× |
| get_existing_destination | 1 | 796.870511 | — | 1.129× |

Each operation had a 900-second process deadline. The four main transfers and both existing-destination refusals passed below the planned 4× deadline margin. Zlib Put/Get and both existing-destination refusals were below 1.5×; the [reviewed qualification amendment](../phases/streaming-copy-qualification.md) requires corrective runs for those four cases. The [separate selected rerun](copy-runtime-live-r5-corrective.md) passed all four with a 3300-second budget and more than 4× observed headroom. A successful original case is retained as functional PASS, not relabeled as meeting the original margin rule. Earlier failed attempts remain in [the qualification history](copy-runtime-live.md).

The four main transfers had matching SHA-256 `989ebe23152bf66312278b9d463c75ba7c808250e58b98349b185ba513fa5c9c`, exact contents, mode 0640 and nanosecond mtime 1700000000.123456789. Paths included literal quotes, spaces, newline, tab, backslash, Unicode, shell substitution and glob characters. Every successful copy had empty stdout, verified commit confirmation and the expected negotiated codec. Compressible and empty-file Put/Get also passed. Missing sources returned `file_not_found`; existing destinations returned `transfer_conflict` without overwriting their original empty contents. These refusals are errors, not successful file transfers.

Both disposable daemons exited 0 and private test state was removed. Independent report checks verified the exact 12-case set, binary hash, positive lengths/hash consistency, codec labels, error exits, speed arithmetic and recorded cleanup. They did not repeat the transfers.

The binary SHA-256 is `ef9f55b88c4b69a267abedf431e73973332f07110309283f50ff93c774bb716d`, matching the immutable snapshot used by the earlier small smoke. Base commit: `33753e271595da1651780a5d80fb0d067b6293f4`; dirty Rust source fingerprint: `c31ae8e848234aeabe71066c12ad9fac4525035ddcb514037cac35820e730738`. Its package version string is 0.2.3, but it is an unreleased source build; the released v0.2.3 assets do not contain copy. The snapshot predates the later post-receipt uncertainty correction in 23105d8. This successful run does not exercise that failure correction.

Elapsed time includes protocol setup, data transfer and commit, so payload bytes/s is observed operation goodput, not steady-state link capacity. The random source is incompressible; no wire byte counts or encrypted-network compression ratio were measured. The separate [66 MiB actual-helper evidence](copy-large-helper-local.md) measures local framed IPC, not this relay.

Not covered: installer or cross-platform operation, Tor, daemon restart, live interrupted durable resume/cancellation, retention GC, files larger than 65 MiB over a relay, or genuine historical installed-format upgrade. See the qualification amendment and Beads `marmish-8ka.5` / `marmish-8ka.6` for the corresponding scope.
