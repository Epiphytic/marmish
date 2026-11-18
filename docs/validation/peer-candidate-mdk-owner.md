# Upstream MDK account custody qualification

Exact fixture source `f6d6d841e577fdfb2b3ec6bc9fd45b25c3e0240e` on
Linux AMD64, upstream MDK `bdf9833`, locked SQLCipher. Stock all-target
strict Clippy passed in 28.30 s. The stock native library test executable
had SHA-256
`c1fb4977ef185d2e8c2a043b6622e30225d398fc98cbedde237bc70d40dbb55f`.
All nine fresh cases passed in 48.35 s. The independent review reran the
same nine in approximately 58 s.

| Case | Observation |
| --- | --- |
| Explicit close with a surviving clone | Main database descriptors were gone, sidecars were removed, the clone refused access and its later drop left the files unchanged. Cold reopen read the lifecycle. |
| Actual last strong reference drop | Release marker was set, main database descriptors were gone and cold reopen read the lifecycle. |
| Wrong key | Open refused under the still-intact dispatcher and original files were unchanged. |
| Cut before account open | Admission refused without changing measured files. |
| Cut in the forwarding VFS before the first progress callback | Admitted open finished. The next admission refused and the caller retained the account and original lease. |
| Cut in the upstream open progress callback | Admitted open finished and the next independent effect was refused. |
| Cut before an independent smoke close | The account and lease were parked, and the release marker stayed unset. |
| Cut before independent reopen | The already closed files stayed unchanged. |
| Cut with a reopened live account | The surviving clone refused ordinary storage access. Main, WAL and SHM fingerprints and main-file descriptors stayed unchanged through parking. |

The stock cases also passed under a process-following syscall trace in
400.92 s. The trace included filesystem opens, deletes, syncs and raw
write calls without decoded buffers. Its aggregate covers fixture setup,
real native operations and cleanup. It does not attribute each write to
a VFS phase. Exact cut assertions use the forwarding open callback, the
upstream progress callback, file fingerprints and /proc main-file
descriptor observation. Neither the release marker nor this trace by
itself proves release of every possible SQLite handle. No assertion
requires interrupting a native call already admitted to run.
