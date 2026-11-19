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
