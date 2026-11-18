# Live Peer pending join custody on Linux AMD64

The integrated fixture uses two disposable Nostr identities, a private
loopback TLS WebSocket relay with NIP-42 authentication, SQLCipher, and
the ten workspace dependencies resolved from upstream MDK master at
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`. It uses a fresh profile for
each enrollment and never contacts a public relay.

```sh
cargo test --locked -p marmish-native --lib peer_private_wss -j 2 \
  -- --ignored --nocapture --test-threads=1
cargo test --locked -p marmish-native \
  --test peer_cursor_store --test peer_control_wire -j 2 -- --test-threads=1
cargo test --locked -p marmish-native --lib prepared_consent -j 2 \
  -- --test-threads=1
cargo test --locked -p marmish-native --lib \
  exact_joined_snapshot_proof_requires_closed_image_and_receipt -j 2 \
  -- --ignored --test-threads=1
cargo clippy --workspace --locked --all-targets -j 2 -- -D warnings
```

The Manual run delivers Welcome before the matching Consent ACK. It
interrupts the owner after durable settlement and before waking deferred
work, then recovers Held from the timed retry. The Automatic run delivers
Welcome after the ACK and commits Held from the live subscription. The
relay correlates the signed event ID and hashes the exact stored Consent
bytes at ingress. It observes only four events per enrollment, including
the intentional competing Delivery and the single recipient event,
Consent. There is no recipient receipt, rotation or telemetry.

Both paths reject the competing signed carrier and wrong generation, and
continue to produce the exact replay disposition through Held, JoinStarted,
Pending and Activating. The cold recovery cut after native close reopens
encrypted snapshots and persists Pending. A simulated hot-journal marker
stays untouched. The mark also proves the legacy Consent publish returns
no work before opening the native stage. An independent case stops after
JoinStarted and before native mutation. Restart returns unavailable, leaves
the original digest unchanged, and does not ingest the same Welcome again.

The reopened blocked policy refuses approval for a correctly shaped
same-UID request. The Manual row stays manual under a later Automatic
policy and needs the kernel-authenticated same-UID decision. Auto mode uses
the same decision comparison. After the decision CAS, cold readback
returns Activating. The test also checks exact history replay, logical
retention boundaries, and rejection of an unqualified application clock.

## Results

The private WSS suite at `b8cf197710196ce3538c84adf8bb92d8b4c507e0`
passed all three tests in 121.51 s with the host running other local
builds. That includes both enrollment modes, the before-ingest restart
case and the previous three-flight test. These are single observations
per mode from that run, with no percentile estimate.

| Sample | Native join and close | Restart readback and Pending | Verified decision |
| --- | ---: | ---: | ---: |
| Manual | 1,298 ms | 565 ms | 518 ms |
| Automatic | 1,482 ms | 568 ms | 514 ms |

The final fixture refused expired JoinStarted, Pending and Activating
rows at the shared custody deadline. The direct activation commit used
verified native readback. The full default `peer_invitation::` selection
passed 71 tests in 209.97 s, with 25 opt-in cases skipped.

The unchanged wire and cursor targets passed 8 and 36 tests, respectively,
with five opt-in cases skipped. The Prepared Consent filter passed nine
tests with two opt-in cases skipped. The exact independent-snapshot test
passed in 49.31 s. The maximum-size Terminal frame through private WSS
passed in 9.85 s and measured 39–41 ms for each 4,096-byte direction's
publish, receive and decrypt. Strict workspace all-target Clippy and
scoped Rust format passed.

The standalone [checkpoint validation](peer-m2-custody-checkpoint.md)
covers schema and transition guards. The standalone
[native validation](peer-m2-native-join-readback.md)
covers exact-signature mismatches, SQLCipher image and sidecar integrity,
and independent snapshot evidence. This test-only milestone exercises
no installed Peer CLI or daemon service and has Linux AMD64 evidence only.
