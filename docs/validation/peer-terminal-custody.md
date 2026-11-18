# Local retirement of joined Pending Peer custody

Linux AMD64 validation uses the [approved terminal custody design](../phases/peer-terminal-custody.md)
and upstream MDK master as resolved at
`f89bd59089b92ddbb2492def6972e8bc75d09cf6`. The daemon and filesystem
fixtures use separate disposable identities and profiles, encrypted
SQLCipher data and an authenticated loopback TLS WebSocket relay.

The daemon case joins Pending custody through real Offer, Consent and
Delivery. It exercises a same-UID Decline, an interrupted reply with a
durable terminal decision, and a separately timed Expired invitation. Both
reach local unlink through the original generation with no additional Peer
publication. The filesystem case closes and reopens the protected stage
between durable decision, intent, unlink and completion. It also observes
retirement with the founder blocked, Peer mode Off, and the signed
custody window elapsed. Clock rollback and a later sidecar leave the charged
slot in place. The cases model interrupted operations with explicit fixture
cuts and reopen.

| Initial exact phase branch sample | Full fixture | Decision to unlink |
| --- | ---: | ---: |
| Daemon, Declined and Expired | 85.29 s | 463 ms Declined, 956 ms Expired |
| Filesystem cuts | 31.80 s | — |

The schema protects Pending-only terminal decisions, exact generation and
protected-file identity, monotonic wall, retained replay evidence, and
explicit decline cooldown. Reservation release and invitation deletion are
atomic. The existing foreign keys require clearing the exact slot before
deleting the invitation in that transaction, after verified file absence.

On the composed integration, the decline and expiry case passed in
73.27 s (317 ms and 1,062 ms to unlink), and the filesystem case passed in
27.10 s. The full sequential daemon WSS regression on the exact composed
source `6609151` passed four tests in 254.23 s, including ordinary Command
execution during an independent Peer relay outage. The isolated PostJoin
recovery passed again in 33.65 s.

The IPC approval and decline race, daemon timeout, closed schema, cooldown
admission, capacity and pruning boundaries, and private file identity have
focused regressions. The core CI workflow selects the expiry boundary and
three filesystem tests. The TLS WebSocket cases are opt-in.

```sh
cargo test --locked -p marmish-native --lib pending_terminal
cargo test --locked -p marmish-native --lib joined_retirement_
cargo test --locked -p marmish-native --lib ipc::peer::tests
cargo test --locked -p marmish-native --lib \
  peer_daemon_wss_decline_and_expiry_custody -- --ignored --nocapture
cargo test --locked -p marmish-native --lib \
  peer_private_wss_pending_retirement_recovers_each_filesystem_cut -- --ignored --nocapture
```

These observations establish local discard and reservation release. They
record native membership removal as Unconfirmed.
