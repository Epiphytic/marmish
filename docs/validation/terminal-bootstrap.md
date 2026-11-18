# Terminal bootstrap validation

The bootstrap codec and state machine in this increment are compiled into the
native test binary. The [reviewed design](../phases/terminal-bootstrap-codec.md)
covers admission for a future Terminal owner. This fixture creates no Terminal
MLS room, shell grant, runtime reservation or PTY.

## Reproduction

Run from the workspace root with pinned dependencies already in the local
Cargo cache. The explicit relay in the second check is a disposable loopback
TLS fixture.

```sh
export CARGO_NET_OFFLINE=true
cargo test --locked -p marmish-native --lib terminal_bootstrap::core_tests
cargo build --locked -p marmish-native --bins
cargo test --locked -p marmish-native --lib bootstrap_stays_on_bound_alias_lanes -- --ignored --nocapture
cargo test --locked -p marmish-native --lib terminal_copies_never_ingest_in_durable_command_profiles -- --ignored --nocapture
cargo test --locked -p marmish-native --lib benchmark_signed_bootstrap -- --ignored --nocapture
cargo clippy --locked -p marmish-native --tests -- -D warnings
```

The Linux x86_64 host used Rust 1.97.1 and the unoptimized development build.
The five core tests passed in 0.04 seconds after the test binary was built.
The native binary build and Clippy passed.
The core selection covers signed wrapper verification, wrong wrapper and seal
signatures, sealed and rumor signer mismatches, misplaced or extra
recipient tags, forged IDs, exact record shape and hex, size and time bounds,
another binding in an otherwise correctly signed message, alias separation
across bindings and canonical remotes, reordered records, duplicate
re-wrapping, transcript replay after restart, rate limit and expiry. The
revocation case supplies a valid Confirm that would produce Ready, refuses
it before publishing or exposing any Terminal key, and refuses subsequent
retry and confirmation.

Each test exchange holds one live transcript for its binding. Expiry clears
the transcript and leaves the alias codec available for a fresh Probe.
Revocation or owner teardown releases the codec. The per-pair cap across
bindings and the alias lane lifetime belong to the future Terminal owner.

The opt-in loopback test authenticates each alias through an exact NIP-42
read policy. Two SQLCipher Command owners derive the same binding for each of
two live groups and different aliases between their groups. A real native
worker completes a byte-exact Command in each group before the bootstrap.
A valid Probe sent to the alternate group is delivered on that alias. The
test closes the alternate exchange and verifies the original bootstrap
exchange and the canonical Command adapter both refuse the Probe before
starting the main exchange. The intended aliases deliver all four signed
records to the bootstrap checker. The test checks both SQLCipher stores for
ingress markers and processed transport IDs and checks the relay transcript
for additional KeyPackage writes. Ordinary Command execution is rechecked
after closing the alias lanes.

The shared Command fixture consumes a transient Poll Busy receipt before
retrying at identical durable output cursors with a fresh request ID. It checks
cursor preservation and caps these retries at eight replies within a
15-second fixture Command deadline. The fixture waits 25 ms after each Busy
receipt before trying again. Individual loopback relay steps use five seconds.

## Measurements

On this host, 32 warm samples measured from signed wrap to authenticated check
had a 2.517 ms median and 2.588 ms p95. The local four-record lifecycle had a
10.686 ms median and 11.401 ms p95 over 32 warm samples. Initial canonical
key generation and both alias derivations took 1.332 ms. The four-record
loopback exchange took 67.605 ms. The full opt-in proof took 25.60 seconds
including private profile setup, real worker commands, isolated route checks
and cleanup. The existing durable Command proof also passed in 25.64 seconds.
These are unoptimized loopback-host timings. Public relay behavior, Tor and
cross-platform performance need separate measurement.
