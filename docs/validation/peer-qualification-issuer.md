# Authenticated Peer relay qualification issuer validation

Source `bdf3c54675cd93d2b9039806964c590fd4f13b0b` implements the
checked issue/readback increment of the [production Peer route
phase](../phases/peer-production-route.md). The Linux x86_64 checks used
the locked upstream MDK revision `b3cd2cb1`, fresh disposable credentials
and private loopback fixture endpoints.

| Check | Result |
| --- | --- |
| Normal `marmish` and `marmish-worker` debug builds | Passed in 5.83 s |
| `peer_relay_trust::tests::peer_qualification_native_cli_requires_exact_controlling_tty_confirmation` with the built `marmish` | Passed in 0.58 s |
| `identity::tests::qualification_reads_only_sqlcipher_from_selected_keychain` | Passed in 0.25 s |
| Native `peer_relay_trust::` suite | 15 passed in 10.17 s. Its opt-in CLI test ran separately above. |
| Native installed-checks Clippy, all targets, warnings denied | Passed in 43.69 s |
| CLI and stream parser/runtime tests matching `cli::tests::` | 62 passed in 9.43 s |
| `maximum_terminal_records_round_trip_through_private_wss` | Passed in 9.40 s |
| `private_wss_heartbeat_relay_refuses_unregistered_and_out_of_scope_messages` | Passed in 0.10 s |
| Touched-file Rust formatting, staged diff whitespace, and exact ignore paths | Passed |

The CLI test used an actual controlling PTY, TLS private CA trust,
an exact relay-name confirmation and an independent profile lease
after CLI exit. Authenticated readback accepted the durably committed
pair under the same scope. It rejected a key derived independently
from another actual profile and refused forged tags, attestation/probe
digests, wrong build/time/scope and changed config, CA or credential
manifest descriptors. The qualification tests cover the managed file
interruption cut and repair, and preserve the prior files and profile
on a refused or timed-out credential read. The read-only helper test
used a simulated native Keychain executable to verify that only the
selected backend's SQLCipher load was requested and that profile,
account and device each bind key derivation independently.

Run the focused suites with Cargo in the workspace root. Set
`MARMISH_TEST_PEER_CLI` to the matching native debug binary for the
opt-in test and invoke the exact selector with `-- --exact --ignored`.
The proof covers issue/readback on Linux x86_64. macOS, ARM64, packaged
installers and the installed Peer daemon path were outside the run.
The fixture Keychain executable exercises the helper contract. It
does not establish macOS Keychain or Linux Secret Service availability.

The Linux x86_64 composed source `4aab1f2` passed the normal binary
build in 25.79 s, the 15 relay qualification tests in 10.03 s, the
controlling-PTY private WSS issuer case in 0.54 s and the selected
Keychain helper test in 0.18 s. Strict installed-checks Clippy across
all targets passed in 39.61 s. The Rust, dependency and ignore files
remained byte-identical after rebasing onto `cd90f8b`.
