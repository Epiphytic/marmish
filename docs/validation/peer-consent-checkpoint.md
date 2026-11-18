# Peer Offer and Consent checkpoint validation

The offline checkpoint accepted authenticated Offers through the existing raw
cursor transaction seam. Independent outer verification tied every accepted
receipt to the signed index. Two valid outer wrappers of one signed revision
produced two raw receipts and one invitation. The same signed revision on a
second configured origin was refused. The test advanced the cursor only
after it had both exact receipts.

A profile owner could approve an exact pending revision after reopening the
encrypted database. Explicit automatic policy approved either reservation
class. Wrong local UID, previous owner capability, wrong origin, blocked
peer, off mode and stale revision exposed no outbox work. Local contention
and capacity produced authenticated durable retry work. The retry reopened
and verified the original bytes again, then committed one effect and removed
the deferral. The fence budget test seeded prior history in one transaction
and used real signed Offers for admission, replay, full-budget refusal and
post-expiry recovery. Cursor time rebase alone could not recover expired
authority.

SQL failure and deliberate unwind rolled back intake. Injected SQL failures
at approval, outbox creation and discard rolled back each transaction.
Reopening recovered the original stage and, after a failed discard, the
original outbox bytes. The package came from a
disposable instance of the pinned native engine. The receiver opened the
signed Consent and matched exact Offer revision and package bytes. Reopening
the SQLCipher store returned byte-identical Consent. The test covered shorter
and longer Consent lifetimes, local native artifact provenance, rejection of
invalid package bytes, expiry with no regeneration and exact discard. A
wrong key, account mismatch and a plain SQLite file failed qualification.
Raw encrypted database bytes omitted the tested IDs and serialized frames.

The pinned native KeyPackage event signs the package but does not encode
the publication endpoints from its MDK publication request. The signed
Offer controls Consent transport. The local stage custodian supplies
artifact provenance for that same origin and exact Offer selector.
Production native custody, actual OS peer credentials, socket publication
and peer acceptance remain outside this fixture.

## Local verification

Run with disposable identities and stores on Linux x86_64. No network
traffic or GitHub Actions jobs were added by this checkpoint.

```sh
cargo test --locked --offline -p marmish-native --test peer_cursor_store peer_consent_checkpoint_core
cargo test --locked --offline -p marmish-native --test peer_cursor_store
cargo clippy --locked --offline -p marmish-native --all-targets -- -D warnings
```

All seven focused cases passed in the independent Claude Code review.
The complete cursor target passed 25 cases with three opt-in benchmarks
ignored in 18.80 s. Strict Clippy passed for all native targets. All recorded
checks used integrated base `35e51c6` with the exact reviewed overlay. The
expected wrong-key tests caused SQLCipher to log page authentication failures.

## Local latency

```sh
cargo test --locked --offline -p marmish-native --test peer_cursor_store peer_consent_checkpoint_benchmark -- --ignored --nocapture
```

100 samples after ten warmups, unoptimized Cargo test profile on the same
Linux x86_64 host. The measured Offer was 2,585 bytes and Consent was 5,997
bytes. Timings include the operation's synchronous SQLCipher transaction.
The outbox write includes native package verification and Consent signing.
Native package generation and relay round trips are outside these timings.
The read measurement reuses a live connection after the committed outbox
write. Restart recovery is covered separately by the correctness test.

| Operation | p50 | p95 |
| --- | ---: | ---: |
| Verify Offer and commit invitation and fence | 49.335 ms | 80.951 ms |
| Commit local approval | 47.199 ms | 83.304 ms |
| Verify package and commit signed Consent | 68.529 ms | 109.172 ms |
| Recover existing immutable outbox bytes | 0.332 ms | 0.565 ms |
