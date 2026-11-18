# Installed Peer relay qualification validation

The [relay CLI increment](../phases/peer-installed-heartbeat-first.md#as-first-patch-relay-cli-and-production-transport-boundary)
ran on Linux AMD64 with Rust 1.97.1 and upstream MDK `b3cd2cb1`. The
reviewed source was `9159298c98b0ae5583a44a24e6b665dc4d73e7d6`.
The test relay listened only on loopback and used an ephemeral private CA
with production TLS verification. The probe identities, route, profile
and attestation were disposable.

```sh
cargo test --locked --offline -p marmish-native --lib peer_qualification -- --nocapture
cargo build --locked --offline -p marmish-native --bins
MARMISH_TEST_PEER_CLI="$(realpath "${CARGO_TARGET_DIR:-target}/debug/marmish")" \
  cargo test --locked --offline -p marmish-native --lib \
  peer_qualification_native_cli_requires_exact_controlling_tty_confirmation -- --ignored
cargo clippy --locked --offline -p marmish-native --all-targets -- -D warnings
```

Keep the ordinary sibling launchers in
the same build directory so native layout validation succeeds.

The focused selector passed eight tests in 4.08 s, with the native CLI
PTY test run separately. The WSS tests qualified only after all evidence
was present, accepted correlated empty wide history on the authenticated
socket, and refused 24 independently injected relay faults. Every fault
matched its specific fixed diagnostic and left no qualification record.
The cases cover signed ACK correlation, authorized and unauthenticated
writes, registered and outsider authentication, size and Group route
refusals, exact history, overbroad reads, reconnect history and actual
socket disconnect. The local tests also check overlap and out-of-order
socket admission, repeat reconnect, traffic/time bounds, profile/CA drift,
clock checks, expiry, and failed atomic replacement. A valid report
carries only abbreviated trust information, bounded times and status.

The controlling-PTY test passed in 0.56 s with the normal native CLI. A missing TTY,
and then a response with a trailing space, opened zero relay sockets.
The exact affirmative response opened four sockets and produced a
qualification that `peer relay show` read back. This covers the native
entry point and production relay trust policy with a process-owned test
relay. Packaged artifacts, deployed private relays, macOS, Linux ARM64
and Peer enrollment or heartbeat startup were outside this run. An
operator still needs the attestation to describe enforcement guarantees
that cannot be observed during the bounded probe.

The existing CLI, streaming, White Noise defaults and Peer TLS selectors
passed 69 tests in 9.47 s. The existing opt-in private Group authorization
denial and maximum-size Terminal WSS round trip passed in 0.09 s and
9.91 s respectively. Their exact test suffixes are
`private_wss_heartbeat_relay_refuses_unregistered_and_out_of_scope_messages`
and `maximum_terminal_records_round_trip_through_private_wss`. Run each
with `cargo test --locked --offline -p marmish-native --lib <suffix> -- --ignored`.

Strict native all-target Clippy and exact diff and formatting checks on
the final Rust changes passed.
