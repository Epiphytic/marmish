# Native copy runtime qualification (in progress)

The source-only Phase 3 implementation has independent source approval; live
and installed qualification are still in progress. None of these tests validates the released v0.2.3 installer.

## Early Linux relay attempt

[Machine-readable result](copy-runtime-live-r1.json): **FAIL**, first raw Put
exceeded its 900-second process deadline. Total harness time was 914.271625
seconds; no complete transfer or throughput sample was accepted. Disposable
client/server daemons both exited 0 during owned cleanup. Private diagnostic
state was retained outside the repository and is not part of this evidence.

This was a dirty development snapshot with native binary SHA256
`fb650c267219e5a72004d4a5ec64862cc9faa408dae43d06f594d1846b029dc0`,
Linux AMD64, explicit `wss://relay.eu.whitenoise.chat`, same-host endpoints.
It predates the bounded 8179-byte Data payload packing and local-worker headstart
changes. The version string is the source package version, not a claim that the
released v0.2.3 binary includes copy. File growth observed during execution was
not counted as a validated durable checkpoint or completed transfer.

Updated-snapshot live results, integrated review and installed cross-platform
qualification remain required. The failure above is retained independently of
any subsequent successful run.

## Second Linux relay attempt

[Machine-readable result](copy-runtime-live-r2.json): **FAIL overall**. Raw Put
completed and independently verified 2,105,344 bytes in 384.671499 seconds
(5,473.1 payload bytes/second). The subsequent Get failed with the finite
`transfer_conflict` diagnostic before remote Open. The client-only daemon's
local receiver Reserve queried the general conversations table instead of the
authenticated client command room. The corrected route and fresh/cached
activation revocation guards passed actor regressions before the third run. Both owned daemons
exited 0. Total suite time was 394.514046 seconds.

This snapshot used native binary SHA256
`7226159b1cd997187f44ced75e447b982cb4838232c13da4bb12fcd3cff26fe6`,
base commit `33753e271595da1651780a5d80fb0d067b6293f4`, and dirty Rust source
fingerprint `2d39e8a3952fc68bc1bc5b6d42bc8b3b8e861708613b9f46543a68c2f96e3671`.
It includes bounded Data packing and worker headstart, but predates subsequent
receiver release guards and codec diagnostics. This is one successful upload,
not complete copy qualification. The explicit relay and same-host Linux scope
are unchanged from the first attempt.

The copy harness defaults to 2 MiB plus 8192 bytes. An explicit `--bytes` value
allows a separately labelled smaller smoke run after fixes; reports retain the
actual size and binary SHA. A smaller smoke does not replace large-file or
cross-platform acceptance.

## Third Linux relay run: small smoke passed

[Machine-readable result](copy-runtime-live-r3.json): **PASS**, all 12 cases in
334.734399 seconds. This deliberately small smoke used 65,536-byte random files,
57,344-byte compressible files, and empty files. It verifies the repaired Get
route, not the default large-file acceptance threshold.

| Operation | Payload bytes | Seconds | Payload bytes/second |
| --- | ---: | ---: | ---: |
| Raw Put | 65,536 | 31.851320 | 2,057.6 |
| Raw Get | 65,536 | 32.173756 | 2,036.9 |
| Zlib Put | 65,536 | 30.888560 | 2,121.7 |
| Zlib Get | 65,536 | 33.135699 | 1,977.8 |
| Compressible zlib Put | 57,344 | 28.493302 | 2,012.5 |
| Compressible zlib Get | 57,344 | 31.852986 | 1,800.3 |
| Empty Put | 0 | 21.559472 | — |
| Empty Get | 0 | 21.263088 | — |

Both directions independently compared exact file hashes. The random-file cases
also checked mode 0640 and nanosecond modification time with literal quotes,
newline, tab, backslash, Unicode, shell substitutions and glob characters in
filenames. All eight successes had empty stdout, explicit commit confirmation,
and the expected negotiated codec. Missing-source and existing-destination
refusals passed in both directions (12.516582–35.921216 seconds), with exact
finite diagnostics, no success marker, and no destination creation or overwrite.
These elapsed measurements include protocol setup and commit; they are not
steady-state link throughput or a wire compression-ratio measurement.

Native binary SHA256:
`ef9f55b88c4b69a267abedf431e73973332f07110309283f50ff93c774bb716d`.
Base commit is `33753e271595da1651780a5d80fb0d067b6293f4`; dirty Rust source
fingerprint at build was
`c31ae8e848234aeabe71066c12ad9fac4525035ddcb514037cac35820e730738`.
Later actor-test assertion/comment changes do not change this runtime snapshot.
Both owned daemons exited 0 and disposable private state was removed.

Scope remains same-host Linux AMD64 through explicit
`wss://relay.eu.whitenoise.chat`. This does not validate an installer, another
platform, Tor, daemon restart, live interrupted resume or cancellation, or
retention garbage collection. Default 2 MiB plus 8192-byte qualification ran separately against these same immutable binaries; see the fourth run below.

## Fourth Linux relay run: large functional suite passed

The [large-run report](copy-runtime-live-r4.md) and [unchanged JSON](copy-runtime-live-r4.json) record all 12 cases passing in 4134.843382 seconds. Each main transfer copied 2,105,344 exact bytes; raw Put/Get measured 5,459.5 / 4,173.2 payload bytes/s and zlib Put/Get 2,845.8 / 2,922.7 bytes/s. Both daemons exited cleanly and private state was removed. Four cases were below 1.5× deadline headroom and require separate corrective reruns under the reviewed amendment. This functional PASS does not claim the original 4× margin, an installed-platform result, or coverage of the later post-receipt uncertainty fix.
