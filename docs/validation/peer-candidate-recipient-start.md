# Candidate recipient owned startup

The Linux AMD64 candidate startup source was validated at
`79c9d18900d957b14e1139944ede5488005b2b5e`. The proof used temporary
account keys and a private loopback TLS relay. It linked and ran the
actual candidate module, hardened SQLCipher cursor, registered dispatcher,
and raw WSS path.

| Check | Result |
| --- | --- |
| Locked offline candidate library-test link | Passed in 44.54 s |
| Candidate all-target Clippy with warnings denied | Passed in 28.50 s |
| Owned-startup ignored selector | Passed, eight isolated subprocess cases in 55.99 s |
| Existing readiness tests | 7 passed |
| Existing recovered-inventory tests | 3 passed in 0.66 s |
| Scoped rustfmt and Git diff whitespace | Passed |

The startup selector is
`peer_invitation::installed_recipient::tests::peer_installed_recipient_owns_start_and_stop`.
Its cases exercise waiting at one readiness edge without creating a cursor
or contacting the runtime endpoint, a fresh cursor before authenticated
WSS and fatal raw cutoff, stopping with NIP-42 AUTH acknowledgement
withheld, reopening a preexisting private empty cursor, refusing a dangling
WAL with an absent cursor, preserving malformed bytes, refusing replacement
between descriptor preparation and the first keyed-owner path witness, and
revoking readiness while the exact route's raw slot still prevents reap.
The first author runtime identified that a new profile had no cursor file.
The reviewed preparation path resolved that failure without modifying the
legacy fixture opener.

Reproduce in the candidate environment with the exact evidence commit
set in `MARMISH_BUILD_COMMIT` and
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`.

```sh
cargo test --offline --locked -p marmish-native --lib --target x86_64-unknown-linux-gnu \
  peer_invitation::installed_recipient::tests::peer_installed_recipient_owns_start_and_stop \
  -- --ignored --exact
cargo clippy --offline --locked -p marmish-native --all-targets \
  --target x86_64-unknown-linux-gnu -- -D warnings
```

This qualifies the unmounted startup leaf on Linux AMD64. Installed
runtime composition, ordinary services running beside the actor,
Pending and healthy-empty observations, and latency measurements
remain in the [Manual actor phase](../phases/peer-candidate-manual-actor-mount.md).
