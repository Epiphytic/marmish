# Exact native SelfRemove qualification

The ignored fixture runs entirely in-process on disposable encrypted local
stages. It uses a real authenticated native Welcome, a non-admin member,
native `SendIntent::Leave`, signed outer Nostr wire, current epoch MLS
exporter, and an adapter that refuses all writes before a transport call.
It checks an exact signed Sent message, duplicate native leave rejection,
stable native replay fingerprint across a SQLCipher-only reopen, and
one exact durable fanout retry at its due time. The restarted runtime
authenticates the same exact request before retry in a single run.

Generated negative inputs include authenticated regular MLS Remove, an
own-leaf MLS private application message, another account's authenticated
SelfRemove, a wrong expected own signer key, an older epoch, a wrong
branch pin, a substituted tag or request account, an Inbox target, a
different acknowledgement threshold, changed event ID or signed bytes,
and missing, blocked or older-epoch native leave state. The SQLCipher
snapshot refuses an explicit write. A separate terminal close of the
disposable native connection makes the OpenMLS load fail closed.

The fixture does not generate a real same-account sibling-device
SelfRemove or a public non-Proposal message. The independently loaded
signer key and own MLS leaf index comparisons remain in the proof. One
private application message exercises the framing refusal. Multi-item
native fanout scan order and an inter-snapshot row replacement are not
exercised. These are still qualification cases for the production
physical egress permit.

No socket is opened, join consent is not deployed, and this proof does
not confirm group leave at another member. If the native leave request
and exact signed Sent row survive a crash without a native fanout, the
design keeps the decline terminal and unconfirmed, with no replacement
send. The test does not exercise that crash window.

Run the opt-in qualification with the checked-in dependency lock.

```sh
cargo test --locked -p marmish-native --lib peer_pending_leave_public_runtime -- --ignored --nocapture
```
