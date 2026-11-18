# Peer native stage retirement validation

The test fixture used Linux x86-64, locked MDK
`0ad3c61088e1acac58e0fcf1a9f1142bae5d997a` and a real SQLCipher cursor.
It generated disposable identities in private profiles. The native case
prepared a signed KeyPackage with MDK and retired the local stage.
The fixture opened no relay connection; the KeyPackage stayed local.

## Author checks

- The focused retirement suite passed all 12 tests. It includes retry
  after a temporarily unavailable cursor path before any durable intent
  and device/inode replacement, unsafe leaf and sidecar checks.
- Four filesystem/journal cuts recovered with new SQLCipher attachments
  and exact receipt retries. SQL rollback was injected after each journal
  transition. A second connection saw the committed schema and its
  concurrent write was refused.
- Direct SQL DML checks ran with foreign keys and recursive triggers
  disabled. The suite covers inventory mismatch, wrong UID and profile,
  corrupted signed Offer, cooldown across relay origins, checked expiry,
  and SQL arithmetic bounds.
- Before the retry correction, 11 existing native stage tests, seven
  Consent tests and 31 standalone cursor tests passed. The correction
  modifies only retirement fixture code and its regression.
- The ordinary library build passed. The rlib and rmeta contained the
  cursor positive control and excluded retirement fixture constructors,
  table names and epoch markers. All-targets Clippy passed with the
  existing large enum allowance. The unadjusted warnings-as-errors run
  reports `large_enum_variant` for the pre-existing `ServerOutcome` and
  `ClientOutcome` in `terminal_selection.rs`.

The cooldown deliberately drops each newly admitted signed Offer from
the declining inviter during the active window. Those ignored Offers
gain no new replay fence. A copy that still passes signed expiry after
the window can be evaluated afresh. The declined Offer keeps its own
signed replay fence.

## Opt-in latency

| Operation | Samples | p50 | p95 |
| --- | ---: | ---: | ---: |
| Decline and release an existing Bound placeholder | 20 | 249.258 ms | 292.540 ms |
| Reopen a FilesRemoved intent and release | 20 | 86.513 ms | 116.356 ms |

The benchmark ran in an unoptimized Rust test profile on local storage.
The first measurement begins after the bound stage and fixture exist. The
second includes reopening SQLCipher and checking the retirement journal.
These are local custody timings. Relay latency, production daemon behavior
and macOS execution are outside this validation.

Run the focused tests and the opt-in benchmark with the checked-in lock.

```sh
cargo test --offline --locked -p marmish-native --lib peer_retirement -- --test-threads=1
cargo test --offline --locked -p marmish-native --lib peer_retirement_local_latency -- --ignored --nocapture --test-threads=1
```
