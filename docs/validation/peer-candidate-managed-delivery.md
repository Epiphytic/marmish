# Managed Delivery on a registered private Peer port

Exact source `35db4bb3a8a24b0d2f23ab65158497e54f3fb76e`. The Linux AMD64
candidate test uses disposable identities, separate private WSS relay origins,
the real MDK runtime and a quota-measured SQLCipher package. A fresh Delivery
from the checked port is deferred before Consent receives its ACK. The same
candidate port settles the real ACK and wakes the saved Delivery. The same
quota owner attests the package from a measured copy and checks the original
encrypted digest again before the saved Delivery can commit Held. The test
refuses a measurement from a different prepared turn before committing the
exact saved row and checks that an unrelated deferred row remains.

The SQLCipher cursor regression checks that a final fence refusal rolls back
Deferred insertion, transactional Held application and retry deletion. It
also checks both wall floors, deferred rows and the retained revision/digest.

The dormant leaf ends at the durable Held checkpoint. It does not mount a
daemon actor, join through native Welcome, or publish a local Pending entry.
The future scheduler needs an explicit policy for a signed-expired retained
row so later rows remain eligible. The checks cover private Linux AMD64 and
do not measure installed public-relay or cross-platform performance.

## Exact results

All seven private WSS cases passed in 126.15 s. The standalone SQLCipher
cursor suite passed 77 tests with four ignored in 78.55 s. Strict
all-target Clippy passed for the native crate in candidate mode (29.24 s)
and stock mode (30.47 s). Scoped Rust formatting and diff checks passed.

| Managed Consent fixture case | Consent turn elapsed ms | Remaining five-second budget ms |
| --- | ---: | ---: |
| Positive ACK, wake and subsequent Held | 575.184 | 4424.816 |
| Wake rollback | 908.853 | 4091.147 |
| Stop before wake | 526.107 | 4473.893 |
| Staged drift | 440.455 | 4559.545 |
| Relay rejection | 459.773 | 4540.227 |
| Lost ACK | 5005.665 | 0 |
| Stop before settlement | 442.102 | 4557.898 |

The fixture times the Consent publication turn. The subsequent retained
Delivery package read and Held commit are covered by the positive case
assertions. They are outside that timing sample. These local debug-profile
observations include injected waits in the rollback and loss cases.

## Retained Command composition

The composed source at `f9bc0ff` passed the candidate libtest link,
strict all-target Clippy (31.20 s), and the complete suite of seven
private WSS scenarios (119.49 s). The positive Consent and wake turn
took 578.915 ms and the wake rollback took 923.570 ms. The lost-ACK
case included its injected five-second wait. The replay onto the
published retained Command proof preserves the reviewed native source
byte for byte. This is a same-owner fixture, with mounted daemon and
public-relay validation still pending.

## Reproduce

Set `MARMISH_BUILD_COMMIT` to the exact source SHA and run with the locked
dependencies. These commands use the unoptimized profile with debug info
disabled and a larger test thread stack for the combined native fixture.

```sh
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 RUST_MIN_STACK=33554432
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --lib managed_consent_single_port_private_wss -- --ignored --nocapture
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --test peer_cursor_store
cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --all-targets -j2 -- -D warnings
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS
cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings
```
