# Isolated Peer PostJoin recovery on Linux AMD64

The [approved design](../phases/peer-postjoin-publication-proof.md) uses two
SQLCipher-backed native MDK accounts and an in-process sink. Cargo resolves
upstream MDK master at `f89bd59089b92ddbb2492def6972e8bc75d09cf6` for this
build. The exact fixture is opt-in.

```sh
cargo test --locked -p marmish-native --lib postjoin_gate::tests
cargo test --locked -p marmish-native --lib \
  exact_postjoin_commit_is_recovered_and_delivered_once -- --ignored --nocapture
```

The test captures the pending commit from a durable Activating stage with
relay publication closed. It verifies the candidate and binds the route,
event ID, serialized bytes and authority to the one-use gate. After an
independent SQLCipher reopen the gate refuses a pre-send attempt, then
delivers the same commit once. The founder's native store records the exact
signed transport event ID and its epoch pin matches the recipient's staged
pin before the gate acknowledges. The recipient confirms the same epoch and
consumes its PostJoin obligation. Reopen and duplicate delivery leave the
epoch unchanged.

| Run on `d99150f` | Full test | Capture | First reopen | Confirmation and final reopen |
| --- | ---: | ---: | ---: | ---: |
| First | 43.91 s | 634 ms | 138 ms | 2,594 ms |
| Repeat | 36.89 s | 652 ms | 155 ms | 2,440 ms |
| Final source `a84ab05` | 23.88 s | 1,667 ms | 75 ms | 1,808 ms |

All runs confirmed without an extra sweep. Strict native all-target Clippy
passed on the final source. The gate suite passed all three
tests. The prior regression executable on the same MDK lock passed 935 native
tests with 99 opt-in tests skipped, three daemon custody WSS tests in 165.31 s,
and three joined custody WSS tests in 111.56 s. Those WSS runs cover the
existing custody paths. The PostJoin measurements use loopback in-process
handoff and do not measure relay latency or installed client activation.
