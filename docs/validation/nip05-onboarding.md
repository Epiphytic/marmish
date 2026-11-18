# Native NIP-05 onboarding validation

The local fixtures run on Linux AMD64 with Rust 1.97.1 and the default
unoptimized Cargo test profile. The fixture TLS trust root and Nostr identities
are disposable. Tests use loopback sockets or in-process inputs.

```sh
cargo test --locked --offline -p marmish-native --lib profile_ -- --include-ignored --nocapture
cargo test --locked --offline -p marmish-native --lib identifier_grant_commits_only_the_verified_key
cargo test --locked --offline -p marmish-native --lib identifier_confirmation_on_controlling_terminal -- --ignored
```

The first command runs the onboarding parser and policy tests, both TLS
fixtures, both WebSocket relay fixtures, the full publication transaction,
and both benchmarks. All 22 selected tests passed. The
second exercises actual setup, the profile lock and atomic config commit
using a verified candidate fixture. A mismatched full npub leaves the config
byte-identical. A match preserves only the public key. A changed directory
response fails against the earlier confirmation and preserves the grant.
The opt-in controlling-terminal fixture passed on Linux. A real `/dev/tty`
rejects a shortened npub, accepts the exact full key, and handles end of file.

The built Linux CLI completed local file-backed setup in an isolated profile.
In separate processes without a controlling terminal, identifier setup and
profile publication refused without explicit confirmation. A malformed
public email was rejected before a network attempt. The original config
remained byte-identical across the three refusals.

The TLS fixtures check certificate trust and hostname, HTTP status and
redirect refusal, response bounds and exact directory parsing. The relay
fixtures exercise signed metadata over real local SDK connections, independent
relay completion, corrupted signature handling and partial publication ACKs.
The full publication test uses a disposable reader for preview and the
profile signer for the final event. It exercises missing headless consent,
a directory result for the wrong account, a changed result after preview,
and one verified and acknowledged publication that preserves existing tags
and unknown fields. Test-only seams provide the terminal state, directory
answers and loopback relay.
The core tests also enforce the unbootstrapped forced-Tor refusal. A live Tor
directory lookup, public-relay publication, and macOS or Linux ARM64
qualification need separate runs.

## Benchmarks

Both benchmarks use monotonic elapsed time and nearest-rank percentiles. The
HTTPS timer includes a new loopback TCP connection, Rustls TLS handshake,
Hyper HTTP request and parsing the directory key. The metadata timer includes
event and signature validation, metadata selection and identifier verification
after relay acquisition. DNS, WAN, Tor and node setup time are outside these
timers. The two benchmark tests ran alongside the filtered fixture suite.

| Operation | Warmups | Successful samples | Failures | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Loopback HTTPS directory lookup | 5 | 100 | 0 | 1,394 µs | 1,738 µs | 1,787 µs |
| Signed Nostr metadata verification | 20 | 200 | 0 | 172 µs | 224 µs | 237 µs |

The self-hosted `interactive-core` workflow selects 11 short tests on the
Linux AMD64 runner for trusted master events. Fixture network tests and
benchmarks remain available as local filters.

An earlier full-library run overlapped with a Cargo rebuild of the same test
executable. In that run, 31 existing subprocess tests tried to reopen the
replaced executable and failed with ENOENT. Two representative tests passed
in isolated reruns. The full-library rerun uses a pinned hard link to the
final harness so a later Cargo build cannot replace the executable mid-test.
That run had 659 passes, six ignored tests, and one failure. The live loopback
publication fixture exceeded its 40-second deadline under the parallel suite.
It passed in 7.34 seconds in an isolated run and in 8.3 seconds in Claude's
independent rerun. Run it separately by name with `-- --ignored`, or use the
`profile_` command above. The core Actions filters are unchanged.
