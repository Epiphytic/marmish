# Pending decline after native leave persists and routing rejects

This opt-in Linux AMD64 proof uses Marmish's native fixture at
`b02b1895a4374d2bc45dc781341effff0a4d8209` and upstream MDK locked at
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`. Disposable SQLCipher
stores receive a real authenticated Welcome. The local leaf is a
non-admin member of the expected room. The first runtime has no
static group route. Its single Leave reports a missing route after
committing the signed Sent event and LeaveRequest. It leaves no
queued intent, pending commit, fanout, adapter request or relay report.

After closing the runtime, a SQLCipher-only reopen preserves the
original group replay fingerprint. Hydrating the replacement runtime
populates MDK's validated-tree cache, which is included in that
fingerprint. The authenticated branch, room authority, stored group,
LeaveRequest and Sent inventory match before and after hydration.
The recovery proof compares the replay fingerprint against the
hydrated baseline for authentication, staging and subsequent opens.

Recovery selects the candidate from the reopened store without the
original error's event ID. The existing typed verifier checks the
signed Nostr event, exporter, persisted MLS bytes, room and local
leaf's SelfRemove. Synthetic selection inputs exercise missing,
wrong epoch, wrong room, ambiguous, malformed and oversized records.
The existing verifier also exercises changed signed bytes and wrong
event ID, route, acknowledgement threshold, expected key and branch.
The sibling case supplies the authenticated regular Remove, private
message and other-leaf SelfRemove vectors. The fixture uses the
existing 256 KiB stored signed-wire decode cap.
The cap applies after the native storage API materializes the rows.

The public Proposal effects path stages the verified event with the
same account, room, endpoint and one-ack threshold. The deny-only
adapter sees one request at staging, zero before the retry deadline
and one exact retry at the deadline. The frozen fanout survives reopen
and remains the only retryable entry. The group and leave evidence
stays unchanged. The retry case starts with the exact fanout created
by the staging step. Attempted restaging and conflicting or ambiguous
existing fanouts remain unexercised.

The fixture opens no socket. Physical delivery, remote MLS convergence,
concurrent ownership, atomic verify-and-stage, process-kill ordering
and cancellation remain outside the proof. The Sent scan is scoped to
disposable fixture data. A production selector needs a bounded native
query or an independently qualified exact identity.

Run both opt-in cases with the checked-in lock.

```sh
cargo test --locked --offline -p marmish-native --lib peer_pending_unrouted_leave_recovers_exact_event -- --ignored
cargo test --locked --offline -p marmish-native --lib peer_pending_leave_public_runtime_redrives_exact_event -- --ignored
```

Both cases passed with Rust 1.97.1 and the unoptimized test profile.
The new route-rejection case took 15.48 seconds. The existing
exact-event and typed SelfRemove case took 12.93 seconds.
