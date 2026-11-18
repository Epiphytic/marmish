# Private WSS Peer enrollment M1

The ignored native M1 selector exercises the Peer control lane on a private, ephemeral loopback TLS WebSocket relay. The receiver and founder use separate temporary state directories and disposable Nostr identities. The fixture uses the singleton relay URL and generated test CA, with direct transport. The test uses the upstream MDK master revision resolved by the workspace lock, `9a29d1e46f11bc8e421046d5a1e20b52b59795df`.

On Linux x86_64 the following exact selector passed in 23.59 seconds in the unoptimized test profile at `5ce1727051a64b4708e4f783e568f3fb724e2984`.

```sh
cargo test --locked --offline -p marmish-native --lib \
  peer_invitation::private_wss_tests::peer_private_wss_offer_consent_delivery \
  -- --ignored --exact --test-threads=1
```

Both actors completed NIP-42 authentication and opened their addressed live subscriptions before the Offer. The recipient reserved and prepared a native KeyPackage on a fresh encrypted stage. It queued the signed Consent, reopened the checkpoint and sent the stored carrier bytes. The relay observed those exact bytes and returned the matching carrier ID. A second publish attempt after the ACK sent nothing.

The founder verified Consent and used its native KeyPackage to prepare a room and one native Welcome with a physical deny adapter. Only the Peer wrapper reached the relay. The recipient verified Delivery using the exact reopened Consent transcript, current authenticated sender, exact signed Offer, and the attested original package. The expected exchange consisted of three writes, all kind 7435. Each was associated with the expected authenticated principal and recipient. The negative case then sent one separately signed Delivery for a different Consent revision. The recipient refused it. The test then quarantined the relay origin through
the cursor's duplicate page check and verified that an exact current live
receipt also failed while the origin was quarantined.

After reconnect, stale live evidence failed. Fetching the valid Delivery twice over WSS returned the exact received frame and left the history cursor unchanged. The original recipient stage image remained unchanged. Native custody checks verified the original unconsumed package and absence of a local group.

The dedicated relay-scope unit test passed in 0.01 seconds. It checks
authentication, exact recipient and kind filtering, the outer signer rule,
and the frame size limit. The earlier broad `peer_invitation::` run on M1's
first source commit passed 68 non-ignored tests in 328.43 seconds, with 21
ignored selectors.

The exact standalone wire target passed eight tests in 1.98 seconds and
ignored one benchmark. The standalone cursor target passed 33 tests in
46.65 seconds and ignored four benchmarks. Strict lint passed in 26.51
seconds.

```sh
cargo test --locked --offline -p marmish-native \
  --test peer_cursor_store --test peer_control_wire -- --test-threads=1
cargo clippy --locked --offline -p marmish-native --lib --tests -- -D warnings
```

The same Rust source was composed on top of the published Prepared Consent
commit `3a17333`. The exact live selector passed again in 26.03 seconds, and
workspace Clippy passed in 28.81 seconds with `--workspace --all-targets` and
warnings denied. All ten upstream MDK lock entries matched the revision above.

Each of the changed Rust files
passed formatting, and the full repository format check reported the
same two unrelated diffs as the base.

The measured end-to-end time includes the platform's SQLCipher and native MDK operations. M1 completes at verified Delivery. Native Welcome ingest, durable Pending, approval, heartbeats, the released CLI and macOS coverage are separate validation items.
