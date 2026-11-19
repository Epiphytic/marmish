# Near-cap SQLCipher fixture validation

The [design](../phases/peer-candidate-near-cap-fixture.md) specifies the
fixture input to installed Manual timing qualification. The database is
a real encrypted native Pending stage. SQLite returns the scratch growth
pages to its freelist, so this workload represents allocated encrypted
image hashing, copying and native readback over an unchanged MLS group.

## Diagnostics during implementation

The first candidate test at `cdfcef4` failed after 20.07 seconds in
`stage_pragma`. The bundled SQLCipher dispatches direct
`PRAGMA page_size` to its cipher setting and exposes its text value.
The SQLite pager setting is available through the checked table-valued
query used by the locked MDK. The fixed fixture reports named errors
for the individual settings.

A small probe linked against the same bundled SQLCipher verified that
increasing the actual pager extent then dropping the scratch table
retained 3,968 pages with 3,961 freelist pages and 16,252,928 logical
and allocated bytes on ext4. This probe used disposable synthetic
SQLite content. A separate low-cap probe reported primary
`SQLITE_FULL` (13) and returned to autocommit with the original
logical extent and integrity. Encryption rewrote bytes in the
restored page, which the rollback test now allows only on the private
copy. These probes isolate SQLite behavior from the native and installed
tests.

## Exact offline native selector

The Linux AMD64 candidate test executable stamped with composed source
`9b30b76` passed the isolated selector after 22.26 seconds.

```text
--ignored --exact peer_invitation::prepared_consent::joined_tests::exact_joined_snapshot_grows_to_near_cap --nocapture
```

The six fixture Rust files and phase design match author source
`c45c926` byte for byte. The test joined a genuine two-account native
stage, grew it to 16,314,368 logical bytes and 16,318,464 allocated bytes
on ext4, and measured 3,983 pages of 4,096 bytes with 3,384 on the
freelist. Native readback on a separate checked copy retained the exact
group, canonical epoch, own leaf and receipt. The selector also checked
the private-copy cap refusal and the digest of the untouched original,
along with the wrong-key, exact inode, file layout, allocation boundary
and active receiver VFS refusals. SQLCipher emitted its expected
wrong-key HMAC diagnostic. This run exercised the native stage helper
without relay traffic. Installed cursor selection, byte and quota
observation, List and Show, and latency remain separate controller
evidence. An explicit corrupted-page control was optional in the design
and was not executed.

## First installed run

The composed Linux AMD64 candidate built, passed strict checks and was
installed from the checksum-verified package at `9b30b76`. The installed
near-cap refresh selector reached live Pending on a fresh image and grew
the exact selected stage. Closed original measurement matched the offline
geometry and extent above. The subsequent cold candidate did not publish a
Pending read view within the controller window. It reported 25
`proof_refused` samples, zero publications, and zero selector failures.
The selector exited failed after 84.44 seconds. This run establishes
the installed-stage growth and selection path. Cold charged List and
Show and the warm timing samples still require a passing installed run.

The refusal label covers both proof assembly and the final ticket clock
check. A separate diagnostic on the linked unoptimized SHA-256 code
measured 244.5–255.7 ms per full image hash. The checked native path hashes
the original three times and the copy twice before publication. The
five hash passes alone therefore exceed the one-second ticket lifetime
at the measured debug speed. A release-profile rerun was started to
check whether that explains the installed refusal.

The separate ordinary Manual regression passed on the same installed
debug package pair in 54.11 s. It reached exact Pending 12.972 s
after Offer, restored the same invitation in 7.601 s after a cold
restart, and made the local view unavailable within 270 ms of the
attributed Peer connection loss. Both binary command results survived
the loss and were read by the separately installed stock daemon.
