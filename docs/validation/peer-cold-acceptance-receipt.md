# Cold Peer acceptance receipt over private WSS

Linux AMD64 source `6f5f087d44eb9449a41667359aebdf421d2b156a`
used upstream MDK master at `122ad45ece73c1d6f687a4e92a541b9b2febde34`.
The opt-in fixture used two disposable accounts, separate SQLCipher
stores and a private TLS WebSocket relay.

After native PostJoin and exact cold confirmation, the recipient committed
an acceptance receipt reservation. It recorded NativeStarted before calling
MDK and refused capture while there was no native outbox record. With
publication closed it staged the native message, reopened the store and
captured the retained signed fanout. The relay accepted the first armed
write and withheld its acknowledgement. The founder verified the retained
native application source against its independently stored enrollment
before committing acceptance. The recipient still had an Armed journal
at that point. After an Uncertain transition and reopen, it retried the
same signed event and read the exact acknowledgement back from native
storage before committing Published.

The relay recorded exactly five physical Group writes across the full
flow. Writes four and five shared the same signed ID and SHA-256 digest.
At the deadline, an unfinished Armed receipt could neither confirm nor
publish. The recipient used exact Published authority to finish removing
one remaining native fanout after independently reopening storage. The
founder used the existing durable acceptance to retire the original
native source. Replays could re-emit its original-ID callback, but after
retirement they did not recreate a retained native pending row or make
another application decision.

| Measurement | Exact loopback sample |
| --- | ---: |
| Full opt-in test | 79.57 s |
| Ordinary Command in the same fixture | 16.019 s |
| Encrypted receipt path | 8.683 s |
| Founder acceptance before retry acknowledgement | 7.356 s |
| Receipt cold capture reopen | 155 ms |
| Deliberately lost receipt acknowledgement | 4.982 s |
| Receipt retry and acknowledgement | 272 ms |

Strict native all-target Clippy passed in 27.92 seconds. The prior
integrated build, with identical runtime behavior and an explicit
test-only lifetime drop, passed three publication/checkpoint tests and
43 cursor-store tests with four opt-in cases skipped. The codec and
standalone cursor tests also passed on the separately reviewed patch.
The final exact source passed the actual WSS test after the lint fix.

```sh
cargo build --locked -p marmish-native --bins
cargo test --locked -p marmish-native --lib private_wss_cold_acceptance_receipt_projects_exactly_once -- --ignored --test-threads=1 --nocapture
cargo test --locked -p marmish-native --lib 'custody::publication::tests'
cargo test --locked -p marmish-native --test peer_cursor_store
cargo clippy --locked -p marmish-native --all-targets -- -D warnings
```

These timings describe a single Linux loopback trial. The cold cuts
close and recreate native and application handles in the test process.
The native Published cleanup proof requires the original stable receipt
epoch and authenticator. An epoch change leaves the native fanout fenced
for separate reconciliation. The fixture cannot authorize installed
Peer traffic or status.
