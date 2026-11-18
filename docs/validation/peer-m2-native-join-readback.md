# Peer M2 native join and readback validation

This isolated Linux AMD64 qualification covers the new native component of
the pending custody fixture. It uses the M1 founder helper and normal signed
Offer, Consent and Delivery codec in memory, a disposable SQLCipher profile
and an adapter with no network connector. The complete WSS and custody
transaction tests have their own evidence.

All ten locked upstream MDK packages resolve to
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`. Cargo used the locked manifest
in offline mode with two jobs. Run the opt-in proof with

```sh
cargo test --offline --locked -j2 -p marmish-native --lib \
  exact_joined_snapshot_proof_requires_closed_image_and_receipt -- --ignored
```

The proof starts from a signed, prepared package and an authenticated
Delivery. A guarded snapshot confirms the original is unjoined and has
the exact signed package, reference and digest. The original opens once
for native ingest in DELETE mode. Each subsequent readback opens a
distinct encrypted disposable copy. Both return the same group ID, epoch
pin and local leaf. The proof checks that the original digest stays
unchanged across both readbacks and that the temporary directory is empty
after the last read. Before ingest, the helper rejects a wrong digest and
a different signed package in the same slot without changing the original.
Repeating the join on the joined file fails for both the exact Delivery
and another signed Delivery. The proof also rejects the wrong origin,
a different signed Welcome, an unjoined image and an original hot journal.
The journal contents stay intact. No native transport or publisher method
may run during join or readback, and the successful native ingest produces
only the exact expected application receipt without publishing.

An early affected regression failed the existing typed storage-error check
because a rejected migration can mutate the disposable unjoined copy.
The correction preserves the native error. It keeps copy shape, inode,
sidecar and original digest checks on both success and failure, and
requires a byte-identical unjoined copy after successful attestation.

| Check | Result | Time |
| --- | --- | ---: |
| Native join and independent snapshots | 1 passed | 47.96 s |
| Prepared Consent regression filter | 7 passed, 2 opt-in tests skipped | 156.46 s |
| Strict Clippy, native crate all targets | Passed | 28.19 s |
| Rust formatting and diff whitespace for touched files | Passed | |

The test time is the observed harness runtime while the host was also
running local builds. These figures apply only to local SQLCipher and
MDK qualification.

The exact source is `ffd3481a5cac2149813acd17f0546ee4ba025433` and the
preserved executable SHA-256 is
`b4ce74517357b14a27ebf61e6729fd88075bcc5134ee3e817fd1f3b2c691bad5`.
See the [independent Claude review](../reviews/peer-m2-native-join-readback.md).
