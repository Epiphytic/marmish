# Peer provisional reservation validation

Linux x86_64, locked offline build using upstream MDK at
`0ad3c61088e1acac58e0fcf1a9f1142bae5d997a`. Tests use fresh local SQLCipher
profiles and disposable signed Offers. The reservation test suite opens no
relay or native application room.

```sh
cargo test --offline --locked -p marmish-native --test peer_cursor_store
cargo test --offline --locked -p marmish-native --test peer_cursor_store \
  peer_provisioning_latency -- --ignored --nocapture
```

The first command passed 31 tests and left four explicit benchmark or soak
tests ignored. The second passed the reservation benchmark. It timed 20
samples per operation using a new profile and signed Offer each time.
Recovery reopened the encrypted checkpoint before the sample started.
The sample includes signed Offer reopen and the synchronous database
transaction. Profile creation, Offer receipt, database reopen, and network
transport are outside the measured interval.

| Local operation | p50 | p95 |
| --- | ---: | ---: |
| Initial reservation | 78.560 ms | 95.314 ms |
| Exact recovery after reopen | 77.749 ms | 93.467 ms |

The fixture checked class isolation and rate charging across reopen,
exact current origin and owner, policy and quarantine, expiry beyond
signed skew and clock rollback, preserved replay fences, and cleanup
with provisioning disabled. It drove mutually exclusive legacy outbox
and provisioning writes in both directions. Forced SQL failures during
approval clearing and retirement preserved prior durable state. Direct
SQL exercised malformed ownership, missing class state and a stale
generation on the same selector. A new signed Offer from the same
inviter uses its native random Offer ID. The old ID paired with its
new revision fails exact lookup.

The standalone checkpoint is under qualification. Native staging and
network publication, pending application activation, private stage
retirement and a bounded terminal release require independently
reviewed transitions before deployment.

The scoped Clippy check passed with warnings denied. The final suite also
verifies both independent SQLite integer limits, expiration of an ordinary
unowned Offer while an expired owner survives, and a version-2 checkpoint
gain of the bounded schema with the existing ciphertext unchanged.
