# Manual receiver saved Delivery validation

The scoped source is `7f4e50cea01820ae34f8e1a607c3434dba4f480f`, reviewed against
[the scheduler design](../phases/peer-candidate-manual-scheduler.md). This
increment exercises the guarded saved Delivery visit as a separate component
on Linux AMD64. The WSS fixture uses one registered receiver, one private
NIP-42 relay, genuine signed v2 flights and SQLCipher. The fixture does not
launch an installed daemon or enable Peer functionality.

| Exact source check | Result |
| --- | --- |
| Candidate libtest link | 45.91 s. Immutable libtest SHA-256 `372e82fbe02ec08d74031269fa289b04b7e88488f558577f162b6929b0f29fc7`. |
| Candidate `saved_delivery` | 2 passed, 1 ignored, 0 failures in 0.16 s. Exact SQL row and floor rollback, different or missing head, same-ID revision change, and narrowing without restarting the signed or elapsed window. |
| Candidate `saved_delivery_guarded_visit_private_wss` | 2 isolated cases passed in 36.97 s total. Waiting took 15.10 s and the eligible Live cutoff took 21.83 s. |
| Candidate `managed_expired_delivery_private_wss` | Both existing signed-retirement cases passed in 45.92 s total. The wrong-key refusal is expected. |
| Stock native all-target Clippy with warnings denied | Passed in 29.86 s. |
| Rust formatter and whitespace | Passed for the changed Rust paths. |

The WSS Waiting case compares the exact retry tuple and both durable clock
floors before and after probing and acquisition refusal. It compares them again
around a refusal immediately before the guarded Waiting commit. It also
rejects a live port with a different Fatal lifetime before touching the row,
then verifies that the correctly bound visit commits one backoff. The WSS Live
case observes an existing signed eligible snapshot before its cutoff,
continues under the original registered window after the cutoff and checks
`Reobserve`, exact durable state and unchanged native effect counters.

The initial JSON build-output pipeline closed the test runner's stdout while
parsing its human-readable records. The build completed, and the results above
come from directly running the immutable libtest outside that pipeline.
