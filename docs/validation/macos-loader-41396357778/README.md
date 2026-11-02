# macOS loader crash evidence

[Hosted diagnostic run 41396357778](https://github.com/Epiphytic/marmish/actions/runs/41396357778)
ran after commit `52f1640`, which corrected owned Darwin zombie cleanup. The
synthetic diagnostic lifecycle now finishes; this does not establish the full
installed-interop lifecycle or a working production confinement boundary.

All unconfined probe controls exited zero. Confined read, TCP and `/usr/bin/true`
all ended with SIGABRT (exit -6), before probe main. The owned crash report places
the abort in dyld CacheFinder / ignition and records namespace 0x23, code 2.
No attributed sandbox denial was found. These reports contain only the bounded,
redacted schemas uploaded by the workflow; no process details, identity keys or
runtime databases are included.

The startup abort remains unresolved. The independently reviewed
[loader permission matrix](../../phases/macos-loader-matrix.md) tests fixed
permission hypotheses while preserving positive and negative capability checks.
An eventual passing matrix selects a hypothesis for a separately reviewed
production change; it does not establish production readiness.
