# Candidate read IPC validation

The real Unix listener test exercises typed List and Show with the candidate
owner absent. Both return `Unavailable`. Raw Approve, Decline, extra fields,
uppercase npub and hex, and zero generation return `InvalidRequest`. The
ordinary queue remains empty, and Status and an ordinary Command IPC
request still use the same socket. The Command assertion observes local
dispatch and reply framing using an in-process queue responder.

The client tests check exact Show coordinates, no more than 16 unique
Pending entries, an expiry after each entry’s Held time, and refusal of
mixed or internal response fields. A stalled listener exercises the
client's single absolute timeout with a paused clock. The existing UID
predicate test covers a foreign UID value. The socket test authenticates
the real same-UID caller, with no privileged second UID process.

The read client and operation remain candidate-only. This increment covers
local unavailable behavior and request/reply validation. Positive Pending
projection, worker readiness, queue overload, the CLI and installed daemon
qualification remain part of the composed [daemon mount](../phases/peer-candidate-daemon-mount.md).

Source `393e33f` was tested on Linux AMD64 with the explicit
`marmish_peer_receiver_candidate` target cfg, exact source stamp and
the locked dependency set.

| Check | Result |
| --- | --- |
| `cargo test --locked --offline -p marmish-native --lib --target x86_64-unknown-linux-gnu -j 2 ipc:: -- --test-threads=1` | 62 passed, two benchmarks ignored. Build took 38.84 s and tests took 0.15 s. |
| `cargo clippy --locked --offline -p marmish-native --lib --bins --target x86_64-unknown-linux-gnu -j 2 -- -D warnings` | Passed in 25.16 s with the non-test candidate. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |
| Integrated stock build `cargo test --locked -p marmish-native --lib ipc::` | 59 passed, two benchmarks ignored. Build took 42.51 s and tests took 0.13 s. |
