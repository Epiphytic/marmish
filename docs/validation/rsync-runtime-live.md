# Native rsync live qualification

Final reviewed same-host Linux AMD64 native source snapshot: **PASS**, 679.772292 seconds.
The [sanitized machine report](rsync-runtime-live-r2.json) records exact hashes,
per-case results, cleanup and uncovered cases. This is source qualification;
released v0.2.3 does not contain the rsync adapter.

Two native daemons with disposable file-backed identities communicated through
explicit `wss://relay.eu.whitenoise.chat`. Both used native SHA256
`3277b64b6b4a6a8ea7ff89e62aeeea04b87c0cb7e1cf05c9a4447a05ecfa5eda`;
local rsync was 3.2.7/protocol31. This snapshot includes the final reviewed signal dispatcher and daemon-startup
interruption corrections. The earlier [passing snapshot](rsync-runtime-live-r1.json)
is retained separately; it is not the evidence for those corrections.

Push and pull preserved the independent tree hash across 16 entries, including
empty directories, symlinks, arbitrary binary bytes and literal special filenames.
Unchanged and deletion runs sent zero literal data. After a small edit, the
Marmish-zlib delta run sent 700 literal bytes and matched 392,516 bytes; rsync's
own compression also preserved the exact resulting tree. Rejected old bootstrap
and missing remote executable returned the expected distinct errors.

One process-group interrupt produced outer rsync exit20 with remote terminal
cleanup confirmed in 2.023536 seconds. A 262,144-byte partial file remained.
Restart matched 261,800 bytes, sent 1,835,352 literal bytes and reproduced the
exact 2MiB source hash. Owned client/server daemon shutdown returned0 and the
private fixture was removed.

Raw initial push/pull processed 413,184 tree bytes in 86.893432/87.030454 seconds
(4,755.1/4,747.6 tree bytes per second). Partial restart completed in
355.856483 seconds (5,893.3 tree bytes per second). These include startup and
protocol overhead, are one sample each and are not wire bandwidth measurements.
Delta, unchanged and compression timings use different existing state and cannot
establish a compression speedup. No installer, macOS, ARM64, cross-platform, Tor
or command-latency qualification is implied. The report explicitly lists other
missing negative, signal, recovery and authority cases.
