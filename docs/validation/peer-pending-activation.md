# Peer pending-invitation validation

The native leave retry capability probe runs against upstream MDK
`0ad3c61088e1acac58e0fcf1a9f1142bae5d997a`, resolved in `Cargo.lock`.
It uses two disposable local identities, encrypted native stage storage,
an authenticated Welcome, an injected wall clock for fanout scheduling and
an adapter with no physical transport. No relay or running daemon is
required.

```sh
cargo test --locked --offline -p marmish-native --lib peer_pending_leave_public_runtime -- --ignored --nocapture
```

The probe verifies the joined room's authenticated inviter, stable
non-admin local membership, and the retained PostJoin obligation with
zero native transport calls. The adapter refuses the Leave publication
before send. SQLCipher reopen retains the native leave request, exact
signed sent-message bytes and the retryable fanout. Retry remains idle
after four injected seconds. A duplicate native Leave returns
`LeaveAlreadyRequested`. After five injected seconds the retry uses
the original exact event, target, account and acknowledgement threshold.

## Open publication gate

The public `LeaveRequest` contains the epoch. The native sent-message
record supplies the signed transport payload and inner MLS wire. The public
MLS projection reports a generic Proposal. These interfaces lack the
authenticated association between the exact event and a SelfRemove
authored by the current local leaf. The native engine's rollback-contained
SelfRemove matcher is private. The public runtime's deferred-commit
method rejects Leave, and its ordinary send result omits the typed
proposal association. The no-I/O retry probe establishes transport
behavior only. Physical leave publication stays disabled.

A separately scoped [public-interface proof](../phases/peer-self-remove-proof.md)
is evaluating authentication against the current MLS leaf with the existing
MDK and OpenMLS interfaces. The runtime can already retry a retained fanout. If a
crash occurs after the native transaction and before the fanout exists,
qualify re-staging from the authenticated exact saved event through
`publish_session_effects` before using that path in production. Pending
application activation, permit enforcement for mixed retained work,
decline convergence and relay qualification require separate validation.
