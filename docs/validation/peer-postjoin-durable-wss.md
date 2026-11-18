# Durable native Peer recovery over private WSS

The [activation design](../phases/peer-postjoin-private-wss.md) was exercised
on Linux AMD64 at source `dae2c94` and locked upstream MDK
`59b7ed3ff007494b25f8404c144c1d82809b3973`. It uses disposable founder and
recipient identities, separate authenticated connections to a local TLS relay,
and checked SQLCipher custody and activation stores.

After the founder's intervening update reaches the joined recipient through
the qualified history cut, the recipient stages its native PostJoin update.
The test closes and reopens the recipient's native and activation stores at
four boundaries. At Reserved, it reconstructs only the frozen native attempt.
At Captured, the saved journal and fresh native readback must agree before
the first write. At Uncertain, a retry stays closed until the founder
processes the exact first event with the missing acknowledgement. Finally,
after native completion and before the journal write, a cold read confirms
the same rotation and finalizes its local state without another native send.

The relay recorded exactly three Group writes. The founder's update was
followed by the recipient's update and a byte-identical retry of that update.
The relay withheld the first acknowledgement and acknowledged the duplicate.
Both participants finished at the same native epoch and active roster. An
expired retry was refused before any additional egress. A later outbound
fanout drain made no commit call and recorded no relay write. The closed
adapter also refused MDK's attempted prepared KeyPackage publication during
maintenance. The relay saw no such publication through that adapter.

| Measurement | Exact fixture sample |
| --- | ---: |
| Full serial test | 61.66 s |
| Setup and stage | 37.771 s |
| Authenticated history and convergence | 2.281 s |
| Cold reopen and reserved capture readback | 379 ms |
| First write with lost acknowledgement | 5.100 s |
| Cold uncertainty recovery and retry | 2.744 s |

These are individual loopback samples. The test recreates selected owners from
closed stores, with no OS process kill at those cuts. Daemon shutdown reported
that the engine stop, MDK stop and cleanup completed after the engine failed
to quiesce during its initial shutdown interval. The receipt and application
status path is a separate opt-in proof. Installed Peer egress remains closed.

```sh
cargo test --locked -p marmish-native --lib private_wss_recovers_lost_postjoin_ack_without_another_native_send -- --ignored --test-threads=1 --nocapture
cargo clippy --locked -p marmish-native --all-targets -- -D warnings
```
