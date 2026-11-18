# Native Peer activation history over private WSS

The Linux AMD64 fixture uses the [private WSS activation design](../phases/peer-postjoin-private-wss.md)
and upstream MDK master as resolved at `f89bd59089b92ddbb2492def6972e8bc75d09cf6`.
Source `4477c1d98159269cadb2b9e9b58fb6bce858de91` includes the existing local
retirement code. Disposable founder and recipient profiles use SQLCipher and
separate authenticated WebSocket connections to a two-account TLS relay.

The founder tracks its genuine Welcome through Offer, Consent and Delivery.
After the daemon records Activating and releases the recipient profile, the
founder publishes a native SelfUpdate. The test compares the original signed
event ID and SHA-256 with the relay's record. The recipient reopens its stage
at the original epoch and roster, obtains that record through an exact history
request, and processes it through native MDK. It reaches the founder's epoch
and authenticator with the expected active leaves and processed transport ID.

| Scenario | Setup through Activating and daemon stop | Founder update | Recipient history and outcome |
| --- | ---: | ---: | --- |
| Complete history | 22.118 s | 160 ms | 1.657 s, converged |
| Matching end marker with omitted update | 26.167 s | 263 ms | 151 ms, original epoch retained |
| Missing end marker | 26.813 s | 296 ms | 5.062 s, native fetch expired and original epoch retained |

The serial run passed all three cases in 106.56 s. Route and signed-event
checks passed two fast tests. The standalone WSS socket test passed in 1.36 s.
It verifies a dropped first acknowledgement, identical retry, one stored
event and connection-scoped readback. The ordinary Command outage regression
passed in 48.09 s, with 5.559 s for command execution during the outage. Strict
all-target native Clippy and focused Rust formatting passed. Routine CI keeps
its existing core subset.

```sh
cargo test --locked -p marmish-native --lib postjoin_private_wss -- --ignored --test-threads=1 --nocapture
cargo test --locked -p marmish-native --lib exact_group_history_and_retry_remain_socket_scoped -- --ignored
cargo test --locked -p marmish-native --lib peer_daemon_wss_peer_outage_preserves_ordinary_commands -- --ignored
```

The elapsed values are individual fixture samples. They measure local
enrollment and history recovery with a test relay. Durable recipient PostJoin
publication, an acceptance receipt, and remote relay qualification follow in
the remaining activation increment.
