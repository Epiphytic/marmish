# Compiler bootstrap disk validation

The authenticated Rust 1.97.1 umbrella archives for Linux AMD64, Linux ARM64 and
macOS ARM64 were inspected in full. [The inventory](installed-bootstrap-archive-inventory.json)
records manifest/archive digests, sizes and member types. Every selected member
was a regular file or directory; no selected symlink, hardlink or other type was
present. Each archive contained its required selected component metadata.

On macOS the archive expands to 1,849,029,532 bytes; the selected components and
flat installer metadata total 588,861,566 bytes. Avoiding the on-disk compressed
archive and unrequested extraction reduces estimated extraction-plus-installation
peak from about 2.64 GB to 1.18 GB, before filesystem rounding and reserve. These
are inventory-derived estimates, not measurements of peak disk use. Initial
runner free space and historical leaks were not established.

An actual disposable Linux AMD64 run used the revised selective extraction and
official pinned installer. Bootstrap completed in 29.667 seconds. Preflight
reported 1,440,653,312 required bytes before extraction (653,217,792 block-rounded
selected bytes twice plus a 128 MiB reserve), and 787,435,520 required bytes before
installation. The filesystem had over 539 GB available, so this validates the
successful path rather than a genuinely full filesystem.

The resulting managed toolchain then passed the existing complete private-copy
verification. Its exact current-job source tree and ownership record were removed
before version checks and real locked dependency fetch/offline warmup, which
completed in 6.261 seconds. [The sanitized local result](installed-bootstrap-disk-local.json)
records those timings and absence checks. This is compiler preparation evidence,
not Marmot transport latency or throughput evidence.

The first final native cleanup conservatively failed at its native-cleanup stage and
retained state. An unchanged retry completed all seven cleanup checks in 0.237
seconds. Inspection at the retry found only the state snapshot's preexisting
unreadable same-user process identities, and no marked survivors. The first
scan's transient cause was not established. No process authentication or cleanup
condition was weakened. After successful cleanup, the exact newly-created harness
was removed; the bootstrap cleanup operation independently reported absent.

Synthetic tests exercise selective extraction, rejected archive paths and links,
missing metadata, exact disk-budget boundaries, finite diagnostics, ownership
identity/context/path/mode checks, safe partial-install removal, idempotent
cleanup, and source preservation when private-copy verification fails. The
Darwin `/tmp` alias test runs only on Darwin. Native macOS and ARM64 executions
remain separate acceptance requirements; these results do not claim those jobs
passed.
