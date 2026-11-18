# Charged Peer founder turn validation

Source `5323b52f9849bc12902316d09a91888237e6cb17` on dual-published
base `03a6191`, with upstream MDK `bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`.
The Linux x86-64 debug-profile run used bundled SQLCipher, loopback TLS with
NIP-42 authentication, disposable keys and two distinct accounts. Each cold
process opened the original profile independently. Test executable SHA-256
`2b9930e94d1bdcbbf8afd439f37e93332aff0a8e6258aa0dea04b95a7607f4d3`.

| Check | Result | Elapsed |
| --- | --- | --- |
| Founder opt-in proof | 5 passed | 78.80 s |
| Existing ordinary Command across Peer outage | 1 passed | 29.74 s |
| Existing authenticated direct WSS and exact send correlation | 1 passed | 20.01 s |
| Existing physical binding across cold reopen and refusal faults | 1 passed | 9.74 s |
| Signed v1 and v2 wire cases | 11 passed, 1 separately opt-in | 5.20 s |
| Offline v1 and v2 wire benchmark | 1 passed | 8.88 s |
| Existing SQLCipher quota transactions and hot-journal recovery | 1 passed | 0.39 s |
| Normal binaries and CLI/daemon help | Passed | 23.12 s build |
| Strict native all-target Clippy | Passed | 32.60 s |

The real loopback fixture counted exactly one Offer, Consent and Delivery.
Both participants verified their signed v2 transcript. The receiver validated
its exact delivered Welcome and derived room. The isolated founder image
retained one tracked native Welcome with its outstanding send withdrawn.
Independent native transport and KeyPackage-publisher tripwires each stayed
at zero across creation, withdrawal, cold readback and runtime close. The
fixture measured 4,910 journal growth writes. A second run delivered an exact
wrapped event but withheld the relay's ACK. The durable result became uncertain
and reopened without a second write. The real grant was also revoked before
Offer write and before native provisioning. Both cases recovered on cold
restart and the latter retained `Consent` with no native image.

Five closed restart cases cut after committed transitions before file creation,
inode persistence, Delivery freeze, egress arm and socket write. A wrong-inode
variant refused its native open. The separate abrupt case stopped the real
native process with code 80 after a database page write to the encrypted
stage and while its synced rollback journal was outstanding. Cold readback
observed a nonempty journal, retained the `Started` charge and refused native
access. The independent ordinary VFS recovery test covers the separate opt-in
recovery behavior. The founder case leaves recovery disabled.

The standalone native case checks exact Consent ancestry, identical replay,
expired and v1 flight refusal, package parsing, the one-poll Pending refusal,
profile ownership conflicts, stock SQLite delegation for unrelated paths,
managed aliases and sidecars, and wrong-key quarantine. The wrong-generation
key test intentionally produced a local SQLCipher decrypt error. Normal
`marmish` and `marmishd` help worked and neither binary contained the founder
fixture selectors. For the ordinary Command check, the first invocation of
the archived unit binary could not locate its sibling `marmishd`. Running the
same exact executable in its Cargo layout after rebuilding both binaries
passed and confirmed exit and command output after Peer outage.

| Loopback sample | Time |
| --- | ---: |
| Positive signed Offer through verified Consent | 4,271 ms |
| Positive charged native turn | 8,231 ms |
| Positive Delivery through correlated ACK | 75 ms |
| Positive independent in-process native readback | 100 ms |
| Positive new-process native readback | 58 ms |
| Accepted Delivery with ACK deliberately withheld | 1,023 ms |
| Ordinary Command reply with Peer outage, initial code | 6,161 ms |
| Ordinary Command reply with Peer outage, exact corrected code | 7,453 ms |

These are single-sample diagnostic timings from a shared local runner, with
some tests concurrently active. They do not establish a latency regression,
public relay throughput, normal installed founder support or performance on
other platforms. Status, verified receipt authority and activation remain
outside this proof.

The independent offline codec benchmark used 100 measured runs per version
after 10 warmups. V2 verify p50/p95 was 1.122/1.165 ms for Offer,
13.513/14.032 ms for Consent, and 3.042/3.174 ms for Delivery. The fixed
samples encoded to 2,585, 5,997 and 11,457 bytes respectively in both versions.
These are signed frame sizes for the fixed payloads, without WebSocket or
network overhead.

Reproduce from the source root with the two debug profiles set to `debug=0`
and an existing private Cargo target. Build the normal launchers first so the
ordinary daemon regression can resolve the expected binary layout.

```sh
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo build --offline --locked -p marmish-native --bins -j2
cargo test --offline --locked -p marmish-native --lib -j2 peer_invitation::founder_turn_tests -- \
  --ignored --skip founder_native_child --skip founder_wss_child --skip founder_cold_child \
  --test-threads=2 --nocapture
cargo test --offline --locked -p marmish-native --lib -j2 \
  peer_daemon_wss_peer_outage_preserves_ordinary_commands -- --ignored --nocapture
cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings
```
